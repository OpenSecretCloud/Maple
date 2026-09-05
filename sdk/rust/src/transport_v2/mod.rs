//! Private client engine for OpenSecret transport v2.
//!
//! [`crate::OpenSecretClient`] is the stable public adapter. Keeping the wire
//! primitives private prevents protocol details from becoming an accidental
//! compatibility surface.

mod auth_bundle;
mod crypto;
mod envelope;
mod network;
mod runtime;
mod session;
mod stream;

pub(crate) use auth_bundle::{
    decode_auth_bundle, encode_auth_bundle, validate_v2_user_token_pair, ValidatedUserTokenPair,
};
pub(crate) use envelope::{
    CacheNamespaceRoot, Credential, HeaderField, LogicalMethod, LogicalRequest, ResponseMode,
};
pub(crate) use network::{TransportV2Client, V2HttpResponse};
pub(crate) use runtime::ApiKeyScope;
pub(crate) use session::V2Session;

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

impl From<TransportV2Error> for crate::error::Error {
    fn from(error: TransportV2Error) -> Self {
        match error {
            TransportV2Error::SessionExpired
            | TransportV2Error::RequestRecordBudgetExhausted
            | TransportV2Error::ResponseRecordBudgetExhausted
            | TransportV2Error::SessionStateUnavailable => Self::Session(error.to_string()),
            TransportV2Error::InvalidRequest | TransportV2Error::LimitExceeded { .. } => {
                Self::Configuration(error.to_string())
            }
            TransportV2Error::InvalidKeyExchange
            | TransportV2Error::NonContributoryKeyExchange
            | TransportV2Error::KeyDerivationFailed => Self::KeyExchange(error.to_string()),
            TransportV2Error::EncryptionFailed | TransportV2Error::RandomnessUnavailable => {
                Self::Encryption(error.to_string())
            }
            TransportV2Error::AuthenticationFailed
            | TransportV2Error::RecordTooShort
            | TransportV2Error::InvalidEncoding
            | TransportV2Error::InvalidJson
            | TransportV2Error::InvalidResponse
            | TransportV2Error::ResponseModeMismatch
            | TransportV2Error::RequestIdCollision
            | TransportV2Error::BindingMismatch
            | TransportV2Error::InvalidStreamFraming
            | TransportV2Error::InvalidStreamRecord
            | TransportV2Error::TruncatedStream
            | TransportV2Error::StreamAlreadyTerminal => Self::InvalidResponse(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests;
