//! One on-demand process owns a profile's backend. Frontends attach over
//! OS-private IPC; closing a frontend never tears down another client's runtime.

mod ipc;
pub mod protocol;
pub(crate) mod receipts;
#[cfg(test)]
mod recovery_tests;

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
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DISCONNECT_GRACE: Duration = Duration::from_secs(30);
const MAX_CLIENTS: usize = 32;
const MAX_CALLS: usize = 16;
type Pending = HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>;

struct ClientState {
    pending: Mutex<Option<Pending>>,
    user_id: RwLock<Option<String>>,
    generation: AtomicU64,
}

impl ClientState {
    fn disconnect(&self) {
        // Closing these senders distinguishes transport loss from a host's
        // application error, even if reconnection wins the response race.
        let mut pending = self.pending.lock().unwrap();
        pending.take();
        self.generation.fetch_add(1, Ordering::Relaxed);
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
        Ok(Self::attach(endpoint, connection, welcome))
    }

    fn attach(endpoint: ipc::Endpoint, connection: ipc::Connection, welcome: Welcome) -> Self {
        let state = Arc::new(ClientState {
            pending: Mutex::new(Some(HashMap::new())),
            user_id: RwLock::new(welcome.user_id),
            generation: AtomicU64::new(0),
        });
        let (requests, request_rx) = mpsc::channel::<Request>(MAX_CALLS);
        let (event_tx, event_rx) = mpsc::channel(256);
        tokio::spawn(maintain_connection(
            endpoint.clone(),
            connection,
            state.clone(),
            request_rx,
            event_tx,
        ));
        Self {
            endpoint,
            requests,
            state,
            next_id: AtomicU64::new(1),
            events: tokio::sync::Mutex::new(Some(event_rx)),
            permits: Semaphore::new(MAX_CALLS),
        }
    }

    pub async fn detach(&self) {
        // Normal desktop Quit is explicit, unlike a socket failure. Bounded
        // so an unresponsive host cannot prevent the window from closing.
        let _ = tokio::time::timeout(Duration::from_secs(2), self.call::<()>("detach", ())).await;
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
        let generation = self.state.generation.load(Ordering::Relaxed);
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| "Host disconnected".to_string())?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, reply) = oneshot::channel();
        {
            let mut pending = self.state.pending.lock().unwrap();
            if generation != self.state.generation.load(Ordering::Relaxed) {
                return Err("The Maple host disconnected before this operation was sent.".into());
            }
            pending
                .as_mut()
                .ok_or("The Maple host disconnected")?
                .insert(id, sender);
        }
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
        if sent.is_err() {
            self.state.disconnect();
        }
        let result = match (sent, reply.await) {
            (Ok(()), Ok(result)) => result?,
            (_, _) => match send_id {
                Some(id) => recover_send(&self.endpoint, &id).await?,
                None => {
                    return Err("The Maple host disconnected. Reconnect before retrying.".into());
                }
            },
        };
        serde_json::from_value(result)
            .map_err(|_| "Incompatible Maple host response; restart Maple".into())
    }
}

