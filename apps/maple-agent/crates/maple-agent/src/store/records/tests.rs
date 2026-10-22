use super::*;
use crate::store::{NewTask, StorePaths, TaskKind};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Store, AccountId, AccountId, String) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(StorePaths::new(directory.path())).unwrap();
    let alice = store.upsert_account("Alice", 1).unwrap();
    let bob = store.upsert_account("Bob", 1).unwrap();
    let task = uuid::Uuid::new_v4().to_string();
    store
        .create_task(
            alice,
            &NewTask {
                uuid: task.clone(),
                project_root: directory.path().to_string_lossy().into_owned(),
                kind: TaskKind::Desktop,
                title: "Synthetic task".into(),
                provisional: false,
                model: None,
                created_ms: 1,
            },
        )
        .unwrap();
    (directory, store, alice, bob, task)
}

fn tokens(label: &str) -> CredentialTokens {
    CredentialTokens {
        access_token: SecretString::new(format!("synthetic-access-{label}")),
        refresh_token: Some(SecretString::new(format!("synthetic-refresh-{label}"))),
    }
}

fn namespace(profile: &str) -> RemoteNamespace {
    RemoteNamespace {
        api_url: "http://127.0.0.1:1234".into(),
        client_id: "synthetic-client".into(),
        profile: profile.into(),
    }
}

fn mcp(value: McpSecretUpdate) -> McpServerEdit {
    McpServerEdit {
        key: "example".into(),
        name: "Example".into(),
        description: "Synthetic server".into(),
        enabled: true,
        timeout_seconds: 30,
        target: McpTarget::Stdio {
            command: "synthetic-mcp --serve".into(),
        },
        environment: vec![McpSecretEdit {
            name: "TOKEN".into(),
            update: value,
        }],
        headers: vec![],
    }
}

#[test]
fn credential_rotation_and_clear_bind_account_session_and_revision() {
    let (_directory, store, alice, bob, _task) = fixture();
    let first = store
        .replace_credentials(
            None,
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            0,
            &tokens("one"),
            1,
        )
        .unwrap();
    let rotated = store.rotate_credentials(&first, &tokens("two"), 2).unwrap();
    assert_eq!(rotated.revision, 1);
    assert_eq!(
        store.rotate_credentials(&first, &tokens("stale"), 3),
        Err(StoreError::Conflict)
    );
    assert!(!store.clear_credentials(&first).unwrap());
    assert_eq!(store.credentials("http://other.invalid").unwrap(), None);
    assert_eq!(
        store
            .credentials("http://127.0.0.1:1234")
            .unwrap()
            .unwrap()
            .tokens,
        tokens("two")
    );
    assert!(store.clear_credentials(&rotated).unwrap());
    assert_eq!(
        store.rotate_credentials(&rotated, &tokens("resurrect"), 4),
        Err(StoreError::Conflict)
    );
    let next = store
        .replace_credentials(
            None,
            bob,
            "http://127.0.0.1:1234",
            "session-b",
            0,
            &tokens("bob"),
            5,
        )
        .unwrap();
    assert!(!store.clear_credentials(&rotated).unwrap());
    assert_eq!(
        store.replace_credentials(
            Some(&rotated),
            alice,
            "http://127.0.0.1:1234",
            "session-c",
            2,
            &tokens("stale-login"),
            6
        ),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store
            .credentials("http://127.0.0.1:1234")
            .unwrap()
            .unwrap()
            .owner,
        next
    );
}

#[test]
fn sign_in_observes_filtered_slot_owner_without_exposing_foreign_origin_tokens() {
    let (_directory, store, alice, bob, _task) = fixture();
    let owner = store
        .replace_credentials(
            None,
            alice,
            "https://old.example",
            "session-a",
            3,
            &tokens("alice"),
            1,
        )
        .unwrap();
    assert_eq!(store.credentials("https://new.example").unwrap(), None);
    assert_eq!(
        store.credential_owner_for_sign_in().unwrap(),
        Some(owner.clone())
    );
    assert_eq!(
        store.replace_credentials(
            None,
            bob,
            "https://new.example",
            "session-b",
            1,
            &tokens("bob"),
            2
        ),
        Err(StoreError::Conflict)
    );
    let replacement = store
        .replace_credentials(
            Some(&owner),
            bob,
            "https://new.example",
            "session-b",
            1,
            &tokens("bob"),
            2,
        )
        .unwrap();
    assert_eq!(store.credentials("https://old.example").unwrap(), None);
    assert_eq!(
        store
            .credentials("https://new.example")
            .unwrap()
            .unwrap()
            .tokens,
        tokens("bob")
    );
    assert_eq!(
        store.rotate_credentials(&owner, &tokens("late-alice"), 3),
        Err(StoreError::Conflict)
    );
    assert!(!store.clear_credentials(&owner).unwrap());
    assert_eq!(
        store.credential_owner_for_sign_in().unwrap(),
        Some(replacement)
    );
}

