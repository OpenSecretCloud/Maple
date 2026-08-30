//! Dormant client engine for OpenSecret transport v2.
//!
//! Nothing in this module is selected by [`crate::OpenSecretClient`] yet. The
//! cutover layer will adapt existing public methods onto these primitives in a
//! later change. Keeping this module private prevents an incomplete transport
//! from becoming a compatibility surface.

#![allow(dead_code)]

mod crypto;
mod envelope;
mod session;
mod stream;

use thiserror::Error;

/// Stable failures from the dormant transport-v2 engine.
///
/// Variants intentionally carry no credentials, plaintext bodies, ciphertext,
/// provider errors, or parser excerpts. A future public adapter can map them
/// into the SDK's public error contract without accidentally logging secrets.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(super) enum TransportV2Error {
    #[error("secure randomness is unavailable")]
    RandomnessUnavailable,
    #[error("transport-v2 key derivation failed")]
    KeyDerivationFailed,
    #[error("transport-v2 key exchange was non-contributory")]
    NonContributoryKeyExchange,
    #[error("transport-v2 record encryption failed")]
    EncryptionFailed,
    #[error("transport-v2 record authentication failed")]
    AuthenticationFailed,
    #[error("transport-v2 encrypted record is too short")]
    RecordTooShort,
    #[error("transport-v2 encoding is invalid")]
    InvalidEncoding,
    #[error("transport-v2 JSON is invalid")]
    InvalidJson,
    #[error("transport-v2 envelope exceeds the {field} limit of {limit} bytes")]
    LimitExceeded { field: &'static str, limit: usize },
    #[error("transport-v2 request is invalid")]
    InvalidRequest,
    #[error("transport-v2 response is invalid")]
    InvalidResponse,
    #[error("transport-v2 response mode does not match the prepared request")]
    ResponseModeMismatch,
    #[error("transport-v2 session has expired")]
    SessionExpired,
    #[error("transport-v2 request record budget is exhausted")]
    RequestRecordBudgetExhausted,
    #[error("transport-v2 response record budget is exhausted")]
    ResponseRecordBudgetExhausted,
    #[error("transport-v2 request identifier collided")]
    RequestIdCollision,
    #[error("transport-v2 session state is unavailable")]
    SessionStateUnavailable,
    #[error("transport-v2 key-exchange response is invalid")]
    InvalidKeyExchange,
    #[error("transport-v2 response binding does not match the request")]
    BindingMismatch,
    #[error("transport-v2 stream framing is invalid")]
    InvalidStreamFraming,
    #[error("transport-v2 stream record is invalid")]
    InvalidStreamRecord,
    #[error("transport-v2 stream ended without an authenticated terminal record")]
    TruncatedStream,
    #[error("transport-v2 stream is already terminal")]
    StreamAlreadyTerminal,
}

pub(super) type Result<T> = std::result::Result<T, TransportV2Error>;

#[cfg(test)]
mod tests;
