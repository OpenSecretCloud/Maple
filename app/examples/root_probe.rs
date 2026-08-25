//! Diagnostic probe: log in through the SDK, start the runtime, and print
//! the status fields the app consumes. Credentials come from
//! MAPLE_TEST_EMAIL / MAPLE_TEST_PASSWORD; only non-secret fields print.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use maple_agent::agent::{
    AgentEventSink, AgentPathLayout, AgentServiceEvent, AgentStartRequest, AgentToolContextSpec,
    MapleAgentHostResources, MapleAgentService,
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

    let config_root = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/maple-probe/config"));
    let data_root = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/maple-probe/data"));
    let paths = AgentPathLayout::from_app_roots(config_root, data_root);
    let tool_context = AgentToolContextSpec::try_new(BTreeMap::new(), BTreeSet::new(), false)
        .expect("tool context");
    let service = MapleAgentService::new(MapleAgentHostResources::new(
        paths,
        Arc::new(DropSink),
        tool_context,
    ));

    let request = AgentStartRequest {
        project_root: Some(
            std::env::current_dir()
                .expect("cwd")
                .to_string_lossy()
                .to_string(),
        ),
        model: None,
        mode: None,
    };
    println!("request root: {:?}", request.project_root);

    let handle = service.handle_for_user(&user_id).await.expect("handle");
    let session = auth.session_for(&user_id).await.expect("session");
    let status = handle.start(session, Some(request)).await.expect("start");
    println!(
        "status: running={} project_root={:?} model={:?}",
        status.running, status.project_root, status.model
    );

    let roots = handle.list_recent_project_roots().await.expect("roots");
    for root in roots {
        println!("recent root: {}", root.path);
    }
}
