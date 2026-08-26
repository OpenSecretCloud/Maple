use crate::error::{Error, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Nonce},
    ChaCha20Poly1305,
};
use p256::elliptic_curve::rand_core::{OsRng, RngCore};
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey, SharedSecret, StaticSecret};

// Re-export for tests
pub use x25519_dalek::PublicKey;

// Public test utilities
pub struct KeyPair {
    pub secret: StaticSecret,
    pub public: X25519PublicKey,
}

pub fn generate_key_pair() -> KeyPair {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = X25519PublicKey::from(&secret);
    KeyPair { secret, public }
}

pub fn derive_shared_secret(secret: &StaticSecret, their_public: &X25519PublicKey) -> SharedSecret {
    secret.diffie_hellman(their_public)
}

pub fn encrypt_message(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    encrypt_data(key, plaintext)
}

pub fn decrypt_message(ciphertext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    decrypt_data(key, ciphertext)
}

pub fn generate_random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

pub fn generate_ephemeral_keypair() -> (EphemeralSecret, PublicKey) {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret, public)
}

pub fn generate_static_keypair() -> (StaticSecret, PublicKey) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret, public)
}

pub fn perform_key_exchange(secret: EphemeralSecret, their_public: &PublicKey) -> SharedSecret {
    secret.diffie_hellman(their_public)
}

pub fn perform_static_key_exchange(
    secret: &StaticSecret,
    their_public: &PublicKey,
) -> SharedSecret {
    secret.diffie_hellman(their_public)
}

#[allow(deprecated)]
pub fn encrypt_data(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| Error::Crypto(format!("Failed to create cipher: {}", e)))?;

    let nonce_bytes = generate_random_bytes::<12>();
    let nonce = Nonce::<ChaCha20Poly1305>::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| Error::Encryption(format!("Encryption failed: {}", e)))?;

    // Prepend nonce to ciphertext
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

#[allow(deprecated)]
pub fn decrypt_data(key: &[u8; 32], encrypted_data: &[u8]) -> Result<Vec<u8>> {
    if encrypted_data.len() < 12 {
        return Err(Error::Decryption("Encrypted data too short".to_string()));
    }

    let (nonce_bytes, ciphertext) = encrypted_data.split_at(12);
    let nonce = Nonce::<ChaCha20Poly1305>::from_slice(nonce_bytes);

    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| Error::Crypto(format!("Failed to create cipher: {}", e)))?;

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| Error::Decryption(format!("Decryption failed: {}", e)))
}

#[allow(deprecated)]
pub fn decrypt_session_key(shared_secret: &SharedSecret, encrypted_data: &str) -> Result<[u8; 32]> {
    let encrypted = BASE64.decode(encrypted_data)?;

    if encrypted.len() < 12 {
        return Err(Error::Decryption(
            "Encrypted session key too short".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = encrypted.split_at(12);
    let nonce = Nonce::<ChaCha20Poly1305>::from_slice(nonce_bytes);

    let cipher = ChaCha20Poly1305::new_from_slice(shared_secret.as_bytes())
        .map_err(|e| Error::Crypto(format!("Failed to create cipher: {}", e)))?;

    let session_key_vec = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| Error::Decryption(format!("Failed to decrypt session key: {}", e)))?;

    if session_key_vec.len() != 32 {
        return Err(Error::Decryption("Invalid session key length".to_string()));
    }

    let mut session_key = [0u8; 32];
    session_key.copy_from_slice(&session_key_vec);
    Ok(session_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = generate_random_bytes::<32>();
        let plaintext = b"Hello, World!";

        let encrypted = encrypt_data(&key, plaintext).unwrap();
        let decrypted = decrypt_data(&key, &encrypted).unwrap();

        assert_eq!(plaintext.to_vec(), decrypted);
    }

    #[test]
    fn test_key_exchange() {
        // Use static secrets for testing since ephemeral secrets are consumed
        let (alice_secret, alice_public) = generate_static_keypair();
        let (bob_secret, bob_public) = generate_static_keypair();

        let alice_shared = perform_static_key_exchange(&alice_secret, &bob_public);
        let bob_shared = perform_static_key_exchange(&bob_secret, &alice_public);

        assert_eq!(alice_shared.as_bytes(), bob_shared.as_bytes());
    }
}
