use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib::{self, ControlFlow};
use gtk::pango;
use gtk::prelude::*;
use gtk4 as gtk;
use maple_core::{AppAction, AppState, AppUpdate, AuthState, ChatMessage, OAuthProvider, Screen};

use crate::manager::AppManager;
use crate::theme::{
    self, background_area, BackgroundKind, ThemeConfig, PEBBLE_400, SPACE_LG, SPACE_MD, SPACE_SM,
    SPACE_XS,
};
use crate::wordmark::{abbr_wordmark, full_wordmark};

const DEFAULT_WINDOW_WIDTH: i32 = 430;
const DEFAULT_WINDOW_HEIGHT: i32 = 860;
const MIN_WINDOW_WIDTH: i32 = 360;
const MIN_WINDOW_HEIGHT: i32 = 640;
const LOGIN_CARD_WIDTH: i32 = 360;
const HISTORY_PREFETCH_THRESHOLD: f64 = 72.0;
const BOTTOM_FOLLOW_THRESHOLD: f64 = 96.0;
const MESSAGE_MAX_WIDTH_CHARS: i32 = 34;

pub fn launch(app: &gtk::Application) {
    match DesktopApp::new(app) {
        Ok(desktop) => desktop.start(),
        Err(error) => show_boot_error(app, &error),
    }
}

#[derive(Clone, Default)]
struct LoginFormState {
    email: String,
    password: String,
    name: String,
    is_sign_up: bool,
}

#[derive(Clone)]
struct Model {
    state: AppState,
    login: LoginFormState,
    compose_text: String,
    splash_min_elapsed: bool,
    toast_serial: u64,
    previous_message_ids: Vec<String>,
    previous_last_message_content: Option<String>,
    previous_is_agent_typing: bool,
    has_settled_initial_scroll: bool,
    pending_history_restore: Option<HistoryLoadScrollStrategy>,
}

impl Model {
    fn new(state: AppState) -> Self {
        Self {
            previous_message_ids: message_ids(&state),
            previous_last_message_content: state
                .messages
                .last()
                .map(|message| message.content.clone()),
            previous_is_agent_typing: state.is_agent_typing,
            state,
            login: LoginFormState::default(),
            compose_text: String::new(),
            splash_min_elapsed: false,
            toast_serial: 0,
            has_settled_initial_scroll: false,
            pending_history_restore: None,
        }
    }

    fn can_send(&self) -> bool {
        !self.compose_text.trim().is_empty() && !self.state.is_agent_typing
    }

    fn login_is_loading(&self) -> bool {
        matches!(self.state.auth, AuthState::LoggingIn | AuthState::SigningUp)
    }

    fn splash_visible(&self) -> bool {
        !self.splash_min_elapsed || self.state.router.default_screen == Screen::Loading
    }
}

#[derive(Clone, Copy)]
enum HistoryLoadScrollStrategy {
    PreserveBottom,
    PreserveOffset {
        upper_before: f64,
        value_before: f64,
    },
}

#[derive(Clone, Copy)]
enum ChatPostRenderAction {
    None,
    ScrollToBottom {
        settle_initial: bool,
    },
    RestoreOffset {
        upper_before: f64,
        value_before: f64,
    },
}

#[derive(Clone, Copy)]
struct ChatViewportSnapshot {
    was_near_bottom: bool,
}

struct Widgets {
    window: gtk::ApplicationWindow,
    screen_stack: gtk::Stack,
    splash_revealer: gtk::Revealer,
    toast_revealer: gtk::Revealer,
    toast_button: gtk::Button,
    toast_label: gtk::Label,
    login_name_revealer: gtk::Revealer,
    login_name_entry: gtk::Entry,
    login_email_entry: gtk::Entry,
    login_password_entry: gtk::Entry,
    login_primary_button: gtk::Button,
    login_primary_label: gtk::Label,
    login_toggle_button: gtk::Button,
    oauth_buttons: Vec<gtk::Button>,
    chat_menu_button: gtk::Button,
    chat_messages_scroller: gtk::ScrolledWindow,
    chat_messages_box: gtk::Box,
    chat_top_spinner: gtk::Spinner,
    chat_center_spinner: gtk::Spinner,
    chat_compose_view: gtk::TextView,
    chat_compose_placeholder: gtk::Label,
    chat_send_button: gtk::Button,
    settings_dialog: gtk::Dialog,
    settings_delete_button: gtk::Button,
    settings_sign_out_button: gtk::Button,
    delete_dialog: gtk::MessageDialog,
}

struct DesktopApp {
    manager: AppManager,
    widgets: Widgets,
    model: RefCell<Model>,
}

impl DesktopApp {
    fn new(application: &gtk::Application) -> Result<Rc<Self>, String> {
        let theme = ThemeConfig::new(theme::detect_dark_mode());
        theme::install_css(theme);

        let manager = AppManager::new()?;
        let model = Model::new(manager.state());
        let widgets = build_widgets(application, theme);

        let app = Rc::new(Self {
            manager,
            widgets,
            model: RefCell::new(model),
        });

        app.connect_events();
        app.sync_screen();
        app.sync_login_form();
        app.sync_chat_inputs();
        app.rebuild_chat_messages(ChatPostRenderAction::None);
        app.sync_chat_loading_state();
        app.sync_toast();
        app.sync_splash();
        app.sync_dialogs();

        Ok(app)
    }

    fn start(self: &Rc<Self>) {
        self.start_update_bridge();
        self.start_timers();
        self.widgets.window.present();
    }

