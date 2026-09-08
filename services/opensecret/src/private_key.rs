use bip39::Mnemonic;
use bitcoin::{
    bip32::{DerivationPath, Xpriv},
    secp256k1::Secp256k1,
    Network,
};
use secp256k1::SecretKey;
use std::{str::FromStr, sync::Arc};
use tracing::error;

use crate::{
    aws_credentials::AwsCredentialManager, encrypt::generate_random_enclave,
    web::protected_routes::validate_bip85_path, Error,
};

#[cfg(test)]
use crate::encrypt::decrypt_with_key;

// Valid BIP-39 word counts - we only support 12, 18, and 24
pub const VALID_BIP39_WORD_COUNTS: [u32; 3] = [12, 18, 24];

pub async fn generate_twelve_word_seed(
    aws_credential_manager: Arc<tokio::sync::RwLock<Option<AwsCredentialManager>>>,
) -> Result<Mnemonic, Error> {
    // the bip39 library supports 12. 15, 18, 21, and 24 word mnemonics
    // we only support 12 words, which is 16 bytes of entropy
    let random_bytes: [u8; 16] = generate_random_enclave::<16>(aws_credential_manager).await;

    let mnemonic =
        Mnemonic::from_entropy(&random_bytes).map_err(|_| Error::PrivateKeyGenerationFailure)?;
    Ok(mnemonic)
}

#[cfg(test)]
pub fn decrypt_user_seed_to_key(
    enclave_key: Vec<u8>,
    encrypted_seed: Vec<u8>,
    derivation_path: Option<&str>,
    seed_phrase_derivation_path: Option<&str>,
) -> Result<SecretKey, Error> {
    let root_mnemonic = decrypt_user_seed_to_mnemonic(enclave_key, encrypted_seed)?;
    derive_mnemonic_to_key(root_mnemonic, derivation_path, seed_phrase_derivation_path)
}

pub fn plaintext_user_seed_to_mnemonic(plaintext_seed: &[u8]) -> Result<Mnemonic, Error> {
    let decrypted_user_seed_str = std::str::from_utf8(plaintext_seed)
        .map_err(|e| Error::EncryptionError(format!("Failed to decode UTF-8: {}", e)))?;
    let user_mnemonic = Mnemonic::from_str(decrypted_user_seed_str)
        .map_err(|e| Error::EncryptionError(e.to_string()))?;
    Ok(user_mnemonic)
}

pub fn plaintext_user_seed_to_key(
    plaintext_seed: &[u8],
    derivation_path: Option<&str>,
    seed_phrase_derivation_path: Option<&str>,
) -> Result<SecretKey, Error> {
    let root_mnemonic = plaintext_user_seed_to_mnemonic(plaintext_seed)?;
    derive_mnemonic_to_key(root_mnemonic, derivation_path, seed_phrase_derivation_path)
}

fn derive_mnemonic_to_key(
    root_mnemonic: Mnemonic,
    derivation_path: Option<&str>,
    seed_phrase_derivation_path: Option<&str>,
) -> Result<SecretKey, Error> {
    // If seed_phrase_derivation_path is provided, derive a child mnemonic using BIP-85
    let source_mnemonic = if let Some(bip85_path) = seed_phrase_derivation_path {
        derive_bip85_mnemonic_from_root(root_mnemonic, bip85_path)?
    } else {
        root_mnemonic
    };

    // Generate seed from the appropriate mnemonic (either the root or the BIP-85 derived one)
    let seed = source_mnemonic.to_seed("");

    // Create extended private key from the seed
    let xprivkey = Xpriv::new_master(Network::Bitcoin, &seed)
        .map_err(|e| Error::EncryptionError(e.to_string()))?;

    // If a derivation path is provided, derive the child key
    if let Some(path) = derivation_path {
        let path = DerivationPath::from_str(path)
            .map_err(|e| Error::InvalidDerivationPath(e.to_string()))?;
        let derived_key = xprivkey
            .derive_priv(&secp256k1::Secp256k1::new(), &path)
            .map_err(|e| Error::KeyDerivationError(e.to_string()))?;
        Ok(derived_key.private_key)
    } else {
        Ok(xprivkey.private_key)
    }
}

