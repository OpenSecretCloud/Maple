mod theme;

use iced::widget::operation;
use iced::widget::{button, canvas, center, column, container, row, scrollable, text, text_input};
use iced::{keyboard, Border, Color, Element, Fill, Font, Point, Subscription, Task, Theme};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

const ARRAY_BOLD_BYTES: &[u8] = include_bytes!("../Array-Bold.ttf");
const ARRAY_BOLD: Font = Font::with_name("Array-Bold");

use maple_core::{
    AppAction, AppReconciler, AppState, AppUpdate, AuthState, ChatMessage, FfiApp, OAuthProvider,
};
use theme::*;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("Maple")
        .font(ARRAY_BOLD_BYTES)
        .theme(App::theme)
        .subscription(App::subscription)
        .run()
}

// ── AppManager ──────────────────────────────────────────────────────────────

const KEYRING_SERVICE: &str = "cloud.opensecret.maple.desktop";
const KEYRING_ACCESS: &str = "access_token";
const KEYRING_REFRESH: &str = "refresh_token";

#[derive(Clone)]
struct AppManager {
    ffi: Arc<FfiApp>,
    update_rx: flume::Receiver<AppUpdate>,
}

impl Hash for AppManager {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.ffi).hash(state);
    }
}

fn configured_api_url() -> String {
    std::env::var("OPEN_SECRET_API_URL")
        .ok()
        .or_else(|| option_env!("OPEN_SECRET_API_URL").map(str::to_owned))
        .unwrap_or_else(maple_core::default_api_url)
}

impl AppManager {
    fn new() -> Result<Self, String> {
        let data_dir = dirs_next::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("Maple")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&data_dir);

        let _ = dotenvy::dotenv();
        let api_url = configured_api_url();
        let client_id = std::env::var("CLIENT_ID")
            .unwrap_or_else(|_| "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6".to_string());

        let ffi = FfiApp::new(api_url, client_id, data_dir);
        let (notify_tx, update_rx) = flume::unbounded();
        ffi.listen_for_updates(Box::new(DesktopReconciler { tx: notify_tx }));

        Ok(Self { ffi, update_rx })
    }

    fn state(&self) -> AppState {
        self.ffi.state()
    }

    fn dispatch(&self, action: AppAction) {
        self.ffi.dispatch(action);
    }

    fn subscribe_updates(&self) -> flume::Receiver<AppUpdate> {
        self.update_rx.clone()
    }
}

struct DesktopReconciler {
    tx: flume::Sender<AppUpdate>,
}

impl AppReconciler for DesktopReconciler {
    fn reconcile(&self, update: AppUpdate) {
        let _ = self.tx.send(update);
    }
}

fn manager_update_stream(manager: &AppManager) -> impl iced::futures::Stream<Item = AppUpdate> {
    let rx = manager.subscribe_updates();
    iced::futures::stream::unfold(rx, |rx| async move {
        match rx.recv_async().await {
            Ok(update) => Some((update, rx)),
            Err(_) => None,
        }
    })
}

fn save_tokens_to_keyring(access_token: &str, refresh_token: &str) {
    if access_token.is_empty() {
        let _ = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCESS)
            .and_then(|e| e.delete_credential());
        let _ = keyring::Entry::new(KEYRING_SERVICE, KEYRING_REFRESH)
            .and_then(|e| e.delete_credential());
    } else {
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCESS) {
            let _ = entry.set_password(access_token);
        }
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_REFRESH) {
            let _ = entry.set_password(refresh_token);
        }
    }
}

fn load_tokens_from_keyring() -> Option<(String, String)> {
    let access = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCESS)
        .ok()?
        .get_password()
        .ok()?;
    let refresh = keyring::Entry::new(KEYRING_SERVICE, KEYRING_REFRESH)
        .ok()?
        .get_password()
        .ok()?;
    Some((access, refresh))
}

// ── App ─────────────────────────────────────────────────────────────────────

enum App {
    BootError {
        error: String,
    },
    Loaded {
        manager: AppManager,
        state: AppState,
        screen: ScreenState,
        show_splash: bool,
        splash_min_passed: bool,
    },
}

enum ScreenState {
    Loading,
    Login(LoginState),
    Chat { compose: String },
}

struct LoginState {
    email: String,
    password: String,
    name: String,
    is_sign_up: bool,
}

