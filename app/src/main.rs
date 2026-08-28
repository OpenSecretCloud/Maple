//! Root application: login/chat routing, backend ownership, and the event
//! pump that forwards agent service events into the active chat screen.

mod assets;
mod backend;
mod billing;
mod notify;
mod platform;
mod settings;
mod ui;

use std::sync::Arc;

use gpui::{
    App, Application, Bounds, Context, Entity, KeyBinding, Pixels, Render, Window, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, size,
};

actions!(maple_app, [QuitApp]);

use backend::AgentBackend;
use ui::chat::{ChatScreen, LoggedOut};
use ui::login::{LoginScreen, LoginSucceeded};
use ui::settings::{Section, SettingsClosed, SettingsScreen, SignOutRequested};
use ui::text_input;
use ui::titlebar::TitleBar;

enum Screen {
    Login(Entity<LoginScreen>),
    Chat(Entity<ChatScreen>),
    Settings(Entity<SettingsScreen>),
}

struct MapleApp {
    backend: Arc<AgentBackend>,
    screen: Screen,
    user_id: Option<String>,
    /// The chat screen is parked while settings is open so Back returns to
    /// it with its state intact.
    parked_chat: Option<Entity<ChatScreen>>,
    settings: crate::settings::AppSettings,
    /// Created once; re-creating it per render leaked an entity per frame.
    titlebar: Entity<TitleBar>,
}

impl MapleApp {
    /// Forward a batch of backend service events to the chat screen when
    /// one exists. One batch is one render, however many events arrived.
    fn handle_service_events(
        &mut self,
        events: Vec<maple_agent::agent::AgentServiceEvent>,
        cx: &mut Context<Self>,
    ) {
        if let Screen::Chat(chat) = &self.screen {
            chat.update(cx, |chat, cx| chat.handle_service_events(events, cx));
        }
    }

    /// Tear down the chat screen and return to a fresh login form.
    fn show_login(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let login = cx.new(|cx| LoginScreen::new(backend, cx));
        self.screen = Screen::Login(login);
        self.user_id = None;
        self.parked_chat = None;
        cx.notify();
    }

    /// Park the chat screen and show settings.
    fn show_settings(&mut self, section: Section, cx: &mut Context<Self>) {
        let Screen::Chat(chat) = &self.screen else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone().unwrap_or_default();
        let settings = self.settings.clone();
        let screen = cx.new(|cx| SettingsScreen::new(backend, user_id, settings, section, cx));
        cx.subscribe(
            &screen,
            |app: &mut MapleApp, _emitter, event: &SettingsClosed, cx| {
                app.close_settings(event.0.clone(), cx);
            },
        )
        .detach();
        cx.subscribe(
            &screen,
            |app: &mut MapleApp, _emitter, _event: &SignOutRequested, cx| {
                if let Some(chat) = app.parked_chat.take() {
                    chat.update(cx, |chat, cx| chat.sign_out(cx));
                }
                app.show_login(cx);
            },
        )
        .detach();
        self.parked_chat = Some(chat.clone());
        self.screen = Screen::Settings(screen);
        cx.notify();
    }

    /// Wire per-chat events: sign-out and the settings gear.
    fn subscribe_chat(&mut self, chat: &Entity<ChatScreen>, cx: &mut Context<Self>) {
        cx.subscribe(
            chat,
            |app: &mut MapleApp, _emitter, _event: &LoggedOut, cx| {
                app.show_login(cx);
            },
        )
        .detach();
        cx.subscribe(
            chat,
            |app: &mut MapleApp, _emitter, _event: &ui::chat::OpenSettings, cx| {
                app.show_settings(Section::General, cx);
            },
        )
        .detach();
        cx.subscribe(
            chat,
            |app: &mut MapleApp, _emitter, event: &ui::settings::OpenSettingsSection, cx| {
                app.show_settings(event.0, cx);
            },
        )
        .detach();
    }

    /// Restore the parked chat screen, applying any changed defaults.
    fn close_settings(&mut self, updated: crate::settings::AppSettings, cx: &mut Context<Self>) {
        self.settings = updated;
        if let Some(chat) = self.parked_chat.take() {
            let settings = self.settings.clone();
            chat.update(cx, |chat, cx| chat.apply_defaults(&settings, cx));
            self.screen = Screen::Chat(chat);
        }
        cx.notify();
    }
}

