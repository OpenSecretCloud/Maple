use thiserror::Error;

/// Phase whose explicit inference-request time budget was exhausted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferenceTimeoutPhase {
    /// Request transmission, response headers, or buffered response body.
    Ordinary,
    /// Session or authentication recovery, including dynamic trust refresh and reattestation.
    Recovery,
}

impl std::fmt::Display for InferenceTimeoutPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Ordinary => "ordinary",
            Self::Recovery => "recovery",
        })
    }
}

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

    #[error(
        "No published trusted enclave release is available for attestation environment '{environment}'"
    )]
    UnreleasedAttestationPolicy { environment: String },

    #[error("Trusted enclave release policy is invalid: {0}")]
    TrustedReleasePolicy(String),

    #[error("Trusted enclave release policy network is unavailable: {0}")]
    TrustedReleaseNetwork(String),

    #[error("Inference {phase} phase timed out after {timeout_secs} seconds")]
    InferenceTimeout {
        phase: InferenceTimeoutPhase,
        timeout_secs: u64,
    },

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
