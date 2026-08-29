//! The gpui desktop window: login/chat routing, window state, and the
//! event pump that forwards agent service events into the active chat
//! screen. Compiled only with the `desktop` feature.

use std::sync::Arc;

use gpui::{
    App, Application, Bounds, Context, Entity, KeyBinding, Pixels, Render, Window, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, size,
};

actions!(maple_app, [QuitApp]);

use crate::backend::AgentBackend;
use crate::ui;
use crate::ui::chat::{ChatScreen, LoggedOut};
use crate::ui::login::{LoginScreen, LoginSucceeded};
use crate::ui::settings::{Section, SettingsClosed, SettingsScreen, SignOutRequested};
use crate::ui::text_input;
use crate::ui::titlebar::TitleBar;

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

    /// A newer release exists: tell the chat screen so it shows the banner.
    /// Without a chat screen (login in progress) nothing is done here; the
    /// result stays in `update::available()` and seeds the next chat.
    fn set_update(&mut self, info: crate::update::UpdateInfo, cx: &mut Context<Self>) {
        let chat = match &self.screen {
            Screen::Chat(chat) => Some(chat.clone()),
            _ => self.parked_chat.clone(),
        };
        if let Some(chat) = chat {
            chat.update(cx, |chat, cx| chat.set_update(info, cx));
        }
        cx.notify();
    }

    /// Tear down the chat screen and return to a fresh login form. A
    /// no-op when the login form is already showing, so a sign-out that
    /// is reported twice (Settings button, then the backend) keeps the
    /// form the user may already be typing in.
    fn show_login(&mut self, cx: &mut Context<Self>) {
        self.user_id = None;
        self.parked_chat = None;
        if matches!(self.screen, Screen::Login(_)) {
            return;
        }
        let backend = self.backend.clone();
        let login = cx.new(|cx| LoginScreen::new(backend, cx));
        self.subscribe_login(&login, cx);
        self.screen = Screen::Login(login);
        cx.notify();
    }

    /// Wire the login form: a successful sign-in opens the chat screen.
    fn subscribe_login(&mut self, login: &Entity<LoginScreen>, cx: &mut Context<Self>) {
        cx.subscribe(
            login,
            |app: &mut MapleApp, _emitter, event: &LoginSucceeded, cx| {
                app.open_chat(event.0.clone(), cx);
            },
        )
        .detach();
    }

    /// Show the chat screen for a signed-in account and wire its events.
    fn open_chat(&mut self, user_id: String, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let chat = cx.new(|cx| ChatScreen::new(backend, user_id.clone(), cx));
        // The release check may have finished while the login screen was
        // up; the banner must not be lost with it.
        if let Some(info) = crate::update::available() {
            chat.update(cx, |chat, cx| chat.set_update(info.clone(), cx));
        }
        self.user_id = Some(user_id);
        self.parked_chat = None;
        self.screen = Screen::Chat(chat.clone());
        self.subscribe_chat(&chat, cx);
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
        ui::theme::set_preference(ui::theme::Preference::parse(&self.settings.theme));
        if let Some(chat) = self.parked_chat.take() {
            let settings = self.settings.clone();
            chat.update(cx, |chat, cx| chat.apply_defaults(&settings, cx));
            self.screen = Screen::Chat(chat);
        }
        cx.notify();
    }
}

impl Render for MapleApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        remember_window_state(window);
        if ui::theme::resolve(window.appearance()) {
            // Every view reads the palette in render; make them all redo it.
            cx.refresh_windows();
        }
        let titlebar = self.titlebar.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family(crate::assets::FONT_BODY)
            .child(titlebar)
            .child(match &self.screen {
                Screen::Login(login) => login.clone().into_any_element(),
                Screen::Chat(chat) => chat.clone().into_any_element(),
                Screen::Settings(screen) => screen.clone().into_any_element(),
            })
    }
}

/// Last seen window geometry, read back when the app quits.
static WINDOW_STATE: std::sync::Mutex<Option<crate::settings::WindowState>> =
    std::sync::Mutex::new(None);

/// Runs on every root render, which includes every resize and maximize.
fn remember_window_state(window: &Window) {
    let Some(state) = window_state_for(window.window_bounds()) else {
        return;
    };
    if let Ok(mut slot) = WINDOW_STATE.lock() {
        *slot = Some(state);
    }
}

/// The state worth saving for `bounds`. Fullscreen yields `None`: its
/// bounds are the monitor, and saving them as "maximized" made the next
/// launch open maximized instead of at the last windowed size.
fn window_state_for(bounds: WindowBounds) -> Option<crate::settings::WindowState> {
    let (bounds, maximized) = match bounds {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) => (bounds, true),
        WindowBounds::Fullscreen(_) => return None,
    };
    Some(crate::settings::WindowState {
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        maximized,
    })
}

