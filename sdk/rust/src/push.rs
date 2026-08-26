use crate::error::{Error, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hkdf::Hkdf;
use p256::{
    ecdh::diffie_hellman,
    pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey},
    PublicKey, SecretKey,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

pub const PUSH_NOTIFICATION_KEY_ALGORITHM: &str = "p256_ecdh_v1";
pub const PUSH_NOTIFICATION_ENVELOPE_ALGORITHM: &str = "p256-hkdf-sha256-aes256gcm";
const PUSH_NOTIFICATION_PAYLOAD_VERSION: i32 = 1;
const PUSH_NOTIFICATION_PREVIEW_INFO: &[u8] = b"opensecret-push-preview-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedPushPreviewEnvelope {
    #[serde(alias = "v")]
    pub enc_v: i32,
    pub alg: String,
    pub kid: String,
    pub epk: String,
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushNotificationPreviewPayload {
    pub v: i32,
    pub notification_id: Uuid,
    pub message_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub deep_link: String,
    pub thread_id: String,
    pub sent_at: i64,
}

pub struct PushNotificationKeyPair {
    private_key: SecretKey,
}

impl PushNotificationKeyPair {
    pub fn generate() -> Self {
        Self {
            private_key: SecretKey::random(&mut p256::elliptic_curve::rand_core::OsRng),
        }
    }

    pub fn from_pkcs8_der(private_key_der: &[u8]) -> Result<Self> {
        let private_key = SecretKey::from_pkcs8_der(private_key_der).map_err(|e| {
            Error::Configuration(format!("Invalid push notification private key: {e}"))
        })?;

        Ok(Self { private_key })
    }

    pub fn from_pkcs8_base64(private_key_base64: &str) -> Result<Self> {
        let private_key_der = BASE64.decode(private_key_base64)?;
        Self::from_pkcs8_der(&private_key_der)
    }

    pub fn private_key_pkcs8_der(&self) -> Result<Vec<u8>> {
        Ok(self
            .private_key
            .to_pkcs8_der()
            .map_err(|e| {
                Error::Configuration(format!(
                    "Failed to export push notification private key: {e}"
                ))
            })?
            .as_bytes()
            .to_vec())
    }

    pub fn private_key_pkcs8_base64(&self) -> Result<String> {
        Ok(BASE64.encode(self.private_key_pkcs8_der()?))
    }

    pub fn public_key_spki_der(&self) -> Result<Vec<u8>> {
        Ok(self
            .private_key
            .public_key()
            .to_public_key_der()
            .map_err(|e| {
                Error::Configuration(format!(
                    "Failed to export push notification public key: {e}"
                ))
            })?
            .as_bytes()
            .to_vec())
    }

    pub fn public_key_spki_base64(&self) -> Result<String> {
        Ok(BASE64.encode(self.public_key_spki_der()?))
    }

    pub fn decrypt_preview_envelope(
        &self,
        envelope: &EncryptedPushPreviewEnvelope,
    ) -> Result<PushNotificationPreviewPayload> {
        if envelope.enc_v != PUSH_NOTIFICATION_PAYLOAD_VERSION {
            return Err(Error::InvalidResponse(format!(
                "Unsupported push preview version: {}",
                envelope.enc_v
            )));
        }

        if envelope.alg != PUSH_NOTIFICATION_ENVELOPE_ALGORITHM {
            return Err(Error::InvalidResponse(format!(
                "Unsupported push preview algorithm: {}",
                envelope.alg
            )));
        }

        let epk_bytes = BASE64.decode(&envelope.epk)?;
        let salt = BASE64.decode(&envelope.salt)?;
        let nonce_bytes = BASE64.decode(&envelope.nonce)?;
        let ciphertext = BASE64.decode(&envelope.ciphertext)?;

        if salt.len() != 32 {
            return Err(Error::InvalidResponse(format!(
                "Invalid push preview salt length: {}",
                salt.len()
            )));
        }

        let nonce_bytes: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| Error::InvalidResponse("Invalid push preview nonce length".to_string()))?;

        let ephemeral_public = PublicKey::from_sec1_bytes(&epk_bytes)
            .map_err(|e| Error::Decryption(format!("Invalid push preview ephemeral key: {e}")))?;
        let shared_secret = diffie_hellman(
            self.private_key.to_nonzero_scalar(),
            ephemeral_public.as_affine(),
        );

        let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared_secret.raw_secret_bytes().as_ref());
        let mut key = [0_u8; 32];
        hkdf.expand(PUSH_NOTIFICATION_PREVIEW_INFO, &mut key)
            .map_err(|_| Error::Crypto("Failed to derive push preview key".to_string()))?;

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| Error::Crypto(format!("Failed to create push preview cipher: {e}")))?;
        let nonce = Nonce::from(nonce_bytes);
        let plaintext = cipher.decrypt(&nonce, ciphertext.as_ref()).map_err(|e| {
            Error::Decryption(format!("Failed to decrypt push preview payload: {e}"))
        })?;

        let payload: PushNotificationPreviewPayload = serde_json::from_slice(&plaintext)
            .map_err(|e| Error::InvalidResponse(format!("Invalid push preview payload: {e}")))?;

        if payload.v != PUSH_NOTIFICATION_PAYLOAD_VERSION {
            return Err(Error::InvalidResponse(format!(
                "Unsupported push preview payload version: {}",
                payload.v
            )));
        }

        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::generate_random_bytes;
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use p256::{
        ecdh::EphemeralSecret,
        elliptic_curve::{rand_core::OsRng, sec1::ToEncodedPoint},
        pkcs8::DecodePublicKey,
        PublicKey,
    };

    fn encrypt_preview_payload_for_test(
        recipient_public_key_der: &[u8],
        payload: &PushNotificationPreviewPayload,
    ) -> EncryptedPushPreviewEnvelope {
        let recipient_key = PublicKey::from_public_key_der(recipient_public_key_der).unwrap();
        let ephemeral_secret = EphemeralSecret::random(&mut OsRng);
        let ephemeral_public = PublicKey::from(&ephemeral_secret);
        let shared_secret = ephemeral_secret.diffie_hellman(&recipient_key);

        let salt = generate_random_bytes::<32>();
        let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared_secret.raw_secret_bytes().as_ref());
        let mut key = [0_u8; 32];
        hkdf.expand(PUSH_NOTIFICATION_PREVIEW_INFO, &mut key)
            .unwrap();

        let nonce_bytes = generate_random_bytes::<12>();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce = Nonce::from(nonce_bytes);
        let plaintext = serde_json::to_vec(payload).unwrap();
        let ciphertext = cipher.encrypt(&nonce, plaintext.as_ref()).unwrap();

        EncryptedPushPreviewEnvelope {
            enc_v: 1,
            alg: PUSH_NOTIFICATION_ENVELOPE_ALGORITHM.to_string(),
            kid: Uuid::new_v4().to_string(),
            epk: BASE64.encode(ephemeral_public.to_encoded_point(false).as_bytes()),
            salt: BASE64.encode(salt),
            nonce: BASE64.encode(nonce_bytes),
            ciphertext: BASE64.encode(ciphertext),
        }
    }

    #[test]
    fn push_keypair_pkcs8_round_trips() {
        let key_pair = PushNotificationKeyPair::generate();
        let pkcs8_base64 = key_pair.private_key_pkcs8_base64().unwrap();

        let restored = PushNotificationKeyPair::from_pkcs8_base64(&pkcs8_base64).unwrap();

        assert_eq!(
            key_pair.public_key_spki_base64().unwrap(),
            restored.public_key_spki_base64().unwrap()
        );
    }

    #[test]
    fn decrypt_preview_envelope_round_trips() {
        let key_pair = PushNotificationKeyPair::generate();
        let payload = PushNotificationPreviewPayload {
            v: PUSH_NOTIFICATION_PAYLOAD_VERSION,
            notification_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            kind: "agent.message".to_string(),
            title: "New Maple message".to_string(),
            body: "Open Maple to view your encrypted message".to_string(),
            deep_link: "opensecret://agent/subagent/123".to_string(),
            thread_id: "agent:subagent:123".to_string(),
            sent_at: 1_772_800_000,
        };

        let envelope =
            encrypt_preview_payload_for_test(&key_pair.public_key_spki_der().unwrap(), &payload);
        let decrypted = key_pair.decrypt_preview_envelope(&envelope).unwrap();

        assert_eq!(decrypted, payload);
    }

    #[test]
    fn decrypt_preview_envelope_rejects_unknown_algorithm() {
        let key_pair = PushNotificationKeyPair::generate();
        let payload = PushNotificationPreviewPayload {
            v: PUSH_NOTIFICATION_PAYLOAD_VERSION,
            notification_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            kind: "agent.message".to_string(),
            title: "New Maple message".to_string(),
            body: "Open Maple to view your encrypted message".to_string(),
            deep_link: "opensecret://agent".to_string(),
            thread_id: "agent:main".to_string(),
            sent_at: 1_772_800_000,
        };

        let mut envelope =
            encrypt_preview_payload_for_test(&key_pair.public_key_spki_der().unwrap(), &payload);
        envelope.alg = "not-supported".to_string();

        let error = key_pair.decrypt_preview_envelope(&envelope).unwrap_err();
        assert!(
            matches!(error, Error::InvalidResponse(message) if message.contains("Unsupported push preview algorithm"))
        );
    }

    #[test]
    fn decrypt_preview_envelope_rejects_unknown_payload_version() {
        let key_pair = PushNotificationKeyPair::generate();
        let payload = PushNotificationPreviewPayload {
            v: 2,
            notification_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            kind: "agent.message".to_string(),
            title: "New Maple message".to_string(),
            body: "Open Maple to view your encrypted message".to_string(),
            deep_link: "opensecret://agent".to_string(),
            thread_id: "agent:main".to_string(),
            sent_at: 1_772_800_000,
        };

        let envelope =
            encrypt_preview_payload_for_test(&key_pair.public_key_spki_der().unwrap(), &payload);

        let error = key_pair.decrypt_preview_envelope(&envelope).unwrap_err();
        assert!(
            matches!(error, Error::InvalidResponse(message) if message.contains("Unsupported push preview payload version"))
        );
    }
}