    fn connect_events(self: &Rc<Self>) {
        {
            let app = Rc::clone(self);
            self.widgets.login_name_entry.connect_changed(move |entry| {
                app.model.borrow_mut().login.name = entry.text().to_string();
            });
        }
        {
            let app = Rc::clone(self);
            self.widgets
                .login_email_entry
                .connect_changed(move |entry| {
                    app.model.borrow_mut().login.email = entry.text().to_string();
                    app.sync_login_form();
                });
        }
        {
            let app = Rc::clone(self);
            self.widgets
                .login_password_entry
                .connect_changed(move |entry| {
                    app.model.borrow_mut().login.password = entry.text().to_string();
                    app.sync_login_form();
                });
        }
        {
            let app = Rc::clone(self);
            self.widgets
                .login_name_entry
                .connect_activate(move |_| app.submit_auth());
        }
        {
            let app = Rc::clone(self);
            self.widgets.login_email_entry.connect_activate(move |_| {
                app.widgets.login_password_entry.grab_focus();
            });
        }
        {
            let app = Rc::clone(self);
            self.widgets
                .login_password_entry
                .connect_activate(move |_| app.submit_auth());
        }
        {
            let app = Rc::clone(self);
            self.widgets.login_primary_button.connect_clicked(move |_| {
                app.submit_auth();
            });
        }
        {
            let app = Rc::clone(self);
            self.widgets.login_toggle_button.connect_clicked(move |_| {
                {
                    let mut model = app.model.borrow_mut();
                    model.login.is_sign_up = !model.login.is_sign_up;
                }
                app.sync_login_form();
            });
        }

        let oauth_providers = [
            OAuthProvider::Github,
            OAuthProvider::Google,
            OAuthProvider::Apple,
        ];
        for (button, provider) in self
            .widgets
            .oauth_buttons
            .iter()
            .zip(oauth_providers.iter().cloned())
        {
            let app = Rc::clone(self);
            button.connect_clicked(move |_| {
                app.manager.dispatch(AppAction::InitiateOAuth {
                    provider: provider.clone(),
                    invite_code: None,
                });
            });
        }

        {
            let app = Rc::clone(self);
            self.widgets.chat_menu_button.connect_clicked(move |_| {
                app.manager.dispatch(AppAction::ToggleSettings);
            });
        }

        {
            let app = Rc::clone(self);
            self.widgets.chat_send_button.connect_clicked(move |_| {
                app.send_message();
            });
        }

        {
            let app = Rc::clone(self);
            let buffer = self.widgets.chat_compose_view.buffer();
            buffer.connect_changed(move |buffer| {
                let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
                app.model.borrow_mut().compose_text = text.to_string();
                app.sync_chat_inputs();
            });
        }

        {
            let app = Rc::clone(self);
            self.widgets
                .chat_messages_scroller
                .vadjustment()
                .connect_value_changed(move |adjustment| {
                    if adjustment.value() <= HISTORY_PREFETCH_THRESHOLD {
                        app.request_older_messages();
                    }
                });
        }

        {
            let app = Rc::clone(self);
            self.widgets.toast_button.connect_clicked(move |_| {
                app.manager.dispatch(AppAction::ClearToast);
            });
        }

        {
            let app = Rc::clone(self);
            self.widgets
                .settings_delete_button
                .connect_clicked(move |_| {
                    app.manager.dispatch(AppAction::RequestDeleteAgent);
                });
        }

        {
            let app = Rc::clone(self);
            self.widgets
                .settings_sign_out_button
                .connect_clicked(move |_| {
                    app.manager.dispatch(AppAction::ToggleSettings);
                    app.manager.dispatch(AppAction::Logout);
                });
        }

        {
            let app = Rc::clone(self);
            self.widgets
                .settings_dialog
                .connect_close_request(move |_| {
                    if app.model.borrow().state.show_settings {
                        app.manager.dispatch(AppAction::ToggleSettings);
                    }
                    glib::Propagation::Proceed
                });
        }

        {
            let app = Rc::clone(self);
            self.widgets
                .delete_dialog
                .connect_response(move |dialog, response| {
                    dialog.hide();
                    match response {
                        gtk::ResponseType::Accept => {
                            app.manager.dispatch(AppAction::ConfirmDeleteAgent)
                        }
                        _ => app.manager.dispatch(AppAction::CancelDeleteAgent),
                    }
                });
        }

        {
            let app = Rc::clone(self);
            self.widgets.delete_dialog.connect_close_request(move |_| {
                if app.model.borrow().state.confirm_delete_agent {
                    app.manager.dispatch(AppAction::CancelDeleteAgent);
                }
                glib::Propagation::Proceed
            });
        }
    }

    fn start_update_bridge(self: &Rc<Self>) {
        let updates = self.manager.subscribe_updates();
        let app = Rc::clone(self);
        glib::timeout_add_local(Duration::from_millis(16), move || {
            while let Ok(update) = updates.try_recv() {
                app.apply_update(update);
            }
            ControlFlow::Continue
        });
    }

    fn start_timers(self: &Rc<Self>) {
        let app = Rc::clone(self);
        glib::timeout_add_local_once(Duration::from_secs(1), move || {
            app.model.borrow_mut().splash_min_elapsed = true;
            app.sync_splash();
        });

        let app = Rc::clone(self);
        glib::timeout_add_local(Duration::from_secs(30), move || {
            app.manager.dispatch(AppAction::RefreshTimestamps);
            ControlFlow::Continue
        });
    }