#[test]
fn pending_deletion_reserves_physical_slot_even_for_another_origin_and_exact_owner() {
    let (_directory, store, alice, bob, _task) = fixture();
    let owner = store
        .replace_credentials(
            None,
            alice,
            "https://old.example",
            "session-a",
            3,
            &tokens("alice"),
            1,
        )
        .unwrap();
    let generation = uuid::Uuid::new_v4().to_string();
    store.begin_account_deletion(alice, &generation, 2).unwrap();
    assert_eq!(
        store.credential_owner_for_sign_in(),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.replace_credentials(
            Some(&owner),
            bob,
            "https://new.example",
            "session-b",
            1,
            &tokens("bob"),
            3
        ),
        Err(StoreError::Conflict)
    );
    let retained = store
        .pending_deletion_credentials(alice, &generation, "https://old.example")
        .unwrap()
        .unwrap();
    assert_eq!(retained.owner, owner);
    assert_eq!(retained.tokens, tokens("alice"));
    store.abort_account_deletion(alice, &generation).unwrap();
    assert_eq!(store.credential_owner_for_sign_in().unwrap(), Some(owner));
}

#[test]
fn confirmed_deletion_releases_only_target_credentials_before_history_collection() {
    let (_directory, store, alice, bob, task) = fixture();
    let owner = store
        .replace_credentials(
            None,
            alice,
            "https://api.example",
            "session-a",
            3,
            &tokens("alice"),
            1,
        )
        .unwrap();
    let generation = uuid::Uuid::new_v4().to_string();
    store.begin_account_deletion(alice, &generation, 2).unwrap();
    store.confirm_account_deletion(alice, &generation).unwrap();
    assert_eq!(store.collect_account_step(alice), Err(StoreError::Busy));
    assert_eq!(store.credential_owner_for_sign_in().unwrap(), None);
    let replacement = store
        .replace_credentials(
            None,
            bob,
            "https://api.example",
            "session-b",
            1,
            &tokens("bob"),
            3,
        )
        .unwrap();
    store.confirm_account_deletion(alice, &generation).unwrap();
    assert_eq!(
        store.credential_owner_for_sign_in().unwrap(),
        Some(replacement.clone())
    );
    assert_eq!(
        store.rotate_credentials(&owner, &tokens("late-alice"), 4),
        Err(StoreError::Conflict)
    );
    assert!(store.collect_task_step(alice, &task).unwrap().done);
    let mut collected = false;
    for _ in 0..32 {
        if store.collect_account_step(alice).unwrap().done {
            collected = true;
            break;
        }
    }
    assert!(collected);
    assert_eq!(
        store.credential_owner_for_sign_in().unwrap(),
        Some(replacement)
    );
    assert_eq!(
        store
            .credentials("https://api.example")
            .unwrap()
            .unwrap()
            .tokens,
        tokens("bob")
    );
}

#[test]
fn deletion_confirmation_credential_failure_rolls_back_phase_and_task_tombstones() {
    let (_directory, store, alice, _bob, task) = fixture();
    let owner = store
        .replace_credentials(
            None,
            alice,
            "https://api.example",
            "session-a",
            3,
            &tokens("alice"),
            1,
        )
        .unwrap();
    let generation = uuid::Uuid::new_v4().to_string();
    store.begin_account_deletion(alice, &generation, 2).unwrap();
    store
        .writer
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER refuse_credential_delete BEFORE DELETE ON credentials \
         BEGIN SELECT RAISE(ABORT, 'synthetic credential delete failure'); END;",
        )
        .unwrap();
    assert!(store.confirm_account_deletion(alice, &generation).is_err());
    assert!(matches!(
        store.account_deletion(alice).unwrap(),
        Some(crate::store::AccountDeletion::Pending { .. })
    ));
    assert!(store.task(alice, &task).unwrap().is_some());
    assert_eq!(
        store
            .pending_deletion_credentials(alice, &generation, "https://api.example")
            .unwrap()
            .unwrap()
            .owner,
        owner
    );
    store
        .writer
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_credential_delete;")
        .unwrap();
    store.confirm_account_deletion(alice, &generation).unwrap();
    assert_eq!(store.credential_owner_for_sign_in().unwrap(), None);
    assert!(store.task(alice, &task).unwrap().is_none());
}

