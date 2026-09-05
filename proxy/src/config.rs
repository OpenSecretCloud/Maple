use clap::{
    error::{ContextKind, ContextValue},
    Parser,
};
use opensecret::{Pcr0Environment, TransportV2CacheNamespaceRoot};
use serde::Serialize;
use std::{ffi::OsString, net::SocketAddr, time::Duration};

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

    /// OpenSecret/Maple backend URL
    #[arg(
        long,
        env = "MAPLE_BACKEND_URL",
        default_value = "https://enclave.trymaple.ai"
    )]
    pub backend_url: String,

    /// PCR0 trust-root environment for backend attestation
    #[arg(
        long,
        env = "MAPLE_PCR0_ENVIRONMENT",
        default_value = "production",
        value_parser = parse_pcr0_environment
    )]
    pub pcr0_environment: Pcr0Environment,

    /// Default API key for Maple/OpenSecret (can be overridden by client Authorization header)
    #[arg(long, env = "MAPLE_API_KEY", hide_env_values = true)]
    pub default_api_key: Option<String>,

    /// Optional stable transport-v2 provider-cache namespace root.
    ///
    /// This is a secret, canonical padded-base64 encoding of exactly 32 bytes.
    /// When omitted, the proxy generates one root for the lifetime of the
    /// process and shares it across all API-key clients.
    #[arg(
        long,
        env = "MAPLE_CACHE_NAMESPACE_ROOT",
        hide_env_values = true,
        value_parser = parse_cache_namespace_root
    )]
    pub cache_namespace_root: Option<TransportV2CacheNamespaceRoot>,

    /// Enable debug logging
    #[arg(short, long, env = "MAPLE_DEBUG")]
    pub debug: bool,

    /// Enable CORS for all origins (useful for web clients)
    #[arg(long, env = "MAPLE_ENABLE_CORS")]
    pub enable_cors: bool,

    /// Timeout for backend request setup and non-streaming responses, in seconds
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
    /// Parse process arguments while ensuring secret cache-root values never
    /// appear in clap's rendered validation errors.
    pub fn parse() -> Self {
        match Self::try_parse() {
            Ok(config) => config,
            Err(error) => error.exit(),
        }
    }

    /// Parse process arguments and return a redacted validation error.
    pub fn try_parse() -> Result<Self, clap::Error> {
        Self::try_parse_from(std::env::args_os())
    }

    /// Parse supplied arguments, exiting with a redacted validation error.
    pub fn parse_from<I, T>(arguments: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        match Self::try_parse_from(arguments) {
            Ok(config) => config,
            Err(error) => error.exit(),
        }
    }

    /// Parse supplied arguments with cache-root validation errors redacted.
    pub fn try_parse_from<I, T>(arguments: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        <Self as Parser>::try_parse_from(arguments).map_err(redact_cache_namespace_root_error)
    }

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
            pcr0_environment: Pcr0Environment::Production,
            default_api_key: None,
            cache_namespace_root: None,
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

    /// Builder-style method to select the backend PCR0 trust-root environment
    pub fn with_pcr0_environment(mut self, pcr0_environment: Pcr0Environment) -> Self {
        self.pcr0_environment = pcr0_environment;
        self
    }

    /// Builder-style method to set the API key
    pub fn with_api_key(mut self, api_key: String) -> Self {
        self.default_api_key = Some(api_key);
        self
    }

    /// Supply a caller-persisted transport-v2 provider-cache namespace root.
    pub fn with_cache_namespace_root(
        mut self,
        cache_namespace_root: TransportV2CacheNamespaceRoot,
    ) -> Self {
        self.cache_namespace_root = Some(cache_namespace_root);
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

    /// Builder-style method to set the backend request timeout
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

fn parse_pcr0_environment(value: &str) -> Result<Pcr0Environment, String> {
    match value {
        "production" => Ok(Pcr0Environment::Production),
        "development" => Ok(Pcr0Environment::Development),
        _ => Err("PCR0 environment must be 'production' or 'development'".to_string()),
    }
}

fn parse_cache_namespace_root(value: &str) -> Result<TransportV2CacheNamespaceRoot, String> {
    TransportV2CacheNamespaceRoot::from_base64(value).map_err(|_| {
        "cache namespace root must be canonical padded base64 for exactly 32 bytes".to_string()
    })
}

fn redact_cache_namespace_root_error(mut error: clap::Error) -> clap::Error {
    let is_cache_namespace_root = matches!(
        error.get(ContextKind::InvalidArg),
        Some(ContextValue::String(argument)) if argument.contains("--cache-namespace-root")
    );
    if is_cache_namespace_root {
        error.insert(
            ContextKind::InvalidValue,
            ContextValue::String("[REDACTED]".to_string()),
        );
    }
    error
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
    use clap::{error::ErrorKind, CommandFactory};
    use std::sync::Mutex;

    static CONFIG_PARSING_LOCK: Mutex<()> = Mutex::new(());

    struct EnvironmentRestore {
        name: &'static str,
        previous: Option<OsString>,
    }

    impl Drop for EnvironmentRestore {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var(self.name, previous),
                None => std::env::remove_var(self.name),
            }
        }
    }

    fn with_environment_variable<T>(
        name: &'static str,
        value: Option<&str>,
        run: impl FnOnce() -> T,
    ) -> T {
        let _guard = CONFIG_PARSING_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _pcr0_restore = EnvironmentRestore {
            name: "MAPLE_PCR0_ENVIRONMENT",
            previous: std::env::var_os("MAPLE_PCR0_ENVIRONMENT"),
        };
        let _cache_root_restore = EnvironmentRestore {
            name: "MAPLE_CACHE_NAMESPACE_ROOT",
            previous: std::env::var_os("MAPLE_CACHE_NAMESPACE_ROOT"),
        };

        std::env::remove_var("MAPLE_PCR0_ENVIRONMENT");
        std::env::remove_var("MAPLE_CACHE_NAMESPACE_ROOT");
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }

        run()
    }

    fn try_parse_config_from<I, T>(arguments: I) -> Result<Config, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let _guard = CONFIG_PARSING_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _pcr0_restore = EnvironmentRestore {
            name: "MAPLE_PCR0_ENVIRONMENT",
            previous: std::env::var_os("MAPLE_PCR0_ENVIRONMENT"),
        };
        let _cache_root_restore = EnvironmentRestore {
            name: "MAPLE_CACHE_NAMESPACE_ROOT",
            previous: std::env::var_os("MAPLE_CACHE_NAMESPACE_ROOT"),
        };
        std::env::remove_var("MAPLE_PCR0_ENVIRONMENT");
        std::env::remove_var("MAPLE_CACHE_NAMESPACE_ROOT");
        Config::try_parse_from(arguments)
    }

    fn with_pcr0_environment_env<T>(value: Option<&str>, run: impl FnOnce() -> T) -> T {
        with_environment_variable("MAPLE_PCR0_ENVIRONMENT", value, run)
    }

    fn with_cache_namespace_root_env<T>(value: Option<&str>, run: impl FnOnce() -> T) -> T {
        with_environment_variable("MAPLE_CACHE_NAMESPACE_ROOT", value, run)
    }

    #[test]
    fn config_new_uses_timeout_defaults() {
        let config = Config::new(
            "127.0.0.1".to_string(),
            8080,
            "https://enclave.trymaple.ai".to_string(),
        );

        assert_eq!(config.request_timeout_secs, DEFAULT_REQUEST_TIMEOUT_SECS);
        assert_eq!(config.pcr0_environment, Pcr0Environment::Production);
        assert!(config.cache_namespace_root.is_none());
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
    fn pcr0_environment_defaults_to_production_for_cli() {
        let config =
            with_pcr0_environment_env(None, || Config::try_parse_from(["maple-proxy"]).unwrap());

        assert_eq!(config.pcr0_environment, Pcr0Environment::Production);
    }

    #[test]
    fn pcr0_environment_accepts_explicit_development_cli_value() {
        let config =
            try_parse_config_from(["maple-proxy", "--pcr0-environment", "development"]).unwrap();

        assert_eq!(config.pcr0_environment, Pcr0Environment::Development);
    }

    #[test]
    fn pcr0_environment_builder_selects_development() {
        let config = Config::new(
            "127.0.0.1".to_string(),
            8080,
            "https://enclave.secretgpt.ai".to_string(),
        )
        .with_pcr0_environment(Pcr0Environment::Development);

        assert_eq!(config.pcr0_environment, Pcr0Environment::Development);
    }

    #[test]
    fn pcr0_environment_accepts_explicit_development_env_value() {
        let config = with_pcr0_environment_env(Some("development"), || {
            Config::try_parse_from(["maple-proxy"]).unwrap()
        });

        assert_eq!(config.pcr0_environment, Pcr0Environment::Development);
    }

    #[test]
    fn pcr0_environment_rejects_unknown_values() {
        let error =
            try_parse_config_from(["maple-proxy", "--pcr0-environment", "staging"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::ValueValidation);
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
            try_parse_config_from(["maple-proxy", "--request-timeout-secs", "0"]).unwrap_err();
        assert_eq!(request_timeout_error.kind(), ErrorKind::ValueValidation);

        let stream_idle_timeout_error =
            try_parse_config_from(["maple-proxy", "--stream-idle-timeout-secs", "0"]).unwrap_err();
        assert_eq!(stream_idle_timeout_error.kind(), ErrorKind::ValueValidation);
    }

    #[test]
    fn cache_namespace_root_accepts_canonical_cli_and_builder_values() {
        let encoded = TransportV2CacheNamespaceRoot::from_bytes([0x42; 32]).to_base64();
        let cli =
            try_parse_config_from(["maple-proxy", "--cache-namespace-root", encoded.as_str()])
                .unwrap();
        assert_eq!(
            cli.cache_namespace_root.as_ref().unwrap().to_base64(),
            encoded
        );

        let built = Config::new(
            "127.0.0.1".to_string(),
            8080,
            "https://enclave.trymaple.ai".to_string(),
        )
        .with_cache_namespace_root(TransportV2CacheNamespaceRoot::from_bytes([0x42; 32]));
        assert_eq!(
            built.cache_namespace_root.as_ref().unwrap().to_base64(),
            encoded
        );
    }

    #[test]
    fn cache_namespace_root_accepts_canonical_environment_value() {
        let encoded = TransportV2CacheNamespaceRoot::from_bytes([0x24; 32]).to_base64();
        let config = with_cache_namespace_root_env(Some(&encoded), || {
            Config::try_parse_from(["maple-proxy"]).unwrap()
        });

        assert_eq!(
            config.cache_namespace_root.as_ref().unwrap().to_base64(),
            encoded
        );
    }

    #[test]
    fn cache_namespace_root_rejects_noncanonical_or_wrong_length_values() {
        for value in [
            "not-base64",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            let error = try_parse_config_from(["maple-proxy", "--cache-namespace-root", value])
                .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation);
            let rendered = error.to_string();
            assert!(rendered.contains("[REDACTED]"));
            assert!(!rendered.contains(value));
        }
    }

    #[test]
    fn cache_namespace_root_environment_errors_redact_the_value() {
        let value = "ENV_CACHE_NAMESPACE_ROOT_CANARY";
        let error = with_cache_namespace_root_env(Some(value), || {
            Config::try_parse_from(["maple-proxy"]).unwrap_err()
        });

        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        let rendered = error.to_string();
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains(value));
    }

    #[test]
    fn config_debug_redacts_cache_namespace_root() {
        let root = TransportV2CacheNamespaceRoot::from_bytes([0x5a; 32]);
        let encoded = root.to_base64();
        let config = Config::new(
            "127.0.0.1".to_string(),
            8080,
            "https://enclave.trymaple.ai".to_string(),
        )
        .with_cache_namespace_root(root);

        let rendered = format!("{config:?}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains(&encoded));
    }

    #[test]
    fn help_hides_secret_environment_values() {
        let command = Config::command();

        for id in ["default_api_key", "cache_namespace_root"] {
            let argument = command
                .get_arguments()
                .find(|argument| argument.get_id().as_str() == id)
                .unwrap();
            assert!(argument.is_hide_env_values_set());
        }
    }
}