    fn apply_update(self: &Rc<Self>, update: AppUpdate) {
        AppManager::persist_side_effect(&update);

        let AppUpdate::FullState(new_state) = update else {
            return;
        };

        let old_state = self.model.borrow().state.clone();
        if new_state.rev <= old_state.rev {
            return;
        }

        let viewport = if old_state.router.default_screen == Screen::Chat {
            Some(self.capture_chat_viewport())
        } else {
            None
        };

        let old_toast = old_state.toast.clone();
        let pending_auth_url = new_state.pending_auth_url.clone();
        let new_ids = message_ids(&new_state);

        let post_render = {
            let mut model = self.model.borrow_mut();
            let old_screen = model.state.router.default_screen.clone();
            let new_screen = new_state.router.default_screen.clone();

            if old_screen != new_screen {
                if new_screen == Screen::Login {
                    model.login = LoginFormState::default();
                }
                if new_screen != Screen::Chat {
                    model.compose_text.clear();
                    model.has_settled_initial_scroll = false;
                    model.pending_history_restore = None;
                    model.previous_message_ids.clear();
                    model.previous_last_message_content = None;
                    model.previous_is_agent_typing = false;
                }
                if new_screen == Screen::Chat && old_screen != Screen::Chat {
                    model.compose_text.clear();
                    model.has_settled_initial_scroll = false;
                    model.pending_history_restore = None;
                }
            }

            let prepended = did_prepend_messages(&model.previous_message_ids, &new_ids);
            let appended = did_append_messages(&model.previous_message_ids, &new_ids);
            let content_changed = new_state
                .messages
                .last()
                .map(|message| message.content.clone())
                != model.previous_last_message_content;
            let typing_changed = new_state.is_agent_typing != model.previous_is_agent_typing;

            let post_render = if new_screen == Screen::Chat {
                if new_ids.is_empty() {
                    model.has_settled_initial_scroll = false;
                    model.pending_history_restore = None;
                    ChatPostRenderAction::None
                } else if !model.has_settled_initial_scroll {
                    ChatPostRenderAction::ScrollToBottom {
                        settle_initial: true,
                    }
                } else if prepended {
                    match model.pending_history_restore.take() {
                        Some(HistoryLoadScrollStrategy::PreserveBottom) => {
                            ChatPostRenderAction::ScrollToBottom {
                                settle_initial: false,
                            }
                        }
                        Some(HistoryLoadScrollStrategy::PreserveOffset {
                            upper_before,
                            value_before,
                        }) => ChatPostRenderAction::RestoreOffset {
                            upper_before,
                            value_before,
                        },
                        None => ChatPostRenderAction::None,
                    }
                } else if appended
                    && viewport
                        .map(|snapshot| snapshot.was_near_bottom)
                        .unwrap_or(true)
                {
                    ChatPostRenderAction::ScrollToBottom {
                        settle_initial: false,
                    }
                } else if model.pending_history_restore.is_none()
                    && viewport
                        .map(|snapshot| snapshot.was_near_bottom)
                        .unwrap_or(true)
                    && (content_changed || typing_changed)
                {
                    ChatPostRenderAction::ScrollToBottom {
                        settle_initial: false,
                    }
                } else {
                    if !new_state.is_loading_history
                        && model.pending_history_restore.is_some()
                        && new_ids == model.previous_message_ids
                    {
                        model.pending_history_restore = None;
                    }
                    ChatPostRenderAction::None
                }
            } else {
                ChatPostRenderAction::None
            };

            if old_toast != new_state.toast {
                model.toast_serial = model.toast_serial.saturating_add(1);
            }

            model.state = new_state.clone();
            model.previous_message_ids = new_ids;
            model.previous_last_message_content = new_state
                .messages
                .last()
                .map(|message| message.content.clone());
            model.previous_is_agent_typing = new_state.is_agent_typing;
            post_render
        };

        self.sync_screen();
        self.sync_login_form();
        self.sync_chat_inputs();
        self.rebuild_chat_messages(post_render);
        self.sync_chat_loading_state();
        self.sync_toast();
        self.sync_splash();
        self.sync_dialogs();

        if old_toast != self.model.borrow().state.toast {
            self.schedule_toast_dismissal();
        }

        if let Some(url) = pending_auth_url {
            open_external_url(&url);
            self.manager.dispatch(AppAction::ClearPendingAuthUrl);
        }
    }

    fn submit_auth(&self) {
        let model = self.model.borrow();
        if model.login_is_loading()
            || model.login.email.is_empty()
            || model.login.password.is_empty()
        {
            return;
        }

        if model.login.is_sign_up {
            self.manager.dispatch(AppAction::SignUpWithEmail {
                email: model.login.email.clone(),
                password: model.login.password.clone(),
                name: model.login.name.clone(),
            });
        } else {
            self.manager.dispatch(AppAction::LoginWithEmail {
                email: model.login.email.clone(),
                password: model.login.password.clone(),
            });
        }
    }

    fn send_message(&self) {
        let trimmed = self.model.borrow().compose_text.trim().to_string();
        if trimmed.is_empty() || self.model.borrow().state.is_agent_typing {
            return;
        }

        self.manager
            .dispatch(AppAction::SendMessage { content: trimmed });
        self.model.borrow_mut().compose_text.clear();
        set_text_view_text(&self.widgets.chat_compose_view, "");
        self.sync_chat_inputs();
    }

    fn schedule_toast_dismissal(self: &Rc<Self>) {
        let (serial, toast) = {
            let model = self.model.borrow();
            let Some(toast) = model.state.toast.clone() else {
                return;
            };
            (model.toast_serial, toast)
        };

        let app = Rc::clone(self);
        glib::timeout_add_local_once(Duration::from_secs(4), move || {
            let model = app.model.borrow();
            if model.toast_serial == serial && model.state.toast.as_deref() == Some(toast.as_str())
            {
                drop(model);
                app.manager.dispatch(AppAction::ClearToast);
            }
        });
    }

    fn capture_chat_viewport(&self) -> ChatViewportSnapshot {
        let adjustment = self.widgets.chat_messages_scroller.vadjustment();
        ChatViewportSnapshot {
            was_near_bottom: is_near_bottom(&adjustment),
        }
    }

    fn request_older_messages(&self) {
        let adjustment = self.widgets.chat_messages_scroller.vadjustment();
        let mut model = self.model.borrow_mut();

        if !model.has_settled_initial_scroll
            || model.pending_history_restore.is_some()
            || model.state.is_loading_history
            || !model.state.has_older_messages
        {
            return;
        }

        model.pending_history_restore = Some(
            if viewport_is_underfilled(&adjustment) && is_near_bottom(&adjustment) {
                HistoryLoadScrollStrategy::PreserveBottom
            } else {
                HistoryLoadScrollStrategy::PreserveOffset {
                    upper_before: adjustment.upper(),
                    value_before: adjustment.value(),
                }
            },
        );

        drop(model);
        self.manager.dispatch(AppAction::LoadOlderMessages);
    }