#[cfg(test)]
pub fn decrypt_user_seed_to_mnemonic(
    enclave_key: Vec<u8>,
    encrypted_seed: Vec<u8>,
) -> Result<Mnemonic, Error> {
    let enclave_secret_key =
        SecretKey::from_slice(&enclave_key).map_err(|e| Error::EncryptionError(e.to_string()))?;
    let decrypted_user_seed_bytes = decrypt_with_key(&enclave_secret_key, &encrypted_seed)
        .map_err(|e| Error::EncryptionError(e.to_string()))?;
    let decrypted_user_seed_str = String::from_utf8(decrypted_user_seed_bytes)
        .map_err(|e| Error::EncryptionError(format!("Failed to decode UTF-8: {}", e)))?;
    let user_mnemonic = Mnemonic::from_str(&decrypted_user_seed_str)
        .map_err(|e| Error::EncryptionError(e.to_string()))?;
    Ok(user_mnemonic)
}

/// Derive a BIP-85 child mnemonic from the user's root mnemonic,
/// given a BIP-85 derivation path (e.g. m/83696968'/39'/0'/12'/0').
///
/// # Arguments
/// * `enclave_key` - The key used to decrypt the encrypted seed
/// * `encrypted_seed` - The encrypted seed to decrypt
/// * `bip85_path` - The BIP-85 derivation path in format m/PURPOSE'/APP'/LANGUAGE'/WORDS'/INDEX'
///   where:
///   - PURPOSE' is the BIP-85 purpose value (83696968', must be hardened)
///   - APP' is for BIP-39 mnemonics (39', must be hardened)
///   - LANGUAGE' is 0' for English (must be hardened); other languages are not currently supported
///   - WORDS' must be one of VALID_BIP39_WORD_COUNTS (12', 18', 24', must be hardened)
///   - INDEX' is the derivation index (must be hardened)
///
/// # Returns
/// * `Result<Mnemonic, Error>` - The derived BIP-39 mnemonic or an error
#[cfg(test)]
pub fn decrypt_and_derive_bip85_mnemonic(
    enclave_key: Vec<u8>,
    encrypted_seed: Vec<u8>,
    bip85_path: &str,
) -> Result<Mnemonic, Error> {
    // 1. Validate BIP-85 path format
    // Convert ApiError to our Error type
    validate_bip85_path(bip85_path).map_err(|_| {
        error!("BIP-85 path validation failed: {}", bip85_path);
        Error::InvalidDerivationPath(format!("Invalid BIP-85 path format: {}", bip85_path))
    })?;

    // 2. Decrypt user root mnemonic
    let root_mnemonic = decrypt_user_seed_to_mnemonic(enclave_key, encrypted_seed)?;
    derive_bip85_mnemonic_from_root(root_mnemonic, bip85_path)
}

