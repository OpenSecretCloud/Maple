mod common;

use opensecret::{Error, OpenSecretClient, Pcr0Environment, Result};
use std::env;

#[tokio::test]
async fn test_attestation_handshake_localhost() -> Result<()> {
    // Skip if not running against localhost
    let base_url = env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    if !base_url.contains("localhost") && !base_url.contains("127.0.0.1") {
        eprintln!("Skipping localhost test - not running against localhost");
        return Ok(());
    }

    let client = common::new_test_client(base_url)?;

    // Perform attestation handshake with mock attestation
    client.perform_attestation_handshake().await?;

    // Verify session was established
    let session_id = client
        .get_session_id()?
        .expect("Session ID should be set after successful handshake");

    assert!(!session_id.to_string().is_empty());

    Ok(())
}

#[tokio::test]
async fn test_attestation_handshake_hosted_selected_environment() -> Result<()> {
    // Try to load .env.local if it exists (for local testing)
    if std::path::Path::new("../.env.local").exists() {
        dotenv::from_path("../.env.local").ok();
    }

    // This test requires VITE_OPEN_SECRET_API_URL to be set to a hosted endpoint.
    let base_url = env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    if base_url.contains("localhost") || base_url.contains("127.0.0.1") {
        println!("Skipping hosted attestation test - running against localhost");
        return Ok(());
    }

    let pcr0_environment = common::selected_pcr0_environment()?;
    let client = common::new_test_client(base_url.clone())?;

    // Perform attestation handshake with real AWS Nitro attestation
    client.perform_attestation_handshake().await?;

    // Verify session was established
    let session_id = client
        .get_session_id()?
        .expect("Session ID should be set after successful handshake");

    assert!(!session_id.to_string().is_empty());
    println!(
        "✅ Hosted {:?} attestation successful against {}",
        pcr0_environment, base_url
    );
    println!("   Session ID: {}", session_id);

    Ok(())
}

#[tokio::test]
async fn test_hosted_development_rejects_default_production_policy() -> Result<()> {
    let base_url = env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    if base_url.contains("localhost") || base_url.contains("127.0.0.1") {
        println!("Skipping hosted policy-separation test - running against localhost");
        return Ok(());
    }
    if common::selected_pcr0_environment()? != Pcr0Environment::Development {
        println!("Skipping hosted development policy-separation test");
        return Ok(());
    }

    let client = OpenSecretClient::new(base_url)?;
    let error = client.perform_attestation_handshake().await.unwrap_err();

    assert!(matches!(error, Error::AttestationVerificationFailed(_)));
    assert!(client.get_session_id()?.is_none());
    Ok(())
}

#[tokio::test]
async fn test_attestation_nonce_verification() -> Result<()> {
    let base_url = env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let client = common::new_test_client(base_url.clone())?;

    // The handshake should generate a unique nonce internally
    client.perform_attestation_handshake().await?;
    let first_session = client.get_session_id()?.expect("Should have session");

    // Create a new client and do another handshake - should get different session
    let client2 = common::new_test_client(base_url)?;
    client2.perform_attestation_handshake().await?;
    let second_session = client2.get_session_id()?.expect("Should have session");

    // Sessions should be different (different nonces)
    assert_ne!(
        first_session, second_session,
        "Each handshake should create a unique session"
    );

    Ok(())
}

#[tokio::test]
async fn test_connection_health_check() -> Result<()> {
    let base_url = env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let client = common::new_test_client(base_url)?;

    // Test basic connection without attestation
    let response = client.test_connection().await?;
    assert!(!response.is_empty());

    Ok(())
}
