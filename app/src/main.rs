//! Root application: login/chat routing, backend ownership, and the event
//! pump that forwards agent service events into the chat screen.

mod backend;
mod ui;

use std::sync::Arc;

use gpui::{
    App, Application, Bounds, Context, Entity, Global, Pixels, Render, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};

use backend::AgentBackend;
use ui::chat::ChatScreen;
use ui::login::{LoginScreen, LoginSucceeded};
use ui::text_input;

struct Globals {
    backend: Arc<AgentBackend>,
}

impl Global for Globals {}

enum Screen {
    Login(Entity<LoginScreen>),
    Chat(Entity<ChatScreen>),
}

struct MapleApp {
    screen: Screen,
    user_id: Option<String>,
}

impl Render for MapleApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(match &self.screen {
            Screen::Login(login) => login.clone().into_any_element(),
            Screen::Chat(chat) => chat.clone().into_any_element(),
        })
    }
}

fn main() {
    env_logger::init();

    let api_url = std::env::var("MAPLE_API_URL")
        .unwrap_or_else(|_| "https://enclave.trymaple.ai".to_string());
    let backend = Arc::new(AgentBackend::new(api_url).expect("failed to initialize agent backend"));

    Application::new().run(move |cx: &mut App| {
        text_input::register_key_bindings(cx);
        cx.set_global(Globals {
            backend: backend.clone(),
        });

        let bounds: Bounds<Pixels> = Bounds::centered(None, size(px(1280.), px(860.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("Maple".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    let login = cx.new(|cx| LoginScreen::new(backend.clone(), cx));
                    cx.new(|_| MapleApp {
                        screen: Screen::Login(login.clone()),
                        user_id: None,
                    })
                },
            )
            .expect("failed to open main window");

        // Route login success into the chat screen and start the event pump.
        window
            .update(cx, |app: &mut MapleApp, _window, cx| {
                let Screen::Login(login) = &app.screen else {
                    return;
                };
                cx.subscribe(login, {
                    let backend = backend.clone();
                    move |app: &mut MapleApp, _emitter, event: &LoginSucceeded, cx| {
                        let user_id = event.0.user_id.clone();
                        let chat =
                            cx.new(|cx| ChatScreen::new(backend.clone(), user_id.clone(), cx));
                        app.user_id = Some(user_id);
                        app.screen = Screen::Chat(chat.clone());
                        start_event_pump(cx, &backend, &chat);
                        cx.notify();
                    }
                })
                .detach();
            })
            .expect("subscribe login");

        cx.activate(true);
    });
}

/// Drain the backend event stream on the UI executor and forward each event
/// into the chat screen.
fn start_event_pump(
    cx: &mut Context<MapleApp>,
    backend: &Arc<AgentBackend>,
    chat: &Entity<ChatScreen>,
) {
    let backend = backend.clone();
    let chat = chat.clone();
    let (spawn_backend, take_backend) = (backend.clone(), backend.clone());
    let rx = spawn_backend.spawn(async move { take_backend.take_events().await });
    cx.spawn(async move |_this, cx| {
        let Some(mut rx) = rx.await.ok().flatten() else {
            return;
        };
        while let Some(event) = rx.recv().await {
            chat.update(cx, |chat, cx| chat.handle_service_event(event, cx))
                .ok();
        }
    })
    .detach();
}
