//! The gpui desktop window: login/chat routing, window state, and the
//! event pump that forwards agent service events into the active chat
//! screen. Compiled only with the `desktop` feature.

use std::sync::Arc;

use gpui::{
    App, Bounds, Context, Entity, Pixels, Render, Window, WindowBounds, WindowOptions, actions,
    div, prelude::*, px, size,
};

actions!(maple_app, [QuitApp, Hide, HideOthers, ShowAll]);

use crate::backend::AgentBackend;
use crate::ui;
use crate::ui::chat::computers::{Computer, Destination, Navigate, NavigationChanged};
use crate::ui::chat::{ChatScreen, LoggedOut};
use crate::ui::login::{LoginScreen, LoginSucceeded};
use crate::ui::settings::{
    AccountDeleted, Section, SettingsClosed, SettingsScreen, ShortcutSettingsChange,
    ShortcutSettingsRequested, SignOutRequested,
};
use crate::ui::titlebar::TitleBar;

enum Screen {
    /// Construction placeholder. `run` replaces it before the first paint
    /// with the chat screen (saved sign-in) or the login form.
    Restoring,
    Login(Entity<LoginScreen>),
    Chat(Entity<ChatScreen>),
    Settings(Entity<SettingsScreen>),
}

/// One cache and one initial connection attempt per paired endpoint. Once
/// attached, the existing IPC/Iroh supervisor owns reconnects and snapshots.
struct RemoteComputer {
    name: String,
    grant: [u8; 16],
    chat: Option<Entity<ChatScreen>>,
    attempt: ConnectionAttempt,
}

#[derive(Clone)]
struct PendingNavigation {
    request: Navigate,
    source: Option<(gpui::EntityId, u64)>,
}

enum ConnectionAttempt {
    Idle,
    Connecting(u64),
    Failed {
        message: String,
        retry_at: std::time::Instant,
    },
}
impl RemoteComputer {
    fn should_connect(&self, now: std::time::Instant) -> bool {
        self.chat.is_none()
            && match &self.attempt {
                ConnectionAttempt::Idle => true,
                ConnectionAttempt::Connecting(_) => false,
                ConnectionAttempt::Failed { retry_at, .. } => now >= *retry_at,
            }
    }
    fn error(&self) -> Option<String> {
        match &self.attempt {
            ConnectionAttempt::Failed { message, .. } => Some(message.clone()),
            _ => None,
        }
    }
}

struct MapleApp {
    backend: Arc<AgentBackend>,
    screen: Screen,
    user_id: Option<String>,
    /// The chat screen is parked while settings is open so Back returns to
    /// it with its state intact.
    parked_chat: Option<Entity<ChatScreen>>,
    local_chat: Option<Entity<ChatScreen>>,
    selected_host: Option<String>,
    remote_computers: std::collections::BTreeMap<String, RemoteComputer>,
    next_connection_attempt: u64,
    requested_navigation: Option<PendingNavigation>,
    computers_sync_pending: bool,
    remote_enabled: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    tray: Option<crate::tray::StatusIcon>,
    remote_bridges: std::cell::RefCell<Vec<gpui::Task<()>>>,
    settings: crate::settings::AppSettings,
    /// The complete live shortcut map. Settings mutations prepare a full
    /// replacement before this runtime clears GPUI's process-wide bindings.
    shortcuts: crate::shortcuts::ShortcutRuntime,
    /// Created once; re-creating it per render leaked an entity per frame.
    titlebar: Entity<TitleBar>,
}

impl MapleApp {
    /// Forward a batch of backend service events to the chat screen when
    /// one exists, parked behind Settings included, so a run that ends
    /// meanwhile shows ended on return. One batch is one render, however
    /// many events arrived.
    fn handle_service_events(
        &mut self,
        events: Vec<maple_agent::agent::AgentServiceEvent>,
        cx: &mut Context<Self>,
    ) {
        let mut batch = Vec::new();
        for event in events {
            if let maple_agent::agent::AgentServiceEvent::AccountChanged(user_id) = &event {
                // Deliver the old account's queued frames only to its screen.
                // A reconnect may discover a sign-out/account change while away.
                self.forward_service_events(std::mem::take(&mut batch), cx);
                if &self.user_id != user_id {
                    match user_id {
                        Some(user_id) => self.open_chat(user_id.clone(), cx),
                        None => self.show_login(cx),
                    }
                }
            } else {
                batch.push(event);
            }
        }
        self.forward_service_events(batch, cx);
    }

