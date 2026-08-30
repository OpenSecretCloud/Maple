use std::{
    collections::HashSet,
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use p256::elliptic_curve::rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::{
    crypto::{
        decrypt_key_exchange_record, DirectionalKeys, SessionMaster, KEY_LEN, MIN_RECORD_LEN,
        RECORD_NONCE_LEN,
    },
    envelope::{
        CacheNamespaceRoot, Credential, EncodedBytes, EncryptedOuterRecord, EnvelopeLimits,
        LogicalRequest, RequestEnvelope, RequestId, ResponseMode, UnaryResponseEnvelope, Version2,
        MAX_KEY_EXCHANGE_BYTES, MAX_OUTER_REQUEST_BYTES,
    },
    stream::StreamDecoder,
    Result, TransportV2Error,
};

const MAX_ATTESTATION_NONCE_BYTES: usize = 512;
const MAX_REQUEST_RECORDS: usize = 65_536;
const MAX_RESPONSE_RECORDS: usize = 65_536;
const MAX_REQUEST_ID_GENERATION_ATTEMPTS: usize = 16;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct KeyExchangeRequest<'a> {
    nonce: &'a str,
    client_public_key: EncodedBytes,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyExchangeResponse {
    session_id: String,
    encrypted_session_key: EncodedBytes,
}

/// One prepared, one-shot key-exchange request.
///
/// The engine offers no resend loop. The future network adapter consumes this
/// value into one outer request body and one response completion capability.
pub(super) struct PreparedKeyExchange {
    request_body: Vec<u8>,
    completion: KeyExchangeCompletion,
}

impl PreparedKeyExchange {
    pub(super) fn new(nonce: String, enclave_public_key: [u8; KEY_LEN]) -> Result<Self> {
        if nonce.is_empty() || nonce.len() > MAX_ATTESTATION_NONCE_BYTES {
            return Err(TransportV2Error::InvalidKeyExchange);
        }

        let mut secret_bytes = Zeroizing::new([0_u8; KEY_LEN]);
        OsRng
            .try_fill_bytes(&mut *secret_bytes)
            .map_err(|_| TransportV2Error::RandomnessUnavailable)?;
        let client_secret = StaticSecret::from(*secret_bytes);
        let client_public_key = PublicKey::from(&client_secret);
        let request = KeyExchangeRequest {
            nonce: &nonce,
            client_public_key: EncodedBytes::from_bytes(client_public_key.as_bytes().to_vec()),
        };
        let request_body =
            serde_json::to_vec(&request).map_err(|_| TransportV2Error::InvalidJson)?;
        if request_body.len() > MAX_KEY_EXCHANGE_BYTES {
            return Err(TransportV2Error::LimitExceeded {
                field: "key exchange",
                limit: MAX_KEY_EXCHANGE_BYTES,
            });
        }

        Ok(Self {
            request_body,
            completion: KeyExchangeCompletion {
                client_secret,
                enclave_public_key: PublicKey::from(enclave_public_key),
            },
        })
    }

    pub(super) fn into_parts(self) -> (Vec<u8>, KeyExchangeCompletion) {
        (self.request_body, self.completion)
    }
}

impl fmt::Debug for PreparedKeyExchange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedKeyExchange")
            .field("request_body_bytes", &self.request_body.len())
            .field("completion", &"[REDACTED]")
            .finish()
    }
}

/// Consumed capability that turns one authenticated key-exchange response into
/// a v2 session.
pub(super) struct KeyExchangeCompletion {
    client_secret: StaticSecret,
    enclave_public_key: PublicKey,
}

