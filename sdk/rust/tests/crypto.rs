use base64::Engine;
use opensecret::crypto::{
    decrypt_message, decrypt_session_key, derive_shared_secret, encrypt_message, generate_key_pair,
};
use opensecret::Result;

#[test]
fn test_key_generation() {
    let keypair = generate_key_pair();

    // Public key should be 32 bytes
    assert_eq!(keypair.public.as_bytes().len(), 32);

    // Secret key should be 32 bytes
    assert_eq!(keypair.secret.as_bytes().len(), 32);

    // Keys should be different
    assert_ne!(keypair.public.as_bytes(), keypair.secret.as_bytes());
}

#[test]
fn test_ecdh_key_exchange() {
    let alice = generate_key_pair();
    let bob = generate_key_pair();

    // Derive shared secrets
    let alice_shared = derive_shared_secret(&alice.secret, &bob.public);
    let bob_shared = derive_shared_secret(&bob.secret, &alice.public);

    // Both should derive the same shared secret
    assert_eq!(alice_shared.as_bytes(), bob_shared.as_bytes());

    // Shared secret should be 32 bytes
    assert_eq!(alice_shared.as_bytes().len(), 32);
}

#[test]
fn test_encryption_decryption() -> Result<()> {
    let key = [42u8; 32]; // Test key
    let plaintext = b"Hello, OpenSecret!";

    // Encrypt
    let ciphertext = encrypt_message(plaintext, &key)?;

    // Ciphertext should be different from plaintext
    assert_ne!(&ciphertext[..], plaintext);

    // Ciphertext should be longer (includes nonce and tag)
    assert!(ciphertext.len() > plaintext.len());

    // Decrypt
    let decrypted = decrypt_message(&ciphertext, &key)?;

    // Should recover original plaintext
    assert_eq!(decrypted, plaintext);

    Ok(())
}

#[test]
fn test_encryption_with_different_nonces() -> Result<()> {
    let key = [42u8; 32];
    let plaintext = b"Test message";

    // Encrypt the same message twice
    let ciphertext1 = encrypt_message(plaintext, &key)?;
    let ciphertext2 = encrypt_message(plaintext, &key)?;

    // Ciphertexts should be different (different nonces)
    assert_ne!(ciphertext1, ciphertext2);

    // But both should decrypt to the same plaintext
    let decrypted1 = decrypt_message(&ciphertext1, &key)?;
    let decrypted2 = decrypt_message(&ciphertext2, &key)?;

    assert_eq!(decrypted1, plaintext);
    assert_eq!(decrypted2, plaintext);

    Ok(())
}

#[test]
fn test_decryption_with_wrong_key_fails() {
    let key1 = [1u8; 32];
    let key2 = [2u8; 32];
    let plaintext = b"Secret message";

    // Encrypt with key1
    let ciphertext = encrypt_message(plaintext, &key1).unwrap();

    // Try to decrypt with key2 - should fail
    let result = decrypt_message(&ciphertext, &key2);

    assert!(result.is_err());
}

#[test]
fn test_session_key_decryption() -> Result<()> {
    // Simulate server and client key exchange
    let client_keypair = generate_key_pair();
    let server_keypair = generate_key_pair();

    // Derive shared secret (what the server would do)
    let shared_secret = derive_shared_secret(&server_keypair.secret, &client_keypair.public);

    // Server creates a session key
    let session_key = [99u8; 32];

    // Server encrypts the session key with the shared secret
    let encrypted_session_key = encrypt_message(&session_key, shared_secret.as_bytes())?;
    let encrypted_b64 = base64::engine::general_purpose::STANDARD.encode(&encrypted_session_key);

    // Client decrypts the session key
    let decrypted_key = decrypt_session_key(&shared_secret, &encrypted_b64)?;

    assert_eq!(decrypted_key, session_key);

    Ok(())
}

#[test]
fn test_invalid_base64_session_key() {
    let shared_secret =
        derive_shared_secret(&generate_key_pair().secret, &generate_key_pair().public);

    let result = decrypt_session_key(&shared_secret, "not-valid-base64!");
    assert!(result.is_err());
}

#[test]
fn test_corrupted_ciphertext() {
    let key = [42u8; 32];
    let plaintext = b"Test";

    let mut ciphertext = encrypt_message(plaintext, &key).unwrap();

    // Corrupt the ciphertext
    if let Some(byte) = ciphertext.last_mut() {
        *byte = byte.wrapping_add(1);
    }

    // Decryption should fail
    let result = decrypt_message(&ciphertext, &key);
    assert!(result.is_err());
}
