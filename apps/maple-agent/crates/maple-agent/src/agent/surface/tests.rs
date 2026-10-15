//! A calling surface holding tasks: its leases, its runs and its
//! provisional tasks, on the scripted runtime.

use std::time::Duration;

use pi_ai::faux::{FauxProvider, faux_tool_call};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::agent::tests::{Harness, finished};
use crate::agent::{
    AgentMcpKeyValue, AgentMcpTransport, AgentRunTerminal, AgentSendMessageRequest,
    AgentServiceEvent,
};

fn new_task(harness: &Harness) -> AgentCreateSessionRequest {
    AgentCreateSessionRequest {
        project_root: Some(harness.project.path().to_string_lossy().into_owned()),
        title: None,
        model: Some("glm-5-3".to_string()),
        context_limit: None,
        mcp_server_names: None,
    }
}

fn prompt(session_id: &str, text: &str) -> AgentSendMessageRequest {
    AgentSendMessageRequest {
        session_id: session_id.to_string(),
        text: text.to_string(),
        model: Some("glm-5-3".to_string()),
        context_limit: None,
        vision_capable: false,
        steer: false,
        queue_id: None,
        attachments: Vec::new(),
    }
}

async fn listed_ids(harness: &Harness) -> Vec<String> {
    harness
        .handle
        .list_sessions(None)
        .await
        .unwrap()
        .into_iter()
        .map(|session| session.id)
        .collect()
}