    fn sync_screen(&self) {
        let model = self.model.borrow();
        let screen_name = match model.state.router.default_screen {
            Screen::Loading => "loading",
            Screen::Login => "login",
            Screen::Chat => "chat",
        };
        self.widgets
            .screen_stack
            .set_visible_child_name(screen_name);

        match model.state.router.default_screen {
            Screen::Login => {
                if model.login.is_sign_up {
                    self.widgets.login_name_entry.grab_focus();
                } else {
                    self.widgets.login_email_entry.grab_focus();
                }
            }
            Screen::Chat => {
                self.widgets.chat_compose_view.grab_focus();
            }
            Screen::Loading => {}
        }
    }

    fn sync_login_form(&self) {
        let model = self.model.borrow();
        let is_loading = model.login_is_loading();
        let can_submit =
            !model.login.email.is_empty() && !model.login.password.is_empty() && !is_loading;

        set_entry_text_if_needed(&self.widgets.login_name_entry, &model.login.name);
        set_entry_text_if_needed(&self.widgets.login_email_entry, &model.login.email);
        set_entry_text_if_needed(&self.widgets.login_password_entry, &model.login.password);

        self.widgets
            .login_name_revealer
            .set_reveal_child(model.login.is_sign_up);
        self.widgets
            .login_primary_label
            .set_text(if model.login.is_sign_up {
                "Sign Up"
            } else {
                "Sign In"
            });
        self.widgets.login_primary_button.set_sensitive(can_submit);
        self.widgets
            .login_toggle_button
            .set_label(if model.login.is_sign_up {
                "Already have an account? Sign In"
            } else {
                "Don't have an account? Sign Up"
            });

        for button in &self.widgets.oauth_buttons {
            button.set_sensitive(!is_loading);
        }
    }

    fn sync_chat_inputs(&self) {
        let model = self.model.borrow();
        self.widgets
            .chat_compose_placeholder
            .set_visible(model.compose_text.is_empty());
        self.widgets
            .chat_send_button
            .set_sensitive(model.can_send());
    }

    fn sync_chat_loading_state(&self) {
        let model = self.model.borrow();
        let show_history_spinner =
            model.state.is_loading_history && !model.state.messages.is_empty();
        let show_initial_spinner = (model.state.is_loading_history
            && model.state.messages.is_empty())
            || (!model.has_settled_initial_scroll && !model.state.messages.is_empty());

        self.widgets
            .chat_top_spinner
            .set_visible(show_history_spinner);
        if show_history_spinner {
            self.widgets.chat_top_spinner.start();
        } else {
            self.widgets.chat_top_spinner.stop();
        }

        self.widgets
            .chat_center_spinner
            .set_visible(show_initial_spinner);
        if show_initial_spinner {
            self.widgets.chat_center_spinner.start();
        } else {
            self.widgets.chat_center_spinner.stop();
        }

        self.widgets.chat_messages_box.set_opacity(
            if model.has_settled_initial_scroll || model.state.messages.is_empty() {
                1.0
            } else {
                0.0
            },
        );
    }

    fn sync_toast(&self) {
        let model = self.model.borrow();
        let is_chat = model.state.router.default_screen == Screen::Chat;
        let visible = model.state.toast.is_some();
        self.widgets
            .toast_button
            .set_margin_bottom(if is_chat { 112 } else { 24 });
        self.widgets.toast_button.set_visible(visible);
        self.widgets.toast_revealer.set_reveal_child(visible);
        self.widgets.toast_revealer.set_can_target(visible);
        self.widgets
            .toast_label
            .set_text(model.state.toast.as_deref().unwrap_or_default());
    }

    fn sync_splash(&self) {
        let visible = self.model.borrow().splash_visible();
        self.widgets.splash_revealer.set_reveal_child(visible);
        self.widgets.splash_revealer.set_can_target(visible);
    }

    fn sync_dialogs(&self) {
        let model = self.model.borrow();

        self.widgets
            .settings_delete_button
            .set_sensitive(!model.state.is_deleting_agent);

        if model.state.show_settings {
            self.widgets.settings_dialog.present();
        } else {
            self.widgets.settings_dialog.hide();
        }

        if model.state.confirm_delete_agent {
            self.widgets.delete_dialog.present();
        } else {
            self.widgets.delete_dialog.hide();
        }
    }

    fn rebuild_chat_messages(self: &Rc<Self>, post_render: ChatPostRenderAction) {
        clear_box(&self.widgets.chat_messages_box);

        let model = self.model.borrow();
        for message in &model.state.messages {
            self.widgets
                .chat_messages_box
                .append(&build_message_row(message));
        }

        if model.state.is_agent_typing {
            let typing = gtk::Label::new(Some("Maple is typing..."));
            typing.add_css_class("maple-meta");
            typing.set_halign(gtk::Align::Start);
            typing.set_margin_start(SPACE_XS);
            self.widgets.chat_messages_box.append(&typing);
        }

        drop(model);

        let app = Rc::clone(self);
        glib::idle_add_local(move || {
            match post_render {
                ChatPostRenderAction::None => {}
                ChatPostRenderAction::ScrollToBottom { settle_initial } => {
                    app.scroll_to_bottom();
                    if settle_initial {
                        app.model.borrow_mut().has_settled_initial_scroll = true;
                    }
                }
                ChatPostRenderAction::RestoreOffset {
                    upper_before,
                    value_before,
                } => {
                    let adjustment = app.widgets.chat_messages_scroller.vadjustment();
                    let delta = adjustment.upper() - upper_before;
                    let target = value_before + delta;
                    adjustment.set_value(target.max(0.0));
                }
            }

            if matches!(post_render, ChatPostRenderAction::None)
                && !app.model.borrow().state.messages.is_empty()
                && !app.model.borrow().has_settled_initial_scroll
            {
                app.scroll_to_bottom();
                app.model.borrow_mut().has_settled_initial_scroll = true;
            }

            app.sync_chat_loading_state();
            app.maybe_request_older_messages_for_underfilled_viewport();
            ControlFlow::Break
        });
    }

