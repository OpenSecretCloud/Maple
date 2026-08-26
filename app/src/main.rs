//! Root application: login/chat routing, backend ownership, and the event
//! pump that forwards agent service events into the active chat screen.

mod backend;
mod settings;
mod ui;

use std::sync::Arc;

use gpui::{
    App, Application, Bounds, Context, Entity, Global, KeyBinding, Pixels, Render, Window,
    WindowBounds, WindowOptions, actions, div, prelude::*, px, size,
};

actions!(maple_app, [QuitApp]);

use backend::AgentBackend;
use ui::chat::{ChatScreen, LoggedOut};
use ui::login::{LoginScreen, LoginSucceeded};
use ui::settings::{SettingsClosed, SettingsScreen};
use ui::text_input;
use ui::titlebar::TitleBar;

struct Globals {
    backend: Arc<AgentBackend>,
}

impl Global for Globals {}

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
}

impl MapleApp {
    /// Forward one backend service event to the chat screen when one exists.
    fn handle_service_event(
        &mut self,
        event: maple_agent::agent::AgentServiceEvent,
        cx: &mut Context<Self>,
    ) {
        if let Screen::Chat(chat) = &self.screen {
            chat.update(cx, |chat, cx| chat.handle_service_event(event, cx));
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
    fn show_settings(&mut self, cx: &mut Context<Self>) {
        let Screen::Chat(chat) = &self.screen else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone().unwrap_or_default();
        let settings = self.settings.clone();
        let screen = cx.new(|cx| SettingsScreen::new(backend, user_id, settings, cx));
        cx.subscribe(
            &screen,
            |app: &mut MapleApp, _emitter, event: &SettingsClosed, cx| {
                app.close_settings(event.0.clone(), cx);
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
                app.show_settings(cx);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let titlebar = cx.new(|_| TitleBar::new("Maple"));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(titlebar)
            .child(match &self.screen {
                Screen::Login(login) => login.clone().into_any_element(),
                Screen::Chat(chat) => chat.clone().into_any_element(),
                Screen::Settings(screen) => screen.clone().into_any_element(),
            })
    }
}

fn main() {
    env_logger::init();

    let api_url = std::env::var("MAPLE_API_URL")
        .unwrap_or_else(|_| "https://enclave.trymaple.ai".to_string());
    let backend = Arc::new(AgentBackend::new(api_url).expect("failed to initialize agent backend"));

    // Restore a persisted session before the UI starts so sign-in can be
    // skipped entirely when the credentials are still valid.
    let restored_user = backend.restore_now();

    Application::new().run(move |cx: &mut App| {
        text_input::register_key_bindings(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-q", QuitApp, None),
            KeyBinding::new("cmd-q", QuitApp, None),
        ]);
        cx.on_action(|_: &QuitApp, cx| cx.quit());
        cx.set_global(Globals {
            backend: backend.clone(),
        });

        let bounds: Bounds<Pixels> = Bounds::centered(None, size(px(1280.), px(860.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some(format!("Maple v{}", ui::titlebar::TitleBar::version()).into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    if let Some(user_id) = restored_user.clone() {
                        let chat =
                            cx.new(|cx| ChatScreen::new(backend.clone(), user_id.clone(), cx));
                        let root = cx.new(|cx| {
                            let mut app = MapleApp {
                                backend: backend.clone(),
                                screen: Screen::Chat(chat.clone()),
                                user_id: Some(user_id),
                                parked_chat: None,
                                settings: crate::settings::load_settings(),
                            };
                            app.subscribe_chat(&chat, cx);
                            app
                        });
                        root
                    } else {
                        let login = cx.new(|cx| LoginScreen::new(backend.clone(), cx));
                        cx.new(|_| MapleApp {
                            backend: backend.clone(),
                            screen: Screen::Login(login.clone()),
                            user_id: None,
                            parked_chat: None,
                            settings: crate::settings::load_settings(),
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
                if root
                    .update(cx, |app, cx| app.handle_service_event(event, cx))
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