impl KeyExchangeCompletion {
    pub(super) fn complete(self, response_body: &[u8]) -> Result<V2Session> {
        if response_body.len() > MAX_KEY_EXCHANGE_BYTES {
            return Err(TransportV2Error::LimitExceeded {
                field: "key exchange",
                limit: MAX_KEY_EXCHANGE_BYTES,
            });
        }
        let response: KeyExchangeResponse =
            serde_json::from_slice(response_body).map_err(|_| TransportV2Error::InvalidJson)?;

        let outer_session_id = parse_canonical_session_id(&response.session_id)?;
        let shared_secret = self.client_secret.diffie_hellman(&self.enclave_public_key);
        if !shared_secret.was_contributory() {
            return Err(TransportV2Error::NonContributoryKeyExchange);
        }
        let payload = decrypt_key_exchange_record(
            shared_secret.as_bytes(),
            response.encrypted_session_key.as_slice(),
        )?;
        if payload.session_id != outer_session_id {
            return Err(TransportV2Error::BindingMismatch);
        }

        V2Session::from_parts(
            outer_session_id,
            payload.session_master,
            payload.expires_at_unix_seconds,
        )
    }

    #[cfg(test)]
    pub(super) fn from_parts_for_test(
        client_secret: [u8; KEY_LEN],
        enclave_public_key: [u8; KEY_LEN],
    ) -> Self {
        Self {
            client_secret: StaticSecret::from(client_secret),
            enclave_public_key: PublicKey::from(enclave_public_key),
        }
    }
}

impl fmt::Debug for KeyExchangeCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KeyExchangeCompletion([REDACTED])")
    }
}

fn parse_canonical_session_id(encoded: &str) -> Result<Uuid> {
    let session_id = Uuid::parse_str(encoded).map_err(|_| TransportV2Error::InvalidKeyExchange)?;
    if session_id.hyphenated().to_string() != encoded {
        return Err(TransportV2Error::InvalidKeyExchange);
    }
    Ok(session_id)
}

/// Exact crypto context for one attested transport-v2 session.
pub(super) struct V2Session {
    session_id: Uuid,
    expires_at_unix_seconds: u64,
    keys: Arc<DirectionalKeys>,
    usage: Arc<SessionUsage>,
}

struct RequestMaterial {
    request_id: RequestId,
    fixed_nonce: Option<[u8; RECORD_NONCE_LEN]>,
}

struct RequestUsage {
    records: usize,
    request_ids: HashSet<RequestId>,
}

pub(super) struct SessionUsage {
    request: Mutex<RequestUsage>,
    response_records: AtomicUsize,
    request_limit: usize,
    response_limit: usize,
}

impl SessionUsage {
    pub(super) fn new(request_limit: usize, response_limit: usize) -> Self {
        Self {
            request: Mutex::new(RequestUsage {
                records: 0,
                request_ids: HashSet::new(),
            }),
            response_records: AtomicUsize::new(0),
            request_limit,
            response_limit,
        }
    }

    fn reserve_random_request(&self) -> Result<RequestId> {
        let mut request = self
            .request
            .lock()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?;
        if request.records >= self.request_limit {
            return Err(TransportV2Error::RequestRecordBudgetExhausted);
        }
        for _ in 0..MAX_REQUEST_ID_GENERATION_ATTEMPTS {
            let request_id = RequestId::random()?;
            if request.request_ids.insert(request_id) {
                request.records += 1;
                return Ok(request_id);
            }
        }
        Err(TransportV2Error::RequestIdCollision)
    }

    #[cfg(test)]
    fn reserve_fixed_request(&self, request_id: RequestId) -> Result<()> {
        let mut request = self
            .request
            .lock()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?;
        if request.records >= self.request_limit {
            return Err(TransportV2Error::RequestRecordBudgetExhausted);
        }
        if !request.request_ids.insert(request_id) {
            return Err(TransportV2Error::RequestIdCollision);
        }
        request.records += 1;
        Ok(())
    }

    fn release_request(&self, request_id: RequestId) {
        if let Ok(mut request) = self.request.lock() {
            if request.request_ids.remove(&request_id) {
                request.records = request.records.saturating_sub(1);
            }
        }
    }

