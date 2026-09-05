//! Direction-separated cryptographic primitives for transport v2.

use std::fmt;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use hkdf::Hkdf;
use p256::elliptic_curve::rand_core::{OsRng, RngCore};
use sha2::Sha256;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::{
    envelope::{check_limit, RequestId},
    Result, TransportV2Error,
};

pub(super) const KEY_LEN: usize = 32;
pub(super) const RECORD_NONCE_LEN: usize = 12;
const RECORD_TAG_LEN: usize = 16;
pub(super) const MIN_RECORD_LEN: usize = RECORD_NONCE_LEN + RECORD_TAG_LEN;

const HANDSHAKE_PAYLOAD_VERSION: u8 = 2;
pub(super) const HANDSHAKE_PAYLOAD_LEN: usize = 1 + 16 + KEY_LEN + 8;
pub(super) const HANDSHAKE_RECORD_LEN: usize = MIN_RECORD_LEN + HANDSHAKE_PAYLOAD_LEN;

const HANDSHAKE_KEY_INFO: &[u8] = b"opensecret/transport-v2/handshake-key";
const REQUEST_KEY_INFO: &[u8] = b"opensecret/transport-v2/client-request";
const RESPONSE_KEY_INFO: &[u8] = b"opensecret/transport-v2/enclave-response";

const KEY_EXCHANGE_AAD: &[u8] = b"opensecret/transport-v2/key-exchange";
const REQUEST_RECORD_AAD: &[u8] = b"opensecret/transport-v2/request-record";
const UNARY_RESPONSE_RECORD_AAD: &[u8] = b"opensecret/transport-v2/unary-response-record";
const STREAM_RESPONSE_RECORD_AAD: &[u8] = b"opensecret/transport-v2/stream-response-record";

#[derive(Zeroize, ZeroizeOnDrop)]
pub(super) struct SessionMaster([u8; KEY_LEN]);

impl SessionMaster {
    #[cfg(test)]
    pub(super) const fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != KEY_LEN {
            return Err(TransportV2Error::InvalidKeyExchange);
        }
        let mut master = Self([0; KEY_LEN]);
        master.0.copy_from_slice(bytes);
        Ok(master)
    }

    fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl fmt::Debug for SessionMaster {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionMaster([REDACTED])")
    }
}

pub(super) struct DecryptedHandshakePayload {
    pub(super) session_id: Uuid,
    pub(super) session_master: SessionMaster,
    pub(super) expires_at_unix_seconds: u64,
}

impl fmt::Debug for DecryptedHandshakePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecryptedHandshakePayload")
            .field("session_id", &self.session_id)
            .field("session_master", &"[REDACTED]")
            .field("expires_at_unix_seconds", &self.expires_at_unix_seconds)
            .finish()
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct RecordKey([u8; KEY_LEN]);

impl RecordKey {
    fn derive(input_key_material: &[u8], info: &[u8]) -> Result<Self> {
        let hkdf = Hkdf::<Sha256>::new(None, input_key_material);
        let mut key = Self([0; KEY_LEN]);
        hkdf.expand(info, &mut key.0)
            .map_err(|_| TransportV2Error::KeyDerivationFailed)?;
        Ok(key)
    }

    fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0_u8; RECORD_NONCE_LEN];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| TransportV2Error::RandomnessUnavailable)?;
        self.encrypt_with_nonce(plaintext, aad, nonce)
    }

    fn encrypt_with_nonce(
        &self,
        plaintext: &[u8],
        aad: &[u8],
        nonce: [u8; RECORD_NONCE_LEN],
    ) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(&self.0)
            .map_err(|_| TransportV2Error::EncryptionFailed)?;
        let nonce = Nonce::from(nonce);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| TransportV2Error::EncryptionFailed)?;

        let mut record = Vec::with_capacity(RECORD_NONCE_LEN + ciphertext.len());
        record.extend_from_slice(&nonce);
        record.extend_from_slice(&ciphertext);
        Ok(record)
    }

    fn decrypt(&self, record: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        if record.len() < MIN_RECORD_LEN {
            return Err(TransportV2Error::RecordTooShort);
        }

        let (nonce, ciphertext) = record.split_at(RECORD_NONCE_LEN);
        let nonce = Nonce::from(
            <[u8; RECORD_NONCE_LEN]>::try_from(nonce)
                .map_err(|_| TransportV2Error::RecordTooShort)?,
        );
        let cipher = ChaCha20Poly1305::new_from_slice(&self.0)
            .map_err(|_| TransportV2Error::AuthenticationFailed)?;
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| TransportV2Error::AuthenticationFailed)
    }
}

