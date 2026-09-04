//! Process entry: picks the startup mode from the command line and runs
//! the desktop window, the ACP agent, or the OpenAI-compatible proxy.
//! Every mode shares the backend, settings, and logging below.

#[cfg(feature = "desktop")]
mod assets;
#[cfg(feature = "desktop")]
mod audio;
mod backend;
mod billing;
#[cfg(feature = "desktop")]
mod desktop;
mod env;
#[cfg(feature = "desktop")]
mod keymap;
#[cfg(feature = "desktop")]
mod notify;
#[cfg(feature = "desktop")]
mod platform;
mod settings;
#[cfg(feature = "desktop")]
mod shortcuts;
#[cfg(feature = "desktop")]
mod ui;
#[cfg(feature = "desktop")]
mod update;

#[cfg(feature = "acp")]
use backend::AgentBackend;
use clap::{Args, Parser, Subcommand};

/// Shown when a mode was compiled out with `--no-default-features`.
#[cfg(not(all(feature = "desktop", feature = "acp", feature = "proxy")))]
fn disabled_mode(mode: &str, feature: &str) -> ! {
    eprintln!("maple-gpui {mode} is not available: this build lacks the `{feature}` feature.");
    std::process::exit(2);
}

/// Command line for the binary. With no subcommand it opens the desktop
/// window; `acp` and `proxy` run headless services.
/// The `--version` string: package version plus the git revision baked in
/// by `build.rs`, so a running binary can be matched back to a checkout.
/// Clap wants a `&'static str` and both inputs are compile-time constants,
/// but `format!` is still runtime, hence the leak of one small string.
fn version_string() -> &'static str {
    let hash = option_env!("MAPLE_GIT_HASH").unwrap_or("unknown");
    Box::leak(format!("{} ({})", env!("CARGO_PKG_VERSION"), hash).into_boxed_str())
}

#[derive(Debug, Parser)]
#[command(name = "maple-gpui", version = version_string(), about, disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    mode: Option<Mode>,
    /// Arguments a desktop launcher may append (file paths, `%u` expansions).
    /// They are ignored so the window still opens.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    desktop_args: Vec<String>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
enum Mode {
    /// Serve the Agent Client Protocol on stdio.
    ///
    /// Trailing arguments belong to the ACP client and are ignored.
    #[command(disable_version_flag = true, disable_help_flag = true)]
    Acp {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        client_args: Vec<String>,
    },
    /// Serve an OpenAI-compatible HTTP endpoint in front of Maple.
    Proxy(ProxyArgs),
}

/// Settings for `maple-gpui proxy`, from flags with environment fallbacks.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
struct ProxyArgs {
    /// Bind address.
    #[arg(long, env = "MAPLE_PROXY_HOST", default_value = "127.0.0.1")]
    host: String,
    /// Bind port.
    #[arg(long, env = "MAPLE_PORT", default_value_t = 8080)]
    port: u16,
    /// Maple API key for requests without an Authorization header.
    ///
    /// Never combined with --cors: a browser-reachable proxy must not
    /// spend a saved credential.
    #[arg(long, env = "MAPLE_API_KEY")]
    api_key: Option<String>,
    /// Allow browser origins; every request must then carry its own key.
    #[arg(
        long,
        env = "MAPLE_ENABLE_CORS",
        value_parser = clap::builder::BoolishValueParser::new(),
    )]
    cors: bool,
}

impl ProxyArgs {
    /// Rejects `--cors` together with a default key. This is checked here
    /// and not with clap's `conflicts_with`: an env var counts as present
    /// to clap even when it holds `0` or `false`.
    fn validate(&self) -> Result<(), String> {
        if self.cors && self.api_key.is_some() {
            return Err(
                "--cors (MAPLE_ENABLE_CORS) cannot be combined with a default API key \
                 (--api-key or MAPLE_API_KEY): a browser-reachable proxy must not spend \
                 a saved credential. Drop --cors or the key."
                    .to_string(),
            );
        }
        Ok(())
    }
}

/// Process start time, for the startup milestone logs. Set once at the
/// top of `main`; `startup_elapsed` reads it from anywhere.
#[cfg(feature = "desktop")]
static PROCESS_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Milliseconds since `main` began.
#[cfg(feature = "desktop")]
pub(crate) fn startup_elapsed() -> u128 {
    PROCESS_START
        .get()
        .map(|start| start.elapsed().as_millis())
        .unwrap_or(0)
}