    fn reserve_response_records(&self, count: usize) -> Result<()> {
        self.response_records
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |records| {
                records
                    .checked_add(count)
                    .filter(|reserved| *reserved <= self.response_limit)
            })
            .map(|_| ())
            .map_err(|_| TransportV2Error::ResponseRecordBudgetExhausted)
    }

    fn release_response_records(&self, count: usize) {
        let released =
            self.response_records
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |records| {
                    records.checked_sub(count)
                });
        debug_assert!(released.is_ok(), "response record accounting underflow");
    }

    fn reserve_initial_response(&self, response_mode: ResponseMode) -> Result<usize> {
        let records = match response_mode {
            ResponseMode::Unary => 1,
            ResponseMode::Stream => 2,
            ResponseMode::Auto => return Err(TransportV2Error::InvalidRequest),
        };
        self.reserve_response_records(records)?;
        Ok(records)
    }

    pub(super) fn reserve_stream_chunk(&self) -> Result<()> {
        self.reserve_response_records(1)
    }
}

impl V2Session {
    fn from_parts(
        session_id: Uuid,
        session_master: SessionMaster,
        expires_at_unix_seconds: u64,
    ) -> Result<Self> {
        Self::from_parts_with_budgets(
            session_id,
            session_master,
            expires_at_unix_seconds,
            MAX_REQUEST_RECORDS,
            MAX_RESPONSE_RECORDS,
        )
    }

    fn from_parts_with_budgets(
        session_id: Uuid,
        session_master: SessionMaster,
        expires_at_unix_seconds: u64,
        request_limit: usize,
        response_limit: usize,
    ) -> Result<Self> {
        let keys = Arc::new(DirectionalKeys::derive(&session_master)?);
        Ok(Self {
            session_id,
            expires_at_unix_seconds,
            keys,
            usage: Arc::new(SessionUsage::new(request_limit, response_limit)),
        })
    }

    #[cfg(test)]
    pub(super) fn from_master_for_test(
        session_id: Uuid,
        session_master: [u8; KEY_LEN],
        expires_at_unix_seconds: u64,
    ) -> Result<Self> {
        Self::from_parts(
            session_id,
            SessionMaster::from_bytes(session_master),
            expires_at_unix_seconds,
        )
    }

    #[cfg(test)]
    pub(super) fn from_master_with_budgets_for_test(
        session_id: Uuid,
        session_master: [u8; KEY_LEN],
        expires_at_unix_seconds: u64,
        request_limit: usize,
        response_limit: usize,
    ) -> Result<Self> {
        Self::from_parts_with_budgets(
            session_id,
            SessionMaster::from_bytes(session_master),
            expires_at_unix_seconds,
            request_limit,
            response_limit,
        )
    }

    pub(super) const fn session_id(&self) -> Uuid {
        self.session_id
    }

    pub(super) const fn expires_at_unix_seconds(&self) -> u64 {
        self.expires_at_unix_seconds
    }

