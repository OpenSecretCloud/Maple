//! One on-demand process owns a profile's backend. Frontends attach over
//! OS-private IPC; closing a frontend never tears down another client's runtime.

mod ipc;
pub mod protocol;
pub(crate) mod receipts;

use crate::backend::{self, LocalBackend};
use maple_agent::agent::AgentServiceEvent;
use protocol::{ConnectionMode, Hello, Message, Request, Welcome};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, broadcast, mpsc, oneshot};

const START_TIMEOUT: Duration = Duration::from_secs(15);
const IDLE_GRACE: Duration = Duration::from_secs(2);
const MAX_CLIENTS: usize = 32;
const MAX_CALLS: usize = 16;
type Pending = HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>;

struct ClientState {
    pending: Mutex<Option<Pending>>,
    user_id: RwLock<Option<String>>,
}

impl ClientState {
    fn disconnect(&self) {
        if let Some(pending) = self.pending.lock().unwrap().take() {
            for (_, sender) in pending {
                let _ = sender.send(Err(
                    "The Maple host disconnected. Reload the task before retrying a message."
                        .into(),
                ));
            }
        }
    }
}

struct PendingCall<'a> {
    state: &'a ClientState,
    id: u64,
}
impl Drop for PendingCall<'_> {
    fn drop(&mut self) {
        if let Some(pending) = self.state.pending.lock().unwrap().as_mut() {
            pending.remove(&self.id);
        }
    }
}

pub struct RemoteClient {
    endpoint: ipc::Endpoint,
    requests: mpsc::Sender<Request>,
    state: Arc<ClientState>,
    next_id: AtomicU64,
    #[cfg_attr(not(feature = "desktop"), allow(dead_code))]
    events: tokio::sync::Mutex<Option<mpsc::Receiver<AgentServiceEvent>>>,
    permits: Semaphore,
}

impl RemoteClient {
    pub async fn connect(api_url: &str) -> Result<Self, String> {
        let endpoint = ipc::Endpoint::new(api_url)?;
        let (connection, welcome) = connect(&endpoint, ConnectionMode::Client, true).await?;
        let state = Arc::new(ClientState {
            pending: Mutex::new(Some(HashMap::new())),
            user_id: RwLock::new(welcome.user_id),
        });
        let (requests, mut request_rx) = mpsc::channel::<Request>(MAX_CALLS);
        let (event_tx, event_rx) = mpsc::channel(256);
        let closed = tokio_util::sync::CancellationToken::new();
        let reader_closed = closed.clone();
        let (mut read, mut write) = tokio::io::split(connection);
        let reader_state = state.clone();
        tokio::spawn(async move {
            loop {
                let message = tokio::select! {
                    _ = reader_closed.cancelled() => break,
                    message = protocol::read::<Message>(&mut read) => match message { Ok(message) => message, Err(_) => break },
                };
                match message {
                    Message::Response { id, result } => {
                        if let Some(sender) = reader_state
                            .pending
                            .lock()
                            .unwrap()
                            .as_mut()
                            .and_then(|pending| pending.remove(&id))
                        {
                            let _ = sender.send(result);
                        }
                    }
                    Message::Event(event) => {
                        if let AgentServiceEvent::AccountChanged(user_id) = &*event {
                            *reader_state.user_id.write().unwrap() = user_id.clone();
                        }
                        if event_tx.try_send(*event).is_err() {
                            break;
                        }
                    }
                }
            }
            reader_closed.cancel();
            reader_state.disconnect();
            // Delivery waits only after closing the socket; a stalled UI never
            // backpressures the host. Reconnect requires a fresh snapshot.
            let _ = event_tx.send(AgentServiceEvent::HostDisconnected).await;
        });
        let writer_state = state.clone();
        tokio::spawn(async move {
            loop {
                let request = tokio::select! {
                    _ = closed.cancelled() => break,
                    request = request_rx.recv() => match request { Some(request) => request, None => break },
                };
                let result = tokio::select! {
                    _ = closed.cancelled() => break,
                    result = protocol::write(&mut write, &request) => result,
                };
                if result.is_err() {
                    break;
                }
            }
            closed.cancel();
            writer_state.disconnect();
        });
        Ok(Self {
            endpoint,
            requests,
            state,
            next_id: AtomicU64::new(1),
            events: tokio::sync::Mutex::new(Some(event_rx)),
            permits: Semaphore::new(MAX_CALLS),
        })
    }

    pub fn user_id(&self) -> Option<String> {
        self.state.user_id.read().unwrap().clone()
    }

    #[cfg_attr(not(feature = "desktop"), allow(dead_code))]
    pub async fn take_events(&self) -> Option<mpsc::Receiver<AgentServiceEvent>> {
        self.events.lock().await.take()
    }

