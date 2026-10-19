use super::*;

fn endpoint() -> (tempfile::TempDir, ipc::Endpoint) {
    #[cfg(unix)]
    let root = tempfile::Builder::new()
        .prefix("maple-recovery-")
        .tempdir_in("/tmp")
        .unwrap();
    #[cfg(windows)]
    let root = tempfile::tempdir().unwrap();
    let endpoint = ipc::Endpoint::for_test(root.path().to_path_buf());
    (root, endpoint)
}

async fn accept(
    listener: &mut ipc::Listener,
    endpoint: &ipc::Endpoint,
    user: &str,
) -> ipc::Connection {
    let mut connection = listener.accept().await.unwrap();
    assert_eq!(
        accept_handshake(&mut connection, &endpoint.profile, Some(user.into()))
            .await
            .unwrap(),
        ConnectionMode::Client
    );
    connection
}

async fn client(
    endpoint: &ipc::Endpoint,
    listener: &mut ipc::Listener,
) -> (RemoteClient, ipc::Connection) {
    let (connection, server) = tokio::join!(
        connect(endpoint, ConnectionMode::Client, false),
        accept(listener, endpoint, "first-user")
    );
    let (connection, welcome) = connection.unwrap();
    (
        RemoteClient::attach(endpoint.clone(), connection, welcome),
        server,
    )
}

async fn reconnected(events: &mut mpsc::Receiver<AgentServiceEvent>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = events.recv().await {
            if matches!(event, AgentServiceEvent::HostReconnected) {
                return;
            }
            assert!(!matches!(event, AgentServiceEvent::HostDisconnected));
        }
        panic!("event stream ended before reconnection");
    })
    .await
    .unwrap();
}

