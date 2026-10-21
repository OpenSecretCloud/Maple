//! Account adapters cannot outlive the store or account identity they cache.

use super::*;
use crate::store::{Store, StorePaths, keys::KdfParams, lifecycle::ERASE_CONFIRMATION};

const ALICE: &str = "cache-alice@example.test";
const BOB: &str = "cache-bob@example.test";

fn fixture() -> (tempfile::TempDir, Arc<Store>, MapleAgentService) {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(StorePaths::new(directory.path().join("local"))).unwrap());
    let service = MapleAgentService::new(
        MapleAgentHostResources::new(
            AgentPathLayout::from_resource_root(directory.path().join("resources"))
                .with_home(Some(directory.path().join("home"))),
            Arc::new(tests::Recorder::default()),
            "You are Maple.".into(),
        )
        .with_store(Arc::clone(&store)),
    );
    (directory, store, service)
}

fn task(directory: &Path, number: u8) -> store::TaskRow {
    store::TaskRow::new(
        format!("00000000-0000-7000-8000-{number:012x}"),
        "Account-bound task".into(),
        directory.join("project").to_string_lossy().into_owned(),
        store::TaskKind::Desktop,
        None,
        1,
    )
}

#[tokio::test]
async fn erase_setup_reverses_numeric_ids_without_reusing_account_adapters() {
    let (directory, store, service) = fixture();
    let old_alice = service.handle_for_user(ALICE).await.unwrap();
    let old_bob = service.handle_for_user(BOB).await.unwrap();
    let alice_cache = old_alice.store().unwrap();
    let bob_cache = old_bob.store().unwrap();
    let alice_id = alice_cache.account();
    let bob_id = bob_cache.account();
    alice_cache.insert(&task(directory.path(), 1)).unwrap();
    let generation = store.store_id().unwrap();

    store.erase_local_data(ERASE_CONFIRMATION).unwrap();
    service.invalidate_store_session();
    store.setup_store(None, &[], KdfParams::floor()).unwrap();
    assert_ne!(store.store_id().unwrap(), generation);

    // Reverse creation order to reuse Alice's old numeric id for Bob.
    let bob = service.handle_for_user(BOB).await.unwrap();
    let bob_tasks = bob.store().unwrap();
    let alice = service.handle_for_user(ALICE).await.unwrap();
    let alice_tasks = alice.store().unwrap();
    assert_eq!(bob_tasks.account(), alice_id);
    assert_eq!(alice_tasks.account(), bob_id);
    assert!(!Arc::ptr_eq(&bob_cache, &bob_tasks));
    assert!(!Arc::ptr_eq(&alice_cache, &alice_tasks));
    assert!(old_alice.list_sessions(None).await.is_err());
    assert!(old_bob.list_sessions(None).await.is_err());
    assert!(alice_cache.insert(&task(directory.path(), 2)).is_err());
    assert!(bob_cache.list(None).is_err());

    let row = task(directory.path(), 3);
    bob_tasks.insert(&row).unwrap();
    assert!(store.task(bob_tasks.account(), &row.id).unwrap().is_some());
    assert!(
        store
            .task(alice_tasks.account(), &row.id)
            .unwrap()
            .is_none()
    );
    assert_eq!(bob.list_sessions(None).await.unwrap().len(), 1);
    assert!(alice.list_sessions(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn generation_replacement_itself_fences_handles_and_cached_adapters() {
    let (directory, store, service) = fixture();
    let old = service.handle_for_user(ALICE).await.unwrap();
    let cached = old.store().unwrap();
    store.erase_local_data(ERASE_CONFIRMATION).unwrap();
    store.setup_store(None, &[], KdfParams::floor()).unwrap();
    // Defense in depth: even if a reset caller omitted explicit invalidation,
    // the same Store Arc now represents a different durable store identity.
    assert!(old.verify_generation().await.is_err());
    assert!(old.store().is_err());
    assert!(cached.insert(&task(directory.path(), 4)).is_err());
    let fresh = service.handle_for_user(ALICE).await.unwrap();
    let fresh_cache = fresh.store().unwrap();
    assert!(!Arc::ptr_eq(&cached, &fresh_cache));
    assert!(fresh.list_sessions(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn collected_account_id_reuse_cannot_reauthorize_another_accounts_cache() {
    let (directory, store, service) = fixture();
    let alice = service.handle_for_user(ALICE).await.unwrap();
    let cached = alice.store().unwrap();
    let old_id = cached.account();
    let deletion = uuid::Uuid::new_v4().to_string();
    store.begin_account_deletion(old_id, &deletion, 1).unwrap();
    store.confirm_account_deletion(old_id, &deletion).unwrap();
    for _ in 0..32 {
        if store.collect_account_step(old_id).unwrap().done {
            break;
        }
    }
    assert_eq!(store.account_id(ALICE).unwrap(), None);
    let bob = service.handle_for_user(BOB).await.unwrap();
    let bob_tasks = bob.store().unwrap();
    assert_eq!(bob_tasks.account(), old_id);
    assert!(cached.insert(&task(directory.path(), 5)).is_err());
    assert!(cached.get(&task(directory.path(), 5).id).is_err());

    // Cache lookup checks the actual current owner even without a process
    // restart or a whole-store generation replacement.
    let fresh_alice = alice.store().unwrap();
    assert!(!Arc::ptr_eq(&cached, &fresh_alice));
    assert_ne!(fresh_alice.account(), bob_tasks.account());
    let row = task(directory.path(), 6);
    fresh_alice.insert(&row).unwrap();
    assert!(
        store
            .task(fresh_alice.account(), &row.id)
            .unwrap()
            .is_some()
    );
    assert!(store.task(bob_tasks.account(), &row.id).unwrap().is_none());
}

#[tokio::test]
async fn account_drain_discards_the_cache_before_same_user_collection_and_relogin() {
    let (_directory, store, service) = fixture();
    let old = service.handle_for_user(ALICE).await.unwrap();
    let cached = old.store().unwrap();
    let account = cached.account();
    service.drain_account(ALICE).await.unwrap();
    let deletion = uuid::Uuid::new_v4().to_string();
    store.begin_account_deletion(account, &deletion, 1).unwrap();
    store.confirm_account_deletion(account, &deletion).unwrap();
    for _ in 0..32 {
        if store.collect_account_step(account).unwrap().done {
            break;
        }
    }
    assert_eq!(store.account_id(ALICE).unwrap(), None);
    let fresh = service.handle_for_user(ALICE).await.unwrap();
    let fresh_cache = fresh.store().unwrap();
    assert_eq!(fresh_cache.account(), account);
    assert!(!Arc::ptr_eq(&cached, &fresh_cache));
    assert!(old.verify_generation().await.is_err());
    assert!(cached.ensure_current().is_err());
}

#[tokio::test]
async fn rejected_reset_preserves_live_handles_and_their_cache() {
    let (directory, store, service) = fixture();
    let alice = service.handle_for_user(ALICE).await.unwrap();
    let cached = alice.store().unwrap();
    let generation = store.store_id().unwrap();
    assert!(store.erase_local_data("incorrect confirmation").is_err());
    assert_eq!(store.store_id().unwrap(), generation);
    alice.verify_generation().await.unwrap();
    assert!(Arc::ptr_eq(&cached, &alice.store().unwrap()));
    cached.insert(&task(directory.path(), 7)).unwrap();
    assert_eq!(alice.list_sessions(None).await.unwrap().len(), 1);
}