#[tokio::test]
async fn a_surface_task_is_unlisted_until_prompted_and_discarded_untouched() {
    let harness = Harness::new().await;
    let created = harness
        .handle
        .create_surface_session(new_task(&harness), Vec::new())
        .await
        .unwrap();
    let untouched = created.detail.session.id.clone();
    assert!(created.detail.session.acp);
    assert_eq!(created.detail.session.title, "Maple ACP");
    assert!(listed_ids(&harness).await.is_empty());
    created.lease.discard_created_if_untouched().await;
    let store = harness.handle.store().unwrap();
    assert!(store.get(&untouched).unwrap().is_none());

    // Prompted, a task stays, under its prompt's title.
    let created = harness
        .handle
        .create_surface_session(new_task(&harness), Vec::new())
        .await
        .unwrap();
    let prompted = created.detail.session.id.clone();
    harness.faux.push_text("Hello from Maple.");
    let mut run = harness
        .handle
        .send_surface_message(
            &created.lease.access(),
            prompt(&prompted, "Say hello"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(finished(&mut run).await, AgentRunTerminal::Completed);
    assert_eq!(listed_ids(&harness).await, std::slice::from_ref(&prompted));
    created.lease.discard_created_if_untouched().await;
    let row = store.get(&prompted).unwrap().unwrap();
    assert_eq!(row.title, "Say hello");
    assert_eq!(row.message_count, 2);
}

#[tokio::test]
async fn a_surface_task_shares_host_tools_and_events() {
    let harness = Harness::new().await;
    let created = harness
        .handle
        .create_surface_session(new_task(&harness), Vec::new())
        .await
        .unwrap();
    let task = created.detail.session.id.clone();
    harness.faux.push_text("Done.");
    let mut run = harness
        .handle
        .send_surface_message(
            &created.lease.access(),
            prompt(&task, "Hello"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(finished(&mut run).await, AgentRunTerminal::Completed);

    let requests = harness.faux.requests();
    let declared: Vec<String> = pi_ai::transcript::current_tools(&requests[0].context.messages)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert!(declared.contains(&"bash".to_string()), "{declared:?}");
    for name in ["todo_write", "request_user_input"] {
        assert!(declared.contains(&name.to_string()), "{declared:?}");
    }
    assert!(harness.recorder.events().iter().any(
        |event| matches!(event, AgentServiceEvent::Run { run_id, .. } if *run_id == run.run_id)
    ));
    created.lease.release().await;
}

#[tokio::test]
async fn desktop_and_two_surfaces_share_one_task() {
    let harness = Harness::new().await;
    let task = harness.create_task().await;
    let attached = harness
        .handle
        .attach_surface_session(&task, Vec::new())
        .await
        .unwrap();
    assert_eq!(attached.detail.session.id, task);
    let access = attached.lease.access();
    let second = harness
        .handle
        .attach_surface_session(&task, Vec::new())
        .await
        .unwrap();
    harness.faux.push_text("From the shared host.");
    let mut desktop = harness
        .handle
        .send_message(prompt(&task, "From the desktop"))
        .await
        .unwrap();
    assert_eq!(finished(&mut desktop).await, AgentRunTerminal::Completed);
    second.lease.release().await;

    let runtime = harness.runtime().await;
    harness.faux.push_text("Loaded.");
    let mut run = harness
        .handle
        .send_surface_message(&access, prompt(&task, "Hi"), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(finished(&mut run).await, AgentRunTerminal::Completed);
    assert!(runtime.loaded_session(&task).await.is_some());

    // Released, the context is gone with the loaded session, and the task
    // is the desktop's again.
    attached.lease.release().await;
    assert!(!runtime.surface_holds(&access));
    assert!(runtime.loaded_session(&task).await.is_some());
    let gone = harness
        .handle
        .send_surface_message(&access, prompt(&task, "Again"), CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(gone, AGENT_SURFACE_INACTIVE_ERROR);
    harness.faux.push_text("Back home.");
    let mut run = harness
        .handle
        .send_message(prompt(&task, "From the desktop"))
        .await
        .unwrap();
    assert_eq!(finished(&mut run).await, AgentRunTerminal::Completed);
    let declared: Vec<String> =
        pi_ai::transcript::current_tools(&harness.faux.requests().pop().unwrap().context.messages)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
    assert!(declared.contains(&"todo_write".to_string()), "{declared:?}");
}

#[tokio::test]
async fn detaching_does_not_stop_a_run_but_explicit_cancel_does() {
    let harness =
        Harness::with_faux(FauxProvider::new().with_chunk_delay(Duration::from_millis(10))).await;
    let created = harness
        .handle
        .create_surface_session(new_task(&harness), Vec::new())
        .await
        .unwrap();
    let task = created.detail.session.id.clone();
    harness.faux.push_hang();
    let cancellation = CancellationToken::new();
    let mut run = harness
        .handle
        .send_surface_message(
            &created.lease.access(),
            prompt(&task, "Wait forever"),
            cancellation.clone(),
        )
        .await
        .unwrap();
    crate::agent::tests::eventually(|| !harness.faux.requests().is_empty()).await;
    cancellation.cancel();
    assert!(run.terminal.borrow().is_none());
    harness.handle.cancel_session(&task).await.unwrap();
    assert_eq!(finished(&mut run).await, AgentRunTerminal::Cancelled);
    // The next prompt runs once the stopped one has ended.
    harness.faux.push_text("Ready.");
    let mut run = harness
        .handle
        .send_surface_message(
            &created.lease.access(),
            prompt(&task, "Now answer"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(finished(&mut run).await, AgentRunTerminal::Completed);
    created.lease.release().await;
}

#[tokio::test]
async fn a_surface_brings_its_own_mcp_servers() {
    let harness = Harness::new().await;
    let fake = crate::agent::mcp::fake_server::FakeServer::start(None).await;
    let server = AgentMcpServer {
        name: "Client".to_string(),
        description: "ACP session MCP server".to_string(),
        enabled: true,
        timeout_seconds: 30,
        transport: AgentMcpTransport::StreamableHttp {
            url: fake.url.clone(),
            environment: Vec::new(),
            headers: vec![AgentMcpKeyValue {
                key: "Authorization".to_string(),
                value: "Bearer session".to_string(),
            }],
        },
    };
    let created = harness
        .handle
        .create_surface_session(new_task(&harness), vec![server])
        .await
        .unwrap();
    let task = created.detail.session.id.clone();
    harness.faux.push_message(vec![faux_tool_call(
        "mcp__client__echo",
        json!({"text": "through the client"}),
    )]);
    harness.faux.push_text("Echoed.");
    let mut run = harness
        .handle
        .send_surface_message(
            &created.lease.access(),
            prompt(&task, "Echo"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(finished(&mut run).await, AgentRunTerminal::Completed);
    assert_eq!(
        fake.calls(),
        [("echo".to_string(), json!({"text": "through the client"}))]
    );
    assert!(
        fake.authorization()
            .iter()
            .all(|value| value.as_deref() == Some("Bearer session"))
    );
    created.lease.release().await;
}

#[test]
fn a_surface_server_cannot_replace_a_saved_binding() {
    let server = |name: &str, url: &str| AgentMcpServer {
        name: name.to_string(),
        description: String::new(),
        enabled: true,
        timeout_seconds: 30,
        transport: AgentMcpTransport::StreamableHttp {
            url: url.to_string(),
            environment: Vec::new(),
            headers: Vec::new(),
        },
    };
    let servers = with_surface_servers(
        vec![
            server("GitHub", "https://saved.example/github"),
            server("Docs", "https://saved.example/docs"),
        ],
        vec![server("github", "https://client.example/github")],
    );
    assert!(servers.unwrap_err().contains("conflicts"));
    let distinct = with_surface_servers(
        vec![server("Docs", "https://saved.example/docs")],
        vec![server("Client", "https://client.example/tool")],
    )
    .unwrap();
    assert_eq!(distinct.len(), 2);
}

#[tokio::test]
async fn a_start_removes_provisional_tasks_a_crash_stranded() {
    let harness = Harness::new().await;
    let store = harness.handle.store().unwrap();
    let now = pi_ai::now_ms();
    let row = |id: &str, age_ms: i64, provisional: bool| {
        let mut row = TaskRow::new(
            id.to_string(),
            "Maple ACP".to_string(),
            harness.project.path().to_string_lossy().into_owned(),
            TaskKind::Acp,
            None,
            now - age_ms,
        );
        if provisional {
            set_provisional(&mut row);
        }
        row
    };
    store
        .insert(&row("stranded", 11 * 60 * 1000, true))
        .unwrap();
    store.insert(&row("starting", 60 * 1000, true)).unwrap();
    store
        .insert(&row("prompted", 11 * 60 * 1000, false))
        .unwrap();
    sweep_stale_provisional(&store, harness.handle.paths(), &harness.handle.user_id);
    assert!(store.get("stranded").unwrap().is_none());
    assert!(store.get("starting").unwrap().is_some());
    assert!(store.get("prompted").unwrap().is_some());
}
