//! Diagnostic probe: switch the runtime's project root and verify new
//! sessions adopt it. Credentials from MAPLE_TEST_EMAIL / MAPLE_TEST_PASSWORD.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use maple_agent::agent::{
    AgentCreateSessionRequest, AgentEventSink, AgentPathLayout, AgentServiceEvent,
    AgentStartRequest, AgentToolContextSpec, MapleAgentHostResources, MapleAgentService,
};
use maple_agent::maple_api::{MapleApiAuthRequest, MapleApiAuthState, NoopAuthEventSink};
use opensecret::OpenSecretClient;
use uuid::Uuid;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        eprintln!("set {name}");
        std::process::exit(2);
    })
}

struct DropSink;
impl AgentEventSink for DropSink {
    fn emit(&self, _event: &AgentServiceEvent) {}
}

#[tokio::main]
async fn main() {
    let api_url =
        std::env::var("MAPLE_API_URL").unwrap_or_else(|_| "https://enclave.trymaple.ai".into());
    let api_url = maple_agent::maple_api::validate_api_url(&api_url).expect("url");
    let email = env("MAPLE_TEST_EMAIL");
    let password = env("MAPLE_TEST_PASSWORD");
    let client_id: Uuid = "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6".parse().unwrap();
    let client = OpenSecretClient::new_with_pcr0_environment(api_url.clone(), Default::default())
        .expect("client");
    let response = client
        .login(email, password, client_id)
        .await
        .expect("login");
    let user_id = response.id.to_string();

    let auth = MapleApiAuthState::new();
    auth.set_auth(
        Arc::new(NoopAuthEventSink),
        MapleApiAuthRequest {
            user_id: user_id.clone(),
            api_url,
            access_token: response.access_token,
            refresh_token: Some(response.refresh_token),
        },
    )
    .await
    .expect("set_auth");

    let config_root = std::path::PathBuf::from("/tmp/maple-probe/config");
    let data_root = std::path::PathBuf::from("/tmp/maple-probe/data");
    let paths = AgentPathLayout::from_app_roots(config_root, data_root);
    let tool_context =
        AgentToolContextSpec::try_new(BTreeMap::new(), BTreeSet::new(), false).expect("ctx");
    let service = MapleAgentService::new(MapleAgentHostResources::new(
        paths,
        Arc::new(DropSink),
        tool_context,
    ));
    let handle = service.handle_for_user(&user_id).await.expect("handle");
    let session = auth.session_for(&user_id).await.expect("session");

    let root_a = "/tmp/maple-probe/root-a";
    let root_b = "/tmp/maple-probe/root-b";
    std::fs::create_dir_all(root_a).unwrap();
    std::fs::create_dir_all(root_b).unwrap();

    handle
        .start(
            session.clone(),
            Some(AgentStartRequest {
                project_root: Some(root_a.to_string()),
                model: None,
                mode: None,
            }),
        )
        .await
        .expect("start root-a");

    let created_a = handle
        .create_session(Some(AgentCreateSessionRequest {
            project_root: None,
            title: None,
            model: None,
            context_limit: None,
            mode: None,
            mcp_server_names: None,
        }))
        .await
        .expect("create under root-a");
    println!(
        "session under root-a: working in {}",
        created_a.session.project_root
    );

    handle
        .save_recent_project_root(root_b.to_string())
        .await
        .expect("save root-b");
    let status = handle
        .restart(
            session,
            Some(AgentStartRequest {
                project_root: Some(root_b.to_string()),
                model: None,
                mode: None,
            }),
        )
        .await
        .expect("restart root-b");
    println!("after restart: project_root={:?}", status.project_root);

    let listed = handle.list_sessions(None).await.expect("list");
    println!("sessions visible under root-b: {}", listed.len());
    for s in &listed {
        println!("  {} -> {}", s.id, s.project_root);
    }

    let created_b = handle
        .create_session(Some(AgentCreateSessionRequest {
            project_root: None,
            title: None,
            model: None,
            context_limit: None,
            mode: None,
            mcp_server_names: None,
        }))
        .await
        .expect("create under root-b");
    println!(
        "session under root-b: working in {}",
        created_b.session.project_root
    );

    handle
        .delete_session(created_a.session.id.clone())
        .await
        .expect("cleanup a");
    handle
        .delete_session(created_b.session.id.clone())
        .await
        .expect("cleanup b");
}