fn main() {
    // SAFETY: this is the first statement of `main`. No other thread exists
    // yet, so mutating the process environment here cannot race a reader.
    unsafe { maple_agent::prepare_process_environment() };
    #[cfg(feature = "desktop")]
    let _ = PROCESS_START.set(std::time::Instant::now());
    let cli = Cli::parse();
    match cli.mode {
        Some(Mode::Acp { .. }) => {
            // stdout is the ACP channel. Logs go to the log file only, so
            // the client's stderr capture stays quiet. The inherited
            // RUST_LOG belongs to the client (Buzz sets `buzz_acp=info`),
            // so it is ignored here or it would silence Maple's own log.
            #[cfg(not(feature = "acp"))]
            disabled_mode("acp", "acp");
            #[cfg(feature = "acp")]
            {
                init_logging(LogOutput::FileOnly);
                if let Err(error) = run_acp() {
                    log::error!("{error}");
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        Some(Mode::Proxy(args)) => {
            if let Err(message) = args.validate() {
                eprintln!("{message}");
                std::process::exit(2);
            }
            #[cfg(not(feature = "proxy"))]
            {
                let _ = args;
                disabled_mode("proxy", "proxy");
            }
            #[cfg(feature = "proxy")]
            {
                init_logging(LogOutput::FileAndStderr);
                if let Err(error) = run_proxy(args) {
                    log::error!("{error}");
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        None => {
            #[cfg(feature = "desktop")]
            desktop::run();
            #[cfg(not(feature = "desktop"))]
            disabled_mode("(desktop)", "desktop");
        }
    }
}

/// `maple-gpui proxy`: an OpenAI-compatible endpoint in front of Maple's
/// enclave, for tools that speak the OpenAI API. Runs until killed.
#[cfg(feature = "proxy")]
fn run_proxy(args: ProxyArgs) -> Result<(), String> {
    use axum::http::{Method, StatusCode, header::ORIGIN};
    use axum::response::IntoResponse;
    use tower_http::cors::{AllowHeaders, Any, CorsLayer};

    let backend_url = maple_agent::maple_api::validate_api_url(&configured_api_url())?;
    let pcr0 = maple_agent::open_secret_config::configured_pcr0_environment()?;
    let mut config = maple_proxy::Config::new(args.host.clone(), args.port, backend_url)
        .with_pcr0_environment(pcr0)
        .with_debug(false)
        // The browser boundary is owned below so the Authorization header
        // can be allowed explicitly and browser requests rejected when CORS
        // is off; maple-proxy's permissive layer stays out.
        .with_cors(false);
    if let Some(api_key) = args.api_key.clone() {
        config = config.with_api_key(api_key);
    }
    let addr = config
        .socket_addr()
        .map_err(|error| format!("Invalid proxy address: {error}"))?;
    let app = maple_proxy::create_app(config);
    let app = if args.cors {
        app.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                // Authorization is not covered by `*` under Fetch; mirror
                // the preflight list so bearer keys and SDK headers pass.
                .allow_headers(AllowHeaders::mirror_request()),
        )
    } else {
        // Disabling CORS alone only hides responses. A no-cors browser POST
        // can still reach loopback and spend the saved key, so fail closed
        // on browser-only headers before the body is read.
        app.layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                if request.headers().contains_key(ORIGIN)
                    || request.headers().contains_key("sec-fetch-site")
                {
                    return StatusCode::FORBIDDEN.into_response();
                }
                next.run(request).await
            },
        ))
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Failed to start the proxy runtime: {error}"))?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|error| format!("Failed to bind {addr}: {error}"))?;
        log::info!(
            "Maple proxy listening on http://{addr} (cors={}, default key={})",
            args.cors,
            args.api_key.is_some()
        );
        eprintln!("Maple proxy listening on http://{addr}");
        eprintln!("  GET  /v1/models    POST /v1/chat/completions    POST /v1/embeddings");
        if args.api_key.is_none() {
            eprintln!("  Requests must send: Authorization: Bearer <Maple API key>");
        }
        axum::serve(listener, app)
            .await
            .map_err(|error| format!("Proxy server error: {error}"))
    })
}

fn configured_api_url() -> String {
    env::env_string("MAPLE_API_URL").unwrap_or_else(|| "https://enclave.trymaple.ai".to_string())
}

/// `maple-gpui acp`: a standalone ACP agent over stdio. It reuses the
/// sign-in saved by the desktop app and hosts its own agent runtime, so
/// the desktop app does not need to run.
#[cfg(feature = "acp")]
fn run_acp() -> Result<(), String> {
    let harness_instructions = settings::load_settings().effective_harness_instructions();
    let backend = AgentBackend::new(configured_api_url(), harness_instructions)?;
    let user_id = backend.restore_now().ok_or_else(|| {
        "No saved Maple sign-in. Open the desktop app and sign in first.".to_string()
    })?;
    backend.run_acp_stdio(&user_id)
}

/// Log to stderr and to `<data dir>/logs/maple-gpui.log` so a freeze or
/// crash leaves evidence on disk. `RUST_LOG` still controls the level;
/// the default is `info`. Panics are logged as well.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LogOutput {
    /// Log file plus stderr, with `RUST_LOG` honored. The desktop default.
    FileAndStderr,
    /// Log file only, with the default filter. For stdio protocol modes.
    #[cfg(feature = "acp")]
    FileOnly,
}