/// Own the socket independently of the UI. Reconnection never starts a host or
/// replays a request: pending callers fail (sends query their admission receipt).
async fn maintain_connection(
    endpoint: ipc::Endpoint,
    mut connection: ipc::Connection,
    state: Arc<ClientState>,
    mut requests: mpsc::Receiver<Request>,
    events: mpsc::Sender<AgentServiceEvent>,
) {
    loop {
        let graceful = client_connection(connection, &state, &mut requests, &events).await;
        state.disconnect();
        if graceful || requests.is_closed() || events.is_closed() {
            return;
        }
        let recovery = async {
            events
                .send(AgentServiceEvent::HostReconnecting)
                .await
                .map_err(|_| ())?;
            loop {
                // No startup arbitration or spawn here: explicit host stop and
                // crashes must never be undone by a frontend reconnect loop.
                if let Ok((next, welcome)) = connect(&endpoint, ConnectionMode::Client, false).await
                {
                    *state.user_id.write().unwrap() = welcome.user_id.clone();
                    events
                        .send(AgentServiceEvent::AccountChanged(welcome.user_id))
                        .await
                        .map_err(|_| ())?;
                    // Publish readiness before the marker can trigger snapshot
                    // calls. The socket already subscribes to host events.
                    *state.pending.lock().unwrap() = Some(HashMap::new());
                    events
                        .send(AgentServiceEvent::HostReconnected)
                        .await
                        .map_err(|_| ())?;
                    return Ok::<_, ()>(next);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        match tokio::time::timeout(RECONNECT_TIMEOUT, recovery).await {
            Ok(Ok(next)) => connection = next,
            _ => {
                state.disconnect();
                let _ = events.send(AgentServiceEvent::HostDisconnected).await;
                return;
            }
        }
    }
}

/// Reads are never cancelled midway through a frame. All queues are bounded;
/// an overflow drops the socket, then the supervisor requests a fresh snapshot.
async fn client_connection(
    connection: ipc::Connection,
    state: &ClientState,
    requests: &mut mpsc::Receiver<Request>,
    events: &mpsc::Sender<AgentServiceEvent>,
) -> bool {
    let (mut read, mut write) = tokio::io::split(connection);
    let (messages, mut incoming) = mpsc::channel(256);
    let reader = tokio::spawn(async move {
        while let Ok(message) = protocol::read::<Message>(&mut read).await {
            if messages.send(message).await.is_err() {
                break;
            }
        }
    });
    let _reader = AbortOnDrop(reader.abort_handle());
    loop {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else {
                    let _ = tokio::time::timeout(Duration::from_secs(2), protocol::write(&mut write, &Request {
                        id: 0, send_id: None, method: "detach".into(), params: serde_json::Value::Null,
                    })).await;
                    return true;
                };
                // A request queued before a disconnect lost its caller. It
                // must not become a mutation on the replacement connection.
                if !state.pending.lock().unwrap().as_ref().is_some_and(|pending| pending.contains_key(&request.id)) {
                    continue;
                }
                if !matches!(tokio::time::timeout(START_TIMEOUT, protocol::write(&mut write, &request)).await, Ok(Ok(()))) {
                    return false;
                }
                if request.method == "detach" {
                    // No reconnection after an intentional detach, even if
                    // the acknowledgement is lost or the host is unresponsive.
                    let _ = tokio::time::timeout(Duration::from_secs(2), async {
                    while let Some(message) = incoming.recv().await {
                        if let Message::Response { id, result } = message {
                            if let Some(sender) = state.pending.lock().unwrap().as_mut().and_then(|pending| pending.remove(&id)) {
                                let _ = sender.send(result);
                            }
                            if id == request.id { break; }
                        }
                    }
                    }).await;
                    return true;
                }
            }
            message = incoming.recv() => match message {
                Some(Message::Response { id, result }) => {
                    if let Some(sender) = state.pending.lock().unwrap().as_mut().and_then(|pending| pending.remove(&id)) {
                        let _ = sender.send(result);
                    }
                }
                Some(Message::Event(event)) => {
                    if let AgentServiceEvent::AccountChanged(user_id) = &*event {
                        *state.user_id.write().unwrap() = user_id.clone();
                    }
                    if events.try_send(*event).is_err() { return false; }
                }
                None => return false,
            },
        }
    }
}

async fn handshake(
    mut stream: ipc::Connection,
    profile: &str,
    mode: ConnectionMode,
) -> Result<(ipc::Connection, Welcome), String> {
    protocol::write(
        &mut stream,
        &Hello {
            version: protocol::VERSION,
            profile: profile.into(),
            build: protocol::BUILD.into(),
            mode,
        },
    )
    .await?;
    let welcome: Result<Welcome, String> = protocol::read(&mut stream).await?;
    let welcome = welcome?;
    if mode != ConnectionMode::Stop
        && (welcome.version != protocol::VERSION || welcome.build != protocol::BUILD)
    {
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
        return tokio::time::timeout(START_TIMEOUT, handshake(stream, &endpoint.profile, mode))
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
        return tokio::time::timeout_at(deadline, handshake(stream, &endpoint.profile, mode))
            .await
            .map_err(|_| "Maple host did not answer".to_string())?;
    }
    spawn_host()?;
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(stream) = endpoint.connect().await {
            let result =
                tokio::time::timeout_at(deadline, handshake(stream, &endpoint.profile, mode))
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
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async {
        // The previous owner removes its listener before draining runs and
        // children. A replacement waits for that drain instead of exiting on
        // the first locked attempt. Never open shared state before ownership.
        wait_for_ownership(&ownership, START_TIMEOUT).await?;
        let backend = Arc::new(LocalBackend::new(
            api_url,
            crate::settings::load_settings().effective_harness_instructions(),
        )?);
        serve(endpoint, backend).await
    })
}

async fn wait_for_ownership(file: &std::fs::File, timeout: Duration) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err("The previous Maple host is still running or shutting down. Try again after it exits.".into());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(format!("Cannot acquire Maple host ownership: {error}")),
        }
    }
}

