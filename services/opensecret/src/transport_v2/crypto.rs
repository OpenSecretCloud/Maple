use std::{fmt, str::FromStr};

use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::ZeroizeOnDrop;

use super::envelope::{RequestId, REQUEST_ID_BYTES};

pub(crate) const HANDSHAKE_CHALLENGE_BYTES: usize = 32;
pub(crate) const X25519_PUBLIC_KEY_BYTES: usize = 32;
pub(crate) const SESSION_ID_BYTES: usize = 16;
pub(crate) const RECORD_NONCE_BYTES: usize = 12;
pub(crate) const RECORD_TAG_BYTES: usize = 16;
pub(crate) const MIN_REQUEST_RECORD_BYTES: usize = REQUEST_ID_BYTES + RECORD_TAG_BYTES;
pub(crate) const MIN_RESPONSE_RECORD_BYTES: usize = RECORD_TAG_BYTES;

const HANDSHAKE_DOMAIN: &[u8] = b"opensecret/transport-v2/session/v1";
const ATTESTATION_USER_DATA_DOMAIN: &[u8] = b"opensecret/transport-v2/session/v1/client-public-key";
const REQUEST_KEY_INFO: &[u8] = b"opensecret/transport-v2/request-key/v1";
const RESPONSE_KEY_INFO: &[u8] = b"opensecret/transport-v2/response-key/v1";
const SESSION_ID_INFO: &[u8] = b"opensecret/transport-v2/session-id/v1";
const REQUEST_SUBKEY_INFO: &[u8] = b"opensecret/transport-v2/request-subkey/v1";
const RESPONSE_SUBKEY_INFO: &[u8] = b"opensecret/transport-v2/response-subkey/v1";
const REQUEST_RECORD_DOMAIN: &[u8] = b"opensecret/transport-v2/request-record/v1";
const RESPONSE_RECORD_DOMAIN: &[u8] = b"opensecret/transport-v2/response-record/v1";

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
#[error("transport-v2 session ID must be exactly 32 lowercase hexadecimal characters")]
pub(crate) struct SessionIdParseError;

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub(crate) enum CryptoError {
    #[error("non-contributory X25519 shared secret")]
    NonContributoryKey,
    #[error("transport-v2 key derivation failed")]
    KeyDerivation,
    #[error("transport-v2 record is truncated")]
    TruncatedRecord,
    #[error("transport-v2 record encryption failed")]
    Encryption,
    #[error("transport-v2 record authentication failed")]
    Authentication,
    #[error("transport-v2 response sequence is exhausted")]
    SequenceExhausted,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) struct SessionId([u8; SESSION_ID_BYTES]);

impl SessionId {
    pub(crate) const fn from_bytes(bytes: [u8; SESSION_ID_BYTES]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; SESSION_ID_BYTES] {
        &self.0
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionId(")?;
        formatter.write_str(&hex::encode(self.0))?;
        formatter.write_str(")")
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl FromStr for SessionId {
    type Err = SessionIdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let encoded = value.as_bytes();
        if encoded.len() != SESSION_ID_BYTES * 2
            || !encoded
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(SessionIdParseError);
        }

        let mut decoded = [0; SESSION_ID_BYTES];
        hex::decode_to_slice(encoded, &mut decoded).map_err(|_| SessionIdParseError)?;
        Ok(Self(decoded))
    }
}

/// Values that a client verifies in one Nitro attestation document before it
/// treats derived transport keys as belonging to an approved enclave.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HandshakeTranscript {
    challenge: [u8; HANDSHAKE_CHALLENGE_BYTES],
    client_public_key: [u8; X25519_PUBLIC_KEY_BYTES],
    server_public_key: [u8; X25519_PUBLIC_KEY_BYTES],
}

impl HandshakeTranscript {
    pub(crate) const fn new(
        challenge: [u8; HANDSHAKE_CHALLENGE_BYTES],
        client_public_key: [u8; X25519_PUBLIC_KEY_BYTES],
        server_public_key: [u8; X25519_PUBLIC_KEY_BYTES],
    ) -> Self {
        Self {
            challenge,
            client_public_key,
            server_public_key,
        }
    }