impl Render for MapleApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let titlebar = self.titlebar.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family(assets::FONT_BODY)
            .child(titlebar)
            .child(match &self.screen {
                Screen::Login(login) => login.clone().into_any_element(),
                Screen::Chat(chat) => chat.clone().into_any_element(),
                Screen::Settings(screen) => screen.clone().into_any_element(),
            })
    }
}

/// What the process does, decided from the first command-line argument.
#[derive(Debug, PartialEq, Eq)]
enum StartupMode {
    /// `maple-gpui acp`: serve the Agent Client Protocol on stdio.
    Acp,
    /// `maple-gpui proxy`: serve an OpenAI-compatible HTTP endpoint.
    Proxy(Result<ProxyArgs, String>),
    Version,
    Desktop,
}

/// Settings for `maple-gpui proxy`, from flags with environment fallbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyArgs {
    host: String,
    port: u16,
    /// Maple API key used when a request carries no `Authorization` header.
    /// Never combined with `cors`: browser-reachable endpoints must not
    /// spend a saved credential.
    api_key: Option<String>,
    /// Accept requests from any browser origin.
    cors: bool,
}

fn startup_mode(args: impl IntoIterator<Item = String>) -> StartupMode {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("acp") => StartupMode::Acp,
        Some("proxy") => StartupMode::Proxy(parse_proxy_args(args)),
        Some("--version" | "-V") => StartupMode::Version,
        _ => StartupMode::Desktop,
    }
}

const PROXY_USAGE: &str =
    "usage: maple-gpui proxy [--host HOST] [--port PORT] [--api-key KEY] [--cors]
  --host HOST     bind address (default 127.0.0.1, env MAPLE_PROXY_HOST)
  --port PORT     bind port (default 8080, env MAPLE_PROXY_PORT)
  --api-key KEY   Maple API key for requests without an Authorization header
                  (env MAPLE_API_KEY); not allowed together with --cors
  --cors          allow browser origins; every request must then carry its own key";

fn parse_proxy_args(args: impl Iterator<Item = String>) -> Result<ProxyArgs, String> {
    let env = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    };
    let mut host = env("MAPLE_PROXY_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let mut port = match env("MAPLE_PROXY_PORT") {
        Some(value) => value
            .parse::<u16>()
            .map_err(|_| format!("MAPLE_PROXY_PORT is not a port number: {value}"))?,
        None => 8080,
    };
    let mut api_key = env("MAPLE_API_KEY");
    let mut cors = false;
    let mut args = args;
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            args.next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("{flag} needs a value\n{PROXY_USAGE}"))
        };
        match arg.as_str() {
            "--host" => host = value("--host")?,
            "--port" => {
                let text = value("--port")?;
                port = text
                    .parse::<u16>()
                    .map_err(|_| format!("--port is not a port number: {text}"))?;
            }
            "--api-key" => api_key = Some(value("--api-key")?),
            "--cors" => cors = true,
            "--help" | "-h" => return Err(PROXY_USAGE.to_string()),
            other => return Err(format!("unknown argument: {other}\n{PROXY_USAGE}")),
        }
    }
    if cors && api_key.is_some() {
        return Err(
            "--cors cannot be combined with a default API key (--api-key or MAPLE_API_KEY): \
             a browser-reachable proxy must not spend a saved credential. Drop --cors or the key."
                .to_string(),
        );
    }
    Ok(ProxyArgs {
        host,
        port,
        api_key,
        cors,
    })
}

fn version_text() -> &'static str {
    concat!("maple-gpui ", env!("CARGO_PKG_VERSION"))
}