    fn forward_service_events(
        &mut self,
        events: Vec<maple_agent::agent::AgentServiceEvent>,
        cx: &mut Context<Self>,
    ) {
        if let Some(chat) = &self.local_chat {
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
        self.set_remote_enabled(false, cx);
        self.parked_chat = None;
        self.local_chat = None;
        self.remote_computers.clear();
        self.requested_navigation = None;
        self.selected_host = None;
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
        log::debug!(
            "startup: chat screen open at {} ms",
            crate::startup_elapsed()
        );
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
        self.local_chat = Some(chat.clone());
        self.remote_computers.clear();
        self.requested_navigation = None;
        self.selected_host = None;
        self.subscribe_chat(&chat, cx);
        cx.notify();
    }

    fn update_computers(&mut self, status: crate::remote::manager::Status, cx: &mut Context<Self>) {
        self.set_remote_enabled(status.enabled, cx);
        let computers = status
            .peers
            .into_iter()
            .filter(|(_, peer)| peer.can_connect)
            .map(|(key, peer)| {
                let alias = self
                    .settings
                    .remote_sidebars
                    .get(&key)
                    .map_or("", |s| s.display_name.as_str());
                let label = ui::remote::computer_name(&key, &peer.name, alias);
                (key, (label, peer.grant))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut changed = self.remote_computers.len() != computers.len();
        let mut selected_grant_changed = false;
        self.remote_computers
            .retain(|key, _| computers.contains_key(key));
        for (key, (name, grant)) in computers {
            match self.remote_computers.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    changed = true;
                    entry.insert(RemoteComputer {
                        name,
                        grant,
                        chat: None,
                        attempt: ConnectionAttempt::Idle,
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if entry.get().grant != grant {
                        changed = true;
                        selected_grant_changed |= self.selected_host.as_ref() == Some(entry.key());
                        let computer = entry.get_mut();
                        computer.grant = grant;
                        computer.chat = None;
                        computer.attempt = ConnectionAttempt::Idle;
                    }
                    if entry.get().name != name {
                        changed = true;
                        entry.get_mut().name = name;
                    }
                }
            }
        }
        if selected_grant_changed
            || self
                .selected_host
                .as_ref()
                .is_some_and(|key| !self.remote_computers.contains_key(key))
        {
            self.navigate(
                Navigate {
                    computer: None,
                    destination: Destination::Overview,
                },
                cx,
            );
        }
        if changed {
            self.sync_computers(cx);
        }
        let now = std::time::Instant::now();
        let ready = self
            .remote_computers
            .iter()
            .filter(|(_, computer)| computer.should_connect(now))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for peer in ready {
            self.connect_computer(peer, cx);
        }
        for chat in self.local_chat.iter().chain(
            self.remote_computers
                .values()
                .filter_map(|c| c.chat.as_ref()),
        ) {
            chat.update(cx, |chat, cx| chat.refresh_navigation_if_due(cx));
        }
    }

    fn sync_computers(&mut self, cx: &mut Context<Self>) {
        let Some(local) = &self.local_chat else {
            return;
        };
        let mut computers = vec![local.read(cx).computer_snapshot("This computer".into(), cx)];
        for (key, computer) in &self.remote_computers {
            computers.push(match &computer.chat {
                Some(chat) => chat
                    .read(cx)
                    .computer_snapshot(computer.name.clone().into(), cx),
                None => Computer::pending(key.clone(), computer.name.clone(), computer.error()),
            });
        }
        let computers = Arc::new(computers);
        for chat in std::iter::once(local).chain(
            self.remote_computers
                .values()
                .filter_map(|c| c.chat.as_ref()),
        ) {
            chat.update(cx, |chat, cx| chat.set_computers(computers.clone(), cx));
        }
    }

    fn schedule_computers_sync(&mut self, cx: &mut Context<Self>) {
        if self.computers_sync_pending {
            return;
        }
        self.computers_sync_pending = true;
        let weak = cx.entity().downgrade();
        cx.defer(move |cx| {
            weak.update(cx, |app, cx| {
                app.computers_sync_pending = false;
                app.sync_computers(cx);
            })
            .ok();
        });
    }

    fn navigate(&mut self, request: Navigate, cx: &mut Context<Self>) {
        self.requested_navigation = Some(PendingNavigation {
            request: request.clone(),
            source: self.navigation_source(cx),
        });
        let chat = match &request.computer {
            None => self.local_chat.clone(),
            Some(peer) => self
                .remote_computers
                .get(peer)
                .and_then(|computer| computer.chat.as_ref())
                .filter(|chat| !chat.read(cx).connection_ended())
                .cloned(),
        };
        if let Some(chat) = chat {
            let browse = match &self.screen {
                Screen::Chat(previous) if previous.entity_id() != chat.entity_id() => {
                    Some(previous.read(cx).sidebar_browse_state(cx))
                }
                _ => None,
            };
            for other in self.local_chat.iter().chain(
                self.remote_computers
                    .values()
                    .filter_map(|c| c.chat.as_ref()),
            ) {
                other.update(cx, |other, cx| other.set_visible(false, cx));
            }
            self.selected_host = request.computer;
            self.screen = Screen::Chat(chat.clone());
            self.parked_chat = None;
            chat.update(cx, |chat, cx| {
                chat.set_visible(true, cx);
                if let Some(browse) = browse {
                    chat.restore_sidebar_browse_state(browse, cx);
                }
                chat.navigate(request.destination, cx);
            });
            self.requested_navigation = None;
            cx.notify();
        } else if let Some(peer) = request.computer {
            self.connect_computer(peer, cx);
        }
    }

    fn navigation_source(&self, cx: &gpui::App) -> Option<(gpui::EntityId, u64)> {
        match &self.screen {
            Screen::Chat(chat) => Some((chat.entity_id(), chat.read(cx).navigation_generation())),
            _ => None,
        }
    }

    fn set_remote_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self
            .remote_enabled
            .swap(enabled, std::sync::atomic::Ordering::AcqRel)
            == enabled
        {
            return;
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        if let Some(tray) = &self.tray {
            tray.set_enabled(enabled);
        }
        cx.notify();
    }

    fn connect_computer(&mut self, peer: String, cx: &mut Context<Self>) {
        let Some(user_id) = self.user_id.clone() else {
            return;
        };
        let Some(computer) = self.remote_computers.get_mut(&peer) else {
            return;
        };
        if matches!(computer.attempt, ConnectionAttempt::Connecting(_)) {
            return;
        }
        self.next_connection_attempt += 1;
        let attempt = self.next_connection_attempt;
        computer.attempt = ConnectionAttempt::Connecting(attempt);
        let backend = self.backend.clone();
        let target = peer.clone();
        self.sync_computers(cx);
        let bridge = ui::task::call(
            &self.backend,
            async move { backend.remote(target).await },
            cx,
            move |app, result, cx| {
                if app.user_id.as_deref() != Some(&user_id) {
                    return;
                }
                // Unpairing/re-pairing or signing out discards an old attempt.
                // A late completion never resurrects that computer or its view.
                if !app.remote_computers.get(&peer).is_some_and(|computer| matches!(computer.attempt, ConnectionAttempt::Connecting(id) if id == attempt)) {
                    return;
                }
                match result {
                    Ok(backend) => {
                        let source = backend.clone();
                        let rx = backend.spawn(async move { source.take_events().await });
                        let chat = cx.new(|cx| ChatScreen::new_catalog(backend, user_id, cx));
                        app.subscribe_chat(&chat, cx);
                        let weak = chat.downgrade();
                        cx.spawn(async move |_, cx| {
                            let Some(mut rx) = rx.await.ok().flatten() else {
                                return;
                            };
                            while let Some(event) = rx.recv().await {
                                let mut batch = vec![event];
                                while batch.len() < 256 {
                                    if let Ok(event) = rx.try_recv() {
                                        batch.push(event);
                                    } else {
                                        break;
                                    }
                                }
                                if weak
                                    .update(cx, |chat, cx| chat.handle_service_events(batch, cx))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        })
                        .detach();
                        if let Some(computer) = app.remote_computers.get_mut(&peer) {
                            computer.chat = Some(chat.clone());
                            computer.attempt = ConnectionAttempt::Idle;
                        }
                        if let Some(pending) = app.requested_navigation.clone()
                            && pending.request.computer.as_deref() == Some(&peer)
                        {
                            if pending.source == app.navigation_source(cx) {
                                app.navigate(pending.request, cx);
                            } else {
                                app.requested_navigation = None;
                            }
                        }
                    }
                    Err(error) => {
                        if let Some(computer) = app.remote_computers.get_mut(&peer) {
                            computer.attempt = ConnectionAttempt::Failed {
                                message: error.clone(),
                                retry_at: std::time::Instant::now()
                                    + std::time::Duration::from_secs(30),
                            };
                        }
                        if app.requested_navigation.as_ref().is_some_and(|pending| {
                            pending.request.computer.as_deref() == Some(&peer)
                        }) && let Screen::Chat(chat) = &app.screen
                        {
                            chat.update(cx, |chat, cx| chat.navigation_notice(error, cx));
                            app.requested_navigation = None;
                        }
                    }
                }
                app.sync_computers(cx);
                cx.notify();
            },
        );
        ui::task::retain(&self.remote_bridges, bridge);
    }

    /// Park the chat screen and show settings.
    fn show_settings(&mut self, section: Section, cx: &mut Context<Self>) {
        let Screen::Chat(chat) = &self.screen else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone().unwrap_or_default();
        let settings = self.settings.clone();
        let shortcut_snapshot = self.shortcuts.snapshot();
        let integrations_backend = chat.read(cx).execution_backend();
        chat.update(cx, |chat, cx| chat.set_visible(false, cx));
        let screen = cx.new(|cx| {
            SettingsScreen::new(
                backend,
                integrations_backend,
                user_id,
                settings,
                shortcut_snapshot,
                section,
                cx,
            )
        });
        cx.subscribe(
            &screen,
            |app, _, event: &ui::remote::ComputersChanged, cx| {
                app.update_computers(event.0.clone(), cx);
            },
        )
        .detach();
        cx.subscribe(&screen, |app, _, event: &ui::remote::ComputerNamed, cx| {
            app.settings
                .remote_sidebars
                .entry(event.key.clone())
                .or_default()
                .display_name = event.name.clone();
            if let Some(computer) = app.remote_computers.get_mut(&event.key) {
                computer.name = event.name.clone();
                app.sync_computers(cx);
            }
        })
        .detach();
        cx.subscribe(
            &screen,
            |app, _, event: &ui::remote::ConnectComputer, cx| {
                app.navigate(
                    Navigate {
                        computer: Some(event.0.clone()),
                        destination: Destination::Overview,
                    },
                    cx,
                );
            },
        )
        .detach();
        cx.subscribe(
            &screen,
            |app: &mut MapleApp, _emitter, event: &SettingsClosed, cx| {
                app.close_settings(event.0.clone(), cx);
            },
        )
        .detach();
        cx.subscribe(
            &screen,
            |app: &mut MapleApp, emitter, event: &ShortcutSettingsRequested, cx| {
                let result = app.apply_shortcut_change(&event.0, cx);
                let shortcut_overrides = app.settings.shortcut_overrides.clone();
                let snapshot = app.shortcuts.snapshot();
                emitter.update(cx, |screen, cx| {
                    screen.apply_shortcut_result(shortcut_overrides, snapshot, result, cx);
                });
            },
        )
        .detach();
        cx.subscribe(
            &screen,
            |app: &mut MapleApp, _emitter, _event: &SignOutRequested, cx| {
                if let Some(chat) = app.local_chat.take() {
                    chat.update(cx, |chat, cx| chat.sign_out(cx));
                }
                app.show_login(cx);
            },
        )
        .detach();
        cx.subscribe(
            &screen,
            |app: &mut MapleApp, _emitter, _event: &AccountDeleted, cx| {
                // The runtime stopped and the credentials went before the
                // event; the parked chat has nothing left to sign out of.
                app.parked_chat = None;
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
        cx.subscribe(chat, |app, _, _: &NavigationChanged, cx| {
            app.schedule_computers_sync(cx)
        })
        .detach();
        cx.subscribe(chat, |app, _, request: &Navigate, cx| {
            app.navigate(request.clone(), cx)
        })
        .detach();
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
        ui::theme::apply_preference(ui::theme::Preference::parse(&self.settings.theme), cx);
        ui::typography::apply(
            ui::typography::ChatFontFamily::parse(&self.settings.chat_font_family),
            self.settings.chat_font_size,
        );
        for chat in self.local_chat.iter().chain(
            self.remote_computers
                .values()
                .filter_map(|c| c.chat.as_ref()),
        ) {
            chat.update(cx, |chat, cx| chat.apply_defaults(&self.settings, cx));
        }
        if let Some(chat) = self.parked_chat.take() {
            chat.update(cx, |chat, cx| chat.set_visible(true, cx));
            self.screen = Screen::Chat(chat);
        }
        cx.notify();
    }

    fn apply_shortcut_change(
        &mut self,
        change: &ShortcutSettingsChange,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut overrides = self.settings.shortcut_overrides.clone();
        crate::shortcuts::migrate_renamed_slots(&mut overrides);
        match change {
            ShortcutSettingsChange::Set {
                slot_id,
                sequence,
                disable_conflicts,
            } => {
                for conflict in disable_conflicts {
                    if conflict != slot_id {
                        overrides.insert(conflict.clone(), None);
                    }
                }
                overrides.insert(slot_id.clone(), Some(sequence.clone()));
            }
            ShortcutSettingsChange::Disable { slot_id } => {
                overrides.insert(slot_id.clone(), None);
            }
            ShortcutSettingsChange::Reset { slot_id } => {
                overrides.remove(slot_id);
            }
            ShortcutSettingsChange::ResetAll => overrides.clear(),
        }

        self.shortcuts.replace(&overrides, cx)?;
        self.settings.shortcut_overrides = overrides.clone();
        crate::settings::update_settings_in_background(move |settings| {
            settings.shortcut_overrides = overrides;
        });
        Ok(())
    }
}

impl Render for MapleApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        static FIRST_RENDER: std::sync::Once = std::sync::Once::new();
        FIRST_RENDER.call_once(|| {
            log::debug!("startup: first render at {} ms", crate::startup_elapsed());
        });
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
            // The default for any text that sets no color of its own.
            // GPUI's own default is black, which vanishes on the dark
            // surfaces, so every omission used to be a dark-mode bug.
            .text_color(gpui::rgb(ui::theme::text_primary()))
            // Chrome default 14 px (`text_sm`). Chat reading size is set
            // on transcript/composer via `typography::chat_reading`.
            .text_sm()
            .child(titlebar)
            .child(match &self.screen {
                Screen::Restoring => restoring_view().into_any_element(),
                Screen::Login(login) => login.clone().into_any_element(),
                Screen::Chat(chat) => chat.clone().into_any_element(),
                Screen::Settings(screen) => screen.clone().into_any_element(),
            })
    }
}

fn install_close_behavior(
    window: &mut Window,
    cx: &mut App,
    remote_enabled: Arc<std::sync::atomic::AtomicBool>,
    can_background: bool,
) {
    window.on_window_should_close(cx, move |_, cx| {
        persist_window_state();
        if remote_enabled.load(std::sync::atomic::Ordering::Acquire) {
            #[cfg(target_os = "macos")]
            {
                let _ = can_background;
                cx.hide();
                false
            }
            #[cfg(not(target_os = "macos"))]
            {
                can_background
            }
        } else {
            cx.quit();
            true
        }
    });
}

/// Shown only if a paint happens before `run` picks the first screen,
/// which places it in the same event-loop turn as the window open.
fn restoring_view() -> gpui::Div {
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .justify_center()
        .items_center()
        .bg(gpui::rgb(ui::theme::bg_app()))
        .child(ui::titlebar::drag_strip())
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(ui::theme::text_secondary()))
                .child("Signing in…"),
        )
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

/// Every key binding the window uses. Tests register the same set, so
/// what they exercise is what ships.
#[cfg(test)]
pub(crate) fn register_key_bindings(cx: &mut App) {
    crate::keymap::bootstrap(cx);
}

fn main_window_options(window_bounds: WindowBounds) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(window_bounds),
        // X11 WM_CLASS and Wayland app_id must match the packaged desktop
        // entry, so Dev, Prod, Research, and local debug windows group apart.
        app_id: Some(crate::profile::BUNDLE_ID.to_string()),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(ui::titlebar::WINDOW_TITLE.into()),
            appears_transparent: ui::titlebar::TRANSPARENT_TITLEBAR,
            traffic_light_position: ui::titlebar::TRANSPARENT_TITLEBAR
                .then_some(ui::titlebar::TRAFFIC_LIGHT_POSITION),
        }),
        app_owns_titlebar_drag: ui::titlebar::TRANSPARENT_TITLEBAR,
        ..Default::default()
    }
}