async fn response(connection: &mut ipc::Connection, expected: &str, value: serde_json::Value) {
    let request: Request = protocol::read(connection).await.unwrap();
    assert_eq!(
        request.method, expected,
        "an old command must never be replayed"
    );
    protocol::write(
        connection,
        &Message::Response {
            id: request.id,
            result: Ok(value),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn interrupted_calls_fail_then_reconnect_to_the_current_account_without_replay() {
    let (_root, endpoint) = endpoint();
    let mut listener = endpoint.bind().unwrap();
    let (client, mut server) = client(&endpoint, &mut listener).await;
    let mut events = client.take_events().await.unwrap();
    let call = client.call::<()>("test-mutation", ());
    tokio::pin!(call);
    tokio::select! {
        _ = &mut call => panic!("mutation must await its response"),
        request = protocol::read::<Request>(&mut server) => assert_eq!(request.unwrap().method, "test-mutation"),
    }
    drop(server);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), call)
            .await
            .unwrap()
            .is_err()
    );
    let mut server = accept(&mut listener, &endpoint, "second-user").await;
    reconnected(&mut events).await;
    assert_eq!(client.user_id().as_deref(), Some("second-user"));
    let (result, ()) = tokio::join!(
        client.call::<String>("inspect", ()),
        response(&mut server, "inspect", serde_json::json!("fresh"))
    );
    assert_eq!(result.unwrap(), "fresh");
    let ((), ()) = tokio::join!(
        client.detach(),
        response(&mut server, "detach", serde_json::Value::Null)
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn calls_waiting_for_capacity_do_not_cross_a_connection_generation() {
    use std::{future::Future, task::Poll};
    let (_root, endpoint) = endpoint();
    let mut listener = endpoint.bind().unwrap();
    let (client, server) = client(&endpoint, &mut listener).await;
    let mut events = client.take_events().await.unwrap();
    let capacity = client.permits.acquire_many(MAX_CALLS as u32).await.unwrap();
    let queued = client.call::<()>("queued-mutation", ());
    tokio::pin!(queued);
    std::future::poll_fn(|cx| {
        assert!(queued.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(server);
    let (mut server, ()) = tokio::join!(
        accept(&mut listener, &endpoint, "first-user"),
        reconnected(&mut events)
    );
    drop(capacity);
    assert!(
        queued
            .await
            .unwrap_err()
            .contains("before this operation was sent")
    );
    let (result, ()) = tokio::join!(
        client.call::<()>("new-command", ()),
        response(&mut server, "new-command", serde_json::Value::Null)
    );
    result.unwrap();
    let ((), ()) = tokio::join!(
        client.detach(),
        response(&mut server, "detach", serde_json::Value::Null)
    );
}

#[tokio::test]
async fn slow_frontend_recovers_after_a_bounded_event_queue_overflow() {
    let (_root, endpoint) = endpoint();
    let mut listener = endpoint.bind().unwrap();
    let (client, mut server) = client(&endpoint, &mut listener).await;
    let mut events = client.take_events().await.unwrap();
    // Don't consume UI events until the reader has dropped its old socket.
    tokio::time::timeout(Duration::from_secs(5), async {
        for _ in 0..2000 {
            if protocol::write(
                &mut server,
                &Message::Event(Box::new(AgentServiceEvent::QuestionResolved {
                    request_id: "fixture".into(),
                })),
            )
            .await
            .is_err()
            {
                return;
            }
        }
        let _: Result<Request, _> = protocol::read(&mut server).await;
    })
    .await
    .unwrap();
    assert!(events.len() <= 256);
    drop(server);
    let (mut server, ()) = tokio::join!(
        accept(&mut listener, &endpoint, "first-user"),
        reconnected(&mut events)
    );
    let (result, ()) = tokio::join!(
        client.call::<bool>("inspect", ()),
        response(&mut server, "inspect", serde_json::json!(true))
    );
    assert!(result.unwrap());
    let ((), ()) = tokio::join!(
        client.detach(),
        response(&mut server, "detach", serde_json::Value::Null)
    );
}

#[tokio::test]
async fn a_lost_send_response_queries_the_receipt_without_resending() {
    let (_root, endpoint) = endpoint();
    let mut listener = endpoint.bind().unwrap();
    let (client, mut first) = client(&endpoint, &mut listener).await;
    let mut events = client.take_events().await.unwrap();
    let server = tokio::spawn(async move {
        let request: Request = protocol::read(&mut first).await.unwrap();
        assert_eq!(request.method, "send_message");
        let identity = request.send_id.unwrap();
        drop(first);
        // One connection restores the stream, the other queries admission.
        let mut connections = tokio::task::JoinSet::new();
        for _ in 0..2 {
            let mut connection = accept(&mut listener, &endpoint, "first-user").await;
            let identity = identity.clone();
            connections.spawn(async move {
                let request: Request = protocol::read(&mut connection).await.unwrap();
                match request.method.as_str() {
                    "send_outcome" => {
                        assert_eq!(request.params, serde_json::json!(identity));
                        protocol::write(
                            &mut connection,
                            &Message::Response {
                                id: request.id,
                                result: Ok(serde_json::to_value(receipts::Outcome::Finished(Ok(
                                    serde_json::json!("accepted-run"),
                                )))
                                .unwrap()),
                            },
                        )
                        .await
                        .unwrap();
                    }
                    "detach" => protocol::write(
                        &mut connection,
                        &Message::Response {
                            id: request.id,
                            result: Ok(serde_json::Value::Null),
                        },
                    )
                    .await
                    .unwrap(),
                    other => panic!("unexpected replay: {other}"),
                }
            });
        }
        while let Some(result) = connections.join_next().await {
            result.unwrap();
        }
    });
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        client.call::<String>("send_message", ()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result, "accepted-run");
    reconnected(&mut events).await;
    client.detach().await;
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn concurrent_clients_wait_for_the_next_pipe_instance() {
    use std::{future::Future, task::Poll};
    let (_root, endpoint) = endpoint();
    let mut listener = endpoint.bind().unwrap();
    let _first_client = endpoint.connect().await.unwrap();
    // Keep the first connection unaccepted so the only pipe instance is busy.
    // The next open must wait, rather than reporting that the host is absent.
    let waiting = endpoint.connect();
    tokio::pin!(waiting);
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let _first_server = listener.accept().await.unwrap();
    let (second_client, second_server) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(waiting, listener.accept())
    })
    .await
    .unwrap();
    let mut second_client = second_client.unwrap();
    let mut second_server = second_server.unwrap();
    let (sent, received) = tokio::join!(
        protocol::write(&mut second_client, &"second-client"),
        protocol::read::<String>(&mut second_server)
    );
    sent.unwrap();
    assert_eq!(received.unwrap(), "second-client");
}

#[tokio::test]
async fn unexpected_disconnect_keeps_the_host_alive_but_explicit_stop_still_wins() {
    let (root, endpoint) = endpoint();
    let backend = Arc::new(LocalBackend::isolated_for_test(root.path()));
    let receipts = Arc::new(receipts::Receipts::open(&root.path().join("receipts.db")).unwrap());
    let server_endpoint = endpoint.clone();
    let server =
        tokio::spawn(async move { serve_with_receipts(server_endpoint, backend, receipts).await });
    let (connection, _) = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(connection) = connect(&endpoint, ConnectionMode::Client, false).await {
                break connection;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(connection);
    tokio::time::sleep(IDLE_GRACE + Duration::from_millis(100)).await;
    assert!(
        !server.is_finished(),
        "an unexpected loss must not use the normal quit grace"
    );
    let (_stop, _) = connect(&endpoint, ConnectionMode::Stop, false)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn host_loss_exhausts_recovery_without_starting_a_replacement() {
    let (_root, endpoint) = endpoint();
    let mut listener = endpoint.bind().unwrap();
    let (client, server) = client(&endpoint, &mut listener).await;
    let mut events = client.take_events().await.unwrap();
    drop(listener);
    drop(server);
    assert!(matches!(
        events.recv().await,
        Some(AgentServiceEvent::HostReconnecting)
    ));
    assert!(matches!(
        tokio::time::timeout(RECONNECT_TIMEOUT + Duration::from_secs(2), events.recv())
            .await
            .unwrap(),
        Some(AgentServiceEvent::HostDisconnected)
    ));
    assert!(endpoint.connect().await.is_err());
    assert!(client.call::<()>("must-not-run", ()).await.is_err());
}

#[tokio::test]
async fn graceful_detach_keeps_other_clients_running_and_last_quit_shuts_down() {
    let (root, endpoint) = endpoint();
    let backend = Arc::new(LocalBackend::isolated_for_test(root.path()));
    let receipts = Arc::new(receipts::Receipts::open(&root.path().join("receipts.db")).unwrap());
    let server_endpoint = endpoint.clone();
    let server =
        tokio::spawn(async move { serve_with_receipts(server_endpoint, backend, receipts).await });
    let (connection, welcome) = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(connection) = connect(&endpoint, ConnectionMode::Client, false).await {
                break connection;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let first = RemoteClient::attach(endpoint.clone(), connection, welcome);
    let (connection, welcome) = connect(&endpoint, ConnectionMode::Client, false)
        .await
        .unwrap();
    let second = RemoteClient::attach(endpoint.clone(), connection, welcome);
    first.detach().await;
    tokio::time::sleep(IDLE_GRACE + Duration::from_millis(100)).await;
    assert!(!server.is_finished());
    second
        .call::<()>("set_harness_instructions", ("fixture",))
        .await
        .unwrap();
    second.detach().await;
    tokio::time::timeout(IDLE_GRACE + Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(endpoint.connect().await.is_err());
}

#[tokio::test]
async fn remote_protocol_refusal_ends_recovery_without_replaying_or_reconnecting() {
    let (_root, endpoint) = endpoint();
    let mut listener = endpoint.bind().unwrap();
    let (client, mut server) = client(&endpoint, &mut listener).await;
    let mut events = client.take_events().await.unwrap();
    protocol::write(
        &mut server,
        &Message::Closed {
            reason: "Update both computers".into(),
        },
    )
    .await
    .unwrap();
    assert!(
        matches!(tokio::time::timeout(Duration::from_secs(2), events.recv()).await.unwrap(),
        Some(AgentServiceEvent::RemoteAccessEnded(reason)) if reason == "Update both computers")
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .is_none()
    );
    assert!(client.call::<()>("send_message", ()).await.is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}
