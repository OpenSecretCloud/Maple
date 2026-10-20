//! Paired control and the A-daemon bridge. No prompt is queued or replayed by
//! this transport; accepted sends and their receipts belong exclusively to B.
use super::{
    manager::{Manager, Node},
    streams, wire,
};
use crate::host::protocol::{self, Message, Request};
use iroh::{
    EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
};
use maple_agent::agent::AgentServiceEvent;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

async fn read_control_ack(reader: &mut (impl tokio::io::AsyncRead + Unpin)) -> Result<(), String> {
    let accepted: Result<(), wire::Refusal> = tokio::time::timeout(
        Duration::from_secs(10),
        wire::read(reader, wire::MAX_PREFACE),
    )
    .await
    .map_err(|_| "Remote handshake timed out")?
    .map_err(|error| match error {
        wire::Error::InvalidFrame | wire::Error::Incompatible => {
            format!("Remote access ended: {error}")
        }
        _ => error.to_string(),
    })?;
    accepted.map_err(wire::Refusal::into_error)
}

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
                            let outcome = async {
                            let hello = wire::read_hello(&mut rx).await?;
                            node.ensure_valid().await?;
                            if hello.account != node.account || hello.project != node.project || hello.deployment != node.deployment || hello.channel != crate::profile::PROFILE {
                                return Err(wire::Refusal::terminal("Remote account or deployment mismatch"));
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
                                        if store.peers().get(&peer.to_string()).is_some_and(|p| p.controls_this_computer) && store.peers().get(&peer.to_string()).is_some_and(|p| p.grant == window) {
                                            Ok(window)
                                        } else { Err(wire::Refusal::terminal("No current grant matches this pairing")) }
                                    };
                                    wire::write(&mut tx, &answer, wire::MAX_PREFACE).await.map_err(|e| e.to_string())?;
                                    let _ = tx.finish();
                                    let _ = tokio::time::timeout(Duration::from_secs(3), tx.stopped()).await;
                                    Ok(None)
                                }
                                wire::Mode::Control => {
                                    if !node.store.lock().unwrap().permits(peer) { return Err(wire::Refusal::terminal("Remote control grant is missing or revoked")); }
                                    let slot = node.controllers.clone().try_acquire_owned().map_err(|_| "Remote connection limit reached")?;
                                    wire::write(&mut tx, &Ok::<(), wire::Refusal>(()), wire::MAX_PREFACE).await.map_err(|e| e.to_string())?;
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
    _rx: RecvStream,
) {
    let _questions = node.backend.question_responder();
    let mut events = node.events.subscribe();
    let mut revoked = node.revoked.subscribe();
    let capacity = Arc::new(streams::Capacity::new());
    let mut calls = tokio::task::JoinSet::new();
    // A slow event consumer cannot hold the request accept/dispatch loop.
    let event_node = node.clone();
    let _ = tx.set_priority(1);
    let mut event_writer = tokio::task::JoinSet::new();
    event_writer.spawn(async move {
        loop {
            let event = events
                .recv()
                .await
                .map_err(|_| "Remote event stream fell behind")?;
            event_node.ensure_valid().await?;
            if !event_node.store.lock().unwrap().permits(peer)
                || matches!(&*event, AgentServiceEvent::AccountChanged(_))
            {
                return Ok::<(), String>(());
            }
            if remote_event(&event) {
                send(&mut tx, &Message::Event(Box::new((*event).clone()))).await?;
            }
        }
    });
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = node.stopped.cancelled() => break,
            _ = connection.closed() => break,
            _ = event_writer.join_next() => break,
            _ = tick.tick() => {
                if !node.valid().await || !node.store.lock().unwrap().permits(peer) { break; }
            }
            event = revoked.recv() => {
                if event.is_err() || event.ok().is_some_and(|key| key == peer || key == node.endpoint.id()) { break; }
            }
            _ = calls.join_next(), if !calls.is_empty() => {},
            incoming = connection.accept_bi() => {
                let Ok((mut tx, mut rx)) = incoming else { break; };
                if calls.len() >= streams::MAX_REQUESTS {
                    let _ = tx.reset(1u32.into());
                    let _ = rx.stop(1u32.into());
                    continue;
                }
                let node = node.clone();
                let capacity = capacity.clone();
                calls.spawn(async move {
                    if let Err(error) = serve_request(node, peer, tx, rx, capacity).await {
                        log::debug!("Remote request stream ended: {error}");
                    }
                });
            }
        }
    }
    event_writer.abort_all();
    connection.close(0u32.into(), b"remote stream closed");
    // Closing streams stops incomplete transfers. An already admitted mutation
    // still finishes on B, as with local IPC; A never resends it.
    while calls.join_next().await.is_some() {}
}

