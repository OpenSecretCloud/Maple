use clap::Parser;
use serde::Serialize;
use std::{net::SocketAddr, time::Duration};

pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_STREAM_IDLE_TIMEOUT_SECS: u64 = 300;

#[derive(Parser, Debug, Clone)]
#[command(name = "maple-proxy")]
#[command(about = "Lightweight OpenAI-compatible proxy server for Maple/OpenSecret")]
#[command(version)]
pub struct Config {
    /// Host to bind the server to
    #[arg(long, env = "MAPLE_HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// Port to bind the server to
    #[arg(short, long, env = "MAPLE_PORT", default_value = "8080")]
    pub port: u16,

    /// OpenSecret/Maple backend URL. The packaged binary supplies trust policy
    /// for exact official origins; arbitrary remote HTTPS deployments require
    /// a library integration with an explicit TrustedReleaseConfig.
    #[arg(
        long,
        env = "MAPLE_BACKEND_URL",
        default_value = "https://enclave.trymaple.ai"
    )]
    pub backend_url: String,

    /// Default API key for Maple/OpenSecret (can be overridden by client Authorization header)
    #[arg(long, env = "MAPLE_API_KEY")]
    pub default_api_key: Option<String>,

    /// Enable debug logging
    #[arg(short, long, env = "MAPLE_DEBUG")]
    pub debug: bool,

    /// Enable CORS for all origins (useful for web clients)
    #[arg(long, env = "MAPLE_ENABLE_CORS")]
    pub enable_cors: bool,

    /// Per-attempt timeout through response headers and any buffered
    /// non-streaming body, in seconds. Initial attestation and each inference
    /// call's cumulative SDK recovery budget use at least 15 minutes so a
    /// bounded TUF root-rotation sequence is not truncated by a shorter timeout.
    #[arg(
        long,
        env = "MAPLE_REQUEST_TIMEOUT_SECS",
        default_value_t = DEFAULT_REQUEST_TIMEOUT_SECS,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub request_timeout_secs: u64,

    /// Maximum time to wait between streaming response chunks, in seconds
    #[arg(
        long,
        env = "MAPLE_STREAM_IDLE_TIMEOUT_SECS",
        default_value_t = DEFAULT_STREAM_IDLE_TIMEOUT_SECS,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub stream_idle_timeout_secs: u64,
}

impl Config {
    pub fn socket_addr(&self) -> anyhow::Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .map_err(|e| anyhow::anyhow!("Invalid socket address '{}': {}", addr, e))
    }

    pub fn load() -> Self {
        // Load from .env file if it exists
        let _ = dotenvy::dotenv();

        Config::parse()
    }

    /// Create a new Config programmatically (for library usage)
    pub fn new(host: String, port: u16, backend_url: String) -> Self {
        Self {
            host,
            port,
            backend_url,
            default_api_key: None,
            debug: false,
            enable_cors: false,
            request_timeout_secs: DEFAULT_REQUEST_TIMEOUT_SECS,
            stream_idle_timeout_secs: DEFAULT_STREAM_IDLE_TIMEOUT_SECS,
        }
    }

    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }

    pub fn stream_idle_timeout(&self) -> Duration {
        Duration::from_secs(self.stream_idle_timeout_secs)
    }

    /// Builder-style method to set the API key
    pub fn with_api_key(mut self, api_key: String) -> Self {
        self.default_api_key = Some(api_key);
        self
    }

    /// Builder-style method to enable debug mode
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Builder-style method to enable CORS
    pub fn with_cors(mut self, enable_cors: bool) -> Self {
        self.enable_cors = enable_cors;
        self
    }

    /// Builder-style method to set the per-attempt inference timeout. Values
    /// below 15 minutes do not shorten the separate attestation/recovery floor.
    pub fn with_request_timeout_secs(mut self, request_timeout_secs: u64) -> Self {
        self.request_timeout_secs = request_timeout_secs;
        self
    }

    /// Builder-style method to set the streaming idle timeout
    pub fn with_stream_idle_timeout_secs(mut self, stream_idle_timeout_secs: u64) -> Self {
        self.stream_idle_timeout_secs = stream_idle_timeout_secs;
        self
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAIError {
    error: OpenAIErrorDetails,
}

#[derive(Debug, Serialize)]
struct OpenAIErrorDetails {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    param: Option<String>,
    code: Option<String>,
}

impl OpenAIError {
    fn new(message: impl Into<String>, error_type: impl Into<String>) -> Self {
        Self {
            error: OpenAIErrorDetails {
                message: message.into(),
                error_type: error_type.into(),
                param: None,
                code: None,
            },
        }
    }

    pub(crate) fn authentication_error(message: impl Into<String>) -> Self {
        Self::new(message, "invalid_request_error")
    }

    pub(crate) fn server_error(message: impl Into<String>) -> Self {
        Self::new(message, "server_error")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{error::ErrorKind, Parser};

    #[test]
    fn config_new_uses_timeout_defaults() {
        let config = Config::new(
            "127.0.0.1".to_string(),
            8080,
            "https://enclave.trymaple.ai".to_string(),
        );

        assert_eq!(config.request_timeout_secs, DEFAULT_REQUEST_TIMEOUT_SECS);
        assert_eq!(
            config.stream_idle_timeout_secs,
            DEFAULT_STREAM_IDLE_TIMEOUT_SECS
        );
        assert_eq!(
            config.request_timeout(),
            Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS)
        );
        assert_eq!(
            config.stream_idle_timeout(),
            Duration::from_secs(DEFAULT_STREAM_IDLE_TIMEOUT_SECS)
        );
    }

    #[test]
    fn timeout_builder_methods_override_defaults() {
        let config = Config::new(
            "127.0.0.1".to_string(),
            8080,
            "https://enclave.trymaple.ai".to_string(),
        )
        .with_request_timeout_secs(45)
        .with_stream_idle_timeout_secs(15);

        assert_eq!(config.request_timeout(), Duration::from_secs(45));
        assert_eq!(config.stream_idle_timeout(), Duration::from_secs(15));
    }

    #[test]
    fn timeout_cli_values_must_be_positive() {
        let request_timeout_error =
            Config::try_parse_from(["maple-proxy", "--request-timeout-secs", "0"]).unwrap_err();
        assert_eq!(request_timeout_error.kind(), ErrorKind::ValueValidation);

        let stream_idle_timeout_error =
            Config::try_parse_from(["maple-proxy", "--stream-idle-timeout-secs", "0"]).unwrap_err();
        assert_eq!(stream_idle_timeout_error.kind(), ErrorKind::ValueValidation);
    }
}