    pub(super) fn prepare_request(
        &self,
        response_mode: ResponseMode,
        credential: Option<Credential>,
        cache_namespace_root: Option<CacheNamespaceRoot>,
        request: LogicalRequest,
    ) -> Result<PreparedRequest> {
        let now_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TransportV2Error::SessionExpired)?
            .as_secs();
        self.validate_request_start(now_unix_seconds, response_mode)?;
        let request_id = self.usage.reserve_random_request()?;
        let initial_response_records = match self.usage.reserve_initial_response(response_mode) {
            Ok(records) => records,
            Err(error) => {
                self.usage.release_request(request_id);
                return Err(error);
            }
        };
        let result = self.prepare_reserved_request(
            RequestMaterial {
                request_id,
                fixed_nonce: None,
            },
            response_mode,
            credential,
            cache_namespace_root,
            request,
        );
        if result.is_err() {
            self.usage.release_request(request_id);
            self.usage
                .release_response_records(initial_response_records);
        }
        result
    }

    fn validate_request_start(
        &self,
        now_unix_seconds: u64,
        response_mode: ResponseMode,
    ) -> Result<()> {
        if now_unix_seconds >= self.expires_at_unix_seconds {
            return Err(TransportV2Error::SessionExpired);
        }
        if response_mode == ResponseMode::Auto {
            return Err(TransportV2Error::InvalidRequest);
        }
        Ok(())
    }

    fn prepare_reserved_request(
        &self,
        material: RequestMaterial,
        response_mode: ResponseMode,
        credential: Option<Credential>,
        cache_namespace_root: Option<CacheNamespaceRoot>,
        request: LogicalRequest,
    ) -> Result<PreparedRequest> {
        let envelope = RequestEnvelope {
            version: Version2,
            request_id: material.request_id,
            response_mode,
            credential,
            cache_namespace_root_base64: cache_namespace_root,
            request,
        };
        let plaintext = Zeroizing::new(envelope.to_json_vec(&EnvelopeLimits::DEFAULT)?);
        let encrypted = match material.fixed_nonce {
            #[cfg(test)]
            Some(nonce) => {
                self.keys
                    .encrypt_request_record_with_nonce(&self.session_id, &plaintext, nonce)?
            }
            #[cfg(not(test))]
            Some(_) => return Err(TransportV2Error::InvalidRequest),
            None => self
                .keys
                .encrypt_request_record(&self.session_id, &plaintext)?,
        };
        let outer_body = EncryptedOuterRecord {
            encrypted: EncodedBytes::from_bytes(encrypted),
        }
        .to_json_vec(MAX_OUTER_REQUEST_BYTES)?;

        Ok(PreparedRequest {
            session_id: self.session_id,
            request_id: material.request_id,
            response_mode,
            outer_body,
            response: ResponseContext {
                session_id: self.session_id,
                request_id: material.request_id,
                response_mode,
                keys: Arc::clone(&self.keys),
                usage: Arc::clone(&self.usage),
            },
        })
    }

    #[cfg(test)]
    pub(super) fn prepare_request_with_nonce_for_test(
        &self,
        material: (u64, RequestId, [u8; RECORD_NONCE_LEN]),
        response_mode: ResponseMode,
        credential: Option<Credential>,
        cache_namespace_root: Option<CacheNamespaceRoot>,
        request: LogicalRequest,
    ) -> Result<PreparedRequest> {
        let (now_unix_seconds, request_id, nonce) = material;
        self.validate_request_start(now_unix_seconds, response_mode)?;
        self.usage.reserve_fixed_request(request_id)?;
        let initial_response_records = match self.usage.reserve_initial_response(response_mode) {
            Ok(records) => records,
            Err(error) => {
                self.usage.release_request(request_id);
                return Err(error);
            }
        };
        let result = self.prepare_reserved_request(
            RequestMaterial {
                request_id,
                fixed_nonce: Some(nonce),
            },
            response_mode,
            credential,
            cache_namespace_root,
            request,
        );
        if result.is_err() {
            self.usage.release_request(request_id);
            self.usage
                .release_response_records(initial_response_records);
        }
        result
    }
}

impl fmt::Debug for V2Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("V2Session")
            .field("session_id", &self.session_id)
            .field("expires_at_unix_seconds", &self.expires_at_unix_seconds)
            .field("keys", &"[REDACTED]")
            .finish()
    }
}

/// One encrypted request body and its exact-session response capability.
///
/// This type is deliberately not `Clone`. Consuming it into parts gives a
/// network adapter one body to send and one context with which to authenticate
/// the response; there is no retry or fallback behavior in this engine.
pub(super) struct PreparedRequest {
    session_id: Uuid,
    request_id: RequestId,
    response_mode: ResponseMode,
    outer_body: Vec<u8>,
    response: ResponseContext,
}

impl PreparedRequest {
    pub(super) const fn session_id(&self) -> Uuid {
        self.session_id
    }

    pub(super) const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub(super) const fn response_mode(&self) -> ResponseMode {
        self.response_mode
    }

    pub(super) fn into_parts(self) -> (Vec<u8>, ResponseContext) {
        (self.outer_body, self.response)
    }
}

impl fmt::Debug for PreparedRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRequest")
            .field("session_id", &self.session_id)
            .field("request_id", &self.request_id)
            .field("response_mode", &self.response_mode)
            .field("outer_body_bytes", &self.outer_body.len())
            .field("response", &"[BOUND]")
            .finish()
    }
}

