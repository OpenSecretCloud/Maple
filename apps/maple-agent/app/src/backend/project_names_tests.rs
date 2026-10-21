//! Exercise the shared owner through the real local framing and Iroh request
//! streams. Network authentication/pairing has its own tests and live probes;
//! these fixtures use a saved synthetic account and loopback-only endpoints.
use super::*;
use crate::host::{ipc, protocol};
use crate::remote::{streams, wire};
use iroh::{Endpoint, RelayMode, endpoint::presets};
use serde_json::{Value, json};
use std::time::Duration;

const USER: &str = "project-name-owner";

fn owner(root: &std::path::Path) -> LocalBackend {
    let backend = LocalBackend::isolated_for_test(root);
    backend
        .persisted_auth
        .claim_and_persist(&MapleApiAuthSnapshot {
            user_id: USER.into(),
            access_token: "fixture-token".into(),
            refresh_token: None,
            native_instance_id: "fixture-native".into(),
            session_id: "fixture-session".into(),
            revision: 1,
        });
    backend
}

fn request(method: &str, params: Value) -> protocol::Request {
    protocol::Request {
        id: 1,
        method: method.into(),
        send_id: None,
        params,
    }
}

fn project<'a>(roots: &'a [RecentProjectRoot], path: &str) -> &'a RecentProjectRoot {
    roots.iter().find(|root| root.path == path).unwrap()
}

async fn project_event(
    events: &mut mpsc::UnboundedReceiver<AgentServiceEvent>,
    path: &str,
    name: &str,
) -> AgentServiceEvent {
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = events.recv().await {
            if let AgentServiceEvent::ProjectsChanged { user_id, roots } = &event {
                assert_eq!(user_id, USER);
                if roots
                    .iter()
                    .any(|root| root.path == path && root.name == name)
                {
                    return event;
                }
            }
        }
        panic!("project change stream closed");
    })
    .await
    .expect("owner rename did not publish project metadata")
}

#[tokio::test]
async fn project_rename_ipc_preserves_account_boundary_and_recovery_metadata() {
    let storage = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let backend = owner(storage.path());
    let registration = backend
        .select_project_root(USER, folder.path().to_string_lossy().into_owned())
        .await
        .unwrap();
    let path = registration.project_root;
    let default_name = project(&registration.roots, &path).name.clone();
    let mut events = backend.take_events().await.unwrap();

    // Unix socket paths need a short temp root; Windows uses an isolated pipe.
    #[cfg(unix)]
    let sockets = tempfile::Builder::new()
        .prefix("maple-names-")
        .tempdir_in("/tmp")
        .unwrap();
    #[cfg(windows)]
    let sockets = tempfile::tempdir().unwrap();
    let endpoint = ipc::Endpoint::for_test(sockets.path().to_path_buf());
    let mut listener = endpoint.bind().unwrap();
    let (client, server) = tokio::join!(endpoint.connect(), listener.accept());
    let mut client = client.unwrap();
    let mut server = server.unwrap();
    let command = request("rename_project_root", json!([USER, path, "Owner project"]));
    let send = async {
        protocol::write(&mut client, &command).await.unwrap();
        protocol::read::<protocol::Message>(&mut client)
            .await
            .unwrap()
    };
    let serve = async {
        let command: protocol::Request = protocol::read(&mut server).await.unwrap();
        let id = command.id;
        let result = crate::backend::dispatch(&backend, command).await;
        protocol::write(&mut server, &protocol::Message::Response { id, result })
            .await
            .unwrap();
    };
    let (message, ()) =
        tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(send, serve) })
            .await
            .unwrap();
    let protocol::Message::Response {
        result: Ok(value), ..
    } = message
    else {
        panic!("rename failed");
    };
    let roots: Vec<RecentProjectRoot> = serde_json::from_value(value).unwrap();
    let renamed = project(&roots, &path);
    assert_eq!(renamed.name, "Owner project");
    assert!(renamed.name_revision > 0);
    project_event(&mut events, &path, "Owner project").await;

    for method in [
        "rename_project_root",
        "recent_project_roots",
        "recovery_snapshot",
    ] {
        let params = match method {
            "rename_project_root" => json!(["another-account", path, "Wrong owner"]),
            "recovery_snapshot" => json!(["another-account", null]),
            _ => json!(["another-account"]),
        };
        assert!(
            crate::backend::dispatch(&backend, request(method, params))
                .await
                .unwrap_err()
                .contains("account changed")
        );
    }
    let snapshot = backend.recovery_snapshot(USER, None).await.unwrap();
    assert_eq!(project(&snapshot.bootstrap.recent_roots, &path), renamed);
    assert_eq!(
        snapshot.bootstrap.project_root.as_deref(),
        Some(path.as_str())
    );
    assert!(snapshot.bootstrap.sessions.is_empty());

    let cleared = backend
        .rename_project_root(USER, path.clone(), default_name.clone())
        .await
        .unwrap();
    let cleared = project(&cleared, &path);
    assert_eq!(cleared.name, default_name);
    assert!(cleared.name_revision > renamed.name_revision);
    let reopened = owner(storage.path());
    assert_eq!(
        project(
            &reopened.local_bootstrap(USER).await.unwrap().recent_roots,
            &path
        ),
        cleared
    );
    assert!(
        folder.path().is_dir(),
        "a label rename must not move the folder"
    );
}