/// Direction-separated request and response keys for one v2 session.
#[derive(Zeroize, ZeroizeOnDrop)]
pub(super) struct DirectionalKeys {
    request: RecordKey,
    response: RecordKey,
}

impl DirectionalKeys {
    pub(super) fn derive(session_master: &SessionMaster) -> Result<Self> {
        Ok(Self {
            request: RecordKey::derive(session_master.as_bytes(), REQUEST_KEY_INFO)?,
            response: RecordKey::derive(session_master.as_bytes(), RESPONSE_KEY_INFO)?,
        })
    }

    pub(super) fn encrypt_request_record(
        &self,
        session_id: &Uuid,
        plaintext: &[u8],
    ) -> Result<Vec<u8>> {
        self.request
            .encrypt(plaintext, &request_record_aad(session_id))
    }

    pub(super) fn decrypt_unary_response_record(
        &self,
        session_id: &Uuid,
        request_id: &RequestId,
        record: &[u8],
    ) -> Result<Vec<u8>> {
        self.response
            .decrypt(record, &unary_response_record_aad(session_id, request_id))
    }

    pub(super) fn decrypt_stream_response_record(
        &self,
        session_id: &Uuid,
        request_id: &RequestId,
        sequence: u64,
        record: &[u8],
    ) -> Result<Vec<u8>> {
        self.response.decrypt(
            record,
            &stream_response_record_aad(session_id, request_id, sequence),
        )
    }

    #[cfg(test)]
    pub(super) fn encrypt_request_record_with_nonce(
        &self,
        session_id: &Uuid,
        plaintext: &[u8],
        nonce: [u8; RECORD_NONCE_LEN],
    ) -> Result<Vec<u8>> {
        self.request
            .encrypt_with_nonce(plaintext, &request_record_aad(session_id), nonce)
    }

    #[cfg(test)]
    pub(super) fn encrypt_unary_response_record_for_test(
        &self,
        session_id: &Uuid,
        request_id: &RequestId,
        plaintext: &[u8],
    ) -> Result<Vec<u8>> {
        self.response.encrypt(
            plaintext,
            &unary_response_record_aad(session_id, request_id),
        )
    }

    #[cfg(test)]
    pub(super) fn encrypt_stream_response_record_for_test(
        &self,
        session_id: &Uuid,
        request_id: &RequestId,
        sequence: u64,
        plaintext: &[u8],
    ) -> Result<Vec<u8>> {
        self.response.encrypt(
            plaintext,
            &stream_response_record_aad(session_id, request_id, sequence),
        )
    }

    #[cfg(test)]
    pub(super) fn request_key_bytes(&self) -> &[u8; KEY_LEN] {
        &self.request.0
    }

    #[cfg(test)]
    pub(super) fn response_key_bytes(&self) -> &[u8; KEY_LEN] {
        &self.response.0
    }
}

impl fmt::Debug for DirectionalKeys {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DirectionalKeys([REDACTED])")
    }
}

pub(super) fn decrypt_key_exchange_record(
    x25519_shared_secret: &[u8; KEY_LEN],
    record: &[u8],
) -> Result<DecryptedHandshakePayload> {
    if x25519_shared_secret.iter().all(|byte| *byte == 0) {
        return Err(TransportV2Error::NonContributoryKeyExchange);
    }
    if record.len() != HANDSHAKE_RECORD_LEN {
        return Err(TransportV2Error::InvalidKeyExchange);
    }

    let key = RecordKey::derive(x25519_shared_secret, HANDSHAKE_KEY_INFO)?;
    let plaintext = Zeroizing::new(key.decrypt(record, KEY_EXCHANGE_AAD)?);
    if plaintext.len() != HANDSHAKE_PAYLOAD_LEN || plaintext[0] != HANDSHAKE_PAYLOAD_VERSION {
        return Err(TransportV2Error::InvalidKeyExchange);
    }

    let session_id = Uuid::from_bytes(
        plaintext[1..17]
            .try_into()
            .map_err(|_| TransportV2Error::InvalidKeyExchange)?,
    );
    let session_master = SessionMaster::from_slice(&plaintext[17..49])?;
    let expires_at_unix_seconds = u64::from_be_bytes(
        plaintext[49..57]
            .try_into()
            .map_err(|_| TransportV2Error::InvalidKeyExchange)?,
    );

    Ok(DecryptedHandshakePayload {
        session_id,
        session_master,
        expires_at_unix_seconds,
    })
}

