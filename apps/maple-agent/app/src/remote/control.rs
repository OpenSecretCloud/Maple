//! Paired control and the A-daemon bridge. No prompt is queued or replayed by
//! this transport; accepted sends and their receipts belong exclusively to B.
use super::{
    manager::{Manager, Node},
    wire,
};
use crate::host::protocol::{self, Message, Request};
use iroh::{
    EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
};
use maple_agent::agent::AgentServiceEvent;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, mpsc};

pub(super) fn listen(node: Arc<Node>) {
    tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = node.stopped.cancelled() => break,
                _ = connections.join_next(), if !connections.is_empty() => {},
                incoming = node.endpoint.accept() => {
                    let Some(incoming) = incoming else { break; };
                    let Ok(admission_slot) = node.admissions.clone().try_acquire_owned() else { incoming.refuse(); continue; };
                    let node = node.clone();
                    connections.spawn(async move {
                        let accepted = tokio::time::timeout(Duration::from_secs(15), async {
                            let connection = incoming.await.map_err(|_| "Remote handshake failed")?;
                            let peer = connection.remote_id();
                            let (mut tx, mut rx) = connection.accept_bi().await.map_err(|_| "Remote handshake closed")?;
                            let hello: wire::Hello = wire::read(&mut rx, wire::MAX_PREFACE).await.map_err(|e| e.to_string())?;
                            let outcome = async {
                            hello.check_version().map_err(|e| e.to_string())?;
                            node.ensure_valid().await?;
                            if hello.account != node.account || hello.project != node.project || hello.deployment != node.deployment || hello.channel != crate::profile::PROFILE {
                                return Err("Remote account or deployment mismatch".to_string());
                            }
                            match hello.mode {
                                wire::Mode::Pair { window, admission } => {
                                    node.accept_pairing(peer, window, admission, &mut tx, &mut rx).await?;
                                    let _ = tx.finish();
                                    let _ = tokio::time::timeout(Duration::from_secs(3), tx.stopped()).await;
                                    Ok(None)
                                }
                                wire::Mode::ConfirmGrant { window } => {
                                    let answer = {
                                        let store = node.store.lock().unwrap();
                                        if store.permits(peer) && store.peers().get(&peer.to_string()).is_some_and(|p| p.grant == window) {
                                            Ok(window)
                                        } else { Err("No current grant matches this pairing".to_string()) }
                                    };
                                    wire::write(&mut tx, &answer, wire::MAX_PREFACE).await.map_err(|e| e.to_string())?;
                                    let _ = tx.finish();
                                    let _ = tokio::time::timeout(Duration::from_secs(3), tx.stopped()).await;
                                    Ok(None)
                                }
                                wire::Mode::Control => {
                                    if !node.store.lock().unwrap().permits(peer) { return Err("Remote control grant is missing or revoked".into()); }
                                    let slot = node.controllers.clone().try_acquire_owned().map_err(|_| "Remote connection limit reached")?;
                                    wire::write(&mut tx, &Ok::<(), String>(()), wire::MAX_PREFACE).await.map_err(|e| e.to_string())?;
                                    Ok(Some(slot))
                                }
                            }
                            }.await;
                            // Return a terminal refusal while the stream is
                            // still alive, so a revoked/incompatible peer does
                            // not retry it as a transient network outage.
                            if let Err(error) = &outcome {
                                let _ = wire::write(&mut tx, &Err::<(), _>(error), wire::MAX_PREFACE).await;
                                let _ = tx.finish();
                                let _ = tokio::time::timeout(Duration::from_secs(2), tx.stopped()).await;
                            }
                            outcome.map(|slot| slot.map(|slot| (connection, peer, tx, rx, slot)))
                        }).await;
                        drop(admission_slot);
                        if let Ok(Err(error)) = &accepted { log::debug!("Remote handshake declined: {error}"); }
                        if accepted.is_err() { log::debug!("Remote handshake timed out"); }
                        if let Ok(Ok(Some((connection, peer, tx, rx, _slot)))) = accepted {
                            serve(node, peer, connection, tx, rx).await;
                        }
                    });
                }
            }
        }
        connections.abort_all();
    });
}