    pub async fn call<R: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<R, String> {
        let params = serde_json::to_value(params)
            .map_err(|_| "Could not encode host request".to_string())?;
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| "Host disconnected".to_string())?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, reply) = oneshot::channel();
        self.state
            .pending
            .lock()
            .unwrap()
            .as_mut()
            .ok_or("The Maple host disconnected")?
            .insert(id, sender);
        let _pending = PendingCall {
            state: &self.state,
            id,
        };
        let send_id = (method == "send_message").then(|| uuid::Uuid::new_v4().to_string());
        let sent = self
            .requests
            .send(Request {
                id,
                method: method.into(),
                params,
                send_id: send_id.clone(),
            })
            .await;
        let result = if sent.is_ok() {
            reply
                .await
                .unwrap_or_else(|_| Err("The Maple host disconnected".into()))
        } else {
            Err("The Maple host connection closed".into())
        };
        let disconnected = self.state.pending.lock().unwrap().is_none();
        let result = match (result, send_id) {
            (Err(_), Some(id)) if disconnected => recover_send(&self.endpoint, &id).await?,
            (result, _) => result?,
        };
        serde_json::from_value(result)
            .map_err(|_| "Incompatible Maple host response; restart Maple".into())
    }
}

async fn handshake(
    mut stream: ipc::Connection,
    endpoint: &ipc::Endpoint,
    mode: ConnectionMode,
) -> Result<(ipc::Connection, Welcome), String> {
    protocol::write(
        &mut stream,
        &Hello {
            version: protocol::VERSION,
            profile: endpoint.profile.clone(),
            build: protocol::BUILD.into(),
            mode,
        },
    )
    .await?;
    let welcome: Result<Welcome, String> = protocol::read(&mut stream).await?;
    let welcome = welcome?;
    if welcome.version != protocol::VERSION || welcome.build != protocol::BUILD {
        return Err(
            "A different Maple host version is running. Quit that host before opening this build."
                .into(),
        );
    }
    Ok((stream, welcome))
}

async fn connect(
    endpoint: &ipc::Endpoint,
    mode: ConnectionMode,
    start: bool,
) -> Result<(ipc::Connection, Welcome), String> {
    if let Ok(stream) = endpoint.connect().await {
        return tokio::time::timeout(START_TIMEOUT, handshake(stream, endpoint, mode))
            .await
            .map_err(|_| "Maple host did not answer".to_string())?;
    }
    if !start {
        return Err("No Maple host is running for this profile".into());
    }
    let startup = endpoint.lock("startup.lock")?;
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        match startup.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            Err(error) => return Err(format!("Cannot acquire Maple host startup lock: {error}")),
        }
    }
    if let Ok(stream) = endpoint.connect().await {
        return tokio::time::timeout_at(deadline, handshake(stream, endpoint, mode))
            .await
            .map_err(|_| "Maple host did not answer".to_string())?;
    }
    spawn_host()?;
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(stream) = endpoint.connect().await {
            let result = tokio::time::timeout_at(deadline, handshake(stream, endpoint, mode))
                .await
                .map_err(|_| "Maple host did not answer".to_string())?;
            drop(startup);
            return result;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("Maple host did not start. See the profile's maple-agent.log.".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn spawn_host() -> Result<(), String> {
    let mut command =
        std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .args(["host", "serve"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Host startup must not choose the first editor's project as everyone's
    // default working directory. Each task carries its own explicit cwd.
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only async-signal-safe setsid between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x00000200 | 0x01000000); // detached, new group, break away from client job
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start Maple host: {e}"))?;
    // Reap on Unix while the parent remains alive; the host is not kill-on-drop.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn run(api_url: String) -> Result<(), String> {
    let endpoint = ipc::Endpoint::new(&api_url)?;
    let ownership = endpoint.lock("host.lock")?;
    ownership
        .try_lock()
        .map_err(|_| "A Maple host already owns this profile".to_string())?;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let backend = Arc::new(LocalBackend::new(
            api_url,
            crate::settings::load_settings().effective_harness_instructions(),
        )?);
        serve(endpoint, backend).await
    })
}

async fn serve(endpoint: ipc::Endpoint, backend: Arc<LocalBackend>) -> Result<(), String> {
    let mut listener = endpoint.bind()?;
    let receipts = Arc::new(receipts::Receipts::open(
        &backend::local::host_data_root().join("send-receipts.db"),
    )?);
    let (events, _) = broadcast::channel::<Arc<AgentServiceEvent>>(256);
    let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
    let mut source = backend
        .take_events()
        .await
        .ok_or("Host event source already taken")?;
    let sink = events.clone();
    let pump = tokio::spawn(async move {
        while let Some(event) = source.recv().await {
            let _ = sink.send(Arc::new(event));
        }
    });
    let mut clients = tokio::task::JoinSet::new();
    let mut idle = tokio::time::Instant::now() + START_TIMEOUT;
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = stop_rx.recv() => break,
            _ = &mut shutdown => break,
            _ = tokio::time::sleep_until(idle), if clients.is_empty() => break,
            result = clients.join_next(), if !clients.is_empty() => {
                if let Some(Err(error)) = result { log::warn!("Host client ended unexpectedly: {error}"); }
                if clients.is_empty() { idle = tokio::time::Instant::now() + IDLE_GRACE; }
            }
            incoming = listener.accept() => {
                let connection = match incoming { Ok(connection) => connection, Err(error) => { log::warn!("Host rejected a connection: {error}"); continue; } };
                if clients.len() >= MAX_CLIENTS { drop(connection); continue; }
                let backend = backend.clone(); let endpoint = endpoint.clone(); let events = events.clone(); let stop = stop_tx.clone(); let receipts = receipts.clone();
                clients.spawn(async move {
                    if let Err(error) = serve_client(connection, endpoint, backend, events, stop, receipts).await {
                        log::debug!("Host connection ended: {error}");
                    }
                });
            }
        }
    }
    // Stop accepting first, then stop/drain the runtime while clients can still
    // receive its final updates. Only this owner shuts down shared children.
    drop(listener);
    backend.shutdown_host().await;
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    pump.abort();
    Ok(())
}

