use super::*;

#[test]
fn only_an_opaque_generated_value_gets_the_generator_exemption_in_all_password_operations() {
    struct ZeroRng;
    impl RngCore for ZeroRng {
        fn next_u32(&mut self) -> u32 {
            unreachable!()
        }
        fn next_u64(&mut self) -> u64 {
            unreachable!()
        }
        fn fill_bytes(&mut self, _: &mut [u8]) {
            unreachable!()
        }
        fn try_fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), rand::Error> {
            destination.fill(0);
            Ok(())
        }
    }
    let generated = keys::generated_for_test(&mut ZeroRng).unwrap();
    assert_eq!(
        generated.password().as_str(),
        "abacus abacus abacus abacus abacus abacus"
    );
    assert!(!generated.password().assess(&[]).allowed());
    let directory = tempfile::tempdir().unwrap();
    let store =
        Store::unavailable(StorePaths::new(directory.path()), StoreState::Maintenance).unwrap();
    store.initialize_recovered().unwrap();
    assert!(matches!(
        store.set_store_password(generated.password(), &[], KdfParams::floor()),
        Err(LifecycleError::Key(KeyError::WeakPassword))
    ));
    store
        .set_store_password_choice(
            PasswordChoice::Suggested(&generated),
            &[],
            KdfParams::floor(),
        )
        .unwrap();
    store
        .change_store_password_choice(
            generated.password(),
            PasswordChoice::Suggested(&generated),
            &[],
            KdfParams::floor(),
            false,
        )
        .unwrap();
    store.erase_local_data(ERASE_CONFIRMATION).unwrap();
    assert_eq!(store.state(), StoreState::NeedsSetup);
    store
        .setup_store_choice(
            Some(PasswordChoice::Suggested(&generated)),
            &[],
            KdfParams::floor(),
        )
        .unwrap();
    let paths = store.paths.clone();
    store.close().unwrap();
    drop(store);
    let restarted = Store::unavailable(paths, StoreState::Maintenance).unwrap();
    restarted.initialize_recovered().unwrap();
    assert_eq!(restarted.state(), StoreState::Locked);
    restarted.unlock_store(generated.password(), None).unwrap();
    assert_eq!(restarted.state(), StoreState::Open);
}