async fn serve_request(
    node: Arc<Node>,
    peer: EndpointId,
    mut tx: SendStream,
    mut rx: RecvStream,
    capacity: Arc<streams::Capacity>,
) -> Result<(), String> {
    let header = match streams::Header::read(&mut rx).await {
        Ok(header) => header,
        Err(reason) => return Err(reason),
    };
    tx.set_priority(if header.method.urgent() { 10 } else { 0 })
        .map_err(|_| "Remote reply closed")?;
    let _slot = match capacity.acquire(header.method) {
        Ok(slot) => slot,
        Err(error) => {
            return streams::reply(
                &mut tx,
                &Message::Response {
                    id: header.id,
                    result: Err(error),
                },
            )
            .await;
        }
    };
    let id = header.id;
    let method = header.method;
    let request = match header.body(&mut rx).await {
        Ok(request) => request,
        Err(reason) => {
            return streams::reply(
                &mut tx,
                &Message::Response {
                    id,
                    result: Err(reason),
                },
            )
            .await;
        }
    };
    // Recheck after the potentially slow upload, before any backend effect.
    node.ensure_valid().await?;
    if !node.store.lock().unwrap().permits(peer) {
        return Err("Remote access revoked".into());
    }
    let id = request.id;
    let result = crate::host::dispatch_remote_with_receipt(
        &node.backend,
        Request {
            id,
            method: if request.method == wire::Method::TaskBootstrap {
                "start_runtime".into()
            } else {
                request.method.name()
            },
            send_id: request.send_id,
            params: if request.method == wire::Method::TaskBootstrap {
                serde_json::json!([node.account, null])
            } else {
                request.params
            },
        },
        node.receipts.clone(),
        &node.session,
        &node.account,
    )
    .await;
    if node.valid().await && node.store.lock().unwrap().permits(peer) {
        let result = streams::encode_result(method, result);
        streams::reply(&mut tx, &Message::Response { id, result }).await?;
    }
    Ok(())
}