    pub(crate) const fn challenge(&self) -> &[u8; HANDSHAKE_CHALLENGE_BYTES] {
        &self.challenge
    }

    pub(crate) const fn client_public_key(&self) -> &[u8; X25519_PUBLIC_KEY_BYTES] {
        &self.client_public_key
    }

    pub(crate) const fn server_public_key(&self) -> &[u8; X25519_PUBLIC_KEY_BYTES] {
        &self.server_public_key
    }

    fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(HANDSHAKE_DOMAIN);
        hasher.update([0]);
        hasher.update(self.challenge);
        hasher.update(self.client_public_key);
        hasher.update(self.server_public_key);
        hasher.finalize().into()
    }
}

/// Exact user-data bytes placed in the attestation document. The document's
/// nonce carries the challenge and its public-key field carries the server key.
pub(crate) fn attestation_user_data(client_public_key: &[u8; X25519_PUBLIC_KEY_BYTES]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ATTESTATION_USER_DATA_DOMAIN.len() + 1 + 32);
    bytes.extend_from_slice(ATTESTATION_USER_DATA_DOMAIN);
    bytes.push(0);
    bytes.extend_from_slice(client_public_key);
    bytes
}

#[derive(ZeroizeOnDrop)]
struct TrafficKey([u8; 32]);

/// Directional keys plus the public identifier derived from one verified
/// attested X25519 transcript. Raw key material is never exposed by this type.
#[derive(ZeroizeOnDrop)]
pub(crate) struct SessionSecrets {
    #[zeroize(skip)]
    session_id: SessionId,
    request_key: TrafficKey,
    response_key: TrafficKey,
}

/// The sole writer for one admitted request's encrypted response.
///
/// A per-request subkey lets every response start at sequence zero without a
/// session-wide response-record counter. This type never rewinds its sequence,
/// even if an encryption attempt fails.
#[derive(ZeroizeOnDrop)]
pub(crate) struct ResponseSealer {
    #[zeroize(skip)]
    session_id: SessionId,
    #[zeroize(skip)]
    request_id: RequestId,
    key: TrafficKey,
    #[zeroize(skip)]
    next_sequence: u64,
}

impl fmt::Debug for ResponseSealer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseSealer")
            .field("session_id", &self.session_id)
            .field("request_id", &self.request_id)
            .field("next_sequence", &self.next_sequence)
            .finish_non_exhaustive()
    }
}

impl ResponseSealer {
    pub(crate) fn seal_next(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(CryptoError::SequenceExhausted)?;
        seal_detached_nonce(
            &self.key,
            &response_aad(self.session_id, self.request_id, sequence),
            response_nonce(sequence),
            plaintext,
        )
    }

    #[cfg(test)]
    pub(crate) const fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
}

impl fmt::Debug for SessionSecrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionSecrets")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl SessionSecrets {
    pub(crate) const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub(crate) fn encrypt_request(
        &self,
        request_id: RequestId,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let key = derive_record_subkey(
            &self.request_key,
            REQUEST_SUBKEY_INFO,
            self.session_id,
            request_id,
        )?;
        let ciphertext = seal_detached_nonce(
            &key,
            &request_aad(self.session_id, request_id),
            [0; RECORD_NONCE_BYTES],
            plaintext,
        )?;
        let mut record = Vec::with_capacity(REQUEST_ID_BYTES + ciphertext.len());
        record.extend_from_slice(request_id.as_bytes());
        record.extend_from_slice(&ciphertext);
        Ok(record)
    }

    pub(crate) fn decrypt_request(
        &self,
        record: &[u8],
    ) -> Result<(RequestId, Vec<u8>), CryptoError> {
        if record.len() < MIN_REQUEST_RECORD_BYTES {
            return Err(CryptoError::TruncatedRecord);
        }
        let request_id = RequestId::from_bytes(
            record[..REQUEST_ID_BYTES]
                .try_into()
                .map_err(|_| CryptoError::TruncatedRecord)?,
        );
        let key = derive_record_subkey(
            &self.request_key,
            REQUEST_SUBKEY_INFO,
            self.session_id,
            request_id,
        )?;
        let plaintext = open_detached_nonce(
            &key,
            &request_aad(self.session_id, request_id),
            [0; RECORD_NONCE_BYTES],
            &record[REQUEST_ID_BYTES..],
        )?;
        Ok((request_id, plaintext))
    }

    pub(super) fn response_sealer(
        &self,
        request_id: RequestId,
    ) -> Result<ResponseSealer, CryptoError> {
        Ok(ResponseSealer {
            session_id: self.session_id,
            request_id,
            key: derive_record_subkey(
                &self.response_key,
                RESPONSE_SUBKEY_INFO,
                self.session_id,
                request_id,
            )?,
            next_sequence: 0,
        })
    }

    pub(crate) fn decrypt_response(
        &self,
        request_id: RequestId,
        sequence: u64,
        record: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let key = derive_record_subkey(
            &self.response_key,
            RESPONSE_SUBKEY_INFO,
            self.session_id,
            request_id,
        )?;
        open_detached_nonce(
            &key,
            &response_aad(self.session_id, request_id, sequence),
            response_nonce(sequence),
            record,
        )
    }

    #[cfg(test)]
    fn request_key_bytes(&self) -> &[u8; 32] {
        &self.request_key.0
    }

    #[cfg(test)]
    fn response_key_bytes(&self) -> &[u8; 32] {
        &self.response_key.0
    }
}