pub fn derive_bip85_mnemonic_from_root(
    root_mnemonic: Mnemonic,
    bip85_path: &str,
) -> Result<Mnemonic, Error> {
    // 1. Validate BIP-85 path format
    // Convert ApiError to our Error type
    validate_bip85_path(bip85_path).map_err(|_| {
        error!("BIP-85 path validation failed: {}", bip85_path);
        Error::InvalidDerivationPath(format!("Invalid BIP-85 path format: {}", bip85_path))
    })?;

    let root_seed = root_mnemonic.to_seed("");

    // 2. Convert root_seed to Xpriv
    let secp = Secp256k1::new();
    let xpriv = Xpriv::new_master(Network::Bitcoin, &root_seed).map_err(|e| {
        error!("Failed to create master key from seed: {}", e);
        Error::EncryptionError(e.to_string())
    })?;

    // 3. Parse the BIP-85 path to extract required parameters
    // Safe to unwrap these values since we already validated the path
    let segments: Vec<&str> = bip85_path.split('/').collect();

    // Extract word count (segment 4)
    let word_count_segment = segments[4].trim_end_matches(&['\'', 'h'][..]);
    let word_count = word_count_segment.parse::<u32>().map_err(|_| {
        error!("Failed to parse word count: {}", word_count_segment);
        Error::InvalidDerivationPath(format!("Invalid word count: {}", word_count_segment))
    })?;

    // Validate word count is one of the allowed values
    if !VALID_BIP39_WORD_COUNTS.contains(&word_count) {
        error!(
            "Invalid word count: {}, must be one of: {:?}",
            word_count, VALID_BIP39_WORD_COUNTS
        );
        return Err(Error::InvalidDerivationPath(format!(
            "Word count must be one of {:?}, got: {}",
            VALID_BIP39_WORD_COUNTS, word_count
        )));
    }

    // Extract derivation index (segment 5)
    let index_segment = segments[5].trim_end_matches(&['\'', 'h'][..]);
    let index = index_segment.parse::<u32>().map_err(|_| {
        error!("Failed to parse index: {}", index_segment);
        Error::InvalidDerivationPath(format!("Invalid index: {}", index_segment))
    })?;

    // 4. Use the bip85_extended crate to derive the mnemonic
    let bip85_mnemonic_result =
        bip85_extended::mnemonic::to_mnemonic(&secp, &xpriv, word_count, index);

    // 5. Return the derived mnemonic or convert error
    match bip85_mnemonic_result {
        Ok(derived_mnemonic) => Ok(derived_mnemonic),
        Err(e) => {
            error!("BIP-85 derivation failed: {}, path: {}", e, bip85_path);
            Err(Error::KeyDerivationError(format!(
                "BIP-85 derivation error: {}. Path: {}",
                e, bip85_path
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encrypt::encrypt_with_key;
    use secp256k1::SecretKey;
    use std::str::FromStr;

    // A basic test mnemonic for testing purposes
    const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    // BIP-32 test derivation path
    const TEST_BIP32_PATH: &str = "m/44'/0'/0'/0/0";

    // We're not testing against specific BIP-85 test vectors since we're only focusing on
    // the parameter extraction and validation logic, not the actual derivation results

    // Helper to create a mock encrypted seed for testing
    async fn create_mock_encrypted_seed(mnemonic: &str) -> (Vec<u8>, Vec<u8>) {
        // Create a deterministic key for testing purposes
        let key_bytes = [1u8; 32];
        let enclave_key = key_bytes.to_vec();
        let secret_key = SecretKey::from_slice(&key_bytes).unwrap();

        // Encrypt the mnemonic
        let encrypted_seed = encrypt_with_key(&secret_key, mnemonic.as_bytes()).await;

        (enclave_key, encrypted_seed)
    }

    #[tokio::test]
    async fn legacy_seed_ciphertext_copy_derives_original_user_key() {
        let (enclave_key, victim_encrypted_seed) = create_mock_encrypted_seed(TEST_MNEMONIC).await;

        let victim_key = decrypt_user_seed_to_key(
            enclave_key.clone(),
            victim_encrypted_seed.clone(),
            None,
            None,
        )
        .expect("victim seed should decrypt with the enclave root key");

        // This is the current threat model issue from the investigation:
        // the decrypt helper only sees root-key ciphertext. It receives no
        // authenticated user/credential binding, so a copied victim seed blob
        // derives the same key in any logical account context.
        let attacker_row_copied_seed = victim_encrypted_seed;
        let attacker_session_key =
            decrypt_user_seed_to_key(enclave_key, attacker_row_copied_seed, None, None)
                .expect("legacy copied seed decrypts because no AAD binding exists");

        assert_eq!(
            victim_key.display_secret().to_string(),
            attacker_session_key.display_secret().to_string()
        );
    }

    #[tokio::test]
    async fn plaintext_seed_key_derivation_matches_legacy_decrypt_path() {
        let (enclave_key, encrypted_seed) = create_mock_encrypted_seed(TEST_MNEMONIC).await;

        let legacy_key =
            decrypt_user_seed_to_key(enclave_key, encrypted_seed, Some(TEST_BIP32_PATH), None)
                .expect("legacy encrypted seed path should derive");
        let plaintext_key =
            plaintext_user_seed_to_key(TEST_MNEMONIC.as_bytes(), Some(TEST_BIP32_PATH), None)
                .expect("plaintext seed path should derive");

        assert_eq!(
            legacy_key.display_secret().to_string(),
            plaintext_key.display_secret().to_string()
        );
    }

    #[test]
    fn test_bip85_integration_with_bip85_extended_crate() {
        // This test verifies that the bip85_extended crate works as expected
        // This is not testing our code directly, but the library we're using
        let test_mnemonic = Mnemonic::from_str(TEST_MNEMONIC).unwrap();
        let seed = test_mnemonic.to_seed("");

        // Create master key from the seed
        let xpriv = Xpriv::new_master(Network::Bitcoin, &seed).unwrap();
        let secp = Secp256k1::new();

        // Test for 12-word mnemonic derivation
        let derived_12 = bip85_extended::mnemonic::to_mnemonic(&secp, &xpriv, 12, 0).unwrap();

        // Just check that we got a valid 12-word mnemonic back
        assert_eq!(derived_12.word_count(), 12, "Should get 12-word mnemonic");

        // Test for 18-word mnemonic derivation
        let derived_18 = bip85_extended::mnemonic::to_mnemonic(&secp, &xpriv, 18, 0).unwrap();
        assert_eq!(derived_18.word_count(), 18, "Should get 18-word mnemonic");

        // Test for 24-word mnemonic derivation
        let derived_24 = bip85_extended::mnemonic::to_mnemonic(&secp, &xpriv, 24, 0).unwrap();
        assert_eq!(derived_24.word_count(), 24, "Should get 24-word mnemonic");
    }

    #[tokio::test]
    async fn test_bip85_word_count_derivation() {
        // Test that our decrypt_and_derive_bip85_mnemonic function creates mnemonics
        // with the correct word count
        let (enclave_key, encrypted_seed) = create_mock_encrypted_seed(TEST_MNEMONIC).await;

        // Test 12-word derivation
        let derived_12 = decrypt_and_derive_bip85_mnemonic(
            enclave_key.clone(),
            encrypted_seed.clone(),
            "m/83696968'/39'/0'/12'/0'",
        )
        .unwrap();
        assert_eq!(
            derived_12.word_count(),
            12,
            "Should derive 12-word mnemonic"
        );

        // Test 18-word derivation
        let derived_18 = decrypt_and_derive_bip85_mnemonic(
            enclave_key.clone(),
            encrypted_seed.clone(),
            "m/83696968'/39'/0'/18'/0'",
        )
        .unwrap();
        assert_eq!(
            derived_18.word_count(),
            18,
            "Should derive 18-word mnemonic"
        );

        // Test 24-word derivation
        let derived_24 = decrypt_and_derive_bip85_mnemonic(
            enclave_key.clone(),
            encrypted_seed.clone(),
            "m/83696968'/39'/0'/24'/0'",
        )
        .unwrap();
        assert_eq!(
            derived_24.word_count(),
            24,
            "Should derive 24-word mnemonic"
        );
    }

    #[tokio::test]
    async fn test_bip85_derivation_with_index() {
        // Test derivation with different indices
        let (enclave_key, encrypted_seed) = create_mock_encrypted_seed(TEST_MNEMONIC).await;

        // Derive with index 0
        let derived_index0 = decrypt_and_derive_bip85_mnemonic(
            enclave_key.clone(),
            encrypted_seed.clone(),
            "m/83696968'/39'/0'/12'/0'",
        )
        .unwrap();

        // Derive with index 1 - should be different
        let derived_index1 = decrypt_and_derive_bip85_mnemonic(
            enclave_key.clone(),
            encrypted_seed.clone(),
            "m/83696968'/39'/0'/12'/1'",
        )
        .unwrap();

        // The two mnemonics should be different
        assert_ne!(
            derived_index0.to_string(),
            derived_index1.to_string(),
            "Mnemonics with different indices should be different"
        );
    }

    #[tokio::test]
    async fn test_bip85_parameter_validation() {
        // Test that parameter validation works correctly
        let (enclave_key, encrypted_seed) = create_mock_encrypted_seed(TEST_MNEMONIC).await;

        // Test valid word counts (only 12, 18, and 24 are supported)
        for word_count in [12, 18, 24] {
            let path = format!("m/83696968'/39'/0'/{}'/0'", word_count);
            let result = decrypt_and_derive_bip85_mnemonic(
                enclave_key.clone(),
                encrypted_seed.clone(),
                &path,
            );
            assert!(
                result.is_ok(),
                "Valid word count {} should work",
                word_count
            );
        }

        // Test invalid word counts
        for invalid_count in [13, 15, 21] {
            let path = format!("m/83696968'/39'/0'/{}'/0'", invalid_count);
            let result = decrypt_and_derive_bip85_mnemonic(
                enclave_key.clone(),
                encrypted_seed.clone(),
                &path,
            );
            assert!(
                result.is_err(),
                "Invalid word count {} should fail",
                invalid_count
            );
        }

        // Test invalid path format
        let invalid_paths = [
            "m/83696968/39'/0'/12'/0'",    // Purpose not hardened
            "m/83696968'/39/0'/12'/0'",    // Application not hardened
            "m/83696968'/39'/1'/12'/0'",   // Unsupported non-English language
            "m/44'/0'/0'/0/0",             // Not a BIP-85 path
            "m/83696968'/39'/0'/12'/0'/0", // Extra segment
        ];

        for path in invalid_paths {
            let result = decrypt_and_derive_bip85_mnemonic(
                enclave_key.clone(),
                encrypted_seed.clone(),
                path,
            );
            assert!(result.is_err(), "Invalid path {} should fail", path);
        }
    }

    #[tokio::test]
    async fn test_combined_bip85_and_bip32_derivation() {
        // This test verifies that a BIP-85 derived seed can be further derived with BIP-32
        let (enclave_key, encrypted_seed) = create_mock_encrypted_seed(TEST_MNEMONIC).await;

        // First, derive a child mnemonic using BIP-85
        let bip85_path = "m/83696968'/39'/0'/12'/42'";
        let seed_phrase_derivation_path = Some(bip85_path);
        let bip32_derivation_path = Some(TEST_BIP32_PATH);

        // 1. Get key using both derivation methods
        let derived_key = decrypt_user_seed_to_key(
            enclave_key.clone(),
            encrypted_seed.clone(),
            bip32_derivation_path,
            seed_phrase_derivation_path,
        )
        .expect("Combined derivation should succeed");

        // 2. Get key using only BIP-85 (no BIP-32 path)
        let bip85_only_key = decrypt_user_seed_to_key(
            enclave_key.clone(),
            encrypted_seed.clone(),
            None,
            seed_phrase_derivation_path,
        )
        .expect("BIP-85 only derivation should succeed");

        // 3. Get key using only BIP-32 on the original seed (no BIP-85)
        let bip32_only_key = decrypt_user_seed_to_key(
            enclave_key.clone(),
            encrypted_seed.clone(),
            bip32_derivation_path,
            None,
        )
        .expect("BIP-32 only derivation should succeed");

        // All three keys should be different
        assert_ne!(
            derived_key.display_secret().to_string(),
            bip85_only_key.display_secret().to_string(),
            "Combined derivation should yield different key than BIP-85 only"
        );

        assert_ne!(
            derived_key.display_secret().to_string(),
            bip32_only_key.display_secret().to_string(),
            "Combined derivation should yield different key than BIP-32 only"
        );
    }

    #[tokio::test]
    async fn test_multiple_bip85_and_bip32_paths() {
        // This test verifies combinations of different BIP-85 and BIP-32 paths
        let (enclave_key, encrypted_seed) = create_mock_encrypted_seed(TEST_MNEMONIC).await;

        // Define a matrix of test paths
        let bip85_paths = [
            "m/83696968'/39'/0'/12'/0'",
            "m/83696968'/39'/0'/12'/1'",
            "m/83696968'/39'/0'/24'/0'",
        ];

        let bip32_paths = ["m/44'/0'/0'/0/0", "m/84'/0'/0'/0/0", "m/49'/0'/0'/0/0"];

        // Set to collect unique keys
        let mut derived_keys = std::collections::HashSet::new();

        // Test all combinations
        for bip85_path in bip85_paths.iter() {
            for bip32_path in bip32_paths.iter() {
                let derived_key = decrypt_user_seed_to_key(
                    enclave_key.clone(),
                    encrypted_seed.clone(),
                    Some(bip32_path),
                    Some(bip85_path),
                )
                .unwrap_or_else(|_| {
                    panic!(
                        "Derivation should succeed for paths: BIP-85 = {}, BIP-32 = {}",
                        bip85_path, bip32_path
                    )
                });

                // Add the key to our set of keys
                let key_string = derived_key.display_secret().to_string();
                derived_keys.insert(key_string);
            }
        }

        // All keys should be different - we should have bip85_paths.len() * bip32_paths.len() unique keys
        assert_eq!(
            derived_keys.len(),
            bip85_paths.len() * bip32_paths.len(),
            "All combinations of BIP-85 and BIP-32 paths should yield unique keys"
        );
    }
}