    fn scroll_to_bottom(&self) {
        let adjustment = self.widgets.chat_messages_scroller.vadjustment();
        let bottom = (adjustment.upper() - adjustment.page_size()).max(0.0);
        adjustment.set_value(bottom);
    }

    fn maybe_request_older_messages_for_underfilled_viewport(&self) {
        let adjustment = self.widgets.chat_messages_scroller.vadjustment();
        let model = self.model.borrow();
        let should_request = model.has_settled_initial_scroll
            && model.state.has_older_messages
            && !model.state.is_loading_history
            && model.pending_history_restore.is_none()
            && viewport_is_underfilled(&adjustment)
            && is_near_bottom(&adjustment);
        drop(model);

        if should_request {
            self.request_older_messages();
        }
    }
}

fn build_widgets(application: &gtk::Application, theme: ThemeConfig) -> Widgets {
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("Maple")
        .default_width(DEFAULT_WINDOW_WIDTH)
        .default_height(DEFAULT_WINDOW_HEIGHT)
        .build();
    window.add_css_class("maple-window");
    window.set_size_request(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT);

    let root_overlay = gtk::Overlay::new();
    root_overlay.set_hexpand(true);
    root_overlay.set_vexpand(true);

    let screen_stack = gtk::Stack::new();
    screen_stack.set_hexpand(true);
    screen_stack.set_vexpand(true);

    let loading_screen = build_splash_screen(theme);
    let (
        login_screen,
        login_name_revealer,
        login_name_entry,
        login_email_entry,
        login_password_entry,
        login_primary_button,
        login_primary_label,
        login_toggle_button,
        oauth_buttons,
    ) = build_login_screen(theme);
    let (
        chat_screen,
        chat_menu_button,
        chat_messages_scroller,
        chat_messages_box,
        chat_top_spinner,
        chat_center_spinner,
        chat_compose_view,
        chat_compose_placeholder,
        chat_send_button,
    ) = build_chat_screen(theme);

    screen_stack.add_named(&loading_screen, Some("loading"));
    screen_stack.add_named(&login_screen, Some("login"));
    screen_stack.add_named(&chat_screen, Some("chat"));
    root_overlay.set_child(Some(&screen_stack));

    let toast_revealer = gtk::Revealer::new();
    toast_revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
    toast_revealer.set_halign(gtk::Align::Center);
    toast_revealer.set_valign(gtk::Align::End);
    toast_revealer.set_margin_start(SPACE_MD);
    toast_revealer.set_margin_end(SPACE_MD);

    let toast_button = gtk::Button::new();
    toast_button.add_css_class("flat");
    toast_button.add_css_class("maple-toast");
    let toast_label = gtk::Label::new(None);
    toast_label.set_wrap(true);
    toast_label.set_justify(gtk::Justification::Center);
    toast_label.set_margin_start(16);
    toast_label.set_margin_end(16);
    toast_label.set_margin_top(12);
    toast_label.set_margin_bottom(12);
    toast_button.set_child(Some(&toast_label));
    toast_revealer.set_child(Some(&toast_button));
    root_overlay.add_overlay(&toast_revealer);
    toast_revealer.set_can_target(false);

    let splash_revealer = gtk::Revealer::new();
    splash_revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    splash_revealer.set_hexpand(true);
    splash_revealer.set_vexpand(true);
    splash_revealer.set_child(Some(&build_splash_screen(theme)));
    root_overlay.add_overlay(&splash_revealer);
    splash_revealer.set_can_target(true);

    window.set_child(Some(&root_overlay));

    let settings_dialog = build_settings_dialog(&window);
    let settings_content = settings_dialog.content_area();
    let settings_root = gtk::Box::new(gtk::Orientation::Vertical, SPACE_SM);
    settings_root.add_css_class("maple-settings-card");
    settings_root.set_margin_start(SPACE_MD);
    settings_root.set_margin_end(SPACE_MD);
    settings_root.set_margin_top(SPACE_MD);
    settings_root.set_margin_bottom(SPACE_MD);

    let delete_row = action_button("user-trash-symbolic", "Delete Agent", "maple-danger-action");
    let delete_help = gtk::Label::new(Some(
        "Permanently deletes your agent and conversation history.",
    ));
    delete_help.add_css_class("maple-meta");
    delete_help.set_wrap(true);
    delete_help.set_xalign(0.0);

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("maple-settings-divider");

    let sign_out_row = action_button(
        "system-log-out-symbolic",
        "Sign Out",
        "maple-settings-action",
    );

    settings_root.append(&delete_row.0);
    settings_root.append(&delete_help);
    settings_root.append(&divider);
    settings_root.append(&sign_out_row.0);
    settings_content.append(&settings_root);
    settings_dialog.set_default_size(360, 220);
    settings_dialog.set_hide_on_close(true);

    let delete_dialog = gtk::MessageDialog::new(
        Some(&window),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Question,
        gtk::ButtonsType::None,
        "Delete Agent?",
    );
    delete_dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    delete_dialog.add_button("Delete", gtk::ResponseType::Accept);
    delete_dialog.set_secondary_text(Some(
        "This will permanently delete your agent conversation history. This cannot be undone.",
    ));
    delete_dialog.set_hide_on_close(true);

    Widgets {
        window,
        screen_stack,
        splash_revealer,
        toast_revealer,
        toast_button,
        toast_label,
        login_name_revealer,
        login_name_entry,
        login_email_entry,
        login_password_entry,
        login_primary_button,
        login_primary_label,
        login_toggle_button,
        oauth_buttons,
        chat_menu_button,
        chat_messages_scroller,
        chat_messages_box,
        chat_top_spinner,
        chat_center_spinner,
        chat_compose_view,
        chat_compose_placeholder,
        chat_send_button,
        settings_dialog,
        settings_delete_button: delete_row.0,
        settings_sign_out_button: sign_out_row.0,
        delete_dialog,
    }
}

