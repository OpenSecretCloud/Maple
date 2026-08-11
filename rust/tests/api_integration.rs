mod common;

use opensecret::{KeyOptions, Result, SigningAlgorithm};
use uuid::Uuid;

#[tokio::test]
async fn test_user_profile_api() -> Result<()> {
    // Load environment variables from .env.local in SDK root
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set in .env.local");

    let test_email = std::env::var("VITE_TEST_EMAIL").expect("VITE_TEST_EMAIL must be set");
    let test_password =
        std::env::var("VITE_TEST_PASSWORD").expect("VITE_TEST_PASSWORD must be set");

    // Create client and login
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;

    // Try to login, register if it fails
    let _ = match client
        .login(test_email.clone(), test_password.clone(), client_id)
        .await
    {
        Ok(response) => response,
        Err(_) => {
            client
                .register(test_email.clone(), test_password, client_id, None)
                .await?
        }
    };

    // Test get_user endpoint
    let user_response = client.get_user().await?;
    assert_eq!(user_response.user.email, Some(test_email));
    println!("✓ User profile retrieved successfully");

    Ok(())
}

#[tokio::test]
async fn test_kv_storage_apis() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set");

    // Create client and login as guest for isolation
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;

    let guest_password = "test_kv_password";
    client
        .register_guest(guest_password.to_string(), client_id)
        .await?;

    // Test KV operations
    let test_key = "test_key_rust";
    let test_value = "test_value_from_rust_sdk";

    // PUT
    let put_result = client.kv_put(test_key, test_value.to_string()).await?;
    assert_eq!(put_result, test_value);
    println!("✓ KV PUT successful");

    // GET
    let get_result = client.kv_get(test_key).await?;
    assert_eq!(get_result, test_value);
    println!("✓ KV GET successful");

    // LIST
    let list_result = client.kv_list().await?;
    assert!(list_result.iter().any(|item| item.key == test_key));
    println!("✓ KV LIST successful");

    // DELETE
    client.kv_delete(test_key).await?;
    println!("✓ KV DELETE successful");

    // Verify deletion
    let list_after_delete = client.kv_list().await?;
    assert!(!list_after_delete.iter().any(|item| item.key == test_key));
    println!("✓ KV deletion verified");

    Ok(())
}

#[tokio::test]
async fn test_kv_delete_all() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set");

    // Create client and login as guest for isolation
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;

    client
        .register_guest("test_kv_delete_all".to_string(), client_id)
        .await?;

    // Add multiple keys
    client.kv_put("key1", "value1".to_string()).await?;
    client.kv_put("key2", "value2".to_string()).await?;
    client.kv_put("key3", "value3".to_string()).await?;

    // Verify they exist
    let list_result = client.kv_list().await?;
    assert!(list_result.len() >= 3);
    println!("✓ Keys added successfully");

    // Delete all
    client.kv_delete_all().await?;
    println!("✓ kv_delete_all called");

    // Verify empty
    let list_after_delete = client.kv_list().await?;
    assert!(list_after_delete.is_empty());
    println!("✓ All keys deleted successfully");

    Ok(())
}

#[tokio::test]
async fn test_kv_storage_with_special_characters() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set");

    // Create client and login
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;
    client
        .register_guest("test_kv_special".to_string(), client_id)
        .await?;

    // Test with various special characters that need URL encoding
    let test_cases = vec![
        ("key/with/slashes", "value1"),
        ("key?with=query&params", "value2"),
        ("key#with#hash", "value3"),
        ("key with spaces", "value4"),
        ("key%with%percents", "value5"),
        ("key@with!special$chars", "value6"),
        ("key[with]brackets{and}braces", "value7"),
        ("key:with:colons;semicolons", "value8"),
    ];

    for (key, value) in &test_cases {
        println!("Testing key: {:?}", key);

        // PUT with special characters
        let put_result = client.kv_put(key, value.to_string()).await?;
        assert_eq!(put_result, *value);

        // GET with special characters
        let get_result = client.kv_get(key).await?;
        assert_eq!(get_result, *value);

        // DELETE with special characters
        client.kv_delete(key).await?;

        println!("✓ KV operations successful for key: {:?}", key);
    }

    // Verify all were deleted
    let final_list = client.kv_list().await?;
    for (key, _) in &test_cases {
        assert!(
            !final_list.iter().any(|item| item.key == *key),
            "Key {:?} should have been deleted",
            key
        );
    }
    println!("✓ All special character keys handled correctly");

    Ok(())
}

