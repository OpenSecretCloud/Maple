use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("CBOR error: {0}")]
    Cbor(String),

    #[error("Cryptographic error: {0}")]
    Crypto(String),

    #[error("Attestation verification failed: {0}")]
    AttestationVerificationFailed(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Key exchange failed: {0}")]
    KeyExchange(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Decryption error: {0}")]
    Decryption(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("API error: {status}: {message}")]
    Api { status: u16, message: String },

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("UTF-8 conversion error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("Base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