async fn serve(
    node: Arc<Node>,
    peer: EndpointId,
    connection: Connection,
    mut tx: SendStream,
    mut rx: RecvStream,
) {
    let _questions = node.backend.question_responder();
    let mut events = node.events.subscribe();
    let mut revoked = node.revoked.subscribe();
    let (incoming, mut requests) = mpsc::channel(1);
    let reader = tokio::spawn(async move {
        loop {
            let request = wire::read::<wire::Command>(&mut rx, wire::MAX_FRAME).await;
            let terminal = request.is_err();
            if incoming.send(request).await.is_err() || terminal {
                break;
            }
        }
    });
    let (responses, mut output) = mpsc::channel(16);
    // Desktop bootstrap fans out independent history, model and integration reads.
    // Bound that burst without rejecting ordinary startup; urgent task controls
    // retain their own capacity. Remote work never borrows local IPC slots.
    let permits = Arc::new(Semaphore::new(32));
    let control_permits = Arc::new(Semaphore::new(2));
    let mut calls = tokio::task::JoinSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let result: Result<(), String> = async {
        loop {
            tokio::select! {
                _ = node.stopped.cancelled() => break,
                _ = connection.closed() => break,
                _ = tick.tick() => {
                    node.ensure_valid().await?;
                    if !node.store.lock().unwrap().permits(peer) { break; }
                }
                event = revoked.recv() => {
                    if event.is_err() || event.ok().is_some_and(|key| key == peer || key == node.endpoint.id()) { break; }
                }
                _ = calls.join_next(), if !calls.is_empty() => {},
                request = requests.recv() => {
                    let Some(request) = request else { break; };
                    let request = match request {
                        Ok(request) => request,
                        Err(wire::Error::Disconnected) => break,
                        Err(error) => { send(&mut tx, &Message::Closed { reason: error.to_string() }).await?; break; }
                    };
                    node.ensure_valid().await?;
                    if !node.store.lock().unwrap().permits(peer) { break; }
                    let urgent = matches!(request.method, wire::Method::CancelRun | wire::Method::AnswerQuestion | wire::Method::CancelExternalAgent);
                    let slot = if urgent { control_permits.clone() } else { permits.clone() }.try_acquire_owned();
                    let Ok(slot) = slot else {
                        send(&mut tx, &Message::Response { id: request.id, result: Err("Remote computer is busy; try again".into()) }).await?;
                        continue;
                    };
                    let node = node.clone(); let responses = responses.clone();
                    calls.spawn(async move {
                        let _slot = slot;
                        let id = request.id;
                        let result = crate::host::dispatch_remote_with_receipt(&node.backend, Request {
                            id, method: if request.method == wire::Method::TaskBootstrap { "start_runtime".into() } else { request.method.name() },
                            send_id: request.send_id, params: if request.method == wire::Method::TaskBootstrap { serde_json::json!([node.account, null]) } else { request.params }
                        }, node.receipts.clone(), &node.session, &node.account).await;
                        if node.valid().await && node.store.lock().unwrap().permits(peer) {
                            let _ = responses.send(Message::Response { id, result }).await;
                        }
                    });
                }
                Some(response) = output.recv() => send(&mut tx, &response).await?,
                event = events.recv() => {
                    let event = event.map_err(|_| "Remote event stream fell behind")?;
                    node.ensure_valid().await?;
                    if !node.store.lock().unwrap().permits(peer) { break; }
                    // Account transitions are terminal for this connection.
                    // Local lifecycle markers can never control another UI.
                    if matches!(&*event, AgentServiceEvent::AccountChanged(_)) { break; }
                    if remote_event(&event) { send(&mut tx, &Message::Event(Box::new((*event).clone()))).await?; }
                }
            }
        }
        Ok(())
    }.await;
    let _ = tx.finish();
    let _ = tokio::time::timeout(Duration::from_secs(2), tx.stopped()).await;
    if result.is_err() {
        connection.close(1u32.into(), b"remote stream closed");
    }
    reader.abort();
    // As in local IPC, an admitted mutation finishes even if the viewer leaves.
    drop(output);
    while calls.join_next().await.is_some() {}
}

fn remote_event(event: &AgentServiceEvent) -> bool {
    // Exhaustive on purpose: new local events need an explicit remote decision.
    match event {
        AgentServiceEvent::RuntimeStatus(_)
        | AgentServiceEvent::Question { .. }
        | AgentServiceEvent::QuestionResolved { .. }
        | AgentServiceEvent::ThinkingChanged { .. }
        | AgentServiceEvent::SessionCreated(_)
        | AgentServiceEvent::SessionUpdated { .. }
        | AgentServiceEvent::TimelineItem { .. }
        | AgentServiceEvent::Run { .. }
        | AgentServiceEvent::SideQuestion { .. } => true,
        AgentServiceEvent::AccountChanged(_)
        | AgentServiceEvent::RemoteAccessEnded(_)
        | AgentServiceEvent::HostReconnecting
        | AgentServiceEvent::HostReconnected
        | AgentServiceEvent::HostDisconnected => false,
    }
}

async fn send(tx: &mut SendStream, message: &Message) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_secs(10),
        wire::write(tx, message, wire::MAX_FRAME),
    )
    .await
    .map_err(|_| "Remote consumer stopped reading")?
    .map_err(|e| e.to_string())
}

pub struct Link {
    pub node: Arc<Node>,
    pub peer: EndpointId,
    pub connection: Connection,
    pub tx: SendStream,
    pub rx: RecvStream,
}

