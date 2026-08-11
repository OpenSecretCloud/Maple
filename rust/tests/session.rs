mod common;

use opensecret::Result;
use std::env;

#[tokio::test]
async fn test_session_establishment() -> Result<()> {
    let base_url = env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let client = common::new_test_client(base_url)?;

    // Initially should have no session
    assert!(
        client.get_session_id()?.is_none(),
        "Should not have session before handshake"
    );

    // Perform attestation handshake
    client.perform_attestation_handshake().await?;

    // Should now have a session
    let session_id = client
        .get_session_id()?
        .expect("Should have session after handshake");

    assert!(!session_id.to_string().is_empty());
    assert!(
        session_id.to_string().len() > 20,
        "Session ID should be a reasonable length"
    );

    Ok(())
}

#[tokio::test]
async fn test_session_key_derivation() -> Result<()> {
    use opensecret::crypto::{derive_shared_secret, generate_key_pair};

    // Test key derivation works correctly
    let client_keypair = generate_key_pair();
    let server_keypair = generate_key_pair();

    // Both parties should derive the same shared secret
    let client_shared = derive_shared_secret(&client_keypair.secret, &server_keypair.public);
    let server_shared = derive_shared_secret(&server_keypair.secret, &client_keypair.public);

    assert_eq!(
        client_shared.as_bytes(),
        server_shared.as_bytes(),
        "Shared secrets should match"
    );

    Ok(())
}