async fn serve(endpoint: ipc::Endpoint, backend: Arc<LocalBackend>) -> Result<(), String> {
    let receipts = Arc::new(receipts::Receipts::open(
        &backend::local::host_data_root().join("send-receipts.db"),
    )?);
    serve_with_receipts(endpoint, backend, receipts).await
}

async fn serve_with_receipts(
    endpoint: ipc::Endpoint,
    backend: Arc<LocalBackend>,
    receipts: Arc<receipts::Receipts>,
) -> Result<(), String> {
    let mut listener = endpoint.bind()?;
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
    let mut recovery_until = tokio::time::Instant::now();
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = stop_rx.recv() => break,
            _ = &mut shutdown => break,
            _ = tokio::time::sleep_until(idle), if clients.is_empty() => break,
            result = clients.join_next(), if !clients.is_empty() => {
                if matches!(result, Some(Ok(Ok(false))) | Some(Err(_))) {
                    recovery_until = tokio::time::Instant::now() + DISCONNECT_GRACE;
                }
                if clients.is_empty() {
                    idle = (tokio::time::Instant::now() + IDLE_GRACE).max(recovery_until);
                }
            }
            incoming = listener.accept() => {
                let connection = match incoming { Ok(connection) => connection, Err(error) => { log::warn!("Host rejected a connection: {error}"); continue; } };
                if clients.len() >= MAX_CLIENTS { drop(connection); continue; }
                let backend = backend.clone(); let endpoint = endpoint.clone(); let events = events.clone(); let stop = stop_tx.clone(); let receipts = receipts.clone();
                clients.spawn(async move {
                    serve_client(connection, endpoint, backend, events, stop, receipts).await
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
) -> Result<bool, String> {
    let mut subscription = events.subscribe();
    let mode =
        accept_handshake(&mut connection, &endpoint.profile, backend.saved_user_id()).await?;
    match mode {
        ConnectionMode::Stop => {
            let _ = stop.send(()).await;
            return std::future::pending().await;
        }
        ConnectionMode::Acp => {
            #[cfg(feature = "acp")]
            {
                let (read, write) = tokio::io::split(connection);
                return backend.serve_acp(read, write).await.map(|_| true);
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
    let result: Result<bool, String> = async {
    loop {
        tokio::select! {
            request = request_rx.recv(), if calls.len() < MAX_CALLS => {
                let Some(request) = request else { return Ok(false); };
                if request.method == "detach" {
                    write_message(&mut output, &Message::Response { id: request.id, result: Ok(serde_json::Value::Null) }).await?;
                    return Ok(true);
                }
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
    }.await;
    // Accepted mutations finish even if their response cannot be delivered.
    drop(response_rx);
    while calls.join_next().await.is_some() {}
    if let Err(error) = &result {
        log::debug!("Host connection ended: {error}");
    }
    Ok(result.unwrap_or(false))
}

async fn accept_handshake(
    connection: &mut ipc::Connection,
    profile: &str,
    user_id: Option<String>,
) -> Result<ConnectionMode, String> {
    let hello = tokio::time::timeout(START_TIMEOUT, protocol::read::<Hello>(connection))
        .await
        .map_err(|_| "Host handshake timed out".to_string())??;
    // Stop is an explicit, minimal control operation on the OS-private storage
    // endpoint. It must remain usable when the new build cannot join the old
    // runtime. It grants no access to account data or ordinary host commands.
    let error = if hello.mode != ConnectionMode::Stop
        && (hello.version != protocol::VERSION
            || hello.profile != profile
            || hello.build != protocol::BUILD)
    {
        Some(
            "Incompatible Maple host or profile. Close the existing host or run `maple-agent host stop` for this profile, then reopen Maple.",
        )
    } else if hello.mode == ConnectionMode::Acp && user_id.is_none() {
        Some(
            "Maple ACP requires sign-in. Sign in through Maple Desktop or `maple-agent login` first.",
        )
    } else {
        None
    };
    if let Some(error) = error {
        protocol::write(connection, &Err::<Welcome, _>(error)).await?;
        return Err(error.into());
    }
    protocol::write(
        connection,
        &Ok::<_, String>(Welcome {
            version: protocol::VERSION,
            pid: std::process::id(),
            build: protocol::BUILD.into(),
            user_id,
        }),
    )
    .await?;
    Ok(hello.mode)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn incompatible_clients_cannot_join_but_can_explicitly_stop() {
        for mode in [
            ConnectionMode::Client,
            ConnectionMode::Acp,
            ConnectionMode::Stop,
        ] {
            for mismatch in ["profile", "build", "version"] {
                let (mut client, server) = tokio::io::duplex(4096);
                let mut server: ipc::Connection = Box::new(server);
                let mut hello = Hello {
                    version: protocol::VERSION,
                    profile: "same-storage-profile".into(),
                    build: protocol::BUILD.into(),
                    mode,
                };
                match mismatch {
                    "profile" => hello.profile = "different-backend".into(),
                    "build" => hello.build = "old-build".into(),
                    _ => hello.version += 1,
                }
                let (reply, accepted) = tokio::join!(
                    async {
                        protocol::write(&mut client, &hello).await.unwrap();
                        protocol::read::<Result<Welcome, String>>(&mut client)
                            .await
                            .unwrap()
                    },
                    accept_handshake(
                        &mut server,
                        "same-storage-profile",
                        Some("test-user".into())
                    )
                );
                assert_eq!(reply.is_ok(), mode == ConnectionMode::Stop, "{mismatch}");
                assert_eq!(
                    accepted.ok(),
                    (mode == ConnectionMode::Stop).then_some(mode)
                );
            }
        }
    }

    #[tokio::test]
    async fn stop_client_accepts_an_older_hosts_welcome() {
        for mode in [ConnectionMode::Client, ConnectionMode::Stop] {
            let (client, mut server) = tokio::io::duplex(4096);
            let (result, ()) = tokio::join!(handshake(Box::new(client), "profile", mode), async {
                let _: Hello = protocol::read(&mut server).await.unwrap();
                protocol::write(
                    &mut server,
                    &Ok::<_, String>(Welcome {
                        version: protocol::VERSION + 1,
                        pid: 1,
                        build: "old-build".into(),
                        user_id: None,
                    }),
                )
                .await
                .unwrap();
            });
            assert_eq!(result.is_ok(), mode == ConnectionMode::Stop);
        }
    }

    #[tokio::test]
    async fn signed_out_acp_gets_an_error_while_desktop_can_open_login() {
        for mode in [ConnectionMode::Acp, ConnectionMode::Client] {
            let (client, server) = tokio::io::duplex(4096);
            let mut server: ipc::Connection = Box::new(server);
            let (result, _) = tokio::join!(
                handshake(Box::new(client), "profile", mode),
                accept_handshake(&mut server, "profile", None)
            );
            match (mode, result) {
                (ConnectionMode::Acp, Err(error)) => assert!(error.contains("requires sign-in")),
                (ConnectionMode::Client, Ok((_, welcome))) => assert!(welcome.user_id.is_none()),
                _ => panic!("unexpected signed-out handshake result"),
            }
        }
    }

    #[tokio::test]
    async fn replacement_waits_for_shutdown_to_release_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("host.lock");
        let old = std::fs::File::create(&path).unwrap();
        old.lock().unwrap();
        let replacement = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let waiting = wait_for_ownership(&replacement, Duration::from_secs(5));
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut waiting)
                .await
                .is_err()
        );
        drop(old);
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap();
        let contender = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        assert!(
            wait_for_ownership(&contender, Duration::from_millis(50))
                .await
                .is_err()
        );
    }
}