pub(crate) fn derive_server_session(
    server_secret: EphemeralSecret,
    transcript: &HandshakeTranscript,
) -> Result<SessionSecrets, CryptoError> {
    let client_public_key = PublicKey::from(*transcript.client_public_key());
    derive_session_secrets(
        server_secret.diffie_hellman(&client_public_key).as_bytes(),
        transcript,
    )
}

pub(crate) fn derive_client_session(
    client_secret: EphemeralSecret,
    transcript: &HandshakeTranscript,
) -> Result<SessionSecrets, CryptoError> {
    let server_public_key = PublicKey::from(*transcript.server_public_key());
    derive_session_secrets(
        client_secret.diffie_hellman(&server_public_key).as_bytes(),
        transcript,
    )
}

fn derive_session_secrets(
    shared_secret: &[u8; 32],
    transcript: &HandshakeTranscript,
) -> Result<SessionSecrets, CryptoError> {
    if bool::from(shared_secret.ct_eq(&[0; 32])) {
        return Err(CryptoError::NonContributoryKey);
    }

    let transcript_digest = transcript.digest();
    let hkdf = Hkdf::<Sha256>::new(Some(transcript.challenge()), shared_secret);
    let mut request_key = [0; 32];
    let mut response_key = [0; 32];
    let mut session_id = [0; SESSION_ID_BYTES];
    hkdf.expand(
        &key_info(REQUEST_KEY_INFO, &transcript_digest),
        &mut request_key,
    )
    .map_err(|_| CryptoError::KeyDerivation)?;
    hkdf.expand(
        &key_info(RESPONSE_KEY_INFO, &transcript_digest),
        &mut response_key,
    )
    .map_err(|_| CryptoError::KeyDerivation)?;
    hkdf.expand(
        &key_info(SESSION_ID_INFO, &transcript_digest),
        &mut session_id,
    )
    .map_err(|_| CryptoError::KeyDerivation)?;

    Ok(SessionSecrets {
        session_id: SessionId(session_id),
        request_key: TrafficKey(request_key),
        response_key: TrafficKey(response_key),
    })
}

fn key_info(label: &[u8], transcript_digest: &[u8; 32]) -> Vec<u8> {
    let mut info = Vec::with_capacity(label.len() + 1 + transcript_digest.len());
    info.extend_from_slice(label);
    info.push(0);
    info.extend_from_slice(transcript_digest);
    info
}

fn derive_record_subkey(
    base_key: &TrafficKey,
    label: &[u8],
    session_id: SessionId,
    request_id: RequestId,
) -> Result<TrafficKey, CryptoError> {
    let hkdf = Hkdf::<Sha256>::from_prk(&base_key.0).map_err(|_| CryptoError::KeyDerivation)?;
    let mut info = Vec::with_capacity(label.len() + 1 + SESSION_ID_BYTES + REQUEST_ID_BYTES);
    info.extend_from_slice(label);
    info.push(0);
    info.extend_from_slice(session_id.as_bytes());
    info.extend_from_slice(request_id.as_bytes());

    let mut key = [0; 32];
    hkdf.expand(&info, &mut key)
        .map_err(|_| CryptoError::KeyDerivation)?;
    Ok(TrafficKey(key))
}