fn init_logging(output: LogOutput) {
    let log_dir = backend::local_data_root().join("logs");
    let file = std::fs::create_dir_all(&log_dir).ok().and_then(|_| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("maple-gpui.log"))
            .ok()
    });
    // goose is chatty at info during a run; its warnings still show.
    const DEFAULT_FILTER: &str = "info,goose=warn";
    let mut builder = match output {
        LogOutput::FileAndStderr => env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or(DEFAULT_FILTER),
        ),
        #[cfg(feature = "acp")]
        LogOutput::FileOnly => {
            let mut builder = env_logger::Builder::new();
            builder.parse_filters(DEFAULT_FILTER);
            builder
        }
    };
    let stderr = output == LogOutput::FileAndStderr;
    if let Some(file) = file {
        builder.target(env_logger::Target::Pipe(Box::new(TeeWriter {
            file: std::io::BufWriter::with_capacity(16 * 1024, file),
            stderr: stderr.then(std::io::stderr),
        })));
    }
    builder.init();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));
    log::info!(
        "maple-gpui {} starting; log file: {}",
        env!("CARGO_PKG_VERSION"),
        log_dir.join("maple-gpui.log").display()
    );
}

/// Buffered file sink plus stderr. Warnings and errors flush the file at
/// once so a crash still leaves them on disk; info lines are batched.
struct TeeWriter {
    file: std::io::BufWriter<std::fs::File>,
    stderr: Option<std::io::Stderr>,
}

impl std::io::Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.file.write_all(buf);
        let urgent = buf.windows(4).any(|w| w == b"WARN" || w == b"ERRO");
        if urgent || buf.windows(5).any(|w| w == b"panic") {
            let _ = self.file.flush();
        }
        if let Some(stderr) = &mut self.stderr {
            stderr.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = self.file.flush();
        match &mut self.stderr {
            Some(stderr) => stderr.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, Mode};
    use clap::Parser;

    fn parse(args: &[&str]) -> Result<Option<Mode>, clap::Error> {
        Cli::try_parse_from(std::iter::once("maple-gpui").chain(args.iter().copied()))
            .map(|cli| cli.mode)
    }

    #[test]
    fn version_flags_print_the_package_version() {
        let expected = format!(
            "maple-gpui {} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("MAPLE_GIT_HASH").unwrap_or("unknown")
        );
        for flag in ["--version", "-V"] {
            let error = parse(&[flag]).expect_err("version exits early");
            assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
            assert_eq!(error.to_string().trim(), expected);
        }
    }

    #[test]
    fn acp_subcommand_keeps_precedence_over_following_flags() {
        assert!(matches!(parse(&["acp"]), Ok(Some(Mode::Acp { .. }))));
        assert!(matches!(
            parse(&["acp", "--version"]),
            Ok(Some(Mode::Acp { .. }))
        ));
    }

    #[test]
    fn proxy_flags_parse_with_defaults() {
        let Ok(Some(Mode::Proxy(args))) = parse(&["proxy"]) else {
            panic!("proxy without flags must parse");
        };
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.port, 8080);
        assert!(!args.cors);

        let Ok(Some(Mode::Proxy(args))) =
            parse(&["proxy", "--host", "0.0.0.0", "--port", "9999", "--cors"])
        else {
            panic!("proxy flags must parse");
        };
        assert_eq!(
            (args.host.as_str(), args.port, args.cors),
            ("0.0.0.0", 9999, true)
        );

        assert!(parse(&["proxy", "--port", "x"]).is_err());
        assert!(parse(&["proxy", "--bogus"]).is_err());
        let Ok(Some(Mode::Proxy(args))) = parse(&["proxy", "--cors", "--api-key", "k"]) else {
            panic!("clap accepts the pair; validate rejects it");
        };
        assert!(args.validate().is_err());
        let Ok(Some(Mode::Proxy(args))) = parse(&["proxy", "--api-key", "k"]) else {
            panic!("key alone must parse");
        };
        assert!(args.validate().is_ok());
    }

    #[test]
    fn other_arguments_keep_desktop_startup() {
        assert!(matches!(parse(&[]), Ok(None)));
        assert!(matches!(parse(&["--unknown"]), Ok(None)));
        assert!(matches!(parse(&["some/file.txt"]), Ok(None)));
    }
}