fn build_splash_screen(theme: ThemeConfig) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&background_area(
        BackgroundKind::Splash,
        theme.dark_mode,
    )));

    let content = gtk::Box::new(gtk::Orientation::Vertical, SPACE_MD);
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);

    let wordmark = full_wordmark(theme.splash_wordmark(), 40);
    let tagline = gtk::Label::new(Some("Privacy-first intelligence"));
    tagline.add_css_class("maple-splash-tagline");

    content.append(&wordmark);
    content.append(&tagline);
    overlay.add_overlay(&content);
    overlay
}

#[allow(clippy::type_complexity)]
fn build_login_screen(
    theme: ThemeConfig,
) -> (
    gtk::Overlay,
    gtk::Revealer,
    gtk::Entry,
    gtk::Entry,
    gtk::Entry,
    gtk::Button,
    gtk::Label,
    gtk::Button,
    Vec<gtk::Button>,
) {
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&background_area(
        BackgroundKind::Login,
        theme.dark_mode,
    )));

    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_hexpand(true);
    center.set_vexpand(true);
    center.set_halign(gtk::Align::Center);
    center.set_valign(gtk::Align::Center);
    center.set_margin_start(SPACE_MD);
    center.set_margin_end(SPACE_MD);

    let card_shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card_shell.add_css_class("maple-login-card-shell");
    card_shell.set_width_request(LOGIN_CARD_WIDTH);

    let card = gtk::Box::new(gtk::Orientation::Vertical, 24);
    card.set_margin_start(24);
    card.set_margin_end(24);
    card.set_margin_top(24);
    card.set_margin_bottom(24);

    let wordmark = full_wordmark(theme.login_palette().wordmark, 28);
    wordmark.set_halign(gtk::Align::Center);

    let fields = gtk::Box::new(gtk::Orientation::Vertical, SPACE_SM);

    let (name_shell, name_entry) = entry_shell("Name", true);
    let name_revealer = gtk::Revealer::new();
    name_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    name_revealer.set_child(Some(&name_shell));

    let (email_shell, email_entry) = entry_shell("Email", true);
    let (password_shell, password_entry) = entry_shell("Password", false);

    fields.append(&name_revealer);
    fields.append(&email_shell);
    fields.append(&password_shell);

    let (primary_button, primary_label) = primary_button("Sign In");

    let divider_row = gtk::Box::new(gtk::Orientation::Horizontal, SPACE_XS);
    divider_row.set_valign(gtk::Align::Center);
    let divider_left = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider_left.add_css_class("maple-divider");
    divider_left.set_hexpand(true);
    let divider_label = gtk::Label::new(Some("or"));
    divider_label.add_css_class("maple-divider");
    let divider_right = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider_right.add_css_class("maple-divider");
    divider_right.set_hexpand(true);
    divider_row.append(&divider_left);
    divider_row.append(&divider_label);
    divider_row.append(&divider_right);

    let oauth_box = gtk::Box::new(gtk::Orientation::Vertical, SPACE_XS);
    let github = secondary_button("Continue with GitHub");
    let google = secondary_button("Continue with Google");
    let apple = secondary_button("Continue with Apple");
    oauth_box.append(&github.0);
    oauth_box.append(&google.0);
    oauth_box.append(&apple.0);

    let toggle_button = gtk::Button::with_label("Don't have an account? Sign Up");
    toggle_button.add_css_class("flat");
    toggle_button.add_css_class("maple-link-button");

    card.append(&wordmark);
    card.append(&fields);
    card.append(&primary_button);
    card.append(&divider_row);
    card.append(&oauth_box);
    card.append(&toggle_button);

    card_shell.append(&card);
    center.append(&card_shell);
    overlay.add_overlay(&center);

    (
        overlay,
        name_revealer,
        name_entry,
        email_entry,
        password_entry,
        primary_button,
        primary_label,
        toggle_button,
        vec![github.0, google.0, apple.0],
    )
}