#[test]
fn same_revision_claim_is_idempotent_but_cannot_replace_token_values() {
    let (_directory, store, alice, _bob, _task) = fixture();
    let owner = store
        .replace_credentials(
            None,
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            7,
            &tokens("one"),
            1,
        )
        .unwrap();
    assert_eq!(
        store
            .replace_credentials(
                Some(&owner),
                alice,
                "http://127.0.0.1:1234",
                "session-a",
                7,
                &tokens("one"),
                2
            )
            .unwrap(),
        owner
    );
    assert_eq!(
        store.replace_credentials(
            Some(&owner),
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            7,
            &tokens("different"),
            3
        ),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.replace_credentials(
            Some(&owner),
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            6,
            &tokens("older"),
            4
        ),
        Err(StoreError::Conflict)
    );
    let saved = store.credentials("http://127.0.0.1:1234").unwrap().unwrap();
    assert_eq!(saved.tokens, tokens("one"));
    assert!(!format!("{saved:?} {:?}", saved.tokens).contains("synthetic-access"));
}

#[test]
fn absent_refresh_token_round_trips_without_weakening_access_validation() {
    let (_directory, store, alice, _bob, _task) = fixture();
    let without_refresh = CredentialTokens {
        access_token: SecretString::new("synthetic-access-only".into()),
        refresh_token: None,
    };
    let owner = store
        .replace_credentials(
            None,
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            0,
            &without_refresh,
            1,
        )
        .unwrap();
    assert_eq!(
        store
            .credentials("http://127.0.0.1:1234")
            .unwrap()
            .unwrap()
            .tokens,
        without_refresh
    );
    let empty_access = CredentialTokens {
        access_token: SecretString::new(String::new()),
        refresh_token: None,
    };
    assert_eq!(
        store.rotate_credentials(&owner, &empty_access, 2),
        Err(StoreError::InvalidInput)
    );
    let empty_refresh = CredentialTokens {
        access_token: SecretString::new("synthetic-access".into()),
        refresh_token: Some(SecretString::new(String::new())),
    };
    assert_eq!(
        store.rotate_credentials(&owner, &empty_refresh, 2),
        Err(StoreError::InvalidInput)
    );
}

#[test]
fn pending_deletion_credentials_are_only_available_to_exact_operation_reconciliation() {
    let (_directory, store, alice, bob, _task) = fixture();
    store
        .replace_credentials(
            None,
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            0,
            &tokens("pending"),
            1,
        )
        .unwrap();
    let generation = uuid::Uuid::new_v4().to_string();
    store.begin_account_deletion(alice, &generation, 2).unwrap();
    assert_eq!(store.credentials("http://127.0.0.1:1234").unwrap(), None);
    assert_eq!(
        store
            .pending_account_deletion("http://other.invalid")
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .pending_account_deletion("http://127.0.0.1:1234")
            .unwrap(),
        Some((alice, "Alice".into(), generation.clone(), 2))
    );
    let saved = store
        .pending_deletion_credentials(alice, &generation, "http://127.0.0.1:1234")
        .unwrap()
        .unwrap();
    assert_eq!(saved.owner.account, alice);
    assert_eq!(saved.tokens, tokens("pending"));
    assert_eq!(
        store.pending_deletion_credentials(bob, &generation, "http://127.0.0.1:1234"),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.pending_deletion_credentials(
            alice,
            &uuid::Uuid::new_v4().to_string(),
            "http://127.0.0.1:1234"
        ),
        Err(StoreError::Conflict)
    );
    store.confirm_account_deletion(alice, &generation).unwrap();
    assert_eq!(
        store
            .pending_account_deletion("http://127.0.0.1:1234")
            .unwrap(),
        None
    );
    assert_eq!(
        store.pending_deletion_credentials(alice, &generation, "http://127.0.0.1:1234"),
        Err(StoreError::Conflict)
    );
}