/// Response authenticator bound to the exact session and request that created
/// it. It cannot be rebound by looking up an attacker-controlled outer UUID.
pub(super) struct ResponseContext {
    session_id: Uuid,
    request_id: RequestId,
    response_mode: ResponseMode,
    keys: Arc<DirectionalKeys>,
    usage: Arc<SessionUsage>,
}

impl ResponseContext {
    pub(super) fn decrypt_unary_outer(self, outer_body: &[u8]) -> Result<UnaryResponse> {
        if self.response_mode != ResponseMode::Unary {
            return Err(TransportV2Error::ResponseModeMismatch);
        }
        self.decrypt_unary_envelope(outer_body)
    }

    /// Authenticate an HTTP error returned before a requested stream starts.
    ///
    /// Successful stream responses must use authenticated stream records. The
    /// gateway may only use this unary envelope shape for a pre-Start 4xx/5xx.
    pub(super) fn decrypt_stream_pre_start_error_outer(
        self,
        outer_body: &[u8],
    ) -> Result<UnaryResponse> {
        if self.response_mode != ResponseMode::Stream {
            return Err(TransportV2Error::ResponseModeMismatch);
        }
        let response = self.decrypt_unary_envelope(outer_body)?;
        if !(400..=599).contains(&response.status) {
            return Err(TransportV2Error::InvalidResponse);
        }
        // A requested stream reserves Start and terminal capacity before its
        // request record is emitted. An authenticated pre-Start unary error
        // proves that only one of those two records was used.
        self.usage.release_response_records(1);
        Ok(response)
    }

    fn decrypt_unary_envelope(&self, outer_body: &[u8]) -> Result<UnaryResponse> {
        let outer = EncryptedOuterRecord::from_json_slice(outer_body, MAX_OUTER_REQUEST_BYTES)?;
        let encrypted_limit = EnvelopeLimits::DEFAULT
            .envelope_bytes
            .checked_add(MIN_RECORD_LEN)
            .ok_or(TransportV2Error::InvalidResponse)?;
        if outer.encrypted.len() > encrypted_limit {
            return Err(TransportV2Error::LimitExceeded {
                field: "encrypted response",
                limit: encrypted_limit,
            });
        }
        let plaintext = Zeroizing::new(self.keys.decrypt_unary_response_record(
            &self.session_id,
            &self.request_id,
            outer.encrypted.as_slice(),
        )?);
        let response =
            UnaryResponseEnvelope::from_json_slice(&plaintext, &EnvelopeLimits::DEFAULT)?;
        if response.request_id != self.request_id {
            return Err(TransportV2Error::BindingMismatch);
        }

        Ok(UnaryResponse {
            status: response.status,
            headers: response.headers,
            body: response.body_base64.map(EncodedBytes::into_bytes),
        })
    }

    pub(super) fn into_stream_decoder(self) -> Result<StreamDecoder> {
        if self.response_mode != ResponseMode::Stream {
            return Err(TransportV2Error::ResponseModeMismatch);
        }
        Ok(StreamDecoder::new(
            self.session_id,
            self.request_id,
            self.keys,
            self.usage,
        ))
    }
}

impl fmt::Debug for ResponseContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseContext")
            .field("session_id", &self.session_id)
            .field("request_id", &self.request_id)
            .field("response_mode", &self.response_mode)
            .field("keys", &"[REDACTED]")
            .field("usage", &"[BOUND]")
            .finish()
    }
}

#[derive(Eq, PartialEq)]
pub(super) struct UnaryResponse {
    pub(super) status: u16,
    pub(super) headers: Vec<super::envelope::HeaderField>,
    pub(super) body: Option<Vec<u8>>,
}

impl fmt::Debug for UnaryResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UnaryResponse")
            .field("status", &self.status)
            .field("header_count", &self.headers.len())
            .field(
                "body_bytes",
                &self.body.as_ref().map_or(0, std::vec::Vec::len),
            )
            .finish()
    }
}