/// Write the last seen window state to settings.json.
fn persist_window_state() {
    let state = WINDOW_STATE.lock().ok().and_then(|slot| *slot);
    if let Some(state) = state {
        crate::settings::save_window_state(state);
    }
}

pub fn run() {
    crate::init_logging(crate::LogOutput::FileAndStderr);

    // One read of settings.json for the whole startup: the backend, the
    // theme, the window geometry, and the root view all take it from here.
    let startup_settings = crate::settings::load_settings();
    let backend = Arc::new(
        AgentBackend::new(
            crate::configured_api_url(),
            startup_settings.effective_harness_instructions(),
        )
        .expect("failed to initialize agent backend"),
    );

    // Restore a persisted session before the UI starts so sign-in can be
    // skipped entirely when the credentials are still valid.
    let restored_user = backend.restore_now();

    Application::new()
        .with_assets(crate::assets::Assets)
        .run(move |cx: &mut App| {
            if let Err(error) = cx.text_system().add_fonts(
                crate::assets::FONTS
                    .iter()
                    .map(|bytes| std::borrow::Cow::Borrowed(*bytes))
                    .collect(),
            ) {
                log::warn!("failed to register bundled fonts: {error}");
            }
            text_input::register_key_bindings(cx);
            ui::spell::preload();
            cx.on_action(|_: &QuitApp, cx| cx.quit());
            cx.bind_keys([
                KeyBinding::new("ctrl-q", QuitApp, None),
                KeyBinding::new("cmd-q", QuitApp, None),
                KeyBinding::new("escape", ui::chat::ChatEscape, Some("Chat")),
                KeyBinding::new("ctrl-c", ui::chat::CopySelection, Some("Transcript")),
                KeyBinding::new("cmd-c", ui::chat::CopySelection, Some("Transcript")),
                KeyBinding::new("ctrl-a", ui::chat::SelectAllTranscript, Some("Transcript")),
                KeyBinding::new("cmd-a", ui::chat::SelectAllTranscript, Some("Transcript")),
            ]);
            ui::theme::set_preference(ui::theme::Preference::parse(&startup_settings.theme));
            let saved = startup_settings
                .window
                .map(crate::settings::WindowState::clamped);
            let window_size = saved
                .map(|state| size(px(state.width), px(state.height)))
                .unwrap_or_else(|| size(px(1280.), px(860.)));
            let bounds: Bounds<Pixels> = Bounds::centered(None, window_size, cx);
            let window_bounds = if saved.is_some_and(|state| state.maximized) {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            };
            cx.on_app_quit(|_cx| async {
                persist_window_state();
            })
            .detach();
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(window_bounds),
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
                                    settings: startup_settings.clone(),
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
                                settings: startup_settings.clone(),
                                titlebar: cx.new(|_| TitleBar::new("Maple - Private AI Chat")),
                            })
                        }
                    },
                )
                .expect("failed to open main window");
            // Ask for a newer release off the UI thread; the banner shows
            // in the chat when one exists.
            if crate::update::enabled() {
                let check = backend.spawn(crate::update::check());
                let root_window = window;
                cx.spawn(async move |cx| {
                    if let Ok(Some(info)) = check.await {
                        root_window
                            .update(cx, |app: &mut MapleApp, _window, cx| {
                                app.set_update(info, cx);
                            })
                            .ok();
                    }
                })
                .detach();
            }
            let root = window
                .update(cx, |_, window, cx| {
                    // The only window: closing it from the window manager
                    // ends the app the same way the title bar button does.
                    window.on_window_should_close(cx, |_window, cx| {
                        persist_window_state();
                        cx.quit();
                        true
                    });
                    // A system theme change must reach every view.
                    window
                        .observe_window_appearance(|_window, cx| cx.refresh_windows())
                        .detach();
                    cx.entity()
                })
                .expect("root entity");

            window
                .update(cx, |app: &mut MapleApp, _window, cx| {
                    if let Screen::Login(login) = &app.screen {
                        let login = login.clone();
                        app.subscribe_login(&login, cx);
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(gpui::point(px(0.), px(0.)), size(px(width), px(height)))
    }

    #[test]
    fn fullscreen_does_not_replace_the_windowed_state() {
        let windowed = window_state_for(WindowBounds::Windowed(bounds(1280., 860.))).unwrap();
        assert_eq!((windowed.width, windowed.height), (1280., 860.));
        assert!(!windowed.maximized);

        let maximized = window_state_for(WindowBounds::Maximized(bounds(1280., 860.))).unwrap();
        assert!(maximized.maximized);

        assert!(window_state_for(WindowBounds::Fullscreen(bounds(3840., 2160.))).is_none());
    }
}