#[allow(clippy::type_complexity)]
fn build_chat_screen(
    theme: ThemeConfig,
) -> (
    gtk::Overlay,
    gtk::Button,
    gtk::ScrolledWindow,
    gtk::Box,
    gtk::Spinner,
    gtk::Spinner,
    gtk::TextView,
    gtk::Label,
    gtk::Button,
) {
    let overlay = gtk::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_child(Some(&background_area(
        BackgroundKind::Chat,
        theme.dark_mode,
    )));

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

    let messages_box = gtk::Box::new(gtk::Orientation::Vertical, SPACE_MD);
    messages_box.set_margin_start(SPACE_MD);
    messages_box.set_margin_end(SPACE_MD);
    messages_box.set_margin_top(108);
    messages_box.set_margin_bottom(128);
    scroller.set_child(Some(&messages_box));
    overlay.add_overlay(&scroller);

    let top_spinner = gtk::Spinner::new();
    top_spinner.set_halign(gtk::Align::Center);
    top_spinner.set_valign(gtk::Align::Start);
    top_spinner.set_margin_top(108);
    overlay.add_overlay(&top_spinner);

    let center_spinner = gtk::Spinner::new();
    center_spinner.set_halign(gtk::Align::Center);
    center_spinner.set_valign(gtk::Align::Center);
    overlay.add_overlay(&center_spinner);

    let header_overlay = gtk::Overlay::new();
    header_overlay.set_hexpand(true);
    header_overlay.set_halign(gtk::Align::Fill);
    header_overlay.set_valign(gtk::Align::Start);
    header_overlay.set_margin_start(SPACE_MD);
    header_overlay.set_margin_end(SPACE_MD);
    header_overlay.set_margin_top(SPACE_SM);

    let header_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header_spacer.set_height_request(43);
    header_spacer.set_hexpand(true);
    header_overlay.set_child(Some(&header_spacer));

    let menu_button = icon_pill_button("open-menu-symbolic", "Menu");
    menu_button.set_halign(gtk::Align::Start);
    menu_button.set_valign(gtk::Align::Start);
    header_overlay.add_overlay(&menu_button);

    let search_button = icon_pill_button("edit-find-symbolic", "Search");
    search_button.set_sensitive(false);
    search_button.set_halign(gtk::Align::End);
    search_button.set_valign(gtk::Align::Start);
    header_overlay.add_overlay(&search_button);

    let wordmark_pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    wordmark_pill.add_css_class("maple-header-pill");
    wordmark_pill.set_halign(gtk::Align::Center);
    wordmark_pill.set_valign(gtk::Align::Start);
    let wordmark_inner = gtk::Box::new(gtk::Orientation::Horizontal, SPACE_XS);
    wordmark_inner.set_margin_start(16);
    wordmark_inner.set_margin_end(12);
    wordmark_inner.set_margin_top(12);
    wordmark_inner.set_margin_bottom(12);
    wordmark_inner.append(&abbr_wordmark(theme.chat_palette().header_wordmark, 16));
    let chevron = gtk::Label::new(Some("⌄"));
    chevron.set_markup(&format!(
        "<span foreground=\"{}\" weight=\"heavy\">⌄</span>",
        PEBBLE_400.pango_rgb()
    ));
    wordmark_inner.append(&chevron);
    wordmark_pill.append(&wordmark_inner);
    header_overlay.add_overlay(&wordmark_pill);

    overlay.add_overlay(&header_overlay);

    let compose_shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    compose_shell.add_css_class("maple-compose-shell");
    compose_shell.set_halign(gtk::Align::Fill);
    compose_shell.set_valign(gtk::Align::End);
    compose_shell.set_margin_start(SPACE_MD);
    compose_shell.set_margin_end(SPACE_MD);
    compose_shell.set_margin_bottom(SPACE_XS);

    let compose_row = gtk::Box::new(gtk::Orientation::Horizontal, SPACE_XS);
    compose_row.set_valign(gtk::Align::Center);
    compose_row.set_margin_start(16);
    compose_row.set_margin_end(16);
    compose_row.set_margin_top(12);
    compose_row.set_margin_bottom(14);

    let compose_column = gtk::Box::new(gtk::Orientation::Vertical, 4);
    compose_column.set_hexpand(true);

    let compose_overlay = gtk::Overlay::new();
    compose_overlay.set_hexpand(true);
    let compose_view = gtk::TextView::new();
    compose_view.add_css_class("maple-compose-view");
    compose_view.set_wrap_mode(gtk::WrapMode::WordChar);
    compose_view.set_accepts_tab(false);
    compose_view.set_left_margin(0);
    compose_view.set_right_margin(0);
    compose_view.set_top_margin(0);
    compose_view.set_bottom_margin(0);
    compose_view.set_size_request(-1, 54);
    compose_overlay.set_child(Some(&compose_view));

    let compose_placeholder = gtk::Label::new(Some("Write..."));
    compose_placeholder.add_css_class("maple-compose-placeholder");
    compose_placeholder.set_halign(gtk::Align::Start);
    compose_placeholder.set_valign(gtk::Align::Start);
    compose_overlay.add_overlay(&compose_placeholder);

    let plus_button = gtk::Button::new();
    plus_button.add_css_class("flat");
    plus_button.add_css_class("maple-plus-button");
    plus_button.set_halign(gtk::Align::Start);
    plus_button.set_valign(gtk::Align::Center);
    plus_button.set_size_request(28, 28);
    plus_button.set_child(Some(&gtk::Label::new(Some("+"))));

    compose_column.append(&compose_overlay);
    compose_column.append(&plus_button);

    let (send_button, _) = send_button();
    compose_row.append(&compose_column);
    compose_row.append(&send_button);
    compose_shell.append(&compose_row);
    overlay.add_overlay(&compose_shell);

    (
        overlay,
        menu_button,
        scroller,
        messages_box,
        top_spinner,
        center_spinner,
        compose_view,
        compose_placeholder,
        send_button,
    )
}

fn build_settings_dialog(window: &gtk::ApplicationWindow) -> gtk::Dialog {
    let dialog = gtk::Dialog::new();
    dialog.set_title(Some("Settings"));
    dialog.set_transient_for(Some(window));
    dialog.set_modal(true);
    dialog
}

fn show_boot_error(application: &gtk::Application, error: &str) {
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("Maple")
        .default_width(420)
        .default_height(220)
        .build();
    window.add_css_class("maple-window");

    let content = gtk::Box::new(gtk::Orientation::Vertical, SPACE_MD);
    content.set_margin_start(SPACE_MD);
    content.set_margin_end(SPACE_MD);
    content.set_margin_top(SPACE_LG);
    content.set_margin_bottom(SPACE_LG);

    let title = gtk::Label::new(Some("Maple failed to start"));
    title.add_css_class("title-3");
    title.set_xalign(0.0);

    let body = gtk::Label::new(Some(error));
    body.set_wrap(true);
    body.set_xalign(0.0);

    let quit = gtk::Button::with_label("Close");
    let app = application.clone();
    quit.connect_clicked(move |_| app.quit());

    content.append(&title);
    content.append(&body);
    content.append(&quit);
    window.set_child(Some(&content));
    window.present();
}

fn primary_button(label_text: &str) -> (gtk::Button, gtk::Label) {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("maple-primary-button");

    let label = gtk::Label::new(Some(label_text));
    label.set_margin_start(24);
    label.set_margin_end(24);
    label.set_margin_top(12);
    label.set_margin_bottom(12);
    button.set_child(Some(&label));

    (button, label)
}

fn secondary_button(label_text: &str) -> (gtk::Button, gtk::Label) {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("maple-secondary-button");
    button.set_halign(gtk::Align::Fill);

    let label = gtk::Label::new(Some(label_text));
    label.set_margin_start(24);
    label.set_margin_end(24);
    label.set_margin_top(12);
    label.set_margin_bottom(12);
    button.set_child(Some(&label));

    (button, label)
}