impl LoginState {
    fn new() -> Self {
        Self {
            email: String::new(),
            password: String::new(),
            name: String::new(),
            is_sign_up: false,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    CoreUpdated(AppUpdate),
    TryRestoreSession,
    TabPressed,
    ShiftTabPressed,
    LoginEmailChanged(String),
    LoginPasswordChanged(String),
    LoginNameChanged(String),
    ToggleSignUp,
    FocusEmail,
    FocusPassword,
    SubmitAuth,
    InitiateOAuth(OAuthProvider),
    ComposeChanged(String),
    SendMessage,
    LoadOlderMessages,
    RefreshTimestamps,
    SplashDone,
    Scrolled(scrollable::Viewport),
    Logout,
    ToggleSettings,
    RequestDeleteAgent,
    ConfirmDeleteAgent,
    CancelDeleteAgent,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = match AppManager::new() {
            Ok(manager) => {
                let state = manager.state();
                let screen = screen_from_state(&state);
                Self::Loaded {
                    manager,
                    state,
                    screen,
                    show_splash: true,
                    splash_min_passed: false,
                }
            }
            Err(error) => Self::BootError { error },
        };
        (app, Task::done(Message::TryRestoreSession))
    }

    fn theme(&self) -> Theme {
        Theme::custom(
            "Maple".to_string(),
            iced::theme::Palette {
                background: NEUTRAL_50,
                text: NEUTRAL_900,
                primary: MAPLE_500,
                success: MAPLE_SUCCESS,
                danger: MAPLE_ERROR,
                warning: MAPLE_WARNING,
            },
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        match self {
            App::BootError { .. } => Subscription::none(),
            App::Loaded {
                manager,
                show_splash,
                ..
            } => {
                let core_sub = Subscription::run_with(manager.clone(), manager_update_stream)
                    .map(Message::CoreUpdated);

                let tab_sub = iced::event::listen_with(|event, _status, _id| {
                    if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Tab),
                        modifiers,
                        ..
                    }) = event
                    {
                        if modifiers.shift() {
                            Some(Message::ShiftTabPressed)
                        } else {
                            Some(Message::TabPressed)
                        }
                    } else {
                        None
                    }
                });

                let tick_sub = iced::time::every(std::time::Duration::from_secs(30))
                    .map(|_| Message::RefreshTimestamps);

                let mut subs = vec![core_sub, tab_sub, tick_sub];
                if *show_splash {
                    subs.push(
                        iced::time::every(std::time::Duration::from_millis(1200))
                            .map(|_| Message::SplashDone),
                    );
                }

                Subscription::batch(subs)
            }
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let App::Loaded {
            manager,
            state,
            screen,
            show_splash,
            splash_min_passed,
        } = self
        else {
            return Task::none();
        };

        match message {
            Message::SplashDone => {
                *splash_min_passed = true;
                if !matches!(screen, ScreenState::Loading) {
                    *show_splash = false;
                }
            }
            Message::TabPressed => return operation::focus_next(),
            Message::ShiftTabPressed => return operation::focus_previous(),

            Message::TryRestoreSession => {
                if let Some((access_token, refresh_token)) = load_tokens_from_keyring() {
                    manager.dispatch(AppAction::RestoreSession {
                        access_token,
                        refresh_token,
                    });
                }
            }

            Message::CoreUpdated(ref update) => {
                if let AppUpdate::SessionTokens {
                    access_token,
                    refresh_token,
                    ..
                } = update
                {
                    save_tokens_to_keyring(access_token, refresh_token);
                }

                let latest = manager.state();
                if latest.rev > state.rev {
                    let new_screen = screen_from_state(&latest);
                    match (&screen, &new_screen) {
                        (ScreenState::Login(old), ScreenState::Login(_)) => {
                            *screen = ScreenState::Login(LoginState {
                                email: old.email.clone(),
                                password: old.password.clone(),
                                name: old.name.clone(),
                                is_sign_up: old.is_sign_up,
                            });
                        }
                        (ScreenState::Chat { compose }, ScreenState::Chat { .. }) => {
                            *screen = ScreenState::Chat {
                                compose: compose.clone(),
                            };
                        }
                        _ => *screen = new_screen,
                    }

                    if *splash_min_passed && !matches!(screen, ScreenState::Loading) {
                        *show_splash = false;
                    }

                    if let Some(ref url) = latest.pending_auth_url {
                        let _ = std::process::Command::new("open")
                            .arg(url)
                            .spawn()
                            .or_else(|_| std::process::Command::new("xdg-open").arg(url).spawn());
                        manager.dispatch(AppAction::ClearPendingAuthUrl);
                    }

                    *state = latest;
                }
            }

            Message::LoginEmailChanged(v) => {
                if let ScreenState::Login(s) = screen {
                    s.email = v;
                }
            }
            Message::LoginPasswordChanged(v) => {
                if let ScreenState::Login(s) = screen {
                    s.password = v;
                }
            }
            Message::LoginNameChanged(v) => {
                if let ScreenState::Login(s) = screen {
                    s.name = v;
                }
            }
            Message::ToggleSignUp => {
                if let ScreenState::Login(s) = screen {
                    s.is_sign_up = !s.is_sign_up;
                }
            }
            Message::FocusEmail => {
                return operation::focus(iced::widget::Id::new("login_email"));
            }
            Message::FocusPassword => {
                return operation::focus(iced::widget::Id::new("login_password"));
            }
            Message::SubmitAuth => {
                if let ScreenState::Login(s) = screen {
                    if s.is_sign_up {
                        manager.dispatch(AppAction::SignUpWithEmail {
                            email: s.email.clone(),
                            password: s.password.clone(),
                            name: s.name.clone(),
                        });
                    } else {
                        manager.dispatch(AppAction::LoginWithEmail {
                            email: s.email.clone(),
                            password: s.password.clone(),
                        });
                    }
                }
            }
            Message::InitiateOAuth(provider) => {
                manager.dispatch(AppAction::InitiateOAuth {
                    provider,
                    invite_code: None,
                });
            }

            Message::ComposeChanged(v) => {
                if let ScreenState::Chat { compose } = screen {
                    *compose = v;
                }
            }
            Message::SendMessage => {
                if let ScreenState::Chat { compose } = screen {
                    if !compose.trim().is_empty() {
                        manager.dispatch(AppAction::SendMessage {
                            content: compose.clone(),
                        });
                        *compose = String::new();
                    }
                }
            }
            Message::LoadOlderMessages => {
                manager.dispatch(AppAction::LoadOlderMessages);
            }
            Message::RefreshTimestamps => {
                manager.dispatch(AppAction::RefreshTimestamps);
            }
            Message::Scrolled(viewport) => {
                // With anchor_bottom, absolute_offset_reversed gives distance
                // from bottom. When it's close to content height, user is near top.
                let content_h = viewport.content_bounds().height;
                let view_h = viewport.bounds().height;
                let reversed_y = viewport.absolute_offset_reversed().y;
                let distance_from_top = content_h - view_h - reversed_y;
                if distance_from_top < 200.0
                    && state.has_older_messages
                    && !state.is_loading_history
                {
                    manager.dispatch(AppAction::LoadOlderMessages);
                }
            }
            Message::Logout => {
                manager.dispatch(AppAction::Logout);
            }
            Message::ToggleSettings => {
                manager.dispatch(AppAction::ToggleSettings);
            }
            Message::RequestDeleteAgent => {
                manager.dispatch(AppAction::RequestDeleteAgent);
            }
            Message::ConfirmDeleteAgent => {
                manager.dispatch(AppAction::ConfirmDeleteAgent);
            }
            Message::CancelDeleteAgent => {
                manager.dispatch(AppAction::CancelDeleteAgent);
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        match self {
            App::BootError { error } => center(
                column![
                    text("Maple").size(24).color(MAPLE_500),
                    text(error).color(MAPLE_ERROR),
                ]
                .spacing(SPACE_SM),
            )
            .into(),
            App::Loaded {
                state,
                screen,
                show_splash,
                ..
            } => {
                if *show_splash {
                    return view_splash();
                }
                match screen {
                    ScreenState::Loading => view_splash(),
                    ScreenState::Login(login) => view_login(login, state),
                    ScreenState::Chat { compose } => view_chat(state, compose),
                }
            }
        }
    }
}

fn screen_from_state(state: &AppState) -> ScreenState {
    match state.router.default_screen {
        maple_core::Screen::Loading => ScreenState::Loading,
        maple_core::Screen::Login => ScreenState::Login(LoginState::new()),
        maple_core::Screen::Chat => ScreenState::Chat {
            compose: String::new(),
        },
    }
}

// ── Wordmark ────────────────────────────────────────────────────────────────

const WORDMARK_VB_W: f32 = 248.0;
const WORDMARK_VB_H: f32 = 48.0;

fn wordmark_paths(s: f32) -> Vec<canvas::Path> {
    let p = |x: f32, y: f32| Point::new(x * s, y * s);
    vec![
        // M
        canvas::Path::new(|b| {
            b.move_to(p(0.0, 41.2408));
            b.line_to(p(0.0, 6.06562));
            b.bezier_curve_to(p(0.0, 0.652098), p(5.77774, -1.72459), p(10.1942, 1.4443));
            b.line_to(p(26.9158, 15.561));
            b.line_to(p(43.6375, 1.4443));
            b.bezier_curve_to(
                p(48.0567, -1.72459),
                p(53.8317, 0.652098),
                p(53.8317, 6.06562),
            );
            b.line_to(p(53.8317, 41.2408));
            b.bezier_curve_to(
                p(53.8317, 44.9716),
                p(50.7962, 47.9972),
                p(47.0534, 47.9972),
            );
            b.line_to(p(6.17796, 47.9972));
            b.bezier_curve_to(p(2.16172, 47.9972), p(0.0, 45.6318), p(0.0, 41.2408));
            b.close();
        }),
        // A
        canvas::Path::new(|b| {
            b.move_to(p(58.7892, 39.8362));
            b.line_to(p(79.685, 3.36589));
            b.bezier_curve_to(
                p(82.2553, -1.11494),
                p(86.5647, -1.12899),
                p(89.1435, 3.36589),
            );
            b.line_to(p(110.031, 39.8362));
            b.bezier_curve_to(p(112.77, 44.6148), p(111.082, 48.0), p(105.555, 48.0));
            b.line_to(p(63.2649, 48.0));
            b.bezier_curve_to(p(57.7408, 48.0), p(56.0526, 44.6204), p(58.7892, 39.8362));
            b.close();
        }),
        // P
        canvas::Path::new(|b| {
            b.move_to(p(137.257, 39.4064));
            b.line_to(p(137.257, 41.2408));
            b.bezier_curve_to(
                p(137.257, 44.9716),
                p(134.221, 47.9972),
                p(130.478, 47.9972),
            );
            b.line_to(p(121.49, 47.9972));
            b.bezier_curve_to(
                p(117.745, 47.9972),
                p(114.712, 44.9716),
                p(114.712, 41.2408),
            );
            b.line_to(p(114.712, 7.56014));
            b.bezier_curve_to(
                p(114.712, 3.82658),
                p(117.748, 0.800986),
                p(121.493, 0.800986),
            );
            b.line_to(p(136.831, 0.800986));
            b.bezier_curve_to(
                p(154.652, 0.800986),
                p(160.965, 8.65577),
                p(160.965, 20.1318),
            );
            b.bezier_curve_to(
                p(160.965, 31.6077),
                p(154.753, 39.2827),
                p(137.259, 39.4064),
            );
            b.line_to(p(137.257, 39.4064));
            b.close();
        }),
        // L
        canvas::Path::new(|b| {
            b.move_to(p(164.191, 41.2408));
            b.line_to(p(164.191, 7.56014));
            b.bezier_curve_to(
                p(164.191, 3.82658),
                p(167.227, 0.800986),
                p(170.972, 0.800986),
            );
            b.line_to(p(179.96, 0.800986));
            b.bezier_curve_to(
                p(183.706, 0.800986),
                p(186.739, 3.82658),
                p(186.739, 7.55733),
            );
            b.line_to(p(186.739, 16.5331));
            b.line_to(p(195.743, 16.5331));
            b.bezier_curve_to(
                p(199.489, 16.5331),
                p(202.522, 19.5587),
                p(202.522, 23.2894),
            );
            b.line_to(p(202.522, 41.238));
            b.bezier_curve_to(
                p(202.522, 44.9688),
                p(199.486, 47.9944),
                p(195.743, 47.9944),
            );
            b.line_to(p(170.972, 47.9944));
            b.bezier_curve_to(p(167.227, 47.9944), p(164.194, 44.9688), p(164.194, 41.238));
            b.line_to(p(164.191, 41.2408));
            b.close();
        }),
        // E
        canvas::Path::new(|b| {
            b.move_to(p(240.304, 16.5331));
            b.line_to(p(230.6, 16.5331));
            b.bezier_curve_to(
                p(233.943, 17.4854),
                p(236.386, 20.5532),
                p(236.386, 24.1884),
            );
            b.bezier_curve_to(p(236.386, 28.0259), p(233.669, 31.2257), p(230.05, 31.9842));
            b.line_to(p(240.321, 31.9842));
            b.bezier_curve_to(
                p(244.712, 31.9842),
                p(247.082, 34.3412),
                p(247.082, 38.7237),
            );
            b.line_to(p(247.082, 41.2549));
            b.bezier_curve_to(
                p(247.082, 45.6346),
                p(244.712, 47.9972),
                p(240.315, 47.9972),
            );
            b.line_to(p(212.982, 47.9972));
            b.bezier_curve_to(
                p(208.582, 47.9972),
                p(206.215, 45.6346),
                p(206.215, 41.2549),
            );
            b.line_to(p(206.215, 7.5433));
            b.bezier_curve_to(
                p(206.215, 3.1608),
                p(208.582, 0.800986),
                p(212.982, 0.800986),
            );
            b.line_to(p(240.315, 0.800986));
            b.bezier_curve_to(p(244.712, 0.800986), p(247.082, 3.1608), p(247.082, 7.5433));
            b.line_to(p(247.082, 9.77668));
            b.bezier_curve_to(
                p(247.082, 13.5074),
                p(244.047, 16.5302),
                p(240.306, 16.5302),
            );
            b.line_to(p(240.304, 16.5331));
            b.close();
        }),
    ]
}

struct WordmarkProgram {
    color: Color,
    height: f32,
}

impl<Message> canvas::Program<Message> for WordmarkProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let scale = self.height / WORDMARK_VB_H;
        let paths = wordmark_paths(scale);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for path in &paths {
            frame.fill(path, self.color);
        }
        vec![frame.into_geometry()]
    }
}

