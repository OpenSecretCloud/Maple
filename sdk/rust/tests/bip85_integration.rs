mod common;

use opensecret::{KeyOptions, Result, SigningAlgorithm};
use uuid::Uuid;

// BIP-85 test constants (matching TypeScript tests)
const BIP85_STANDARD_PATH: &str = "m/83696968'/39'/0'/12'/0'";
const BIP85_ALTERNATIVE_PATH: &str = "m/83696968'/39'/0'/12'/1'";
const BIP32_PATH: &str = "m/44'/0'/0'/0/0";

#[tokio::test]
async fn test_bip85_derive_child_mnemonic() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenvy::from_path(env_path).ok();
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
        .register_guest("test_bip85".to_string(), client_id)
        .await?;

    // Get master mnemonic
    let master_mnemonic = client.get_private_key(None).await?;
    assert!(!master_mnemonic.mnemonic.is_empty());
    let master_word_count = master_mnemonic.mnemonic.split_whitespace().count();
    assert!(master_word_count >= 12);
    println!("✓ Master mnemonic has {} words", master_word_count);

    // Get child mnemonic using BIP-85
    let child_options = KeyOptions {
        seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
        private_key_derivation_path: None,
    };
    let child_mnemonic = client.get_private_key(Some(child_options)).await?;
    assert!(!child_mnemonic.mnemonic.is_empty());
    assert_eq!(child_mnemonic.mnemonic.split_whitespace().count(), 12);
    println!("✓ BIP-85 child mnemonic generated (12 words)");

    // Child mnemonic should be different from master
    assert_ne!(child_mnemonic.mnemonic, master_mnemonic.mnemonic);
    println!("✓ Child mnemonic is different from master");

    // Different BIP-85 paths should generate different mnemonics
    let alt_options = KeyOptions {
        seed_phrase_derivation_path: Some(BIP85_ALTERNATIVE_PATH.to_string()),
        private_key_derivation_path: None,
    };
    let alt_child_mnemonic = client.get_private_key(Some(alt_options)).await?;
    assert_ne!(alt_child_mnemonic.mnemonic, child_mnemonic.mnemonic);
    println!("✓ Different BIP-85 paths produce different mnemonics");

    Ok(())
}

#[tokio::test]
async fn test_bip85_private_key_bytes() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenvy::from_path(env_path).ok();
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
        .register_guest("test_bip85_bytes".to_string(), client_id)
        .await?;

    // Get master private key bytes
    let master_key = client.get_private_key_bytes(None).await?;
    assert!(!master_key.private_key.is_empty());
    assert_eq!(master_key.private_key.len(), 64); // 32 bytes hex
    println!("✓ Master private key bytes retrieved");

    // Get BIP-85 derived private key bytes (master key of child seed)
    let bip85_options = KeyOptions {
        seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
        private_key_derivation_path: None,
    };
    let bip85_key = client.get_private_key_bytes(Some(bip85_options)).await?;
    assert!(!bip85_key.private_key.is_empty());
    assert_eq!(bip85_key.private_key.len(), 64);
    println!("✓ BIP-85 derived private key bytes retrieved");

    // BIP-85 derived key should be different from master
    assert_ne!(bip85_key.private_key, master_key.private_key);
    println!("✓ BIP-85 derived key is different from master");

    // Combined BIP-85 + BIP-32 derivation
    let combined_options = KeyOptions {
        seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
        private_key_derivation_path: Some(BIP32_PATH.to_string()),
    };
    let combined_key = client.get_private_key_bytes(Some(combined_options)).await?;
    assert!(!combined_key.private_key.is_empty());
    assert_ne!(combined_key.private_key, bip85_key.private_key);
    assert_ne!(combined_key.private_key, master_key.private_key);
    println!("✓ Combined BIP-85 + BIP-32 derivation works");

    Ok(())
}

#[tokio::test]
async fn test_bip85_public_key_derivation() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenvy::from_path(env_path).ok();
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
        .register_guest("test_bip85_pubkey".to_string(), client_id)
        .await?;

    // Get master public key
    let master_pubkey = client
        .get_public_key(SigningAlgorithm::Schnorr, None)
        .await?;
    assert!(!master_pubkey.public_key.is_empty());
    assert_eq!(master_pubkey.public_key.len(), 64); // 32 bytes hex
    println!("✓ Master public key retrieved");

    // Get BIP-85 derived public key
    let bip85_options = KeyOptions {
        seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
        private_key_derivation_path: None,
    };
    let bip85_pubkey = client
        .get_public_key(SigningAlgorithm::Schnorr, Some(bip85_options))
        .await?;
    assert!(!bip85_pubkey.public_key.is_empty());
    assert_ne!(bip85_pubkey.public_key, master_pubkey.public_key);
    println!("✓ BIP-85 derived public key is different from master");

    // Combined BIP-85 + BIP-32 derivation
    let combined_options = KeyOptions {
        seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
        private_key_derivation_path: Some(BIP32_PATH.to_string()),
    };
    let combined_pubkey = client
        .get_public_key(SigningAlgorithm::Schnorr, Some(combined_options))
        .await?;
    assert!(!combined_pubkey.public_key.is_empty());
    assert_ne!(combined_pubkey.public_key, bip85_pubkey.public_key);
    assert_ne!(combined_pubkey.public_key, master_pubkey.public_key);
    println!("✓ Combined BIP-85 + BIP-32 public key derivation works");

    Ok(())
}

