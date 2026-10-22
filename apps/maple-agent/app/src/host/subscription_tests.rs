//! Real IPC routing across the existing-task -> new-task transition. Task rows
//! and model events are synthetic; provider execution is tested in the runtime.
//! This fixture exercises the production handshake, request dispatcher, client
//! facade, subscription acknowledgement and host event writer together.
use super::*;
use crate::backend::AgentBackend;
use maple_agent::agent::{
    AgentRunEvent, AgentRunTerminal, AgentSendMessageRequest, AgentSessionSummary,
    AgentTimelineItem,
};
use maple_agent::store::records::{CredentialTokens, SecretString};
use maple_agent::store::{NewTask, OpenTaskPageRequest, PageTransport, TaskKind};
use serde_json::json;

const USER: &str = "subscription-fixture@example.test";

fn seed_task(backend: &LocalBackend, root: &std::path::Path) -> AgentSessionSummary {
    let store = backend.store().unwrap();
    let account = store.account_id(USER).unwrap().unwrap();
    let task = uuid::Uuid::now_v7().to_string();
    store
        .create_task(
            account,
            &NewTask {
                uuid: task.clone(),
                project_root: root.to_string_lossy().into_owned(),
                kind: TaskKind::Desktop,
                title: "New IPC task".into(),
                provisional: false,
                model: None,
                created_ms: 1,
            },
        )
        .unwrap();
    let mut page = store
        .get_task_summaries(account, &[task], PageTransport::Local)
        .unwrap();
    crate::backend::paging::summary_to_session(page.tasks.pop().unwrap())
}

fn item(id: &str) -> AgentTimelineItem {
    serde_json::from_value(json!({
        "id": id, "itemType": "message", "role": "assistant",
        "text": "Synthetic streamed reply", "createdMs": 1, "merge": "upsert"
    }))
    .unwrap()
}