fn view_wordmark(height: f32, color: Color) -> Element<'static, Message> {
    let scale = height / WORDMARK_VB_H;
    let width = WORDMARK_VB_W * scale;
    canvas(WordmarkProgram { color, height })
        .width(width)
        .height(height)
        .into()
}

// ── Splash View ─────────────────────────────────────────────────────────────

fn view_splash<'a>() -> Element<'a, Message> {
    center(
        column![
            view_wordmark(40.0, PEBBLE_50),
            text("Privacy-first intelligence")
                .size(16)
                .font(ARRAY_BOLD)
                .color(Color {
                    a: 0.8,
                    ..PEBBLE_100
                }),
        ]
        .spacing(SPACE_LG)
        .align_x(iced::Alignment::Center),
    )
    .style(|_: &Theme| container::Style {
        background: Some(iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(std::f32::consts::PI)
                .add_stop(0.0, PEBBLE_400)
                .add_stop(0.5, BARK_300)
                .add_stop(1.0, MAPLE_500),
        ))),
        ..Default::default()
    })
    .into()
}

// ── Login View ──────────────────────────────────────────────────────────────

fn view_login<'a>(login: &'a LoginState, state: &'a AppState) -> Element<'a, Message> {
    let is_loading = matches!(state.auth, AuthState::LoggingIn | AuthState::SigningUp);

    let title = view_wordmark(28.0, NEUTRAL_900);
    let mut fields = column![].spacing(SPACE_XS).width(320);

    if login.is_sign_up {
        fields = fields.push(
            text_input("Name", &login.name)
                .id(iced::widget::Id::new("login_name"))
                .on_input(Message::LoginNameChanged)
                .on_submit(Message::FocusEmail)
                .padding(10)
                .style(text_input_style),
        );
    }

    fields = fields.push(
        text_input("Email", &login.email)
            .id(iced::widget::Id::new("login_email"))
            .on_input(Message::LoginEmailChanged)
            .on_submit(Message::FocusPassword)
            .padding(10)
            .style(text_input_style),
    );

    fields = fields.push(
        text_input("Password", &login.password)
            .id(iced::widget::Id::new("login_password"))
            .on_input(Message::LoginPasswordChanged)
            .on_submit(Message::SubmitAuth)
            .secure(true)
            .padding(10)
            .style(text_input_style),
    );

    let submit_label = if login.is_sign_up {
        "Sign Up"
    } else {
        "Sign In"
    };
    let mut submit_btn = button(
        container(text(submit_label).size(14).color(WHITE))
            .center_x(Fill)
            .padding([10, 0]),
    )
    .width(Fill)
    .style(primary_button_style);

    if !login.email.is_empty() && !login.password.is_empty() && !is_loading {
        submit_btn = submit_btn.on_press(Message::SubmitAuth);
    }

    let divider = row![
        container(column![])
            .width(Fill)
            .height(1)
            .style(divider_style),
        text("or").size(12).color(PEBBLE_400),
        container(column![])
            .width(Fill)
            .height(1)
            .style(divider_style),
    ]
    .spacing(SPACE_SM)
    .align_y(iced::Alignment::Center)
    .width(320);

    let oauth_buttons = column![
        oauth_button(
            "Continue with GitHub",
            Message::InitiateOAuth(OAuthProvider::Github),
            is_loading
        ),
        oauth_button(
            "Continue with Google",
            Message::InitiateOAuth(OAuthProvider::Google),
            is_loading
        ),
        oauth_button(
            "Continue with Apple",
            Message::InitiateOAuth(OAuthProvider::Apple),
            is_loading
        ),
    ]
    .spacing(6)
    .width(320);

    let toggle_label = if login.is_sign_up {
        "Already have an account? Sign In"
    } else {
        "Don't have an account? Sign Up"
    };
    let toggle_btn = button(text(toggle_label).size(12).color(PEBBLE_500))
        .on_press(Message::ToggleSignUp)
        .style(ghost_button_style);

    let mut content = column![
        title,
        fields,
        submit_btn,
        divider,
        oauth_buttons,
        toggle_btn
    ]
    .spacing(SPACE_MD)
    .align_x(iced::Alignment::Center);

    if let Some(ref toast) = state.toast {
        content = content.push(text(toast).size(12).color(MAPLE_ERROR));
    }

    center(content)
        .style(|_: &Theme| container::Style {
            background: Some(iced::Background::Color(NEUTRAL_50)),
            ..Default::default()
        })
        .into()
}