#[test]
fn pending_deletion_refresh_is_fenced_by_generation_owner_and_api() {
    let (_directory, store, alice, bob, _task) = fixture();
    let api = "http://127.0.0.1:1234";
    let owner = store
        .replace_credentials(
            None,
            alice,
            api,
            "session-original",
            2,
            &tokens("original"),
            1,
        )
        .unwrap();
    let generation = uuid::Uuid::new_v4().to_string();
    store.begin_account_deletion(alice, &generation, 2).unwrap();
    assert_eq!(
        store.rotate_credentials(&owner, &tokens("normal-refresh"), 3),
        Err(StoreError::Conflict)
    );
    assert_eq!(store.clear_credentials(&owner), Err(StoreError::Conflict));
    let wrong_account = CredentialOwner {
        account: bob,
        ..owner.clone()
    };
    let wrong_generation = uuid::Uuid::new_v4().to_string();
    for (expected, operation, requested_api) in [
        (&wrong_account, generation.as_str(), api),
        (&owner, wrong_generation.as_str(), api),
        (&owner, generation.as_str(), "http://other.invalid"),
    ] {
        assert_eq!(
            store.replace_pending_deletion_credentials(
                expected,
                operation,
                requested_api,
                "session-restored",
                0,
                &tokens("rejected"),
                3
            ),
            Err(StoreError::Conflict)
        );
    }
    let refreshed = store
        .replace_pending_deletion_credentials(
            &owner,
            &generation,
            api,
            "session-restored",
            0,
            &tokens("refreshed"),
            4,
        )
        .unwrap();
    assert_eq!(store.credentials(api).unwrap(), None);
    let pending = store
        .pending_deletion_credentials(alice, &generation, api)
        .unwrap()
        .unwrap();
    assert_eq!(pending.owner, refreshed);
    assert_eq!(pending.tokens, tokens("refreshed"));
    assert_eq!(
        store.replace_pending_deletion_credentials(
            &owner,
            &generation,
            api,
            "session-stale",
            1,
            &tokens("stale"),
            5
        ),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.replace_pending_deletion_credentials(
            &refreshed,
            &generation,
            api,
            "session-restored",
            0,
            &tokens("different"),
            5
        ),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store
            .replace_pending_deletion_credentials(
                &refreshed,
                &generation,
                api,
                "session-restored",
                0,
                &tokens("refreshed"),
                5
            )
            .unwrap(),
        refreshed
    );
    store.confirm_account_deletion(alice, &generation).unwrap();
    assert_eq!(
        store.replace_pending_deletion_credentials(
            &refreshed,
            &generation,
            api,
            "session-restored",
            1,
            &tokens("after-confirm"),
            6
        ),
        Err(StoreError::Conflict)
    );
}

#[test]
fn failed_rotation_does_not_publish_new_owner_or_clear_saved_pair() {
    let (_directory, store, alice, _bob, _task) = fixture();
    let owner = store
        .replace_credentials(
            None,
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            0,
            &tokens("saved"),
            1,
        )
        .unwrap();
    store.write(Durability::Durable,|tx| {
        tx.execute_batch("CREATE TEMP TRIGGER synthetic_rotation_failure BEFORE UPDATE ON credentials BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;")?;
        Ok(())
    }).unwrap();
    let error = store
        .rotate_credentials(&owner, &tokens("rejected"), 2)
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains("synthetic"));
    let saved = store.credentials("http://127.0.0.1:1234").unwrap().unwrap();
    assert_eq!(saved.owner, owner);
    assert_eq!(saved.tokens, tokens("saved"));
}

#[test]
fn deleting_accounts_cannot_install_or_inject_privileged_records() {
    let (_directory, store, alice, _bob, _task) = fixture();
    let generation = uuid::Uuid::new_v4().to_string();
    store.begin_account_deletion(alice, &generation, 2).unwrap();
    assert_eq!(
        store.replace_credentials(
            None,
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            0,
            &tokens("one"),
            3
        ),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.remote_identity(alice, &namespace("local")),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.save_mcp_definitions(alice, &[mcp(McpSecretUpdate::Keep)], McpEditOrigin::Local),
        Err(StoreError::Conflict)
    );
}