async fn serve_client(
    mut connection: ipc::Connection,
    endpoint: ipc::Endpoint,
    backend: Arc<LocalBackend>,
    events: broadcast::Sender<Arc<AgentServiceEvent>>,
    stop: mpsc::Sender<()>,
    receipts: Arc<receipts::Receipts>,
) -> Result<(), String> {
    let hello = tokio::time::timeout(START_TIMEOUT, protocol::read::<Hello>(&mut connection))
        .await
        .map_err(|_| "Host handshake timed out".to_string())??;
    let mut subscription = events.subscribe();
    if hello.version != protocol::VERSION
        || hello.profile != endpoint.profile
        || hello.build != protocol::BUILD
    {
        protocol::write(
            &mut connection,
            &Err::<Welcome, _>(
                "Incompatible Maple host or profile; explicitly quit the existing host first",
            ),
        )
        .await?;
        return Ok(());
    }
    protocol::write(
        &mut connection,
        &Ok::<_, String>(Welcome {
            version: protocol::VERSION,
            pid: std::process::id(),
            build: protocol::BUILD.into(),
            user_id: backend.saved_user_id(),
        }),
    )
    .await?;
    match hello.mode {
        ConnectionMode::Stop => {
            let _ = stop.send(()).await;
            return std::future::pending().await;
        }
        ConnectionMode::Acp => {
            #[cfg(feature = "acp")]
            {
                let (read, write) = tokio::io::split(connection);
                return backend.serve_acp(read, write).await;
            }
            #[cfg(not(feature = "acp"))]
            return Err("This host was built without ACP".into());
        }
        ConnectionMode::Client => {}
    }
    let _questions = backend.question_responder();
    let (mut input, mut output) = tokio::io::split(connection);
    let (requests, mut request_rx) = mpsc::channel(1);
    // Framed reads cannot be cancelled between the header and body.
    let reader = tokio::spawn(async move {
        while let Ok(request) = protocol::read::<Request>(&mut input).await {
            if requests.send(request).await.is_err() {
                break;
            }
        }
    });
    let _reader = AbortOnDrop(reader.abort_handle());
    let (responses, mut response_rx) = mpsc::channel(MAX_CALLS);
    let mut calls = tokio::task::JoinSet::new();
    let result = async {
    loop {
        tokio::select! {
            request = request_rx.recv(), if calls.len() < MAX_CALLS => {
                let Some(request) = request else { break; };
                let backend = backend.clone(); let responses = responses.clone(); let events = events.clone(); let receipts = receipts.clone();
                calls.spawn(async move {
                    let id = request.id;
                    let before = backend.saved_user_id();
                    let result = dispatch_with_receipt(&backend, request, receipts).await;
                    let after = backend.saved_user_id();
                    if before != after { let _ = events.send(Arc::new(AgentServiceEvent::AccountChanged(after))); }
                    let _ = responses.send(Message::Response { id, result }).await;
                });
            }
            _ = calls.join_next(), if !calls.is_empty() => {},
            response = response_rx.recv() => {
                if let Some(response) = response { write_message(&mut output, &response).await?; }
            }
            event = subscription.recv() => {
                // A lagging frontend reloads persisted state; it cannot stall
                // the host or silently miss an arbitrary portion of the stream.
                let event = event.map_err(|_| "Host event stream fell behind; reconnect and reload".to_string())?;
                write_message(&mut output, &Message::Event(Box::new((*event).clone()))).await?;
            }
        }
    }
    Ok::<(), String>(())
    }.await;
    // Accepted mutations finish even if their response cannot be delivered.
    drop(response_rx);
    while calls.join_next().await.is_some() {}
    result
}