fn main() {
    match startup_mode(std::env::args().skip(1)) {
        StartupMode::Acp => {
            // stdout is the ACP channel. Logs go to the log file only, so
            // the client's stderr capture stays quiet. The inherited
            // RUST_LOG belongs to the client (Buzz sets `buzz_acp=info`),
            // so it is ignored here or it would silence Maple's own log.
            init_logging(LogOutput::FileOnly);
            if let Err(error) = run_acp() {
                log::error!("{error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        StartupMode::Proxy(args) => {
            let args = match args {
                Ok(args) => args,
                Err(message) => {
                    eprintln!("{message}");
                    std::process::exit(if message == PROXY_USAGE { 0 } else { 2 });
                }
            };
            init_logging(LogOutput::FileAndStderr);
            if let Err(error) = run_proxy(args) {
                log::error!("{error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        StartupMode::Version => println!("{}", version_text()),
        StartupMode::Desktop => run_desktop(),
    }
}

/// `maple-gpui proxy`: an OpenAI-compatible endpoint in front of Maple's
/// enclave, for tools that speak the OpenAI API. Runs until killed.
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
    std::env::var("MAPLE_API_URL").unwrap_or_else(|_| "https://enclave.trymaple.ai".to_string())
}

/// `maple-gpui acp`: a standalone ACP agent over stdio. It reuses the
/// sign-in saved by the desktop app and hosts its own agent runtime, so
/// the desktop app does not need to run.
fn run_acp() -> Result<(), String> {
    let harness_instructions = settings::load_settings().effective_harness_instructions();
    let backend = AgentBackend::new(configured_api_url(), harness_instructions)?;
    let user_id = backend.restore_now().ok_or_else(|| {
        "No saved Maple sign-in. Open the desktop app and sign in first.".to_string()
    })?;
    backend.run_acp_stdio(&user_id)
}

fn run_desktop() {
    init_logging(LogOutput::FileAndStderr);

    let backend = Arc::new(
        AgentBackend::new(
            configured_api_url(),
            settings::load_settings().effective_harness_instructions(),
        )
        .expect("failed to initialize agent backend"),
    );

    // Restore a persisted session before the UI starts so sign-in can be
    // skipped entirely when the credentials are still valid.
    let restored_user = backend.restore_now();

    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            if let Err(error) = cx.text_system().add_fonts(
                assets::FONTS
                    .iter()
                    .map(|bytes| std::borrow::Cow::Borrowed(*bytes))
                    .collect(),
            ) {
                log::warn!("failed to register bundled fonts: {error}");
            }
            text_input::register_key_bindings(cx);
            cx.on_action(|_: &QuitApp, cx| cx.quit());
            cx.bind_keys([
                KeyBinding::new("ctrl-q", QuitApp, None),
                KeyBinding::new("cmd-q", QuitApp, None),
                KeyBinding::new("escape", ui::chat::ChatEscape, Some("Chat")),
                KeyBinding::new("ctrl-c", ui::chat::CopySelection, Some("Transcript")),
                KeyBinding::new("cmd-c", ui::chat::CopySelection, Some("Transcript")),
            ]);
            let bounds: Bounds<Pixels> = Bounds::centered(None, size(px(1280.), px(860.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: Some(
                                format!("Maple v{}", ui::titlebar::TitleBar::version()).into(),
                            ),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    |_, cx| {
                        if let Some(user_id) = restored_user.clone() {
                            let chat =
                                cx.new(|cx| ChatScreen::new(backend.clone(), user_id.clone(), cx));

                            cx.new(|cx| {
                                let mut app = MapleApp {
                                    backend: backend.clone(),
                                    screen: Screen::Chat(chat.clone()),
                                    user_id: Some(user_id),
                                    parked_chat: None,
                                    settings: crate::settings::load_settings(),
                                    titlebar: cx.new(|_| TitleBar::new("Maple - Private AI Chat")),
                                };
                                app.subscribe_chat(&chat, cx);
                                app
                            })
                        } else {
                            let login = cx.new(|cx| LoginScreen::new(backend.clone(), cx));
                            cx.new(|cx| MapleApp {
                                backend: backend.clone(),
                                screen: Screen::Login(login.clone()),
                                user_id: None,
                                parked_chat: None,
                                settings: crate::settings::load_settings(),
                                titlebar: cx.new(|_| TitleBar::new("Maple - Private AI Chat")),
                            })
                        }
                    },
                )
                .expect("failed to open main window");
            let root = window
                .update(cx, |_, _, cx| cx.entity())
                .expect("root entity");

            window
                .update(cx, |app: &mut MapleApp, _window, cx| {
                    let Screen::Login(login) = &app.screen else {
                        return;
                    };
                    // A restored session starts on the chat screen and needs
                    // the same sign-out routing.
                    if let Screen::Chat(chat) = &app.screen {
                        cx.subscribe(
                            chat,
                            |app: &mut MapleApp, _emitter, _event: &LoggedOut, cx| {
                                app.show_login(cx);
                            },
                        )
                        .detach();
                        return;
                    }
                    cx.subscribe(login, {
                        let backend = backend.clone();
                        move |app: &mut MapleApp, _emitter, event: &LoginSucceeded, cx| {
                            let user_id = event.0.clone();
                            let chat =
                                cx.new(|cx| ChatScreen::new(backend.clone(), user_id.clone(), cx));
                            app.user_id = Some(user_id);
                            app.screen = Screen::Chat(chat.clone());
                            app.subscribe_chat(&chat, cx);
                            cx.notify();
                        }
                    })
                    .detach();
                })
                .expect("subscribe login");

            // The event pump runs once for the whole process and routes events to
            // whichever screen is active. It exits when the root entity is gone.
            let (spawn_backend, take_backend) = (backend.clone(), backend.clone());
            let rx = spawn_backend.spawn(async move { take_backend.take_events().await });
            cx.spawn(async move |cx| {
                let Some(mut rx) = rx.await.ok().flatten() else {
                    return;
                };
                while let Some(event) = rx.recv().await {
                    // Drain whatever else is queued so a burst of streaming
                    // chunks costs one update and one render, not one each.
                    let mut batch = vec![event];
                    while let Ok(next) = rx.try_recv() {
                        batch.push(next);
                        if batch.len() >= 256 {
                            break;
                        }
                    }
                    if root
                        .update(cx, |app, cx| app.handle_service_events(batch, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();

            cx.activate(true);
        });
}

/// Log to stderr and to `<data dir>/logs/maple-gpui.log` so a freeze or
/// crash leaves evidence on disk. `RUST_LOG` still controls the level;
/// the default is `info`. Panics are logged as well.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LogOutput {
    /// Log file plus stderr, with `RUST_LOG` honored. The desktop default.
    FileAndStderr,
    /// Log file only, with the default filter. For stdio protocol modes.
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
    use super::{StartupMode, startup_mode, version_text};

    fn mode(args: &[&str]) -> StartupMode {
        startup_mode(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn version_flags_use_the_fast_path() {
        assert_eq!(mode(&["--version"]), StartupMode::Version);
        assert_eq!(mode(&["-V"]), StartupMode::Version);
        assert_eq!(
            version_text(),
            format!("maple-gpui {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn acp_subcommand_keeps_precedence_over_following_flags() {
        assert_eq!(mode(&["acp"]), StartupMode::Acp);
        assert_eq!(mode(&["acp", "--version"]), StartupMode::Acp);
    }

    #[test]
    fn proxy_flags_parse_with_defaults() {
        let StartupMode::Proxy(Ok(args)) = mode(&["proxy"]) else {
            panic!("proxy without flags must parse");
        };
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.port, 8080);
        assert!(!args.cors);

        let StartupMode::Proxy(Ok(args)) =
            mode(&["proxy", "--host", "0.0.0.0", "--port", "9999", "--cors"])
        else {
            panic!("proxy flags must parse");
        };
        assert_eq!(
            (args.host.as_str(), args.port, args.cors),
            ("0.0.0.0", 9999, true)
        );

        assert!(matches!(
            mode(&["proxy", "--port", "x"]),
            StartupMode::Proxy(Err(_))
        ));
        assert!(matches!(
            mode(&["proxy", "--bogus"]),
            StartupMode::Proxy(Err(_))
        ));
        assert!(matches!(
            mode(&["proxy", "--cors", "--api-key", "k"]),
            StartupMode::Proxy(Err(_))
        ));
    }

    #[test]
    fn other_arguments_keep_desktop_startup() {
        assert_eq!(mode(&[]), StartupMode::Desktop);
        assert_eq!(mode(&["--unknown"]), StartupMode::Desktop);
    }
}