#[test]
fn receipt_reservation_is_canonical_account_bound_and_immutable() {
    let (_directory, store, alice, bob, task) = fixture();
    let id = uuid::Uuid::new_v4().to_string();
    let first = json!({"text":"synthetic prompt","sessionId":task,"nested":{"b":2,"a":1}});
    let second: Value = serde_json::from_str(&format!(
        "{{\"nested\":{{\"a\":1,\"b\":2}},\"sessionId\":\"{task}\",\"text\":\"synthetic prompt\"}}"
    ))
    .unwrap();
    assert_eq!(
        store.reserve_send(alice, &id, &task, &first, 1).unwrap(),
        None
    );
    assert_eq!(
        store.reserve_send(alice, &id, &task, &second, 2).unwrap(),
        Some(SendOutcome::Pending)
    );
    assert_eq!(
        store.reserve_send(alice, &id, &task, &json!({"different":"content"}), 3),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.reserve_send(bob, &id, &task, &first, 3),
        Err(StoreError::Conflict)
    );
    assert_eq!(store.send_outcome(bob, &id).unwrap(), SendOutcome::Missing);
    assert_eq!(
        store.finish_send(bob, &id, &Ok(json!("wrong-account"))),
        Err(StoreError::NotFound)
    );
    let result = Ok(json!("synthetic-run"));
    store.finish_send(alice, &id, &result).unwrap();
    store.finish_send(alice, &id, &result).unwrap();
    assert_eq!(
        store.finish_send(alice, &id, &Ok(json!("different-run"))),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.send_outcome(alice, &id).unwrap(),
        SendOutcome::Finished(result)
    );
}

#[test]
fn receipt_restart_keeps_uncertain_ids_reserved_and_prune_keeps_them() {
    let (directory, store, alice, _bob, task) = fixture();
    let pending = uuid::Uuid::new_v4().to_string();
    let finished = uuid::Uuid::new_v4().to_string();
    let parameters = json!({"sessionId":task,"text":"synthetic"});
    store
        .reserve_send(alice, &pending, &task, &parameters, 1)
        .unwrap();
    store
        .reserve_send(alice, &finished, &task, &parameters, 1)
        .unwrap();
    store
        .finish_send(alice, &finished, &Ok(json!("run")))
        .unwrap();
    store.close().unwrap();
    drop(store);
    let reopened = Store::open(StorePaths::new(directory.path())).unwrap();
    assert_eq!(
        reopened.send_outcome(alice, &pending).unwrap(),
        SendOutcome::Interrupted
    );
    assert_eq!(
        reopened
            .reserve_send(alice, &pending, &task, &parameters, 2)
            .unwrap(),
        Some(SendOutcome::Interrupted)
    );
    assert_eq!(
        reopened.finish_send(alice, &pending, &Ok(json!("late"))),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        reopened
            .prune_finished_send_receipts(alice, 8 * 24 * 60 * 60 * 1000)
            .unwrap(),
        1
    );
    assert_eq!(
        reopened.send_outcome(alice, &finished).unwrap(),
        SendOutcome::Missing
    );
    assert_eq!(
        reopened.send_outcome(alice, &pending).unwrap(),
        SendOutcome::Interrupted
    );
}

#[test]
fn unavailable_store_never_answers_missing_or_accepts_a_send() {
    let (_directory, store, alice, _bob, task) = fixture();
    let paths = store.paths().clone();
    store.close().unwrap();
    let locked = Store::unavailable(paths, super::super::StoreState::Locked).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    assert_eq!(locked.send_outcome(alice, &id), Err(StoreError::Locked));
    assert_eq!(
        locked.reserve_send(alice, &id, &task, &json!({}), 1),
        Err(StoreError::Locked)
    );
}