fn oauth_button(label: &str, msg: Message, disabled: bool) -> Element<'_, Message> {
    let mut btn = button(
        container(text(label).size(13).color(PEBBLE_700))
            .center_x(Fill)
            .padding([8, 0]),
    )
    .width(Fill)
    .style(secondary_button_style);
    if !disabled {
        btn = btn.on_press(msg);
    }
    btn.into()
}

// ── Chat View ───────────────────────────────────────────────────────────────

fn view_chat<'a>(state: &'a AppState, compose: &'a str) -> Element<'a, Message> {
    // Floating glass header: centered wordmark pill + gear pill
    let wordmark_pill = container(view_wordmark(20.0, PEBBLE_700))
        .padding([SPACE_XS as u16, SPACE_MD as u16])
        .style(|_: &Theme| container::Style {
            background: Some(iced::Background::Color(Color {
                a: 0.72,
                ..NEUTRAL_0
            })),
            border: Border {
                radius: RADIUS_FULL.into(),
                width: 0.5,
                color: Color {
                    a: 0.25,
                    ..PEBBLE_300
                },
            },
            shadow: iced::Shadow {
                color: Color {
                    a: 0.08,
                    ..Color::BLACK
                },
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        });

    let gear_pill = container(
        button(text("\u{2699}").size(16).color(PEBBLE_600))
            .on_press(Message::ToggleSettings)
            .style(ghost_button_style),
    )
    .padding([4, 6])
    .style(|_: &Theme| container::Style {
        background: Some(iced::Background::Color(Color {
            a: 0.72,
            ..NEUTRAL_0
        })),
        border: Border {
            radius: RADIUS_FULL.into(),
            width: 0.5,
            color: Color {
                a: 0.25,
                ..PEBBLE_300
            },
        },
        shadow: iced::Shadow {
            color: Color {
                a: 0.08,
                ..Color::BLACK
            },
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        ..Default::default()
    });

    let header = row![
        iced::widget::Space::new().width(Fill),
        wordmark_pill,
        iced::widget::Space::new().width(Fill),
    ];

    let header_row = iced::widget::stack![
        container(header).width(Fill),
        column![row![
            iced::widget::Space::new().width(Fill),
            gear_pill,
            iced::widget::Space::new().width(SPACE_SM),
        ]],
    ];

    let floating_header = container(header_row)
        .width(Fill)
        .padding([SPACE_SM as u16, 0]);

    // Message list with padding for floating elements
    let mut msg_col = column![]
        .spacing(SPACE_XS)
        .padding([SPACE_XS as u16, SPACE_MD as u16]);

    // Top spacer so messages don't start behind the header
    msg_col = msg_col.push(iced::widget::Space::new().height(36));

    if state.is_loading_history {
        msg_col = msg_col.push(
            container(text("Loading...").size(12).color(PEBBLE_400))
                .width(Fill)
                .center_x(Fill)
                .padding([4, 0]),
        );
    }

    for msg in &state.messages {
        msg_col = msg_col.push(view_message(msg));
    }

    if state.is_agent_typing {
        msg_col = msg_col.push(text("Maple is typing...").size(12).color(PEBBLE_400));
    }

    // Bottom spacer so messages + timestamps don't end behind compose bar
    msg_col = msg_col.push(iced::widget::Space::new().height(64));

    let message_list = scrollable(msg_col)
        .height(Fill)
        .anchor_bottom()
        .on_scroll(Message::Scrolled);

    // Floating glass compose bar pill
    let mut send_btn = button(
        container(text("Send").size(14).color(WHITE)).padding([SPACE_XS as u16, SPACE_MD as u16]),
    )
    .style(primary_button_style);
    if !compose.trim().is_empty() && !state.is_agent_typing {
        send_btn = send_btn.on_press(Message::SendMessage);
    }

    let compose_bar = container(
        row![
            text_input("Message Maple...", compose)
                .on_input(Message::ComposeChanged)
                .on_submit(Message::SendMessage)
                .width(Fill)
                .style(text_input_style),
            send_btn,
        ]
        .spacing(SPACE_XS)
        .align_y(iced::Alignment::Center),
    )
    .padding([SPACE_XS as u16, SPACE_SM as u16])
    .style(|_: &Theme| container::Style {
        background: Some(iced::Background::Color(Color {
            a: 0.72,
            ..NEUTRAL_0
        })),
        border: Border {
            radius: RADIUS_FULL.into(),
            width: 0.5,
            color: Color {
                a: 0.25,
                ..PEBBLE_300
            },
        },
        shadow: iced::Shadow {
            color: Color {
                a: 0.08,
                ..Color::BLACK
            },
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        ..Default::default()
    });

    let floating_compose = container(compose_bar)
        .width(Fill)
        .padding([SPACE_SM as u16, SPACE_SM as u16]);

    // Stack: gradient bg + messages, then floating header/compose on top
    let chat_bg = iced::Background::Gradient(iced::Gradient::Linear(
        iced::gradient::Linear::new(std::f32::consts::PI)
            .add_stop(0.0, PEBBLE_100)
            .add_stop(0.3, MAPLE_50)
            .add_stop(0.55, NEUTRAL_0)
            .add_stop(0.8, BARK_50)
            .add_stop(1.0, PEBBLE_100),
    ));

    let mut content = iced::widget::stack![
        container(message_list)
            .width(Fill)
            .height(Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(chat_bg),
                ..Default::default()
            }),
        column![
            floating_header,
            iced::widget::Space::new().height(Fill),
            floating_compose,
        ],
    ]
    .width(Fill)
    .height(Fill);

    if let Some(ref toast) = state.toast {
        content = content.push(
            container(text(toast).size(12).color(MAPLE_ERROR))
                .padding([4, SPACE_MD as u16])
                .style(|_: &Theme| container::Style {
                    background: Some(iced::Background::Color(Color {
                        a: 0.1,
                        ..MAPLE_ERROR
                    })),
                    border: Border {
                        radius: RADIUS_SM.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        );
    }

    // Settings dropdown overlay
    if state.show_settings && !state.confirm_delete_agent {
        let settings_menu = container(
            column![
                button(
                    row![text("Delete Agent").size(13).color(MAPLE_ERROR),]
                        .align_y(iced::Alignment::Center)
                )
                .on_press(Message::RequestDeleteAgent)
                .style(ghost_button_style)
                .width(Fill),
                container(column![])
                    .width(Fill)
                    .height(1)
                    .style(divider_style),
                button(
                    row![text("Sign Out").size(13).color(NEUTRAL_800),]
                        .align_y(iced::Alignment::Center)
                )
                .on_press(Message::Logout)
                .style(ghost_button_style)
                .width(Fill),
            ]
            .spacing(2)
            .width(180),
        )
        .padding(SPACE_XS as u16)
        .style(|_: &Theme| container::Style {
            background: Some(iced::Background::Color(Color {
                a: 0.92,
                ..NEUTRAL_0
            })),
            border: Border {
                radius: RADIUS_MD.into(),
                width: 1.0,
                color: Color {
                    a: 0.2,
                    ..PEBBLE_300
                },
            },
            shadow: iced::Shadow {
                color: Color {
                    a: 0.12,
                    ..Color::BLACK
                },
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        });

        let overlay = iced::widget::stack![
            content,
            column![
                iced::widget::Space::new().height(52),
                row![
                    iced::widget::Space::new().width(Fill),
                    settings_menu,
                    iced::widget::Space::new().width(SPACE_SM),
                ],
            ],
        ];

        return overlay.width(Fill).height(Fill).into();
    }

    // Delete confirmation overlay
    if state.confirm_delete_agent {
        let dialog = container(
            column![
                text("Delete Agent?").size(18).color(NEUTRAL_900),
                text("This will permanently delete your agent conversation history. This cannot be undone.")
                    .size(13)
                    .color(PEBBLE_500),
                row![
                    button(
                        container(text("Cancel").size(13).color(NEUTRAL_800)).padding([SPACE_XS as u16, SPACE_MD as u16]),
                    )
                    .on_press(Message::CancelDeleteAgent)
                    .style(secondary_button_style),
                    button(
                        container(text("Delete").size(13).color(WHITE)).padding([SPACE_XS as u16, SPACE_MD as u16]),
                    )
                    .on_press(Message::ConfirmDeleteAgent)
                    .style(|theme: &Theme, status| {
                        let mut style = primary_button_style(theme, status);
                        style.background = Some(iced::Background::Color(MAPLE_ERROR));
                        style
                    }),
                ]
                .spacing(SPACE_SM),
            ]
            .spacing(SPACE_SM)
            .width(320),
        )
        .padding(SPACE_LG as u16)
        .style(|_: &Theme| container::Style {
            background: Some(iced::Background::Color(Color { a: 0.95, ..NEUTRAL_0 })),
            border: Border {
                radius: RADIUS_LG.into(),
                width: 1.0,
                color: Color { a: 0.2, ..PEBBLE_300 },
            },
            shadow: iced::Shadow {
                color: Color { a: 0.15, ..Color::BLACK },
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        });

        let overlay = iced::widget::stack![
            content,
            container(column![])
                .width(Fill)
                .height(Fill)
                .style(|_: &Theme| container::Style {
                    background: Some(iced::Background::Color(Color {
                        a: 0.3,
                        ..Color::BLACK
                    })),
                    ..Default::default()
                }),
            center(dialog),
        ];

        return overlay.width(Fill).height(Fill).into();
    }

    content.into()
}

fn view_message(msg: &ChatMessage) -> Element<'_, Message> {
    let is_user = msg.is_user;

    let mut bubble_content = column![].spacing(4);
    if !is_user && msg.show_sender {
        bubble_content = bubble_content.push(text("Maple").size(11).color(PEBBLE_400));
    }

    bubble_content = bubble_content.push(text(&msg.content).size(14).color(if is_user {
        WHITE
    } else {
        NEUTRAL_800
    }));

    let bubble = container(bubble_content)
        .padding([SPACE_XS as u16, SPACE_SM as u16])
        .max_width(500)
        .style(if is_user {
            user_bubble_style
        } else {
            agent_bubble_style
        });

    let align = if is_user {
        iced::Alignment::End
    } else {
        iced::Alignment::Start
    };

    let bubble_row: Element<'_, Message> = if is_user {
        row![iced::widget::Space::new().width(Fill), bubble]
            .width(Fill)
            .into()
    } else {
        row![bubble, iced::widget::Space::new().width(Fill)]
            .width(Fill)
            .into()
    };

    let mut col = column![bubble_row].spacing(2);
    if msg.show_timestamp {
        let timestamp = text(&msg.timestamp_display).size(10).color(PEBBLE_400);
        col = col.push(
            container(timestamp)
                .width(Fill)
                .align_x(align)
                .padding([0, SPACE_SM as u16]),
        );
    }
    col.into()
}