#[tokio::test]
async fn test_bip85_message_signing() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenvy::from_path(env_path).ok();
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
        .register_guest("test_bip85_signing".to_string(), client_id)
        .await?;

    let test_message = b"Test message for BIP-85";

    // Sign with master key
    let master_sig = client
        .sign_message(test_message, SigningAlgorithm::Schnorr, None)
        .await?;
    assert!(!master_sig.signature.is_empty());
    assert!(!master_sig.message_hash.is_empty());
    println!("✓ Message signed with master key");

    // Sign with BIP-85 derived key
    let bip85_options = KeyOptions {
        seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
        private_key_derivation_path: None,
    };
    let bip85_sig = client
        .sign_message(test_message, SigningAlgorithm::Schnorr, Some(bip85_options))
        .await?;
    assert!(!bip85_sig.signature.is_empty());

    // Signatures should be different (different keys)
    assert_ne!(bip85_sig.signature, master_sig.signature);
    // But message hash should be the same
    assert_eq!(bip85_sig.message_hash, master_sig.message_hash);
    println!("✓ Message signed with BIP-85 derived key");

    // Sign with combined BIP-85 + BIP-32
    let combined_options = KeyOptions {
        seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
        private_key_derivation_path: Some(BIP32_PATH.to_string()),
    };
    let combined_sig = client
        .sign_message(
            test_message,
            SigningAlgorithm::Schnorr,
            Some(combined_options),
        )
        .await?;
    assert_ne!(combined_sig.signature, master_sig.signature);
    assert_ne!(combined_sig.signature, bip85_sig.signature);
    assert_eq!(combined_sig.message_hash, master_sig.message_hash);
    println!("✓ Message signed with combined BIP-85 + BIP-32 derived key");

    Ok(())
}

#[tokio::test]
async fn test_bip85_encryption_decryption() -> Result<()> {
    // Load environment variables
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenvy::from_path(env_path).ok();
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
        .register_guest("test_bip85_encrypt".to_string(), client_id)
        .await?;

    let test_data = "This is a secret message for BIP-85 testing";

    // Test cases array matching TypeScript exactly
    struct TestCase {
        name: &'static str,
        options: Option<KeyOptions>,
    }

    let test_cases = vec![
        TestCase {
            name: "Master key",
            options: None,
        },
        TestCase {
            name: "BIP-32 only",
            options: Some(KeyOptions {
                private_key_derivation_path: Some(BIP32_PATH.to_string()),
                seed_phrase_derivation_path: None,
            }),
        },
        TestCase {
            name: "BIP-85 only",
            options: Some(KeyOptions {
                private_key_derivation_path: None,
                seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
            }),
        },
        TestCase {
            name: "Combined BIP-85 + BIP-32",
            options: Some(KeyOptions {
                private_key_derivation_path: Some(BIP32_PATH.to_string()),
                seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
            }),
        },
    ];

    // Run tests for each derivation option (matching TypeScript)
    for test_case in &test_cases {
        // Encrypt with the specified key
        let encrypt_response = client
            .encrypt_data(test_data.to_string(), test_case.options.clone())
            .await?;
        assert!(!encrypt_response.encrypted_data.is_empty());

        // Decrypt with the same key
        let decrypted_data = client
            .decrypt_data(
                encrypt_response.encrypted_data.clone(),
                test_case.options.clone(),
            )
            .await?;
        assert_eq!(decrypted_data, test_data);
        println!("✓ Encrypt/decrypt with {} works", test_case.name);

        // Mixing derivation paths should fail (matching TypeScript test)
        if test_case.options.is_some() {
            let wrong_options = if test_case.name == "BIP-32 only" {
                Some(KeyOptions {
                    private_key_derivation_path: None,
                    seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
                })
            } else {
                Some(KeyOptions {
                    private_key_derivation_path: Some(BIP32_PATH.to_string()),
                    seed_phrase_derivation_path: None,
                })
            };

            if client
                .decrypt_data(encrypt_response.encrypted_data.clone(), wrong_options)
                .await
                .is_ok()
            {
                panic!(
                    "Should not decrypt {} with wrong derivation path",
                    test_case.name
                )
            }
        }
    }

    // Test that different BIP-85 paths produce different encryption results (matching TypeScript)
    let bip85_encryption1 = client
        .encrypt_data(
            test_data.to_string(),
            Some(KeyOptions {
                private_key_derivation_path: None,
                seed_phrase_derivation_path: Some(BIP85_STANDARD_PATH.to_string()),
            }),
        )
        .await?;

    let bip85_encryption2 = client
        .encrypt_data(
            test_data.to_string(),
            Some(KeyOptions {
                private_key_derivation_path: None,
                seed_phrase_derivation_path: Some(BIP85_ALTERNATIVE_PATH.to_string()),
            }),
        )
        .await?;

    assert_ne!(
        bip85_encryption1.encrypted_data,
        bip85_encryption2.encrypted_data
    );
    println!("✓ Different BIP-85 paths produce different encryption results");

    Ok(())
}