#[tokio::test]
async fn test_private_key_generation() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set");

    // Create client and login as guest
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;
    client
        .register_guest("test_key_gen".to_string(), client_id)
        .await?;

    // Test private key generation without options
    let key_response = client.get_private_key(None).await?;
    assert!(!key_response.mnemonic.is_empty());
    let word_count = key_response.mnemonic.split_whitespace().count();
    assert!(word_count == 12 || word_count == 18 || word_count == 24);
    println!(
        "✓ Private key generated successfully ({} words)",
        word_count
    );

    // Test with BIP-85 derivation path for 24 words
    let options = KeyOptions {
        private_key_derivation_path: None,
        seed_phrase_derivation_path: Some("m/83696968'/39'/0'/24'/0'".to_string()),
    };
    let key_with_path = client.get_private_key(Some(options)).await?;
    assert!(!key_with_path.mnemonic.is_empty());
    assert_eq!(key_with_path.mnemonic.split_whitespace().count(), 24);
    println!("✓ Private key with BIP-85 derivation generated (24 words)");

    // Test private key bytes
    let key_bytes = client.get_private_key_bytes(None).await?;
    assert!(!key_bytes.private_key.is_empty());
    assert_eq!(key_bytes.private_key.len(), 64); // 32 bytes as hex = 64 chars
    println!("✓ Private key bytes retrieved successfully");

    Ok(())
}

#[tokio::test]
async fn test_message_signing() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set");

    // Create client and login
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;
    client
        .register_guest("test_signing".to_string(), client_id)
        .await?;

    let test_message = b"Hello from Rust SDK";

    // Test Schnorr signing
    let schnorr_sig = client
        .sign_message(test_message, SigningAlgorithm::Schnorr, None)
        .await?;
    assert!(!schnorr_sig.signature.is_empty());
    assert!(!schnorr_sig.message_hash.is_empty());
    println!("✓ Schnorr signature created");

    // Test ECDSA signing
    let ecdsa_sig = client
        .sign_message(test_message, SigningAlgorithm::Ecdsa, None)
        .await?;
    assert!(!ecdsa_sig.signature.is_empty());
    assert!(!ecdsa_sig.message_hash.is_empty());
    println!("✓ ECDSA signature created");

    // Test with derivation path
    let key_options = KeyOptions {
        private_key_derivation_path: Some("m/44'/0'/0'/0/0".to_string()),
        seed_phrase_derivation_path: None,
    };
    let sig_with_path = client
        .sign_message(test_message, SigningAlgorithm::Schnorr, Some(key_options))
        .await?;
    assert!(!sig_with_path.signature.is_empty());
    println!("✓ Signature with derivation path created");

    Ok(())
}

#[tokio::test]
async fn test_public_key_retrieval() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set");

    // Create client and login
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;
    client
        .register_guest("test_pubkey".to_string(), client_id)
        .await?;

    // Test Schnorr public key
    let schnorr_pubkey = client
        .get_public_key(SigningAlgorithm::Schnorr, None)
        .await?;
    assert!(!schnorr_pubkey.public_key.is_empty());
    assert!(matches!(
        schnorr_pubkey.algorithm,
        SigningAlgorithm::Schnorr
    ));
    println!("✓ Schnorr public key retrieved");

    // Test ECDSA public key
    let ecdsa_pubkey = client.get_public_key(SigningAlgorithm::Ecdsa, None).await?;
    assert!(!ecdsa_pubkey.public_key.is_empty());
    assert!(matches!(ecdsa_pubkey.algorithm, SigningAlgorithm::Ecdsa));
    println!("✓ ECDSA public key retrieved");

    // Test with derivation path
    let key_options = KeyOptions {
        private_key_derivation_path: Some("m/44'/0'/0'/0/1".to_string()),
        seed_phrase_derivation_path: None,
    };
    let pubkey_with_path = client
        .get_public_key(SigningAlgorithm::Schnorr, Some(key_options))
        .await?;
    assert!(!pubkey_with_path.public_key.is_empty());
    println!("✓ Public key with derivation path retrieved");

    Ok(())
}