pub(super) fn remote_event(event: &AgentServiceEvent) -> bool {
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
        read_control_ack(&mut rx).await?;
        node.ensure_valid().await?;
        tx.finish().map_err(|_| "Remote handshake closed")?;
        Ok(Link {
            node,
            peer,
            connection,
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
    let mut readers = tokio::task::JoinSet::new();
    readers.spawn(async move {
        loop {
            let message = match wire::read::<Message>(&mut link.rx, wire::MAX_FRAME).await {
                Ok(Message::Event(event)) if !remote_event(&event) => Message::Closed {
                    reason: "Unexpected remote event; update both computers".into(),
                },
                Ok(message @ (Message::Event(_) | Message::Closed { .. })) => message,
                Ok(Message::Response { .. }) => Message::Closed {
                    reason: "Unexpected reply on the remote event stream".into(),
                },
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
    readers.spawn(async move {
        while let Ok(request) = protocol::read::<Request>(&mut local_rx).await {
            if requests.send(request).await.is_err() {
                break;
            }
        }
    });
    let capacity = streams::Capacity::new();
    let mut calls = tokio::task::JoinSet::new();
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
                Some(completed) = calls.join_next(), if !calls.is_empty() => {
                    let frame = completed.map_err(|_| "Remote request interrupted")?;
                    link.node.ensure_valid().await?;
                    let terminal = matches!(frame, Message::Closed { .. });
                    local_send(&mut local_tx, &frame).await?;
                    if terminal { return Ok(true); }
                }
                frame = frame_rx.recv() => {
                    let Some(frame) = frame else { return Ok(false); };
                    link.node.ensure_valid().await?;
                    let terminal = matches!(frame, Message::Closed { .. });
                    local_send(&mut local_tx, &frame).await?;
                    if terminal { return Ok(true); }
                }
                request = request_rx.recv() => {
                    let Some(request) = request else { return Ok(true); };
                    if request.method == "detach" {
                        local_send(&mut local_tx, &Message::Response { id: request.id, result: Ok(serde_json::Value::Null) }).await?;
                        return Ok(true);
                    }
                    let selected = if request.method == "start_runtime" { Ok(wire::Method::TaskBootstrap) } else { wire::Method::parse(&request.method) };
                    let method = match selected {
                        Ok(method) => method,
                        Err(error) => { local_send(&mut local_tx, &Message::Response { id: request.id, result: Err(error) }).await?; continue; }
                    };
                    link.node.ensure_valid().await?;
                    let slot = match capacity.acquire(method) {
                        Ok(slot) => slot,
                        Err(error) => {
                            local_send(&mut local_tx, &Message::Response { id: request.id, result: Err(error) }).await?;
                            continue;
                        }
                    };
                    let connection = link.connection.clone();
                    calls.spawn(async move {
                        let _slot = slot;
                        let id = request.id;
                        let send_id = request.send_id.clone();
                        match streams::call(&connection, wire::Command { id, method, send_id: request.send_id, params: request.params }).await {
                            Ok(frame) => streams::decode_result(method, frame),
                            Err(error) => Message::Response { id, result: Err(match send_id {
                                Some(id) => format!("Send outcome unknown ({id}). {error}. Checking the host receipt is safe; resending is not."),
                                None => error,
                            }) },
                        }
                    });
                }
            }
        }
    }.await;
    readers.abort_all();
    link.connection.close(0u32.into(), b"viewer disconnected");
    result
}

async fn local_send(
    tx: &mut (impl tokio::io::AsyncWrite + Unpin),
    message: &Message,
) -> Result<(), String> {
    tokio::time::timeout(streams::IO_TIMEOUT, protocol::write(tx, message))
        .await
        .map_err(|_| "Local remote viewer stopped reading")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn control_handshake_retries_disconnect_but_not_invalid_frames_or_revocation() {
        let (mut tx, mut rx) = tokio::io::duplex(1024);
        tx.write_u32(32).await.unwrap();
        tx.write_all(b"{\"Ok\":").await.unwrap();
        drop(tx);
        assert_eq!(
            read_control_ack(&mut rx).await.unwrap_err(),
            "Remote computer disconnected"
        );
        let (mut tx, mut rx) = tokio::io::duplex(1024);
        wire::write(
            &mut tx,
            &serde_json::json!({"unexpected": true}),
            wire::MAX_PREFACE,
        )
        .await
        .unwrap();
        assert!(
            read_control_ack(&mut rx)
                .await
                .unwrap_err()
                .starts_with("Remote access ended:")
        );
        let (mut tx, mut rx) = tokio::io::duplex(1024);
        wire::write(
            &mut tx,
            &Err::<(), _>(wire::Refusal::terminal("Revoked")),
            wire::MAX_PREFACE,
        )
        .await
        .unwrap();
        assert_eq!(
            read_control_ack(&mut rx).await.unwrap_err(),
            "Remote access ended: Revoked"
        );
        wire::write(&mut tx, &Ok::<(), wire::Refusal>(()), wire::MAX_PREFACE)
            .await
            .unwrap();
        assert!(read_control_ack(&mut rx).await.is_ok());
    }
}