#[cfg(test)]
pub(super) fn encode_canonical_base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

pub(super) fn decode_canonical_base64(encoded: &str, decoded_limit: usize) -> Result<Vec<u8>> {
    let encoded_limit = decoded_limit
        .checked_add(2)
        .and_then(|length| length.checked_div(3))
        .and_then(|groups| groups.checked_mul(4))
        .ok_or(TransportV2Error::LimitExceeded {
            field: "encoded record",
            limit: decoded_limit,
        })?;
    check_limit(encoded.len(), encoded_limit, "encoded record")?;
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|_| TransportV2Error::InvalidEncoding)?;
    if decoded.len() > decoded_limit || STANDARD.encode(&decoded) != encoded {
        return Err(TransportV2Error::InvalidEncoding);
    }
    Ok(decoded)
}

pub(super) fn request_record_aad(session_id: &Uuid) -> Vec<u8> {
    let mut aad = Vec::with_capacity(REQUEST_RECORD_AAD.len() + 1 + 16);
    aad.extend_from_slice(REQUEST_RECORD_AAD);
    aad.push(0);
    aad.extend_from_slice(session_id.as_bytes());
    aad
}

pub(super) fn unary_response_record_aad(session_id: &Uuid, request_id: &RequestId) -> Vec<u8> {
    let mut aad = Vec::with_capacity(UNARY_RESPONSE_RECORD_AAD.len() + 1 + 16 + 16);
    aad.extend_from_slice(UNARY_RESPONSE_RECORD_AAD);
    aad.push(0);
    aad.extend_from_slice(session_id.as_bytes());
    aad.extend_from_slice(request_id.as_bytes());
    aad
}

pub(super) fn stream_response_record_aad(
    session_id: &Uuid,
    request_id: &RequestId,
    sequence: u64,
) -> Vec<u8> {
    let mut aad = Vec::with_capacity(STREAM_RESPONSE_RECORD_AAD.len() + 1 + 16 + 16 + 8);
    aad.extend_from_slice(STREAM_RESPONSE_RECORD_AAD);
    aad.push(0);
    aad.extend_from_slice(session_id.as_bytes());
    aad.extend_from_slice(request_id.as_bytes());
    aad.extend_from_slice(&sequence.to_be_bytes());
    aad
}

#[cfg(test)]
pub(super) fn derive_handshake_key_for_test(
    shared_secret: &[u8; KEY_LEN],
) -> Result<[u8; KEY_LEN]> {
    if shared_secret.iter().all(|byte| *byte == 0) {
        return Err(TransportV2Error::NonContributoryKeyExchange);
    }
    Ok(RecordKey::derive(shared_secret, HANDSHAKE_KEY_INFO)?.0)
}

#[cfg(test)]
pub(super) fn encrypt_key_exchange_record_with_nonce(
    shared_secret: &[u8; KEY_LEN],
    plaintext: &[u8],
    nonce: [u8; RECORD_NONCE_LEN],
) -> Result<Vec<u8>> {
    if shared_secret.iter().all(|byte| *byte == 0) {
        return Err(TransportV2Error::NonContributoryKeyExchange);
    }
    if plaintext.len() != HANDSHAKE_PAYLOAD_LEN {
        return Err(TransportV2Error::InvalidKeyExchange);
    }
    RecordKey::derive(shared_secret, HANDSHAKE_KEY_INFO)?.encrypt_with_nonce(
        plaintext,
        KEY_EXCHANGE_AAD,
        nonce,
    )
}

#[cfg(test)]
pub(super) fn encrypt_key_exchange_record_for_test(
    shared_secret: &[u8; KEY_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    if shared_secret.iter().all(|byte| *byte == 0) {
        return Err(TransportV2Error::NonContributoryKeyExchange);
    }
    if plaintext.len() != HANDSHAKE_PAYLOAD_LEN {
        return Err(TransportV2Error::InvalidKeyExchange);
    }
    RecordKey::derive(shared_secret, HANDSHAKE_KEY_INFO)?.encrypt(plaintext, KEY_EXCHANGE_AAD)
}