fn request_aad(session_id: SessionId, request_id: RequestId) -> Vec<u8> {
    let mut aad =
        Vec::with_capacity(REQUEST_RECORD_DOMAIN.len() + 1 + SESSION_ID_BYTES + REQUEST_ID_BYTES);
    aad.extend_from_slice(REQUEST_RECORD_DOMAIN);
    aad.push(0);
    aad.extend_from_slice(session_id.as_bytes());
    aad.extend_from_slice(request_id.as_bytes());
    aad
}

fn response_aad(session_id: SessionId, request_id: RequestId, sequence: u64) -> Vec<u8> {
    let mut aad = Vec::with_capacity(
        RESPONSE_RECORD_DOMAIN.len() + 1 + SESSION_ID_BYTES + 16 + std::mem::size_of::<u64>(),
    );
    aad.extend_from_slice(RESPONSE_RECORD_DOMAIN);
    aad.push(0);
    aad.extend_from_slice(session_id.as_bytes());
    aad.extend_from_slice(request_id.as_bytes());
    aad.extend_from_slice(&sequence.to_be_bytes());
    aad
}

fn response_nonce(sequence: u64) -> [u8; RECORD_NONCE_BYTES] {
    let mut nonce = [0; RECORD_NONCE_BYTES];
    nonce[4..].copy_from_slice(&sequence.to_be_bytes());
    nonce
}

fn seal_detached_nonce(
    key: &TrafficKey,
    aad: &[u8],
    nonce: [u8; RECORD_NONCE_BYTES],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new((&key.0).into());
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Encryption)?;
    Ok(ciphertext)
}