struct StartupError(String);

impl Render for StartupError {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .bg(gpui::rgb(ui::theme::bg_app()))
            .text_color(gpui::rgb(ui::theme::text_primary()))
            .child(div().text_lg().child("Maple could not start"))
            .child(self.0.clone())
            .child(
                div()
                    .id("close-startup-error")
                    .cursor_pointer()
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(gpui::rgb(ui::theme::bg_elevated()))
                    .child("Close")
                    .on_click(|_, _, cx| cx.quit()),
            )
    }
}

fn show_startup_error(error: String) {
    gpui_platform::application().run(move |cx| {
        cx.set_app_identity(crate::profile::BUNDLE_ID, crate::profile::DISPLAY_NAME);
        cx.on_action(|_: &QuitApp, cx| cx.quit());
        ui::menus::install(cx);
        let bounds = Bounds::centered(None, size(px(560.), px(260.)), cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some(crate::profile::BUNDLE_ID.into()),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(crate::profile::DISPLAY_NAME.into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                window.on_window_should_close(cx, |_, cx| {
                    cx.quit();
                    true
                });
                cx.new(|_| StartupError(error))
            },
        );
        if let Err(error) = result {
            log::error!("Could not show the startup error: {error}");
            cx.quit();
        } else {
            cx.activate(true);
        }
    });
}