#[tokio::test]
async fn new_task_subscription_ack_routes_rows_and_terminals_over_real_ipc() {
    // Keep Unix socket names below the platform path limit.
    #[cfg(unix)]
    let root = tempfile::Builder::new()
        .prefix("maple-sub-")
        .tempdir_in("/tmp")
        .unwrap();
    #[cfg(windows)]
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalBackend::isolated_for_test(root.path()));
    let store = backend.store().unwrap().clone();
    let account = store.upsert_account(USER, 1).unwrap();
    store
        .replace_credentials(
            None,
            account,
            "http://127.0.0.1:1",
            "subscription-session",
            1,
            &CredentialTokens {
                access_token: SecretString::new("synthetic-token".into()),
                refresh_token: None,
            },
            1,
        )
        .unwrap();
    // Match host startup: publishing durable credentials alone does not install
    // the in-memory account owner used to authorize this connection's requests.
    backend.reload_open_store().unwrap();
    assert_eq!(backend.saved_user_id().as_deref(), Some(USER));
    let receipts = Arc::new(receipts::Receipts::new(store.clone()));
    let (events, _) = broadcast::channel(64);
    let remote =
        crate::remote::manager::Manager::new(backend.clone(), events.clone(), receipts.clone());
    let owner = Arc::new(OwnerContext::new(Some(&store)));
    let endpoint = ipc::Endpoint::for_test(root.path().to_path_buf());
    let mut listener = endpoint.bind().unwrap();
    let (stop, _stop_rx) = mpsc::channel(1);
    let server_endpoint = endpoint.clone();
    let server_backend = backend.clone();
    let server_events = events.clone();
    let server_remote = remote.clone();
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.unwrap();
        serve_client(
            connection,
            server_endpoint,
            server_backend,
            server_events,
            stop,
            receipts,
            server_remote,
            owner,
        )
        .await
    });
    let (connection, welcome) = connect(&endpoint, ConnectionMode::Client, false)
        .await
        .unwrap();
    assert_eq!(welcome.user_id.as_deref(), Some(USER));
    let client = Arc::new(AgentBackend::from_host_client_for_test(
        RemoteClient::attach(endpoint, connection, welcome),
    ));
    let mut received = client.take_events().await.unwrap();

    let mut previous = seed_task(&backend, root.path());
    let first_intent = client.next_subscription_intent();
    client
        .open_task(
            USER,
            OpenTaskPageRequest {
                task: previous.id.clone(),
                limit_rows: Some(50),
                limit_bytes: Some(64 * 1024),
            },
            first_intent,
        )
        .await
        .unwrap();
    assert!(
        client
            .subscribe_task("another-account", &previous.id, first_intent + 1)
            .await
            .unwrap_err()
            .contains("account changed")
    );
    let mut previous_intent = first_intent;
    for terminal in [
        AgentRunTerminal::Completed,
        AgentRunTerminal::Cancelled,
        AgentRunTerminal::Failed,
    ] {
        // The runtime's create operation needs an authenticated model session.
        // Seed its durable result and emit the same public creation event so
        // this transport regression needs neither credentials nor inference.
        let intent = client.next_subscription_intent();
        let current = seed_task(&backend, root.path());
        events
            .send(Arc::new(AgentServiceEvent::SessionCreated(current.clone())))
            .unwrap();
        client
            .subscribe_task(USER, &current.id, intent)
            .await
            .unwrap();
        // An older asynchronous open/create can finish late. Its successful
        // acknowledgement must not replace the newest selected task.
        client
            .subscribe_task(USER, &previous.id, previous_intent)
            .await
            .unwrap();

        // The actual request/receipt pipeline also handles an immediate send
        // failure without dropping the created task or losing its subscription.
        let send_id = uuid::Uuid::new_v4().to_string();
        let error = client
            .send_message_identified(
                USER,
                AgentSendMessageRequest {
                    session_id: current.id.clone(),
                    text: "Synthetic new-task request".into(),
                    model: None,
                    context_limit: None,
                    vision_capable: false,
                    steer: false,
                    queue_id: None,
                    attachments: Vec::new(),
                },
                send_id.clone(),
            )
            .await
            .unwrap_err();
        assert!(error.contains("runtime is not running"), "{error}");
        assert!(matches!(
            client.send_outcome(send_id).await.unwrap(),
            receipts::Outcome::Finished(Err(_))
        ));
        let run = format!("synthetic-{intent}");
        for task in [&previous.id, &current.id] {
            events
                .send(Arc::new(AgentServiceEvent::TimelineItem {
                    session_id: task.clone(),
                    run_id: Some(run.clone()),
                    item: item("standalone-row"),
                }))
                .unwrap();
            events
                .send(Arc::new(AgentServiceEvent::Run {
                    session_id: task.clone(),
                    run_id: run.clone(),
                    event: AgentRunEvent::TimelineItem(item("run-row")),
                }))
                .unwrap();
        }
        events
            .send(Arc::new(AgentServiceEvent::Run {
                session_id: current.id.clone(),
                run_id: run,
                event: AgentRunEvent::Finished(terminal),
            }))
            .unwrap();
        let row_count = tokio::time::timeout(Duration::from_secs(5), async {
            let mut rows = 0;
            while let Some(event) = received.recv().await {
                match event {
                    AgentServiceEvent::TimelineItem { session_id, .. }
                    | AgentServiceEvent::Run {
                        session_id,
                        event: AgentRunEvent::TimelineItem(_),
                        ..
                    } => {
                        assert_eq!(session_id, current.id, "the prior task must stay filtered");
                        rows += 1;
                    }
                    AgentServiceEvent::Run {
                        session_id,
                        event: AgentRunEvent::Finished(outcome),
                        ..
                    } if session_id == current.id => {
                        assert_eq!(outcome, terminal);
                        return rows;
                    }
                    _ => {}
                }
            }
            panic!("IPC event stream ended before the terminal event");
        })
        .await
        .unwrap();
        assert_eq!(
            row_count, 2,
            "both transcript event forms reach the new task"
        );
        let restored = client
            .open_task(
                USER,
                OpenTaskPageRequest {
                    task: current.id.clone(),
                    limit_rows: Some(50),
                    limit_bytes: Some(64 * 1024),
                },
                intent,
            )
            .await
            .unwrap();
        assert_eq!(restored.session.id, current.id);
        previous = current;
        previous_intent = intent;
    }
    client.detach().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    remote.shutdown().await;
}