#[tokio::test]
async fn test_encryption_decryption() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set");

    // Create client and login
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;
    client
        .register_guest("test_crypto".to_string(), client_id)
        .await?;

    let test_data = "Hello, World!";

    // Test basic encryption/decryption (matching TypeScript test exactly)
    let encrypted = client.encrypt_data(test_data.to_string(), None).await?;
    assert!(!encrypted.encrypted_data.is_empty());
    println!("✓ Data encrypted successfully");

    let decrypted = client
        .decrypt_data(encrypted.encrypted_data.clone(), None)
        .await?;
    assert_eq!(decrypted, test_data);
    println!("✓ Data decrypted successfully");

    // Test with BIP-32 derivation path (matching TypeScript test)
    let derivation_path = "m/44'/0'/0'/0/0".to_string();
    let key_options = KeyOptions {
        private_key_derivation_path: Some(derivation_path),
        seed_phrase_derivation_path: None,
    };
    let encrypted_with_path = client
        .encrypt_data(test_data.to_string(), Some(key_options.clone()))
        .await?;
    assert!(!encrypted_with_path.encrypted_data.is_empty());

    // Decrypt with same path
    let decrypted_with_path = client
        .decrypt_data(
            encrypted_with_path.encrypted_data.clone(),
            Some(key_options),
        )
        .await?;
    assert_eq!(decrypted_with_path, test_data);
    println!("✓ Encryption/decryption with BIP-32 derivation path successful");

    // Test decrypting with wrong derivation path (should fail) - matching TypeScript
    let wrong_options = KeyOptions {
        private_key_derivation_path: Some("m/44'/0'/0'/0/1".to_string()),
        seed_phrase_derivation_path: None,
    };
    let wrong_decrypt_result = client
        .decrypt_data(
            encrypted_with_path.encrypted_data.clone(),
            Some(wrong_options),
        )
        .await;
    assert!(
        wrong_decrypt_result.is_err(),
        "Should not decrypt with wrong derivation path"
    );
    println!("✓ Correctly failed to decrypt with wrong derivation path");

    // Test decrypting with no path when encrypted with path (should fail) - matching TypeScript
    let no_path_decrypt_result = client
        .decrypt_data(encrypted_with_path.encrypted_data.clone(), None)
        .await;
    assert!(
        no_path_decrypt_result.is_err(),
        "Should not decrypt with missing derivation path"
    );
    println!("✓ Correctly failed to decrypt with missing derivation path");

    // Test with invalid encrypted data (should fail) - matching TypeScript
    let invalid_decrypt_result = client.decrypt_data("invalid-data".to_string(), None).await;
    assert!(
        invalid_decrypt_result.is_err(),
        "Should not decrypt invalid data"
    );
    println!("✓ Correctly failed to decrypt invalid data");

    Ok(())
}

#[tokio::test]
async fn test_third_party_token() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenv::from_path(env_path).ok();
    }

    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .expect("VITE_TEST_CLIENT_ID must be set");

    let test_email = std::env::var("VITE_TEST_EMAIL").expect("VITE_TEST_EMAIL must be set");
    let test_password =
        std::env::var("VITE_TEST_PASSWORD").expect("VITE_TEST_PASSWORD must be set");

    // Create client and login with email user (third party tokens may require email user)
    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;

    let _ = match client
        .login(test_email.clone(), test_password.clone(), client_id)
        .await
    {
        Ok(response) => response,
        Err(_) => {
            client
                .register(test_email, test_password, client_id, None)
                .await?
        }
    };

    // Test without audience
    let token_no_audience = client.generate_third_party_token(None).await?;
    assert!(!token_no_audience.token.is_empty());
    println!("✓ Third party token generated without audience");

    // Test with valid audience
    let valid_audience = "https://billing-dev.opensecret.cloud".to_string();
    let token_with_audience = client
        .generate_third_party_token(Some(valid_audience))
        .await?;
    assert!(!token_with_audience.token.is_empty());
    println!("✓ Third party token generated with audience");

    Ok(())
}