async fn endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .alpns(vec![wire::ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .clear_address_lookup()
        .clear_ip_transports()
        .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .transport_config(streams::transport_config())
        .bind()
        .await
        .unwrap()
}

async fn remote_call(
    backend: &LocalBackend,
    controller: &iroh::endpoint::Connection,
    execution: &iroh::endpoint::Connection,
    method: wire::Method,
    params: Value,
) -> Result<Value, String> {
    let send = streams::call(
        controller,
        wire::Command {
            id: 1,
            method,
            send_id: None,
            params,
        },
    );
    let serve = async {
        let (mut tx, mut rx) = execution.accept_bi().await.unwrap();
        let header = streams::Header::read(&mut rx).await.unwrap();
        let capacity = streams::Capacity::new();
        let _slot = capacity.acquire(header.method).unwrap();
        let command = header.body(&mut rx).await.unwrap();
        assert_eq!(command.method, method);
        let result =
            crate::backend::dispatch(backend, request(&command.method.name(), command.params))
                .await;
        streams::reply(
            &mut tx,
            &protocol::Message::Response {
                id: command.id,
                result,
            },
        )
        .await
        .unwrap();
    };
    let (response, ()) =
        tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(send, serve) })
            .await
            .unwrap();
    match response.unwrap() {
        protocol::Message::Response { result, .. } => result,
        _ => panic!("expected a per-request response"),
    }
}

#[tokio::test]
async fn project_names_cross_iroh_in_both_directions_and_recover_from_snapshot() {
    let storage = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let backend = owner(storage.path());
    let path = backend
        .select_project_root(USER, folder.path().to_string_lossy().into_owned())
        .await
        .unwrap()
        .project_root;
    let mut events = backend.take_events().await.unwrap();
    let a = endpoint().await;
    let b = endpoint().await;
    let (controller, execution) = tokio::join!(
        async { a.connect(b.addr(), wire::ALPN).await.unwrap() },
        async { b.accept().await.unwrap().await.unwrap() },
    );
    let exercise = async {
        let response = remote_call(
            &backend,
            &controller,
            &execution,
            wire::Method::RenameProjectRoot,
            json!([USER, path, "From controller"]),
        )
        .await
        .unwrap();
        let roots: Vec<RecentProjectRoot> = serde_json::from_value(response).unwrap();
        assert_eq!(project(&roots, &path).name, "From controller");
        assert_eq!(
            project(&backend.recent_project_roots(USER).await.unwrap(), &path).name,
            "From controller"
        );
        project_event(&mut events, &path, "From controller").await;

        backend
            .rename_project_root(USER, path.clone(), "From owner desktop".into())
            .await
            .unwrap();
        let event = project_event(&mut events, &path, "From owner desktop").await;
        let (mut tx, _rx) = execution.open_bi().await.unwrap();
        let message = protocol::Message::Event(Box::new(event));
        let sent = async {
            wire::write(&mut tx, &message, wire::MAX_FRAME)
                .await
                .unwrap();
        };
        let received = async {
            let (_tx, mut rx) = controller.accept_bi().await.unwrap();
            wire::read::<protocol::Message>(&mut rx, wire::MAX_FRAME)
                .await
                .unwrap()
        };
        let ((), received) = tokio::join!(sent, received);
        assert!(
            matches!(received, protocol::Message::Event(event) if matches!(*event, AgentServiceEvent::ProjectsChanged { ref user_id, ref roots } if user_id == USER && project(roots, &path).name == "From owner desktop"))
        );

        let denied = remote_call(
            &backend,
            &controller,
            &execution,
            wire::Method::RenameProjectRoot,
            json!(["another-account", path, "Cross-account"]),
        )
        .await
        .unwrap_err();
        assert!(denied.contains("account changed"));
        controller.close(0u32.into(), b"snapshot recovery");
        let (controller, execution) = tokio::join!(
            async { a.connect(b.addr(), wire::ALPN).await.unwrap() },
            async { b.accept().await.unwrap().await.unwrap() },
        );
        let recovered = remote_call(
            &backend,
            &controller,
            &execution,
            wire::Method::RecoverySnapshot,
            json!([USER, null]),
        )
        .await
        .unwrap();
        let snapshot: RecoverySnapshot = serde_json::from_value(recovered).unwrap();
        assert_eq!(
            project(&snapshot.bootstrap.recent_roots, &path).name,
            "From owner desktop"
        );
        assert!(snapshot.bootstrap.sessions.is_empty());
        controller.close(0u32.into(), b"done");
    };
    tokio::time::timeout(Duration::from_secs(30), exercise)
        .await
        .unwrap();
    a.close().await;
    b.close().await;
}
