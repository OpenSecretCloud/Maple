use crate::push::{
    EncryptedPushPreviewEnvelope, PushNotificationKeyPair, PushNotificationPreviewPayload,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Deserialize;

// Frozen before migrating from p256 0.13.2, using Node's independent crypto
// implementation. All key material is deterministic, public and test-only.
// This catches changes to persisted key imports and the existing push wire
// format that a roundtrip between two copies of the new implementation misses.
#[derive(Deserialize)]
struct LegacyPushFixture {
    private_key_pkcs8_base64: String,
    public_key_spki_base64: String,
    envelope: EncryptedPushPreviewEnvelope,
    payload: PushNotificationPreviewPayload,
}

fn legacy_fixture() -> LegacyPushFixture {
    serde_json::from_str(include_str!("fixtures/legacy-p256-push-v1.json"))
        .expect("valid frozen push compatibility fixture")
}

#[test]
fn legacy_private_key_preserves_public_identity() {
    let fixture = legacy_fixture();
    let key = PushNotificationKeyPair::from_pkcs8_base64(&fixture.private_key_pkcs8_base64)
        .expect("legacy PKCS#8 key remains importable");

    assert_eq!(
        key.public_key_spki_base64().unwrap(),
        fixture.public_key_spki_base64
    );

    // Re-saving an imported key must preserve the same registered public key,
    // without requiring any particular equivalent PKCS#8 encoding internally.
    let restored = PushNotificationKeyPair::from_pkcs8_der(&key.private_key_pkcs8_der().unwrap())
        .expect("re-exported PKCS#8 key remains importable");
    assert_eq!(
        restored.public_key_spki_base64().unwrap(),
        fixture.public_key_spki_base64
    );
}

#[test]
fn legacy_encrypted_preview_remains_decryptable() {
    let fixture = legacy_fixture();
    let key =
        PushNotificationKeyPair::from_pkcs8_base64(&fixture.private_key_pkcs8_base64).unwrap();

    assert_eq!(
        key.decrypt_preview_envelope(&fixture.envelope).unwrap(),
        fixture.payload
    );
}

#[test]
fn legacy_encrypted_preview_rejects_modified_authentication_tag() {
    let mut fixture = legacy_fixture();
    let key =
        PushNotificationKeyPair::from_pkcs8_base64(&fixture.private_key_pkcs8_base64).unwrap();
    let mut ciphertext = BASE64.decode(&fixture.envelope.ciphertext).unwrap();
    *ciphertext.last_mut().expect("fixture includes an AEAD tag") ^= 1;
    fixture.envelope.ciphertext = BASE64.encode(ciphertext);

    assert!(key.decrypt_preview_envelope(&fixture.envelope).is_err());
}