pub fn run() {
    crate::init_logging(crate::LogOutput::FileAndStderr);
    log::debug!("startup: logging ready at {} ms", crate::startup_elapsed());

    // One read of settings.json for the whole startup: the backend, the
    // theme, the window geometry, and the root view all take it from here.
    // Name the resolved file so a launcher with its own XDG_CONFIG_HOME
    // makes itself visible: settings that look unsaved usually live in a
    // different root than the one this launch reads.
    let startup_settings = crate::settings::load_settings();
    log::debug!(
        "startup: settings loaded from {} at {} ms",
        crate::backend::app_config_root()
            .join("settings.json")
            .display(),
        crate::startup_elapsed()
    );
    let backend = match AgentBackend::new(
        crate::configured_api_url(),
        startup_settings.effective_harness_instructions(),
    ) {
        Ok(backend) => Arc::new(backend),
        Err(error) => {
            log::error!("Could not start Maple: {error}");
            show_startup_error(error);
            return;
        }
    };
    log::debug!("startup: backend ready at {} ms", crate::startup_elapsed());

    gpui_platform::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx: &mut App| {
            cx.set_app_identity(crate::profile::BUNDLE_ID, crate::profile::DISPLAY_NAME);
            cx.set_quit_mode(gpui::QuitMode::Explicit);
            log::debug!("startup: gpui app ready at {} ms", crate::startup_elapsed());
            if let Err(error) = cx.text_system().add_fonts(
                crate::assets::FONTS
                    .iter()
                    .map(|bytes| std::borrow::Cow::Borrowed(*bytes))
                    .collect(),
            ) {
                log::warn!("failed to register bundled fonts: {error}");
            }
            ui::spell::preload();
            cx.on_action(|_: &QuitApp, cx| cx.quit());
            #[cfg(target_os = "macos")]
            {
                cx.on_action(|_: &Hide, cx| cx.hide());
                cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
                cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
            }
            let shortcut_runtime = crate::shortcuts::ShortcutRuntime::bootstrap(
                &startup_settings.shortcut_overrides,
                cx,
            );
            ui::theme::apply_preference(ui::theme::Preference::parse(&startup_settings.theme), cx);
            ui::typography::apply(
                ui::typography::ChatFontFamily::parse(&startup_settings.chat_font_family),
                startup_settings.chat_font_size,
            );
            ui::menus::install(cx);
            // A click on a notification brings the app forward and shows
            // the task it was about (tags are `task:<session id>`; see
            // `ChatScreen::notify_desktop`).
            cx.on_system_notification_response(|response, cx| {
                cx.activate(true);
                let session_id = response.tag.strip_prefix("task:").map(str::to_owned);
                let remote_task = response
                    .tag
                    .strip_prefix("remote-task:")
                    .and_then(|tag| tag.split_once(':'))
                    .map(|(host, task)| (host.to_owned(), task.to_owned()));
                for window in cx.windows() {
                    window
                        .update(cx, |root, window, cx| {
                            window.activate_window();
                            if let Ok(app) = root.downcast::<MapleApp>() {
                                app.update(cx, |app, cx| {
                                    if let Some((host, task)) = &remote_task {
                                        app.navigate(
                                            Navigate {
                                                computer: Some(host.clone()),
                                                destination: Destination::Task(task.clone()),
                                            },
                                            cx,
                                        );
                                    } else if let Some(task) = &session_id {
                                        app.navigate(
                                            Navigate {
                                                computer: None,
                                                destination: Destination::Task(task.clone()),
                                            },
                                            cx,
                                        );
                                    }
                                    cx.notify();
                                });
                            }
                        })
                        .ok();
                }
            });
            cx.set_reduce_motion(startup_settings.reduce_motion);
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
            let remote_enabled = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let quit_remote_enabled = remote_enabled.clone();
            let quit_backend = backend.clone();
            cx.on_app_quit(move |_cx| {
                let backend = quit_backend.clone();
                let remote_enabled = quit_remote_enabled.clone();
                async move {
                    persist_window_state();
                    if remote_enabled.load(std::sync::atomic::Ordering::Acquire) {
                        let api = backend.api_url().to_owned();
                        let _ = backend
                            .spawn(async move {
                                tokio::task::spawn_blocking(move || crate::host::stop(&api)).await
                            })
                            .await;
                    } else {
                        let _ = backend.detach().await;
                    }
                }
            })
            .detach();
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            let (tray, tray_events) = match crate::tray::StatusIcon::new() {
                Ok((tray, events)) => (Some(tray), Some(events)),
                Err(error) => {
                    log::warn!("{error}");
                    (None, None)
                }
            };
            let root_enabled = remote_enabled.clone();
            let root_backend = backend.clone();
            let root_settings = startup_settings.clone();
            let window = cx
                .open_window(main_window_options(window_bounds), move |_, cx| {
                    cx.new(move |cx| MapleApp {
                        backend: root_backend,
                        screen: Screen::Restoring,
                        user_id: None,
                        parked_chat: None,
                        local_chat: None,
                        selected_host: None,
                        remote_computers: Default::default(),
                        next_connection_attempt: 0,
                        requested_navigation: None,
                        computers_sync_pending: false,
                        remote_enabled: root_enabled,
                        #[cfg(any(target_os = "linux", target_os = "windows"))]
                        tray,
                        remote_bridges: std::cell::RefCell::new(Vec::new()),
                        settings: root_settings,
                        shortcuts: shortcut_runtime,
                        titlebar: cx.new(|_| TitleBar::new(ui::titlebar::WINDOW_TITLE)),
                    })
                })
                .expect("failed to open main window");
            log::debug!("startup: window open at {} ms", crate::startup_elapsed());
            // Saved credentials are trusted at once: the chat screen opens
            // with the account's local task list while the server validates
            // the credentials in the background. Only a definitive rejection
            // returns to the login form; offline the local history stays
            // readable and the runtime start reports the connection error.
            {
                let root_window = window;
                match backend.saved_user_id() {
                    Some(user_id) => {
                        let restore = backend.restore_in_background();
                        root_window
                            .update(cx, |app: &mut MapleApp, _window, cx| {
                                app.open_chat(user_id, cx);
                            })
                            .ok();
                        cx.spawn(async move |cx| {
                            let outcome = restore.await.ok();
                            log::debug!(
                                "startup: credential validation done at {} ms ({outcome:?})",
                                crate::startup_elapsed()
                            );
                            if outcome == Some(crate::backend::RestoreOutcome::Rejected) {
                                root_window
                                    .update(cx, |app: &mut MapleApp, _window, cx| {
                                        app.show_login(cx);
                                    })
                                    .ok();
                            }
                        })
                        .detach();
                    }
                    None => {
                        root_window
                            .update(cx, |app: &mut MapleApp, _window, cx| app.show_login(cx))
                            .ok();
                    }
                }
            }
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
                .update(cx, |app, window, cx| {
                    // The only window: closing it from the window manager
                    // ends the app the same way the title bar button does.
                    #[cfg(target_os = "macos")]
                    let can_background = {
                        let _ = app;
                        true
                    };
                    #[cfg(any(target_os = "linux", target_os = "windows"))]
                    let can_background = app.tray.is_some();
                    install_close_behavior(window, cx, remote_enabled.clone(), can_background);
                    // A system theme change must reach every view.
                    window
                        .observe_window_appearance(|_window, cx| cx.refresh_windows())
                        .detach();
                    cx.entity()
                })
                .expect("root entity");

            // Local status is cheap and does not contact the enrollment
            // service. This also notices changes made through the CLI.
            let status_backend = backend.clone();
            let status_root = root.clone();
            cx.spawn(async move |cx| {
                loop {
                    let account = status_root.update(cx, |app, _| app.user_id.clone());
                    let backend = status_backend.clone();
                    let task = status_backend.spawn(async move {
                        backend
                            .remote_setting::<crate::remote::manager::Status>("remote_status", ())
                            .await
                    });
                    if let Ok(Ok(status)) = task.await {
                        status_root.update(cx, |app, cx| {
                            if app.user_id == account && account.is_some() {
                                app.update_computers(status, cx);
                            }
                        });
                    }
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(5))
                        .await;
                }
            })
            .detach();
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            if let Some(mut events) = tray_events {
                let tray_root = root.clone();
                let enabled = remote_enabled.clone();
                cx.spawn(async move |cx| {
                    while let Some(action) = events.recv().await {
                        match action {
                            crate::tray::Action::Quit => {
                                cx.update(|cx| cx.quit());
                                break;
                            }
                            crate::tray::Action::Open => {
                                cx.update(|cx| {
                                    if cx.windows().is_empty() {
                                        let root = tray_root.clone();
                                        let enabled = enabled.clone();
                                        let bounds =
                                            Bounds::centered(None, size(px(1280.), px(860.)), cx);
                                        let _ = cx.open_window(
                                            main_window_options(WindowBounds::Windowed(bounds)),
                                            move |window, cx| {
                                                install_close_behavior(window, cx, enabled, true);
                                                root
                                            },
                                        );
                                    } else {
                                        for window in cx.windows() {
                                            let _ = window.update(cx, |_, window, _| {
                                                window.activate_window()
                                            });
                                        }
                                    }
                                });
                            }
                        }
                    }
                })
                .detach();
            }

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
                    // This gpui's entity update is infallible; the pump ends
                    // with the channel instead.
                    root.update(cx, |app, cx| app.handle_service_events(batch, cx));
                }
            })
            .detach();

            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn computer_connection_retries_are_independent_and_not_started_twice() {
        let now = std::time::Instant::now();
        let mut computer = super::RemoteComputer {
            name: "Office".into(),
            grant: [0; 16],
            chat: None,
            attempt: super::ConnectionAttempt::Idle,
        };
        assert!(computer.should_connect(now));
        computer.attempt = super::ConnectionAttempt::Connecting(1);
        assert!(!computer.should_connect(now));
        computer.attempt = super::ConnectionAttempt::Failed {
            message: "Offline".into(),
            retry_at: now + std::time::Duration::from_secs(30),
        };
        assert!(!computer.should_connect(now));
        assert!(computer.should_connect(now + std::time::Duration::from_secs(30)));
        let other = super::RemoteComputer {
            name: "Laptop".into(),
            grant: [1; 16],
            chat: None,
            attempt: super::ConnectionAttempt::Idle,
        };
        assert!(other.should_connect(now));
    }

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

    #[test]
    fn window_identity_and_title_match_the_packaging_metadata() {
        let options = main_window_options(WindowBounds::Windowed(bounds(1280., 860.)));
        let info = serde_json::to_value(crate::profile::build_info()).unwrap();
        assert_eq!(options.app_id.as_deref(), info["bundle_id"].as_str());
        assert_eq!(
            options.titlebar.unwrap().title.unwrap().as_ref(),
            info["display_name"].as_str().unwrap()
        );
    }
}