pub fn stop(api_url: &str) -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let endpoint = ipc::Endpoint::new(api_url)?;
        let (mut connection, _) = connect(&endpoint, ConnectionMode::Stop, false).await?;
        use tokio::io::AsyncReadExt;
        let mut byte = [0];
        tokio::time::timeout(Duration::from_secs(45), connection.read(&mut byte))
            .await
            .map_err(|_| "Maple host is still shutting down".to_string())?
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

#[cfg(feature = "acp")]
pub fn acp(api_url: &str) -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let result = runtime.block_on(async {
        let endpoint = ipc::Endpoint::new(api_url)?;
        let (stream, _) = connect(&endpoint, ConnectionMode::Acp, true).await?;
        let (mut read, mut write) = tokio::io::split(stream);
        let mut input = tokio::io::stdin(); let mut output = tokio::io::stdout();
        tokio::select! {
            result = tokio::io::copy(&mut input, &mut write) => result.map(|_| ()).map_err(|e| e.to_string()),
            result = tokio::io::copy(&mut read, &mut output) => result.map(|_| ()).map_err(|e| e.to_string()),
        }
    });
    // Tokio's stdin uses an uncancellable blocking read. If the host closes
    // first (for example on sign-out), runtime Drop would wait for the editor
    // to write to stdin before this adapter could exit.
    runtime.shutdown_background();
    result
}

struct AbortOnDrop(tokio::task::AbortHandle);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn write_message(
    output: &mut (impl tokio::io::AsyncWrite + Unpin),
    message: &Message,
) -> Result<(), String> {
    tokio::time::timeout(START_TIMEOUT, protocol::write(output, message))
        .await
        .map_err(|_| "Host client stopped receiving updates".to_string())?
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn dispatch_with_receipt(
    backend: &LocalBackend,
    request: Request,
    receipts: Arc<receipts::Receipts>,
) -> Result<serde_json::Value, String> {
    if request.method == "send_outcome" {
        let id: String =
            serde_json::from_value(request.params).map_err(|_| "Invalid send identity")?;
        return tokio::task::spawn_blocking(move || {
            receipts
                .outcome(&id)
                .and_then(|result| serde_json::to_value(result).map_err(|e| e.to_string()))
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    if request.method != "send_message" {
        return backend::dispatch(backend, request).await;
    }
    let id = request
        .send_id
        .clone()
        .ok_or("A send requires a stable identity")?;
    let record_id = id.clone();
    let params = request.params.clone();
    let store = receipts.clone();
    match tokio::task::spawn_blocking(move || store.reserve(&record_id, &params))
        .await
        .map_err(|e| e.to_string())??
    {
        Some(receipts::Outcome::Finished(result)) => return result,
        Some(_) => {
            return Err(format!(
                "Send outcome unknown ({id}). Reload this task before sending again."
            ));
        }
        None => {}
    }
    let result = backend::dispatch(backend, request).await;
    let saved = result.clone();
    tokio::task::spawn_blocking(move || receipts.finish(&id, &saved))
        .await
        .map_err(|e| e.to_string())??;
    result
}

async fn recover_send(endpoint: &ipc::Endpoint, id: &str) -> Result<serde_json::Value, String> {
    // Query only. Starting another host or replaying inference is never recovery.
    let recovery = async {
        let (mut connection, _) = connect(endpoint, ConnectionMode::Client, false).await?;
        for _ in 0..20 {
            protocol::write(
                &mut connection,
                &Request {
                    id: 1,
                    send_id: None,
                    method: "send_outcome".into(),
                    params: serde_json::json!(id),
                },
            )
            .await?;
            loop {
                match protocol::read::<Message>(&mut connection).await? {
                    Message::Response { result, .. } => {
                        let outcome: receipts::Outcome =
                            serde_json::from_value(result?).map_err(|_| "Invalid send receipt")?;
                        match outcome {
                            receipts::Outcome::Finished(result) => return result,
                            receipts::Outcome::Missing => return Err(
                                "The host did not admit this message. It is safe to send again."
                                    .into(),
                            ),
                            receipts::Outcome::Pending => break,
                        }
                    }
                    Message::Event(_) => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(format!(
            "Send outcome unknown ({id}). Reload this task before sending again."
        ))
    };
    match tokio::time::timeout(Duration::from_secs(5), recovery).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) if error.starts_with("The host did not admit") => Err(error),
        _ => Err(format!(
            "Send outcome unknown ({id}). Reload this task before sending again; it may already be running."
        )),
    }
}