fn open_detached_nonce(
    key: &TrafficKey,
    aad: &[u8],
    nonce: [u8; RECORD_NONCE_BYTES],
    record: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if record.len() < RECORD_TAG_BYTES {
        return Err(CryptoError::TruncatedRecord);
    }
    ChaCha20Poly1305::new((&key.0).into())
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: record, aad })
        .map_err(|_| CryptoError::Authentication)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    fn transcript() -> HandshakeTranscript {
        HandshakeTranscript::new([0x11; 32], [0x22; 32], [0x33; 32])
    }

    fn secrets(seed: u8) -> SessionSecrets {
        derive_session_secrets(&[seed; 32], &transcript()).unwrap()
    }

    #[test]
    fn one_round_x25519_derives_identical_directional_keys() {
        let client_secret = EphemeralSecret::random_from_rng(OsRng);
        let client_public = PublicKey::from(&client_secret).to_bytes();
        let server_secret = EphemeralSecret::random_from_rng(OsRng);
        let server_public = PublicKey::from(&server_secret).to_bytes();
        let transcript = HandshakeTranscript::new([7; 32], client_public, server_public);

        let client = derive_client_session(client_secret, &transcript).unwrap();
        let server = derive_server_session(server_secret, &transcript).unwrap();

        assert_eq!(client.session_id(), server.session_id());
        assert_eq!(client.request_key_bytes(), server.request_key_bytes());
        assert_eq!(client.response_key_bytes(), server.response_key_bytes());
    }

    #[test]
    fn attestation_fields_and_transcript_changes_are_bound() {
        let expected = [
            ATTESTATION_USER_DATA_DOMAIN,
            &[0],
            &[0x22; X25519_PUBLIC_KEY_BYTES],
        ]
        .concat();
        assert_eq!(attestation_user_data(&[0x22; 32]), expected);

        let original = secrets(0x44);
        let changed = derive_session_secrets(
            &[0x44; 32],
            &HandshakeTranscript::new([0x10; 32], [0x22; 32], [0x33; 32]),
        )
        .unwrap();
        assert_ne!(original.session_id(), changed.session_id());
        assert_ne!(original.request_key_bytes(), changed.request_key_bytes());
    }

    #[test]
    fn session_ids_have_one_canonical_wire_encoding() {
        let session_id = SessionId::from_bytes([0xab; SESSION_ID_BYTES]);
        let encoded = session_id.to_string();
        assert_eq!(encoded, "abababababababababababababababab");
        assert_eq!(SessionId::from_str(&encoded), Ok(session_id));
        assert_eq!(
            SessionId::from_str(&encoded.to_uppercase()),
            Err(SessionIdParseError)
        );
        assert_eq!(SessionId::from_str("ab"), Err(SessionIdParseError));
    }

    #[test]
    fn deterministic_key_and_record_vector() {
        let secrets = secrets(0x44);
        assert_eq!(
            hex::encode(secrets.session_id().as_bytes()),
            "f7258fb103137c612baab47ced4a5a02"
        );
        assert_eq!(
            hex::encode(secrets.request_key_bytes()),
            "00f898a5f2dcd40a703f42221f2a2b842b7e97ed5a555caa362c4153a5e1c491"
        );
        assert_eq!(
            hex::encode(secrets.response_key_bytes()),
            "e4fb003c5c829f5385531eebfdbd0ee3d8430a0bd71322e9f3e41ace915c3190"
        );
        let record = secrets
            .encrypt_request(RequestId::from_bytes([0x55; 16]), b"vector plaintext")
            .unwrap();
        assert_eq!(
            hex::encode(record),
            "55555555555555555555555555555555671f5c411205cb00f769e6b2705052b795e91f44516fc6165e16a152e686b209"
        );

        let mut response = secrets
            .response_sealer(RequestId::from_bytes([0x66; 16]))
            .unwrap();
        let record = response.seal_next(b"vector response").unwrap();
        assert_eq!(
            hex::encode(record),
            "25a2d5ed89864bd7b5e13c83eb49b1f314a70abf8bd7e871b706bb6768c9e1"
        );
    }

    #[test]
    fn request_and_response_records_round_trip() {
        let secrets = secrets(0x44);
        let request_id = RequestId::from_bytes([9; 16]);
        let request = secrets.encrypt_request(request_id, b"request").unwrap();
        assert_eq!(
            secrets.decrypt_request(&request).unwrap(),
            (request_id, b"request".to_vec())
        );

        let mut sealer = secrets.response_sealer(request_id).unwrap();
        for sequence in 0..7 {
            let record = sealer.seal_next(b"earlier chunk").unwrap();
            assert_eq!(
                secrets
                    .decrypt_response(request_id, sequence, &record)
                    .unwrap(),
                b"earlier chunk"
            );
        }
        let response = sealer.seal_next(b"response chunk").unwrap();
        assert_eq!(sealer.next_sequence(), 8);
        assert_eq!(
            secrets.decrypt_response(request_id, 7, &response).unwrap(),
            b"response chunk"
        );
    }

    #[test]
    fn records_cannot_be_transplanted() {
        let first = secrets(0x44);
        let second = secrets(0x45);
        let request = first
            .encrypt_request(RequestId::from_bytes([3; 16]), b"request")
            .unwrap();
        assert_eq!(
            second.decrypt_request(&request),
            Err(CryptoError::Authentication)
        );

        let first_request = RequestId::from_bytes([1; 16]);
        let other_request = RequestId::from_bytes([2; 16]);
        let mut sealer = first.response_sealer(first_request).unwrap();
        let response = sealer.seal_next(b"response").unwrap();
        assert_eq!(
            first.decrypt_response(other_request, 0, &response),
            Err(CryptoError::Authentication)
        );
        assert_eq!(
            first.decrypt_response(first_request, 1, &response),
            Err(CryptoError::Authentication)
        );
    }

    #[test]
    fn rejects_non_contributory_shared_secret_and_truncated_records() {
        assert_eq!(
            derive_session_secrets(&[0; 32], &transcript()).unwrap_err(),
            CryptoError::NonContributoryKey
        );
        assert_eq!(
            secrets(0x44).decrypt_request(&[0; MIN_REQUEST_RECORD_BYTES - 1]),
            Err(CryptoError::TruncatedRecord)
        );
    }
}