impl Manager {
    pub async fn connect(self: &Arc<Self>, peer: &str) -> Result<Link, String> {
        let peer: EndpointId = peer
            .parse()
            .map_err(|_| "Invalid remote computer identity")?;
        let node = self.active().await?;
        if !node
            .store
            .lock()
            .unwrap()
            .peers()
            .get(&peer.to_string())
            .is_some_and(|p| p.can_connect)
        {
            return Err("Remote access ended: pair this computer before connecting".into());
        }
        let (address, _) = node.destination(peer).await?;
        let connection = tokio::time::timeout(
            Duration::from_secs(10),
            node.endpoint.connect(address, wire::ALPN),
        )
        .await
        .map_err(|_| "Remote computer is not reachable")?
        .map_err(|_| "Remote computer is not reachable")?;
        if connection.remote_id() != peer {
            return Err("Remote identity changed".into());
        }
        let (mut tx, mut rx) = connection
            .open_bi()
            .await
            .map_err(|_| "Remote connection closed")?;
        wire::write(&mut tx, &node.hello(wire::Mode::Control), wire::MAX_PREFACE)
            .await
            .map_err(|e| e.to_string())?;
        let accepted: Result<(), String> = tokio::time::timeout(
            Duration::from_secs(10),
            wire::read(&mut rx, wire::MAX_PREFACE),
        )
        .await
        .map_err(|_| "Remote handshake timed out")?
        .map_err(|e| e.to_string())?;
        accepted.map_err(|error| format!("Remote access ended: {error}"))?;
        node.ensure_valid().await?;
        Ok(Link {
            node,
            peer,
            connection,
            tx,
            rx,
        })
    }
}

pub async fn bridge(
    connection: crate::host::ipc::Connection,
    mut link: Link,
) -> Result<bool, String> {
    let (mut local_rx, mut local_tx) = tokio::io::split(connection);
    let (frames, mut frame_rx) = mpsc::channel(16);
    let mut revoked = link.node.revoked.subscribe();
    let reader = tokio::spawn(async move {
        loop {
            let message = match wire::read::<Message>(&mut link.rx, wire::MAX_FRAME).await {
                Ok(Message::Event(event)) if !remote_event(&event) => Message::Closed {
                    reason: "Unexpected remote event; update both computers".into(),
                },
                Ok(message) => message,
                Err(wire::Error::Disconnected) => break,
                Err(error) => Message::Closed {
                    reason: error.to_string(),
                },
            };
            let terminal = matches!(message, Message::Closed { .. });
            if frames.send(message).await.is_err() || terminal {
                break;
            }
        }
    });
    let (requests, mut request_rx) = mpsc::channel(1);
    let local_reader = tokio::spawn(async move {
        while let Ok(request) = protocol::read::<Request>(&mut local_rx).await {
            if requests.send(request).await.is_err() {
                break;
            }
        }
    });
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let result = async {
        loop {
            tokio::select! {
                _ = link.node.stopped.cancelled() => return Ok(false),
                _ = link.connection.closed() => return Ok(false),
                _ = tick.tick() => link.node.ensure_valid().await?,
                change = revoked.recv() => {
                    if change.is_err() || change.ok() == Some(link.peer) { return Ok(false); }
                }
                frame = frame_rx.recv() => {
                    let Some(frame) = frame else { return Ok(false); };
                    link.node.ensure_valid().await?;
                    protocol::write(&mut local_tx, &frame).await?;
                }
                request = request_rx.recv() => {
                    let Some(request) = request else { return Ok(true); };
                    if request.method == "detach" {
                        protocol::write(&mut local_tx, &Message::Response { id: request.id, result: Ok(serde_json::Value::Null) }).await?;
                        return Ok(true);
                    }
                    let selected = if request.method == "start_runtime" { Ok(wire::Method::TaskBootstrap) } else { wire::Method::parse(&request.method) };
                    let method = match selected {
                        Ok(method) => method,
                        Err(error) => { protocol::write(&mut local_tx, &Message::Response { id: request.id, result: Err(error) }).await?; continue; }
                    };
                    link.node.ensure_valid().await?;
                    let sent = wire::write(&mut link.tx, &wire::Command { id: request.id, method, send_id: request.send_id, params: request.params }, wire::MAX_FRAME).await;
                    match sent {
                        Ok(()) => {},
                        Err(wire::Error::InvalidFrame) => protocol::write(&mut local_tx, &Message::Response { id: request.id, result: Err("Remote message is too large. Try fewer or smaller images.".into()) }).await?,
                        Err(error) => return Err(error.to_string()),
                    }
                }
            }
        }
    }.await;
    reader.abort();
    local_reader.abort();
    link.connection.close(0u32.into(), b"viewer disconnected");
    result
}