fn send_button() -> (gtk::Button, gtk::Label) {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("maple-send-button");
    button.set_valign(gtk::Align::Center);
    button.set_size_request(71, 36);

    let label = gtk::Label::new(Some("↑"));
    label.set_margin_start(24);
    label.set_margin_end(24);
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    button.set_child(Some(&label));

    (button, label)
}

fn icon_pill_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("maple-header-pill-button");
    button.set_tooltip_text(Some(tooltip));
    button.set_size_request(43, 43);

    let image = gtk::Image::from_icon_name(icon_name);
    image.set_pixel_size(18);
    button.set_child(Some(&image));
    button
}

fn action_button(icon_name: &str, label_text: &str, css_class: &str) -> (gtk::Button, gtk::Label) {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class(css_class);
    button.set_halign(gtk::Align::Fill);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, SPACE_SM);
    row.set_margin_start(12);
    row.set_margin_end(12);
    row.set_margin_top(10);
    row.set_margin_bottom(10);

    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    let label = gtk::Label::new(Some(label_text));
    label.set_xalign(0.0);

    row.append(&icon);
    row.append(&label);
    button.set_child(Some(&row));

    (button, label)
}

fn entry_shell(placeholder: &str, visible: bool) -> (gtk::Box, gtk::Entry) {
    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("maple-entry-shell");

    let entry = gtk::Entry::new();
    entry.add_css_class("maple-entry");
    entry.set_has_frame(false);
    entry.set_visibility(visible);
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some(placeholder));
    entry.set_margin_start(16);
    entry.set_margin_end(16);
    entry.set_margin_top(14);
    entry.set_margin_bottom(14);

    shell.append(&entry);
    (shell, entry)
}

fn build_message_row(message: &ChatMessage) -> gtk::Box {
    let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 4);
    wrapper.set_hexpand(true);
    wrapper.set_halign(gtk::Align::Fill);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.set_hexpand(true);
    row.set_halign(if message.is_user {
        gtk::Align::End
    } else {
        gtk::Align::Start
    });

    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.set_halign(if message.is_user {
        gtk::Align::End
    } else {
        gtk::Align::Start
    });

    if message.is_user {
        let bubble = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bubble.add_css_class("maple-user-bubble");

        let label = gtk::Label::new(Some(&message.content));
        label.add_css_class("maple-user-message");
        label.set_wrap(true);
        label.set_wrap_mode(pango::WrapMode::WordChar);
        label.set_max_width_chars(MESSAGE_MAX_WIDTH_CHARS);
        label.set_xalign(0.0);
        label.set_margin_start(12);
        label.set_margin_end(12);
        label.set_margin_top(8);
        label.set_margin_bottom(8);

        bubble.append(&label);
        content.append(&bubble);
    } else {
        let label = gtk::Label::new(Some(&message.content));
        label.add_css_class("maple-assistant-message");
        label.set_wrap(true);
        label.set_wrap_mode(pango::WrapMode::WordChar);
        label.set_max_width_chars(MESSAGE_MAX_WIDTH_CHARS);
        label.set_xalign(0.0);
        content.append(&label);
    }

    if message.show_timestamp {
        let timestamp = gtk::Label::new(Some(&message.timestamp_display));
        timestamp.add_css_class("maple-meta");
        timestamp.set_xalign(0.0);
        timestamp.set_margin_start(8);
        timestamp.set_margin_end(8);
        content.append(&timestamp);
    }

    row.append(&content);
    wrapper.append(&row);
    wrapper
}

fn set_text_view_text(view: &gtk::TextView, text: &str) {
    let buffer = view.buffer();
    let current = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    if current.as_str() != text {
        buffer.set_text(text);
    }
}

fn set_entry_text_if_needed(entry: &gtk::Entry, value: &str) {
    if entry.text().as_str() != value {
        entry.set_text(value);
    }
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn message_ids(state: &AppState) -> Vec<String> {
    state
        .messages
        .iter()
        .map(|message| message.id.clone())
        .collect()
}

fn did_prepend_messages(previous_ids: &[String], current_ids: &[String]) -> bool {
    !previous_ids.is_empty()
        && current_ids.len() > previous_ids.len()
        && current_ids[current_ids.len() - previous_ids.len()..] == *previous_ids
}

fn did_append_messages(previous_ids: &[String], current_ids: &[String]) -> bool {
    !previous_ids.is_empty()
        && current_ids.len() > previous_ids.len()
        && current_ids[..previous_ids.len()] == *previous_ids
}

fn distance_from_bottom(upper: f64, value: f64, page_size: f64) -> f64 {
    (upper - (value + page_size)).max(0.0)
}

fn is_near_bottom(adjustment: &gtk::Adjustment) -> bool {
    distance_from_bottom(
        adjustment.upper(),
        adjustment.value(),
        adjustment.page_size(),
    ) <= BOTTOM_FOLLOW_THRESHOLD
}

fn viewport_is_underfilled(adjustment: &gtk::Adjustment) -> bool {
    adjustment.upper() <= adjustment.page_size() + 1.0
}

fn open_external_url(url: &str) {
    if gtk::gio::AppInfo::launch_default_for_uri(url, None::<&gtk::gio::AppLaunchContext>).is_ok() {
        return;
    }

    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::{did_append_messages, did_prepend_messages, distance_from_bottom};

    #[test]
    fn prepended_messages_detect_suffix_match() {
        let previous = vec!["b".to_string(), "c".to_string()];
        let current = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert!(did_prepend_messages(&previous, &current));
    }

    #[test]
    fn appended_messages_detect_prefix_match() {
        let previous = vec!["a".to_string(), "b".to_string()];
        let current = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert!(did_append_messages(&previous, &current));
    }

    #[test]
    fn bottom_distance_clamps_at_zero() {
        assert_eq!(distance_from_bottom(100.0, 80.0, 40.0), 0.0);
        assert_eq!(distance_from_bottom(300.0, 120.0, 100.0), 80.0);
    }
}