#[test]
fn mcp_values_never_enter_definitions_and_keep_is_bound_to_target() {
    let (_directory, store, alice, bob, _task) = fixture();
    let secret = "synthetic-mcp-secret";
    let configured = mcp(McpSecretUpdate::Set(SecretString::new(secret.into())));
    store
        .save_mcp_definitions(
            alice,
            std::slice::from_ref(&configured),
            McpEditOrigin::Local,
        )
        .unwrap();
    let listed = store.mcp_definitions(alice).unwrap();
    assert!(listed[0].environment[0].has_value);
    assert!(!serde_json::to_string(&listed).unwrap().contains(secret));
    let launch = store
        .mcp_launch_secrets(alice, "example", &configured.target)
        .unwrap();
    assert_eq!(launch.environment["TOKEN"].expose(), secret);
    assert!(!format!("{launch:?}").contains(secret));
    assert_eq!(
        store.mcp_launch_secrets(bob, "example", &configured.target),
        Err(StoreError::NotFound)
    );
    store
        .save_mcp_definitions(alice, &[mcp(McpSecretUpdate::Keep)], McpEditOrigin::Local)
        .unwrap();
    assert!(store.mcp_definitions(alice).unwrap()[0].environment[0].has_value);
    let mut redirected = mcp(McpSecretUpdate::Keep);
    redirected.target = McpTarget::Stdio {
        command: "synthetic-other --capture".into(),
    };
    store
        .save_mcp_definitions(alice, &[redirected.clone()], McpEditOrigin::Local)
        .unwrap();
    assert!(!store.mcp_definitions(alice).unwrap()[0].environment[0].has_value);
    assert_eq!(
        store.mcp_launch_secrets(alice, "example", &configured.target),
        Err(StoreError::Conflict)
    );
    assert!(
        store
            .mcp_launch_secrets(alice, "example", &redirected.target)
            .unwrap()
            .environment
            .is_empty()
    );
}

#[test]
fn remote_mcp_edits_cannot_change_secrets_or_redirect_them_and_roll_back() {
    let (_directory, store, alice, _bob, _task) = fixture();
    let configured = mcp(McpSecretUpdate::Set(SecretString::new(
        "synthetic-only".into(),
    )));
    store
        .save_mcp_definitions(
            alice,
            std::slice::from_ref(&configured),
            McpEditOrigin::Local,
        )
        .unwrap();
    let before = store.mcp_definitions(alice).unwrap();
    let mut redirected = mcp(McpSecretUpdate::Keep);
    redirected.target = McpTarget::Stdio {
        command: "synthetic-other".into(),
    };
    assert_eq!(
        store.save_mcp_definitions(alice, &[redirected], McpEditOrigin::Remote),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.save_mcp_definitions(alice, &[mcp(McpSecretUpdate::Clear)], McpEditOrigin::Remote),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.save_mcp_definitions(alice, &[configured], McpEditOrigin::Remote),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.save_mcp_definitions(alice, &[], McpEditOrigin::Remote),
        Err(StoreError::Conflict)
    );
    assert_eq!(store.mcp_definitions(alice).unwrap(), before);
    assert_eq!(
        store
            .mcp_launch_secrets(alice, "example", &before[0].target)
            .unwrap()
            .environment["TOKEN"]
            .expose(),
        "synthetic-only"
    );
    let mut harmless = mcp(McpSecretUpdate::Keep);
    harmless.description = "Changed display label".into();
    store
        .save_mcp_definitions(alice, &[harmless], McpEditOrigin::Remote)
        .unwrap();
}

#[test]
fn http_header_names_are_canonical_and_target_change_with_new_value_is_local() {
    let (_directory, store, alice, _bob, _task) = fixture();
    let mut definition = mcp(McpSecretUpdate::Keep);
    definition.target = McpTarget::StreamableHttp {
        url: "HTTP://EXAMPLE.INVALID:80/mcp".into(),
    };
    definition.environment.clear();
    definition.headers = vec![McpSecretEdit {
        name: "Authorization".into(),
        update: McpSecretUpdate::Set(SecretString::new("synthetic-token".into())),
    }];
    store
        .save_mcp_definitions(alice, &[definition], McpEditOrigin::Local)
        .unwrap();
    let listed = store.mcp_definitions(alice).unwrap();
    assert_eq!(
        listed[0].headers[0],
        McpSecretPresence {
            name: "authorization".into(),
            has_value: true
        }
    );
    assert_eq!(
        store
            .mcp_launch_secrets(alice, "example", &listed[0].target)
            .unwrap()
            .headers["authorization"]
            .expose(),
        "synthetic-token"
    );
    let mut duplicate = mcp(McpSecretUpdate::Keep);
    duplicate.environment.push(McpSecretEdit {
        name: "TOKEN".into(),
        update: McpSecretUpdate::Keep,
    });
    assert_eq!(
        store.save_mcp_definitions(alice, &[duplicate], McpEditOrigin::Local),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(store.mcp_definitions(alice).unwrap(), listed);
}

#[test]
fn remote_identity_is_stable_scoped_and_survives_credential_clear() {
    let (directory, store, alice, bob, _task) = fixture();
    let ns = namespace("local");
    assert_eq!(store.saved_remote_identity(alice, &ns).unwrap(), None);
    let identity = store.remote_identity(alice, &ns).unwrap();
    assert!(!identity.enabled);
    assert_eq!(store.remote_identity(alice, &ns).unwrap(), identity);
    assert_ne!(
        store.remote_identity(bob, &ns).unwrap().secret,
        identity.secret
    );
    assert_ne!(
        store
            .remote_identity(alice, &namespace("dev"))
            .unwrap()
            .secret,
        identity.secret
    );
    let owner = store
        .replace_credentials(
            None,
            alice,
            "http://127.0.0.1:1234",
            "session-a",
            0,
            &tokens("one"),
            1,
        )
        .unwrap();
    store.clear_credentials(&owner).unwrap();
    store.close().unwrap();
    drop(store);
    let reopened = Store::open(StorePaths::new(directory.path())).unwrap();
    assert_eq!(
        reopened.saved_remote_identity(alice, &ns).unwrap(),
        Some(identity)
    );
}

#[test]
fn pairing_confirmation_attempts_and_discovery_preserve_authority() {
    let (directory, store, alice, _bob, _task) = fixture();
    let ns = namespace("local");
    let key = "11".repeat(32);
    let unknown = "22".repeat(32);
    store.remote_identity(alice, &ns).unwrap();
    store
        .reserve_remote_pairing_attempt(alice, &ns, &key, [1; 16])
        .unwrap();
    assert_eq!(
        store.reserve_remote_pairing_attempt(alice, &ns, &key, [1; 16]),
        Err(StoreError::Conflict)
    );
    store
        .remember_remote_host_proof(alice, &ns, &key, [2; 16], "Synthetic peer")
        .unwrap();
    assert_eq!(
        store.confirm_remote_host(alice, &ns, &key, [3; 16]),
        Err(StoreError::Conflict)
    );
    assert!(
        !store
            .saved_remote_identity(alice, &ns)
            .unwrap()
            .unwrap()
            .peers[&key]
            .can_connect
    );
    store
        .confirm_remote_host(alice, &ns, &key, [2; 16])
        .unwrap();
    store
        .grant_remote_peer(
            alice,
            &ns,
            &key,
            RemotePeer {
                name: "Synthetic peer".into(),
                grant: [4; 16],
                controls_this_computer: true,
                can_connect: false,
                pending_confirmation: None,
            },
        )
        .unwrap();
    let before = store.saved_remote_identity(alice, &ns).unwrap().unwrap();
    assert!(before.peers[&key].can_connect && before.peers[&key].controls_this_computer);
    store
        .refresh_remote_peer_names(
            alice,
            &ns,
            &BTreeMap::from([
                (key.clone(), "New label".into()),
                (unknown, "Unknown".into()),
            ]),
        )
        .unwrap();
    let after = store.saved_remote_identity(alice, &ns).unwrap().unwrap();
    assert_eq!(after.peers.len(), 1);
    assert_eq!(after.peers[&key].grant, before.peers[&key].grant);
    assert_eq!(after.attempted_windows, before.attempted_windows);
    assert!(!after.enabled);
    store.close().unwrap();
    drop(store);
    let reopened = Store::open(StorePaths::new(directory.path())).unwrap();
    assert_eq!(
        reopened.reserve_remote_pairing_attempt(alice, &ns, &key, [1; 16]),
        Err(StoreError::Conflict)
    );
    reopened.revoke_remote_peer(alice, &ns, &key).unwrap();
    let saved = reopened.saved_remote_identity(alice, &ns).unwrap().unwrap();
    assert!(saved.peers.is_empty());
    assert_eq!(saved.attempted_windows[&key], vec![[1; 16]]);
}

#[test]
fn remote_record_does_not_duplicate_the_secret_and_rejects_malformed_json() {
    let (_directory, store, alice, _bob, _task) = fixture();
    let ns = namespace("local");
    store.remote_identity(alice, &ns).unwrap();
    let json = store
        .read(|tx| {
            Ok(tx.query_row(
                "SELECT record FROM remote_identities WHERE account_id=?1",
                [alice.0],
                |row| row.get::<_, String>(0),
            )?)
        })
        .unwrap();
    assert!(!json.contains("secret"));
    store
        .write(Durability::Durable, |tx| {
            tx.execute(
                "UPDATE remote_identities SET record='{}' WHERE account_id=?1",
                [alice.0],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        store.saved_remote_identity(alice, &ns),
        Err(StoreError::InvalidData)
    );
}
