//! Settings screen: left navigation with content panes, following Maple's
//! settings layout. Sections: General (defaults), System prompt, MCP
//! servers, Keyboard Shortcuts, Usage, About.

use std::sync::Arc;

use gpui::{
    App, Context, Div, Entity, EventEmitter, Render, ScrollAnchor, ScrollHandle, Subscription,
    Window, div, prelude::*, px,
};

use maple_agent::agent::{AgentMcpKeyValue, AgentMcpServer, AgentMcpTransport};

use crate::ui::icons::icon;
use crate::ui::text_input::TextInput;

use crate::backend::AgentBackend;
use crate::settings::{self, AppSettings, UsageSummary};
use crate::shortcuts::{
    ShortcutConflict, ShortcutConflictKind, ShortcutContextOverlap, ShortcutOverrides,
    ShortcutSnapshot,
};
use crate::ui::theme;
use crate::ui::widgets;

mod navigation;
use self::navigation::{GeneralTarget, SettingsApplicationVimState, SettingsTarget};

/// Emitted when the user leaves settings.
pub struct SettingsClosed(pub AppSettings);

/// Emitted when the user clicks Sign out in the settings header.
pub struct SignOutRequested;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Shortcuts,
    Prompt,
    Mcp,
    Usage,
    About,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Shortcuts => "Keyboard Shortcuts",
            Self::Prompt => "System prompt",
            Self::Mcp => "MCP servers",
            Self::Usage => "Usage",
            Self::About => "About",
        }
    }

    const ALL: [Self; 6] = [
        Self::General,
        Self::Shortcuts,
        Self::Prompt,
        Self::Mcp,
        Self::Usage,
        Self::About,
    ];
}

pub struct SettingsScreen {
    backend: Arc<AgentBackend>,
    user_id: String,
    settings: AppSettings,
    /// `settings.theme` parsed once; render only reads the label.
    theme: theme::Preference,
    section: Section,
    usage: Option<UsageSummary>,
    /// Plan usage meter, same source as the sidebar card.
    plan: Option<crate::billing::PlanUsage>,
    /// Account MCP servers; None until loaded.
    mcp_servers: Option<Vec<AgentMcpServer>>,
    mcp_editor: Option<McpEditor>,
    mcp_notice: Option<String>,
    mcp_saving: bool,
    /// Editor for the opening system prompt text (harness instructions).
    prompt_editor: Entity<TextInput>,
    prompt_notice: Option<String>,
    shortcut_snapshot: ShortcutSnapshot,
    shortcut_search: Entity<TextInput>,
    shortcut_query: String,
    shortcut_list_cache: ShortcutListCache,
    shortcut_notice: Option<String>,
    shortcut_recorder: Option<ShortcutRecorder>,
    shortcut_interceptor: Option<Subscription>,
    shortcut_reset_confirmation: bool,
    application_vim: SettingsApplicationVimState,
    application_focus: gpui::FocusHandle,
    application_focus_pending: bool,
    application_reveal_pending: bool,
    pane_scroll: ScrollHandle,
    application_anchor: ScrollAnchor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShortcutSettingsChange {
    Set {
        slot_id: String,
        sequence: String,
        disable_conflicts: Vec<String>,
    },
    Disable {
        slot_id: String,
    },
    Reset {
        slot_id: String,
    },
    ResetAll,
}

pub(crate) struct ShortcutSettingsRequested(pub(crate) ShortcutSettingsChange);

#[derive(Clone, Debug)]
struct ShortcutRecorder {
    slot_id: String,
    strokes: Vec<String>,
    conflicts: Vec<ShortcutConflict>,
    reviewing_conflicts: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ShortcutListCache {
    visible_indices: Vec<usize>,
    modified_count: usize,
}

impl ShortcutListCache {
    fn rebuild(snapshot: &ShortcutSnapshot, query: &str) -> Self {
        Self {
            visible_indices: snapshot
                .rows
                .iter()
                .enumerate()
                .filter_map(|(index, row)| shortcut_row_matches(row, query).then_some(index))
                .collect(),
            modified_count: snapshot.rows.iter().filter(|row| row.modified).count(),
        }
    }
}

/// Form state for adding or editing one MCP server.
struct McpEditor {
    /// Name of the server being edited; None when adding.
    original_name: Option<String>,
    enabled: bool,
    http: bool,
    timeout_seconds: u64,
    name: Entity<TextInput>,
    description: Entity<TextInput>,
    /// Command line (stdio) or URL (HTTP).
    target: Entity<TextInput>,
    /// `KEY=VALUE; KEY2=VALUE2` pairs.
    environment: Entity<TextInput>,
    headers: Entity<TextInput>,
}

/// Emitted with the section to open (composer "Manage servers" link).
pub struct OpenSettingsSection(pub Section);

impl EventEmitter<SettingsClosed> for SettingsScreen {}
impl EventEmitter<SignOutRequested> for SettingsScreen {}
impl EventEmitter<ShortcutSettingsRequested> for SettingsScreen {}

impl SettingsScreen {
    pub fn new(
        backend: Arc<AgentBackend>,
        user_id: String,
        settings: AppSettings,
        shortcut_snapshot: ShortcutSnapshot,
        section: Section,
        cx: &mut Context<Self>,
    ) -> Self {
        let prompt_text = settings.effective_harness_instructions();
        let application_vim_enabled = settings.application_vim_enabled;
        let application_focus = cx.focus_handle();
        let prompt_application_focus = application_focus.clone();
        let prompt_editor = cx.new(move |cx| {
            let mut input = TextInput::new("You are …", cx)
                .with_tab_index(1)
                .multiline(16)
                .spell_check()
                .application_vim(application_vim_enabled)
                .on_application_escape(move |window, _cx| window.focus(&prompt_application_focus));
            input.set_text(&prompt_text, cx);
            input
        });
        let search_application_focus = application_focus.clone();
        let shortcut_search = cx.new(move |cx| {
            TextInput::new("Search by command, context, or shortcut…", cx)
                .with_tab_index(2)
                .application_vim(application_vim_enabled)
                .on_application_escape(move |window, _cx| window.focus(&search_application_focus))
        });
        cx.observe(&shortcut_search, |this, search, cx| {
            this.shortcut_query = search.read(cx).text().trim().to_lowercase();
            this.rebuild_shortcut_list_cache();
            if this.settings.application_vim_enabled {
                this.reconcile_application_vim_target();
            }
            cx.notify();
        })
        .detach();
        let shortcut_list_cache = ShortcutListCache::rebuild(&shortcut_snapshot, "");
        let pane_scroll = ScrollHandle::new();
        let application_anchor = ScrollAnchor::for_handle(pane_scroll.clone());
        let application_vim = SettingsApplicationVimState::new(section);
        let application_focus_pending = settings.application_vim_enabled;
        let this = Self {
            backend,
            user_id,
            theme: theme::Preference::parse(&settings.theme),
            settings,
            section,
            usage: None,
            plan: None,
            mcp_servers: None,
            mcp_editor: None,
            mcp_notice: None,
            mcp_saving: false,
            prompt_editor,
            prompt_notice: None,
            shortcut_snapshot,
            shortcut_search,
            shortcut_query: String::new(),
            shortcut_list_cache,
            shortcut_notice: None,
            shortcut_recorder: None,
            shortcut_interceptor: None,
            shortcut_reset_confirmation: false,
            application_vim,
            application_focus,
            application_focus_pending,
            application_reveal_pending: false,
            pane_scroll,
            application_anchor,
        };
        this.load_usage(cx);
        this.load_plan(cx);
        this.load_mcp_servers(cx);
        this
    }

    fn load_plan(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.plan_usage(&user_id).await },
            cx,
            |this, result, cx| {
                if let Ok(plan) = result {
                    this.plan = plan;
                    cx.notify();
                }
            },
        );
    }

    fn call<T, F>(
        &self,
        future: F,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
    ) where
        T: Send + 'static,
        F: std::future::Future<Output = Result<T, String>> + Send + 'static,
    {
        crate::ui::task::call(&self.backend, future, cx, then);
    }

    fn load_mcp_servers(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.list_mcp_servers(&user_id).await },
            cx,
            |this, result, cx| {
                match result {
                    Ok(servers) => this.mcp_servers = Some(servers),
                    Err(message) => this.mcp_notice = Some(message),
                }
                cx.notify();
            },
        );
    }

    fn save_mcp_servers(&mut self, servers: Vec<AgentMcpServer>, cx: &mut Context<Self>) {
        if self.mcp_saving {
            return;
        }
        self.mcp_saving = true;
        self.mcp_notice = None;
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.save_mcp_servers(&user_id, servers).await },
            cx,
            |this, result, cx| {
                this.mcp_saving = false;
                match result {
                    Ok(servers) => {
                        this.mcp_servers = Some(servers);
                        this.mcp_editor = None;
                    }
                    Err(message) => this.mcp_notice = Some(message),
                }
                cx.notify();
            },
        );
    }

    fn open_mcp_editor(&mut self, existing: Option<AgentMcpServer>, cx: &mut Context<Self>) {
        let application_focus = self.application_focus.clone();
        let application_vim_enabled = self.settings.application_vim_enabled;
        let mut field = |placeholder: &str, value: &str, index: isize| {
            let value = value.to_string();
            let field_application_focus = application_focus.clone();
            cx.new(move |cx| {
                let mut input = TextInput::new(placeholder, cx)
                    .with_tab_index(index)
                    .application_vim(application_vim_enabled)
                    .on_application_escape(move |window, _cx| {
                        window.focus(&field_application_focus)
                    });
                if !value.is_empty() {
                    input.set_text(&value, cx);
                }
                input
            })
        };
        let (http, target, environment, headers) = match existing.as_ref().map(|s| &s.transport) {
            Some(AgentMcpTransport::Stdio {
                command,
                environment,
            }) => (
                false,
                command.clone(),
                pairs_to_text(environment),
                String::new(),
            ),
            Some(AgentMcpTransport::StreamableHttp {
                url,
                environment,
                headers,
            }) => (
                true,
                url.clone(),
                pairs_to_text(environment),
                pairs_to_text(headers),
            ),
            None => (false, String::new(), String::new(), String::new()),
        };
        self.mcp_editor = Some(McpEditor {
            original_name: existing.as_ref().map(|s| s.name.clone()),
            enabled: existing.as_ref().map(|s| s.enabled).unwrap_or(true),
            http,
            timeout_seconds: existing.as_ref().map(|s| s.timeout_seconds).unwrap_or(300),
            name: field(
                "My server",
                existing.as_ref().map(|s| s.name.as_str()).unwrap_or(""),
                1,
            ),
            description: field(
                "What this server helps the agent do",
                existing
                    .as_ref()
                    .map(|s| s.description.as_str())
                    .unwrap_or(""),
                2,
            ),
            target: field(
                "npx -y @modelcontextprotocol/server-everything stdio",
                &target,
                3,
            ),
            environment: field("KEY=value; OTHER=value", &environment, 4),
            headers: field("Authorization=Bearer …; X-Api-Key=…", &headers, 5),
        });
        self.mcp_notice = None;
        cx.notify();
    }

    fn submit_mcp_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.mcp_editor.as_ref() else {
            return;
        };
        let name = editor.name.read(cx).text().trim().to_string();
        if name.is_empty() {
            self.mcp_notice = Some("Enter a server name".to_string());
            cx.notify();
            return;
        }
        let existing = self.mcp_servers.as_deref().unwrap_or_default();
        if name_collides(existing, editor.original_name.as_deref(), &name) {
            self.mcp_notice = Some(format!("A server named {name:?} already exists"));
            cx.notify();
            return;
        }
        let target = editor.target.read(cx).text().trim().to_string();
        if target.is_empty() {
            self.mcp_notice = Some(if editor.http {
                "Enter the server URL".to_string()
            } else {
                "Enter the server command".to_string()
            });
            cx.notify();
            return;
        }
        let environment = text_to_pairs(&editor.environment.read(cx).text());
        let transport = if editor.http {
            AgentMcpTransport::StreamableHttp {
                url: target,
                environment,
                headers: text_to_pairs(&editor.headers.read(cx).text()),
            }
        } else {
            AgentMcpTransport::Stdio {
                command: target,
                environment,
            }
        };
        let server = AgentMcpServer {
            name,
            description: editor.description.read(cx).text().trim().to_string(),
            enabled: editor.enabled,
            timeout_seconds: editor.timeout_seconds,
            transport,
        };
        let original = editor.original_name.clone();
        let mut servers = self.mcp_servers.clone().unwrap_or_default();
        match original.and_then(|o| servers.iter().position(|s| s.name == o)) {
            Some(index) => servers[index] = server,
            None => servers.push(server),
        }
        self.save_mcp_servers(servers, cx);
    }

    /// Open the editor for the server called `name`, looked up on click so
    /// the list rows do not clone a server per frame.
    fn edit_mcp_server(&mut self, name: &str, cx: &mut Context<Self>) {
        let server = self
            .mcp_servers
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|server| server.name == name)
            .cloned();
        if server.is_some() {
            self.open_mcp_editor(server, cx);
        }
    }

    fn toggle_mcp_server(&mut self, name: &str, cx: &mut Context<Self>) {
        let mut servers = self.mcp_servers.clone().unwrap_or_default();
        if let Some(server) = servers.iter_mut().find(|s| s.name == name) {
            server.enabled = !server.enabled;
        }
        self.save_mcp_servers(servers, cx);
    }

    fn remove_mcp_server(&mut self, name: &str, cx: &mut Context<Self>) {
        let mut servers = self.mcp_servers.clone().unwrap_or_default();
        servers.retain(|s| s.name != name);
        self.save_mcp_servers(servers, cx);
    }

    fn load_usage(&self, cx: &mut Context<Self>) {
        let (spawn_backend, usage_backend) = (self.backend.clone(), self.backend.clone());
        let user_id = self.user_id.clone();
        let task = spawn_backend.spawn(async move {
            let scope = usage_backend.account_scope(&user_id);
            scope.map(|scope| settings::load_usage(&scope))
        });
        cx.spawn(async move |this, cx| {
            let usage = task.await.ok().flatten().unwrap_or_default();
            this.update(cx, |this, cx| {
                this.usage = Some(usage);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Change one setting: apply it to the local copy, queue the write
    /// off the UI thread, and re-render. Every toggle goes through here.
    ///
    /// `apply` must set a value, not toggle one: it runs on the local copy
    /// now and again on the file on disk from the writer thread, so both
    /// end in the same state whatever the file held.
    fn edit_setting(
        &mut self,
        apply: impl FnOnce(&mut AppSettings) + Clone + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        apply.clone()(&mut self.settings);
        settings::update_settings_in_background(apply);
        cx.notify();
    }

    fn toggle_permission_default(&mut self, cx: &mut Context<Self>) {
        let next = self.settings.default_permission_mode.next();
        self.edit_setting(move |settings| settings.default_permission_mode = next, cx);
    }

    fn toggle_web_default(&mut self, cx: &mut Context<Self>) {
        let next = !self.settings.default_web_enabled;
        self.edit_setting(move |settings| settings.default_web_enabled = next, cx);
    }

    fn cycle_theme(&mut self, cx: &mut Context<Self>) {
        let next = self.theme.next();
        self.theme = next;
        self.edit_setting(
            move |settings| settings.theme = next.as_str().to_string(),
            cx,
        );
        // The root view resolves the palette on its next render and
        // refreshes every view when it changed.
        crate::ui::theme::set_preference(next);
        cx.refresh_windows();
    }

    fn toggle_tool_details(&mut self, cx: &mut Context<Self>) {
        let next = !self.settings.tool_details;
        self.edit_setting(move |settings| settings.tool_details = next, cx);
    }

    fn toggle_desktop_notifications(&mut self, cx: &mut Context<Self>) {
        let next = !self.settings.desktop_notifications;
        self.edit_setting(move |settings| settings.desktop_notifications = next, cx);
        if self.settings.desktop_notifications {
            // Fire a test notification so enabling gives immediate feedback
            // and delivery problems surface right away.
            let enabled_at = chrono::Local::now().format("%H:%M").to_string();
            crate::notify::notify_desktop(
                "Desktop notifications on",
                &format!("You will see alerts like this at {enabled_at}."),
            );
        }
    }

    fn cycle_tts_voice(&mut self, cx: &mut Context<Self>) {
        let next = next_cyclic(&settings::TTS_VOICES, |(id, _)| {
            *id == self.settings.tts_voice
        })
        .0
        .to_string();
        self.edit_setting(move |settings| settings.tts_voice = next, cx);
    }

    fn cycle_tts_speed(&mut self, cx: &mut Context<Self>) {
        let next = *next_cyclic(&settings::TTS_SPEEDS, |speed| {
            (*speed - self.settings.tts_speed).abs() < 0.01
        });
        self.edit_setting(move |settings| settings.tts_speed = next, cx);
    }

    fn toggle_tool_summaries(&mut self, cx: &mut Context<Self>) {
        let next = !self.settings.tool_summaries;
        self.edit_setting(move |settings| settings.tool_summaries = next, cx);
    }

    fn toggle_composer_vim(&mut self, cx: &mut Context<Self>) {
        let next = !self.settings.composer_vim_enabled;
        self.edit_setting(move |settings| settings.composer_vim_enabled = next, cx);
    }

    fn toggle_application_vim(&mut self, cx: &mut Context<Self>) {
        let next = !self.settings.application_vim_enabled;
        self.set_application_vim_enabled(next, cx);
    }

    /// Persist the editor text as the harness instructions and hand it to
    /// the running backend. Text equal to the default is saved as empty so
    /// a future default change still applies.
    fn save_prompt(&mut self, cx: &mut Context<Self>) {
        let text = self.prompt_editor.read(cx).text().trim().to_string();
        self.settings.harness_instructions = if text == settings::DEFAULT_HARNESS_INSTRUCTIONS {
            String::new()
        } else {
            text
        };
        let instructions = self.settings.harness_instructions.clone();
        settings::update_settings_in_background(move |s| s.harness_instructions = instructions);
        self.backend
            .set_harness_instructions(self.settings.effective_harness_instructions());
        self.prompt_notice = Some("Saved. New tasks use this prompt.".to_string());
        cx.notify();
    }

    fn reset_prompt(&mut self, cx: &mut Context<Self>) {
        self.prompt_editor.update(cx, |input, cx| {
            input.set_text(settings::DEFAULT_HARNESS_INSTRUCTIONS, cx);
        });
        self.prompt_notice = None;
        cx.notify();
    }

    pub(crate) fn apply_shortcut_result(
        &mut self,
        shortcut_overrides: ShortcutOverrides,
        snapshot: ShortcutSnapshot,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if result.is_ok() {
            merge_shortcut_overrides(&mut self.settings, shortcut_overrides);
            self.shortcut_reset_confirmation = false;
            self.stop_shortcut_recording();
        }
        self.shortcut_snapshot = snapshot;
        self.rebuild_shortcut_list_cache();
        if self.settings.application_vim_enabled {
            self.reconcile_application_vim_target();
        }
        self.shortcut_notice = Some(match result {
            Ok(()) => "Shortcut settings applied.".to_string(),
            Err(message) => format!("Shortcut change failed: {message}"),
        });
        cx.notify();
    }

    fn request_shortcut_change(&mut self, change: ShortcutSettingsChange, cx: &mut Context<Self>) {
        self.shortcut_notice = None;
        cx.emit(ShortcutSettingsRequested(change));
    }

    fn rebuild_shortcut_list_cache(&mut self) {
        self.shortcut_list_cache =
            ShortcutListCache::rebuild(&self.shortcut_snapshot, &self.shortcut_query);
    }

    fn begin_shortcut_recording(&mut self, slot_id: String, cx: &mut Context<Self>) {
        self.stop_shortcut_recording();
        self.shortcut_notice = None;
        self.shortcut_recorder = Some(ShortcutRecorder {
            slot_id,
            strokes: Vec::new(),
            conflicts: Vec::new(),
            reviewing_conflicts: false,
        });

        let settings = cx.entity().downgrade();
        self.shortcut_interceptor = Some(cx.intercept_keystrokes(move |event, _window, app| {
            let Some(settings) = settings.upgrade() else {
                return;
            };
            settings.update(app, |this, cx| {
                this.record_shortcut_keystroke(&event.keystroke, cx);
            });
            // Recording owns the keystroke. No existing shortcut or text
            // input handler may observe it underneath the recorder.
            app.stop_propagation();
        }));
        cx.notify();
    }

    fn record_shortcut_keystroke(&mut self, keystroke: &gpui::Keystroke, cx: &mut Context<Self>) {
        let plain = !keystroke.modifiers.modified();
        match (plain, keystroke.key.as_str()) {
            (true, "escape") => {
                self.stop_shortcut_recording();
                self.shortcut_notice = Some("Shortcut recording cancelled.".to_string());
            }
            (true, "backspace") => {
                if let Some(recorder) = self.shortcut_recorder.as_mut() {
                    recorder.strokes.pop();
                }
            }
            (true, "enter") => self.review_recorded_shortcut(cx),
            _ => {
                let Some(recorder) = self.shortcut_recorder.as_mut() else {
                    return;
                };
                if recorder.strokes.len() < 4 {
                    recorder.strokes.push(portable_keystroke_token(keystroke));
                } else {
                    self.shortcut_notice =
                        Some("A shortcut can contain at most four strokes.".to_string());
                }
            }
        }
        cx.notify();
    }

    fn review_recorded_shortcut(&mut self, cx: &mut Context<Self>) {
        let Some(recorder) = self.shortcut_recorder.as_ref() else {
            return;
        };
        if recorder.strokes.is_empty() {
            self.shortcut_notice = Some("Press at least one shortcut key.".to_string());
            return;
        }
        let slot_id = recorder.slot_id.clone();
        let sequence = recorder.strokes.join(" ");
        match self.shortcut_snapshot.conflicts_for(&slot_id, &sequence) {
            Ok(conflicts) if conflicts.is_empty() => {
                self.stop_shortcut_recording();
                self.request_shortcut_change(
                    ShortcutSettingsChange::Set {
                        slot_id,
                        sequence,
                        disable_conflicts: Vec::new(),
                    },
                    cx,
                );
            }
            Ok(conflicts) => {
                // Conflict review uses ordinary buttons, so release the
                // keystroke interceptor before showing it.
                self.shortcut_interceptor = None;
                if let Some(recorder) = self.shortcut_recorder.as_mut() {
                    recorder.conflicts = conflicts;
                    recorder.reviewing_conflicts = true;
                }
            }
            Err(message) => {
                self.shortcut_notice = Some(format!("Cannot use that shortcut: {message}"));
            }
        }
    }

    fn save_recorded_shortcut(&mut self, replace_conflicts: bool, cx: &mut Context<Self>) {
        let Some(recorder) = self.shortcut_recorder.take() else {
            return;
        };
        self.shortcut_interceptor = None;
        let mut disable_conflicts = if replace_conflicts {
            recorder
                .conflicts
                .into_iter()
                .map(|conflict| conflict.other_slot_id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        disable_conflicts.sort();
        disable_conflicts.dedup();
        self.request_shortcut_change(
            ShortcutSettingsChange::Set {
                slot_id: recorder.slot_id,
                sequence: recorder.strokes.join(" "),
                disable_conflicts,
            },
            cx,
        );
    }

    fn stop_shortcut_recording(&mut self) {
        self.shortcut_interceptor = None;
        self.shortcut_recorder = None;
    }

    fn select_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if section != Section::Shortcuts {
            self.stop_shortcut_recording();
        }
        self.section = section;
        if self.settings.application_vim_enabled {
            self.application_vim.section = section;
            self.reconcile_application_vim_target();
        }
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.stop_shortcut_recording();
        cx.emit(SettingsClosed(self.settings.clone()));
    }
}

fn merge_shortcut_overrides(settings: &mut AppSettings, shortcut_overrides: ShortcutOverrides) {
    settings.shortcut_overrides = shortcut_overrides;
}

impl Render for SettingsScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.application_focus_pending {
            self.application_focus_pending = false;
            if self.settings.application_vim_enabled {
                window.focus(&self.application_focus);
            }
        }
        if self.application_reveal_pending {
            self.application_reveal_pending = false;
            if self.application_vim.target.is_some() {
                // The selected row attaches this anchor later in the same render.
                // ScrollAnchor defers until the fresh layout origin is available.
                self.application_anchor.scroll_to(window, cx);
            }
        }
        div()
            .key_context(self.application_vim_context())
            .track_focus(&self.application_focus)
            .on_action(cx.listener(Self::app_vim_next))
            .on_action(cx.listener(Self::app_vim_previous))
            .on_action(cx.listener(Self::app_vim_first))
            .on_action(cx.listener(Self::app_vim_last))
            .on_action(cx.listener(Self::app_vim_activate))
            .on_action(cx.listener(Self::app_vim_collapse))
            .on_action(cx.listener(Self::app_vim_expand))
            .on_action(cx.listener(Self::app_vim_copy))
            .on_action(cx.listener(Self::app_vim_search))
            .on_action(cx.listener(Self::app_vim_escape))
            .on_action(cx.listener(Self::app_vim_composer))
            .on_action(cx.listener(Self::app_vim_newest_assistant))
            .on_action(cx.listener(Self::app_vim_next_assistant))
            .on_action(cx.listener(Self::app_vim_previous_assistant))
            .on_action(cx.listener(Self::app_vim_next_annotation))
            .on_action(cx.listener(Self::app_vim_previous_annotation))
            .on_action(cx.listener(Self::app_vim_count))
            .on_action(cx.listener(Self::app_vim_move_region))
            .size_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::bg_app()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(gpui::rgb(theme::border()))
                    .child(
                        widgets::ghost_button("settings-back")
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.close(cx);
                            }))
                            .child("← Back"),
                    )
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(gpui::rgb(theme::text_primary()))
                            .child("Settings"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("settings-sign-out")
                            .px_2()
                            .py_1()
                            .rounded(theme::RADIUS_SM)
                            .text_sm()
                            .text_color(gpui::rgb(theme::text_muted()))
                            .hover(|style| {
                                style
                                    .text_color(gpui::rgb(theme::status_error()))
                                    .cursor_pointer()
                            })
                            .on_click(cx.listener(|_this, _event, _window, cx| {
                                cx.emit(SignOutRequested);
                            }))
                            .child("Sign out"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(self.render_nav(cx))
                    .child(self.render_pane(cx)),
            )
    }
}

impl SettingsScreen {
    fn render_nav(&self, cx: &mut Context<Self>) -> Div {
        div()
            .w(gpui::px(220.))
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(gpui::rgb(theme::border()))
            .bg(gpui::rgb(theme::bg_sidebar()))
            .children(Section::ALL.iter().map(|section| {
                let selected = self.section == *section;
                let application_selected = self.application_vim_selects_section(*section);
                div()
                    .id(gpui::SharedString::from(format!(
                        "settings-nav-{}",
                        section.label()
                    )))
                    .px_3()
                    .py_2()
                    .rounded(theme::RADIUS_SM)
                    .text_sm()
                    .text_color(gpui::rgb(if selected {
                        theme::text_primary()
                    } else {
                        theme::text_secondary()
                    }))
                    .bg(gpui::rgb(if selected {
                        theme::bg_elevated()
                    } else {
                        theme::bg_sidebar()
                    }))
                    .when(application_selected, |row| {
                        row.border_l_2().border_color(gpui::rgb(theme::accent()))
                    })
                    .hover(|style| style.bg(gpui::rgb(theme::bg_elevated())).cursor_pointer())
                    .on_click({
                        let section = *section;
                        cx.listener(move |this, _event, _window, cx| {
                            this.select_section(section, cx);
                        })
                    })
                    .child(section.label().to_string())
            }))
    }

    fn render_pane(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let mut pane = div()
            .id("settings-pane")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .track_scroll(&self.pane_scroll)
            .overflow_y_scroll();
        match self.section {
            Section::General => {
                pane = pane
                    .child(section_title("Defaults"))
                    .child({
                        let mode = self.settings.default_permission_mode;
                        self.application_target(
                            || SettingsTarget::General(GeneralTarget::Permission),
                            setting_row(
                                "Default permission mode",
                                mode.note(),
                                mode.label(),
                                cx.listener(|this, _event, _window, cx| {
                                    this.toggle_permission_default(cx);
                                }),
                            ),
                        )
                    })
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::Web),
                        setting_row(
                            "New tasks can use the web",
                            "Offers web_search and open_url to the model. Each task can \
                             switch web access on or off from its composer.",
                            if self.settings.default_web_enabled { "On" } else { "Off" },
                            cx.listener(|this, _event, _window, cx| {
                                this.toggle_web_default(cx);
                            }),
                        ),
                    ))
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::Appearance),
                        setting_row(
                            "Appearance",
                            "Follow the system theme, or force dark or light.",
                            self.theme.label(),
                            cx.listener(|this, _event, _window, cx| {
                                this.cycle_theme(cx);
                            }),
                        ),
                    ))
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::ToolDetails),
                        setting_row(
                            "Show tool call details",
                            "Tool cards include their input and output payloads.",
                            if self.settings.tool_details { "On" } else { "Off" },
                            cx.listener(|this, _event, _window, cx| {
                                this.toggle_tool_details(cx);
                            }),
                        ),
                    ))
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::Notifications),
                        setting_row(
                            "Desktop notifications",
                            "Notify when a task finishes or needs your input while \
                             the window is not focused.",
                            if self.settings.desktop_notifications { "On" } else { "Off" },
                            cx.listener(|this, _event, _window, cx| {
                                this.toggle_desktop_notifications(cx);
                            }),
                        ),
                    ))
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::ToolSummaries),
                        setting_row(
                            "Summarize tool calls",
                            "Completed tool calls with long output get a one-line \
                             summary from the title model on their cards.",
                            if self.settings.tool_summaries { "On" } else { "Off" },
                            cx.listener(|this, _event, _window, cx| {
                                this.toggle_tool_summaries(cx);
                            }),
                        ),
                    ))
                    .child(section_title("Editing"))
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::ComposerVim),
                        setting_row(
                            "Vim mode in composer",
                            "Use Normal, Insert, and Visual editing modes in the main chat composer. Other text fields stay unchanged.",
                            if self.settings.composer_vim_enabled { "On" } else { "Off" },
                            cx.listener(|this, _event, _window, cx| {
                                this.toggle_composer_vim(cx);
                            }),
                        ),
                    ))
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::ApplicationVim),
                        setting_row(
                            "Vim navigation across the app",
                            "Navigate stable sidebar, transcript, dialog, and Settings targets. Ordinary text fields still edit normally.",
                            if self.settings.application_vim_enabled { "On" } else { "Off" },
                            cx.listener(|this, _event, _window, cx| {
                                this.toggle_application_vim(cx);
                            }),
                        ),
                    ))
                    .child(section_title("Voice"))
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::Voice),
                        setting_row(
                            "Speech voice",
                            "The voice that reads messages aloud. Click to move to \
                             the next voice.",
                            settings::tts_voice_label(&self.settings.tts_voice),
                            cx.listener(|this, _event, _window, cx| {
                                this.cycle_tts_voice(cx);
                            }),
                        ),
                    ))
                    .child(self.application_target(
                        || SettingsTarget::General(GeneralTarget::SpeechSpeed),
                        setting_row(
                            "Speech speed",
                            "How fast messages are read aloud.",
                            &format!("{:.1}×", self.settings.tts_speed),
                            cx.listener(|this, _event, _window, cx| {
                                this.cycle_tts_speed(cx);
                            }),
                        ),
                    ));
            }
            Section::Shortcuts => {
                pane = pane.child(self.render_shortcuts_pane(cx));
            }
            Section::Prompt => {
                pane = pane.child(self.render_prompt_pane(cx));
            }
            Section::Mcp => {
                pane = pane.child(self.render_mcp_pane(cx));
            }
            Section::Usage => {
                pane = pane.child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_2()
                        .child(section_title("Plan"))
                        .when_some(self.plan.as_ref(), |row, plan| {
                            row.child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(gpui::rgb(theme::accent()))
                                    .child(plan.plan_label.clone()),
                            )
                        }),
                );
                pane = pane.child(match &self.plan {
                    Some(plan) => plan_card(plan),
                    None => div()
                        .text_sm()
                        .text_color(gpui::rgb(theme::text_faint()))
                        .child("Plan usage unavailable"),
                });
                pane = pane.child(section_title("Usage"));
                if let Some(usage) = &self.usage {
                    pane = pane
                        .child(
                            div()
                                .flex()
                                .gap_6()
                                .child(stat("Turns", usage.totals.turns.to_string()))
                                .child(stat("Sessions", usage.totals.sessions.to_string()))
                                .child(stat(
                                    "Total tokens",
                                    format_tokens(usage.totals.total_tokens),
                                ))
                                .child(stat("Est. cost", format!("${:.2}", usage.totals.cost))),
                        )
                        .child(usage_table("By model", &usage.by_model))
                        .child(usage_table("Recent sessions", &usage.by_session));
                } else {
                    pane = pane.child(
                        div()
                            .text_color(gpui::rgb(theme::text_faint()))
                            .child("Loading usage…"),
                    );
                }
            }
            Section::About => {
                pane = pane
                    .child(section_title("About"))
                    .child(info_row(
                        "Version",
                        format!("v{}", crate::ui::titlebar::TitleBar::version()),
                    ))
                    .child(info_row(
                        "Update",
                        match crate::update::available() {
                            Some(info) => format!("v{} is available at {}", info.version, info.url),
                            None if crate::update::enabled() => "Up to date".to_string(),
                            None => "Update check disabled".to_string(),
                        },
                    ))
                    .child(info_row("Backend", self.backend.api_url().to_string()))
                    .child(info_row(
                        "Config directory",
                        crate::backend::app_config_root()
                            .to_string_lossy()
                            .to_string(),
                    ));
            }
        }
        pane
    }
}

impl SettingsScreen {
    fn render_shortcuts_pane(&self, cx: &mut Context<Self>) -> Div {
        let modified_count = self.shortcut_list_cache.modified_count;
        let rows = &self.shortcut_list_cache.visible_indices;

        let mut pane = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_title("Keyboard Shortcuts"))
                    .when(
                        modified_count > 0 || self.shortcut_snapshot.last_error.is_some(),
                        |row| {
                            row.child(
                                widgets::ghost_button("shortcuts-reset-all")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.shortcut_reset_confirmation = true;
                                        this.shortcut_notice = None;
                                        cx.notify();
                                    }))
                                    .child("Reset all"),
                            )
                        },
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_muted()))
                    .child(
                        "Customize the shortcuts Maple already ships. This page does not add commands or change what an action can do.",
                    ),
            )
            .child(widgets::input_frame().text_sm().child(self.shortcut_search.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::text_muted()))
                    .child(format!(
                        "{} bindings · {modified_count} modified",
                        self.shortcut_snapshot.rows.len()
                    )),
            );

        if let Some(error) = &self.shortcut_snapshot.last_error {
            pane = pane.child(
                widgets::banner(theme::status_warning()).child(format!(
                    "Saved shortcut overrides were not applied; Maple kept the complete default map. {error}"
                )),
            );
        }
        if let Some(warning) = &self.shortcut_snapshot.compatibility_warning {
            pane = pane.child(widgets::banner(theme::status_warning()).child(warning.clone()));
        }
        if let Some(notice) = &self.shortcut_notice {
            pane = pane.child(widgets::banner(theme::status_warning()).child(notice.clone()));
        }
        if self.shortcut_reset_confirmation {
            pane = pane.child(
                widgets::banner(theme::status_warning())
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().child(
                        if self.shortcut_snapshot.last_error.is_some() && modified_count == 0 {
                            "Clear the invalid saved shortcut overrides?".to_string()
                        } else {
                            format!(
                                "Reset all {modified_count} shortcut change{}?",
                                if modified_count == 1 { "" } else { "s" }
                            )
                        },
                    ))
                    .child(
                        widgets::primary_button("shortcuts-reset-all-confirm")
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.request_shortcut_change(ShortcutSettingsChange::ResetAll, cx);
                            }))
                            .child("Reset"),
                    )
                    .child(
                        widgets::ghost_button("shortcuts-reset-all-cancel")
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.shortcut_reset_confirmation = false;
                                cx.notify();
                            }))
                            .child("Cancel"),
                    ),
            );
        }
        if let Some(recorder) = self.shortcut_recorder.as_ref() {
            pane = pane.child(self.render_shortcut_recorder(recorder, cx));
        }

        if rows.is_empty() {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_faint()))
                    .child("No shortcuts match this search."),
            );
        } else {
            pane =
                pane.children(rows.iter().map(|&index| {
                    self.render_shortcut_row(&self.shortcut_snapshot.rows[index], cx)
                }));
        }
        pane
    }

    fn render_shortcut_recorder(&self, recorder: &ShortcutRecorder, cx: &mut Context<Self>) -> Div {
        let label = self
            .shortcut_snapshot
            .rows
            .iter()
            .find(|row| row.slot_id == recorder.slot_id)
            .map(|row| row.label.as_str())
            .unwrap_or(recorder.slot_id.as_str())
            .to_string();
        let sequence = if recorder.strokes.is_empty() {
            "Waiting for keys…".to_string()
        } else {
            recorder.strokes.join(" ")
        };
        let mut card = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded(theme::RADIUS_MD)
            .bg(gpui::rgb(theme::bg_elevated()))
            .border_1()
            .border_color(gpui::rgb(theme::accent()))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(gpui::rgb(theme::text_primary()))
                    .child(format!("Recording: {label}")),
            )
            .child(shortcut_keycap(sequence, true))
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::text_muted()))
                    .child(if recorder.reviewing_conflicts {
                        "Review the conflicts below before saving."
                    } else {
                        "Press up to four keys. Enter saves, Backspace removes the latest stroke, and Escape cancels."
                    }),
            );

        if recorder.reviewing_conflicts {
            let can_keep_both = recorder.conflicts.iter().all(|conflict| {
                !matches!(conflict.kind, ShortcutConflictKind::Exact)
                    || !matches!(conflict.overlap, ShortcutContextOverlap::Equivalent)
            });
            card = card
                .children(recorder.conflicts.iter().map(|conflict| {
                    let other = self
                        .shortcut_snapshot
                        .rows
                        .iter()
                        .find(|row| row.slot_id == conflict.other_slot_id)
                        .map(|row| row.label.as_str())
                        .unwrap_or(conflict.other_slot_id.as_str());
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::status_warning()))
                        .child(format!(
                            "{} with {} ({})",
                            shortcut_conflict_label(conflict),
                            other,
                            conflict.other_slot_id
                        ))
                }))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            widgets::primary_button("shortcut-conflict-replace")
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.save_recorded_shortcut(true, cx);
                                }))
                                .child("Replace conflicts"),
                        )
                        .when(can_keep_both, |actions| {
                            actions.child(
                                widgets::ghost_button("shortcut-conflict-keep")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.save_recorded_shortcut(false, cx);
                                    }))
                                    .child("Keep both"),
                            )
                        })
                        .child(
                            widgets::ghost_button("shortcut-conflict-cancel")
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.stop_shortcut_recording();
                                    cx.notify();
                                }))
                                .child("Cancel"),
                        ),
                );
        } else {
            card = card.child(
                widgets::ghost_button("shortcut-record-cancel")
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.stop_shortcut_recording();
                        cx.notify();
                    }))
                    .child("Cancel"),
            );
        }
        card
    }

    fn render_shortcut_row(
        &self,
        row: &crate::shortcuts::ShortcutRow,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let slot_id = row.slot_id.clone();
        let record_id = slot_id.clone();
        let disable_id = slot_id.clone();
        let reset_id = slot_id.clone();
        let current = row
            .current_sequence
            .clone()
            .unwrap_or_else(|| "Unbound".to_string());
        let card = widgets::card_row()
            .flex()
            .items_center()
            .gap_4()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(gpui::rgb(theme::text_primary()))
                                    .child(row.label.clone()),
                            )
                            .when(row.modified, |title| {
                                title.child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_full()
                                        .text_xs()
                                        .bg(gpui::rgb(theme::bg_sidebar_pill()))
                                        .text_color(gpui::rgb(theme::accent()))
                                        .child("Modified"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(format!(
                                "{} · {} · {}",
                                row.category,
                                shortcut_context_label(row.context.as_deref()),
                                row.slot_id
                            )),
                    )
                    .when(!row.conflicts.is_empty(), |column| {
                        column.child(
                            div()
                                .text_xs()
                                .text_color(gpui::rgb(theme::status_warning()))
                                .child(format!(
                                    "{} shortcut conflict{}",
                                    row.conflicts.len(),
                                    if row.conflicts.len() == 1 { "" } else { "s" }
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap_1()
                    .child(shortcut_keycap(current, row.current_sequence.is_some()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::text_faint()))
                            .child(format!("Default: {}", row.default_sequence)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        widgets::ghost_button(gpui::SharedString::from(format!(
                            "shortcut-record-{slot_id}"
                        )))
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.begin_shortcut_recording(record_id.clone(), cx);
                        }))
                        .child("Record"),
                    )
                    .when(row.current_sequence.is_some(), |actions| {
                        actions.child(
                            widgets::ghost_button(gpui::SharedString::from(format!(
                                "shortcut-disable-{slot_id}"
                            )))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_shortcut_change(
                                    ShortcutSettingsChange::Disable {
                                        slot_id: disable_id.clone(),
                                    },
                                    cx,
                                );
                            }))
                            .child("Disable"),
                        )
                    })
                    .when(row.modified, |actions| {
                        actions.child(
                            widgets::ghost_button(gpui::SharedString::from(format!(
                                "shortcut-reset-{slot_id}"
                            )))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.request_shortcut_change(
                                    ShortcutSettingsChange::Reset {
                                        slot_id: reset_id.clone(),
                                    },
                                    cx,
                                );
                            }))
                            .child("Reset"),
                        )
                    }),
            );
        self.application_target(|| SettingsTarget::Shortcut(row.slot_id.clone()), card)
    }

    fn render_prompt_pane(&self, cx: &mut Context<Self>) -> Div {
        let mut pane = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(section_title("System prompt"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_muted()))
                    .child(
                        "Opens every task's system prompt: who the agent is and how it \
                         behaves. Maple appends its tool and runtime guidance after this \
                         text. Changes apply to tasks started after you save.",
                    ),
            )
            .child(
                self.application_target(
                    || SettingsTarget::PromptEditor,
                    div()
                        .p_3()
                        .rounded(theme::RADIUS_SM)
                        .bg(gpui::rgb(theme::bg_input()))
                        .border_1()
                        .border_color(gpui::rgb(theme::border()))
                        .text_sm()
                        .text_color(gpui::rgb(theme::text_primary()))
                        .child(self.prompt_editor.clone()),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.application_target(
                        || SettingsTarget::PromptSave,
                        pill_button(
                            "prompt-save".to_string(),
                            "Save",
                            true,
                            cx.listener(|this, _event, _window, cx| this.save_prompt(cx)),
                        ),
                    ))
                    .child(self.application_target(
                        || SettingsTarget::PromptReset,
                        pill_button(
                            "prompt-reset".to_string(),
                            "Reset to default",
                            false,
                            cx.listener(|this, _event, _window, cx| this.reset_prompt(cx)),
                        ),
                    )),
            );
        if let Some(notice) = &self.prompt_notice {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .child(notice.clone()),
            );
        }
        pane
    }

    fn render_mcp_pane(&self, cx: &mut Context<Self>) -> Div {
        let mut pane = div().flex().flex_col().gap_4();
        pane = pane.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(section_title("MCP servers"))
                .child(
                    self.application_target(
                        || SettingsTarget::McpAdd,
                        widgets::primary_button("mcp-add")
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.open_mcp_editor(None, cx);
                            }))
                            .child(icon("plus", widgets::ROW_ICON, theme::on_accent()))
                            .child("Add server"),
                    ),
                ),
        );
        pane = pane.child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::text_muted()))
                .child(
                    "Servers configured here are available to every task. \
                     Turn them on or off per task from the composer.",
                ),
        );
        if let Some(notice) = &self.mcp_notice {
            pane = pane.child(widgets::banner(theme::status_warning()).child(notice.clone()));
        }
        if let Some(editor) = &self.mcp_editor {
            pane = pane.child(self.render_mcp_editor(editor, cx));
        }
        match &self.mcp_servers {
            None => {
                pane = pane.child(
                    div()
                        .text_color(gpui::rgb(theme::text_faint()))
                        .child("Loading MCP servers…"),
                );
            }
            Some(servers) if servers.is_empty() => {
                pane = pane.child(
                    div()
                        .text_sm()
                        .text_color(gpui::rgb(theme::text_faint()))
                        .child("No MCP servers configured."),
                );
            }
            Some(servers) => {
                pane = pane.children(servers.iter().map(|server| {
                    let name = server.name.clone();
                    let summary = match &server.transport {
                        AgentMcpTransport::Stdio { command, .. } => format!("STDIO · {command}"),
                        AgentMcpTransport::StreamableHttp { url, .. } => format!("HTTP · {url}"),
                    };
                    let card = widgets::card_row()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(gpui::rgb(theme::text_primary()))
                                        .child(server.name.clone()),
                                )
                                .when(!server.description.is_empty(), |col| {
                                    col.child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::rgb(theme::text_secondary()))
                                            .child(server.description.clone()),
                                    )
                                })
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(gpui::rgb(theme::text_muted()))
                                        .line_clamp(1)
                                        .child(summary),
                                ),
                        )
                        .child(pill_button(
                            format!("mcp-toggle-{name}"),
                            if server.enabled {
                                "Enabled"
                            } else {
                                "Disabled"
                            },
                            server.enabled,
                            cx.listener({
                                let name = name.clone();
                                move |this, _event, _window, cx| this.toggle_mcp_server(&name, cx)
                            }),
                        ))
                        .child(
                            widgets::icon_button(
                                gpui::SharedString::from(format!("mcp-edit-{name}")),
                                "pencil",
                                widgets::ROW_ICON,
                                theme::text_secondary(),
                            )
                            .on_click(cx.listener({
                                let name = name.clone();
                                move |this, _event, _window, cx| this.edit_mcp_server(&name, cx)
                            })),
                        )
                        .child(
                            widgets::icon_button(
                                gpui::SharedString::from(format!("mcp-remove-{name}")),
                                "trash-2",
                                widgets::ROW_ICON,
                                theme::status_error(),
                            )
                            .on_click(cx.listener(
                                move |this, _event, _window, cx| this.remove_mcp_server(&name, cx),
                            )),
                        );
                    self.application_target(|| SettingsTarget::McpServer(server.name.clone()), card)
                }));
            }
        }
        pane
    }

    fn render_mcp_editor(&self, editor: &McpEditor, cx: &mut Context<Self>) -> Div {
        let field = |label: &'static str, hint: &'static str, input: Entity<TextInput>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(gpui::rgb(theme::text_secondary()))
                        .child(label),
                )
                .child(widgets::input_frame().text_sm().child(input))
                .when(!hint.is_empty(), |col| {
                    col.child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(hint),
                    )
                })
        };
        let transport_segment = |id: &'static str, label: &'static str, active: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded(theme::RADIUS_SM)
                .text_sm()
                .text_color(gpui::rgb(if active {
                    theme::text_primary()
                } else {
                    theme::text_secondary()
                }))
                .when(active, |el| el.bg(gpui::rgb(theme::bg_sidebar_row_hover())))
                .hover(|style| style.cursor_pointer())
                .child(label)
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded(theme::RADIUS_MD)
            .bg(gpui::rgb(theme::bg_elevated()))
            .border_1()
            .border_color(gpui::rgb(theme::border()))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(gpui::rgb(theme::text_primary()))
                    .child(if editor.original_name.is_some() {
                        "Edit server"
                    } else {
                        "New server"
                    }),
            )
            .child(field("Name", "", editor.name.clone()))
            .child(field("Description", "", editor.description.clone()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(gpui::rgb(theme::text_secondary()))
                            .child("Transport"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .p_1()
                            .rounded(theme::RADIUS_SM)
                            .bg(gpui::rgb(theme::bg_sidebar_chrome()))
                            .w(px(320.))
                            .child(
                                transport_segment(
                                    "mcp-transport-stdio",
                                    "Standard IO (STDIO)",
                                    !editor.http,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        if let Some(editor) = this.mcp_editor.as_mut() {
                                            editor.http = false;
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                transport_segment(
                                    "mcp-transport-http",
                                    "Streamable HTTP",
                                    editor.http,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        if let Some(editor) = this.mcp_editor.as_mut() {
                                            editor.http = true;
                                        }
                                        cx.notify();
                                    },
                                )),
                            ),
                    ),
            )
            .child(if editor.http {
                field("URL", "", editor.target.clone())
            } else {
                field("Command", "", editor.target.clone())
            })
            .child(field(
                "Environment",
                "KEY=value pairs separated by semicolons.",
                editor.environment.clone(),
            ))
            .when(editor.http, |col| {
                col.child(field(
                    "Headers",
                    "Name=value pairs separated by semicolons.",
                    editor.headers.clone(),
                ))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pt_1()
                    .child(
                        widgets::primary_button("mcp-save")
                            .when(self.mcp_saving, |el| el.opacity(0.5))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.submit_mcp_editor(cx);
                            }))
                            .child("Save"),
                    )
                    .child(
                        widgets::ghost_button("mcp-cancel")
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.mcp_editor = None;
                                this.mcp_notice = None;
                                cx.notify();
                            }))
                            .child("Cancel"),
                    ),
            )
    }
}

/// Plan usage meter: plan pill, percent used, reset date, progress bar.
/// Mirrors the sidebar card at a larger size.
fn plan_card(plan: &crate::billing::PlanUsage) -> Div {
    let fraction = f32::from(plan.percent_used) / 100.0;
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .px_4()
        .py_4()
        .rounded(theme::RADIUS_MD)
        .bg(gpui::rgb(theme::bg_sidebar_card()))
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_base()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(gpui::rgb(theme::text_primary()))
                        .whitespace_nowrap()
                        .child(format!("{}% used", plan.percent_used)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(gpui::rgb(theme::text_muted()))
                        .whitespace_nowrap()
                        .child(format!("· Resets {}", plan.resets_label)),
                ),
        )
        .child(
            div()
                .w_full()
                .h(px(6.))
                .rounded_full()
                // Darker than the card so the empty part of the track shows.
                .bg(gpui::rgb(theme::bg_sidebar_pill()))
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(fraction.clamp(0., 1.)))
                        .rounded_full()
                        .bg(gpui::rgb(theme::accent())),
                ),
        )
}

/// Small on/off pill used in list rows.
fn pill_button(
    id: String,
    label: &'static str,
    on: bool,
    handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(gpui::SharedString::from(id))
        .px_2p5()
        .py_1()
        .rounded_full()
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .bg(gpui::rgb(if on {
            theme::accent()
        } else {
            theme::bg_sidebar_card()
        }))
        .text_color(gpui::rgb(if on {
            theme::on_accent()
        } else {
            theme::text_secondary()
        }))
        .hover(|style| style.cursor_pointer())
        .on_click(handler)
        .child(label)
}

/// The item after the one `is_current` matches, wrapping at the end. An
/// unknown current value restarts from the second item, as the settings
/// rows always did.
fn next_cyclic<T>(items: &[T], is_current: impl Fn(&T) -> bool) -> &T {
    let current = items.iter().position(is_current).unwrap_or(0);
    &items[(current + 1) % items.len()]
}

/// Whether saving a server as `name` would clash with another server.
/// Servers are matched by name, so a rename onto an existing name would
/// have added a second entry instead of replacing the original.
fn name_collides(servers: &[AgentMcpServer], original: Option<&str>, name: &str) -> bool {
    servers
        .iter()
        .any(|server| server.name == name && Some(server.name.as_str()) != original)
}

/// `KEY=value; KEY2=value` for the editor field.
fn pairs_to_text(pairs: &[AgentMcpKeyValue]) -> String {
    pairs
        .iter()
        .map(|pair| format!("{}={}", pair.key, pair.value))
        .collect::<Vec<_>>()
        .join("; ")
}

fn text_to_pairs(text: &str) -> Vec<AgentMcpKeyValue> {
    text.split(';')
        .filter_map(|entry| {
            let entry = entry.trim();
            let (key, value) = entry.split_once('=')?;
            let key = key.trim();
            (!key.is_empty()).then(|| AgentMcpKeyValue {
                key: key.to_string(),
                value: value.trim().to_string(),
            })
        })
        .collect()
}

/// Pane heading in the brand display face, like the section titles of
/// the brand kit.
fn section_title(label: &str) -> Div {
    div()
        .font_family(crate::assets::FONT_DISPLAY)
        .text_size(gpui::px(26.))
        .line_height(gpui::px(32.))
        .text_color(gpui::rgb(theme::display_text()))
        .child(label.to_string())
}

fn shortcut_keycap(label: String, bound: bool) -> Div {
    div()
        .px_2p5()
        .py_1()
        .rounded(theme::RADIUS_SM)
        .border_1()
        .border_color(gpui::rgb(theme::border()))
        .bg(gpui::rgb(theme::bg_input()))
        .text_xs()
        .font_family(crate::assets::FONT_MONO)
        .text_color(gpui::rgb(if bound {
            theme::text_primary()
        } else {
            theme::text_faint()
        }))
        .child(label)
}

fn shortcut_context_label(context: Option<&str>) -> &str {
    match context {
        None => "Global",
        Some("Chat") => "Chat",
        Some("Transcript") => "Transcript",
        Some("RootMenu") => "Project menu",
        Some("TextInput") => "Text fields",
        Some(crate::ui::text_input::vim_actions::NORMAL_CONTEXT) => "Composer — Normal",
        Some(crate::ui::text_input::vim_actions::VISUAL_CONTEXT) => "Composer — Visual",
        Some(crate::ui::text_input::vim_actions::INSERT_CONTEXT) => "Composer — Insert",
        Some(context) => context,
    }
}

fn shortcut_row_matches(row: &crate::shortcuts::ShortcutRow, query: &str) -> bool {
    query.is_empty()
        || row.label.to_lowercase().contains(query)
        || row.slot_id.to_lowercase().contains(query)
        || row.category.to_lowercase().contains(query)
        || row
            .context
            .as_deref()
            .unwrap_or("global")
            .to_lowercase()
            .contains(query)
        || shortcut_context_label(row.context.as_deref())
            .to_lowercase()
            .contains(query)
        || row.default_sequence.to_lowercase().contains(query)
        || row
            .current_sequence
            .as_deref()
            .is_some_and(|sequence| sequence.to_lowercase().contains(query))
}

fn shortcut_conflict_label(conflict: &ShortcutConflict) -> String {
    let kind = match conflict.kind {
        ShortcutConflictKind::Exact => "Exact collision",
        ShortcutConflictKind::Prefix => "Prefix collision",
    };
    let overlap = match conflict.overlap {
        ShortcutContextOverlap::Equivalent => "same context",
        ShortcutContextOverlap::Scoped => "overlapping scoped context",
        ShortcutContextOverlap::Possible => "possibly overlapping context",
    };
    format!("{kind}, {overlap}")
}

fn portable_keystroke_token(keystroke: &gpui::Keystroke) -> String {
    let mut parts = Vec::with_capacity(6);
    if keystroke.modifiers.secondary() {
        parts.push("secondary".to_string());
    }
    if keystroke.modifiers.control && cfg!(target_os = "macos") {
        parts.push("ctrl".to_string());
    }
    if keystroke.modifiers.alt {
        parts.push("alt".to_string());
    }
    if keystroke.modifiers.shift {
        parts.push("shift".to_string());
    }
    if keystroke.modifiers.platform && !cfg!(target_os = "macos") {
        parts.push("cmd".to_string());
    }
    if keystroke.modifiers.function {
        parts.push("fn".to_string());
    }
    parts.push(match keystroke.key.as_str() {
        " " => "space".to_string(),
        key => key.to_lowercase(),
    });
    parts.join("-")
}

fn setting_row(
    title: &str,
    description: &str,
    value: &str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    widgets::card_row()
        .id(gpui::SharedString::from(format!(
            "setting-{}",
            title.to_lowercase()
        )))
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(gpui::rgb(theme::text_primary()))
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::text_muted()))
                        .child(description.to_string()),
                ),
        )
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "setting-value-{}",
                    title.to_lowercase().replace(' ', "-")
                )))
                .px_4()
                .py_2()
                .rounded(theme::RADIUS_SM)
                .bg(gpui::rgb(theme::bg_input()))
                .border_1()
                .border_color(gpui::rgb(theme::border()))
                .text_sm()
                .text_color(gpui::rgb(theme::text_secondary()))
                .hover(|style| style.cursor_pointer())
                .on_click(on_click)
                .child(value.to_string()),
        )
}

fn info_row(label: &str, value: String) -> Div {
    widgets::card_row()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::text_secondary()))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::text_primary()))
                .child(value),
        )
}

fn stat(label: &str, value: String) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme::text_muted()))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(gpui::rgb(theme::text_primary()))
                .child(value),
        )
}

fn usage_table(title: &str, rows: &[crate::settings::UsageRow]) -> Div {
    let mut table = div().flex().flex_col().gap_2().child(
        div()
            .text_sm()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(gpui::rgb(theme::text_primary()))
            .child(title.to_string()),
    );
    for row in rows.iter().take(10) {
        table = table.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .px_3()
                .py_2()
                .rounded(theme::RADIUS_SM)
                .bg(gpui::rgb(theme::bg_elevated()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(gpui::rgb(theme::text_primary()))
                        .line_clamp(1)
                        .child(row.label.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::text_secondary()))
                        .child(format!("{} turns", row.turns)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::text_secondary()))
                        .child(format_tokens(row.total_tokens)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::text_muted()))
                        .child(format!("${:.2}", row.cost)),
                ),
        );
    }
    table
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut_row(
        slot_id: &str,
        label: &str,
        category: &str,
        context: Option<&str>,
        default_sequence: &str,
        current_sequence: Option<&str>,
        modified: bool,
    ) -> crate::shortcuts::ShortcutRow {
        crate::shortcuts::ShortcutRow {
            slot_id: slot_id.to_string(),
            label: label.to_string(),
            category: category.to_string(),
            context: context.map(str::to_string),
            default_sequence: default_sequence.to_string(),
            current_sequence: current_sequence.map(str::to_string),
            modified,
            conflicts: Vec::new(),
        }
    }

    fn server(name: &str) -> AgentMcpServer {
        AgentMcpServer {
            name: name.to_string(),
            description: String::new(),
            enabled: true,
            timeout_seconds: 300,
            transport: AgentMcpTransport::Stdio {
                command: "cmd".to_string(),
                environment: Vec::new(),
            },
        }
    }

    #[test]
    fn next_cyclic_wraps_and_restarts_on_unknown() {
        let items = ["a", "b", "c"];
        assert_eq!(*next_cyclic(&items, |item| *item == "a"), "b");
        assert_eq!(*next_cyclic(&items, |item| *item == "c"), "a");
        assert_eq!(*next_cyclic(&items, |item| *item == "zzz"), "b");
    }

    #[test]
    fn shortcut_result_preserves_unrelated_general_setting() {
        let mut settings = AppSettings::default();
        settings.default_web_enabled = !settings.default_web_enabled;
        let expected_web_enabled = settings.default_web_enabled;
        settings
            .shortcut_overrides
            .insert("chat.focus_search".into(), None);
        let overrides = ShortcutOverrides::from([(
            "chat.new_task".to_owned(),
            Some("secondary-shift-n".to_owned()),
        )]);

        merge_shortcut_overrides(&mut settings, overrides.clone());

        assert_eq!(settings.default_web_enabled, expected_web_enabled);
        assert_eq!(settings.shortcut_overrides, overrides);
    }

    #[test]
    fn renaming_onto_another_server_is_a_collision() {
        let servers = [server("alpha"), server("beta")];
        // Adding a new server with a taken name.
        assert!(name_collides(&servers, None, "alpha"));
        assert!(!name_collides(&servers, None, "gamma"));
        // Editing keeps its own name.
        assert!(!name_collides(&servers, Some("alpha"), "alpha"));
        // Renaming onto a sibling.
        assert!(name_collides(&servers, Some("alpha"), "beta"));
        assert!(!name_collides(&servers, Some("alpha"), "gamma"));
    }

    #[test]
    fn shortcut_recorder_uses_the_portable_primary_modifier() {
        let source = if cfg!(target_os = "macos") {
            "cmd-shift-p"
        } else {
            "ctrl-shift-p"
        };
        let keystroke = gpui::Keystroke::parse(source).expect("valid test keystroke");
        assert_eq!(portable_keystroke_token(&keystroke), "secondary-shift-p");
    }

    #[test]
    fn shortcut_recorder_keeps_literal_control_on_macos() {
        let keystroke = gpui::Keystroke::parse("ctrl-r").expect("valid test keystroke");
        let expected = if cfg!(target_os = "macos") {
            "ctrl-r"
        } else {
            "secondary-r"
        };
        assert_eq!(portable_keystroke_token(&keystroke), expected);
    }

    #[test]
    fn shortcut_search_matches_each_displayed_field() {
        let row = shortcut_row(
            "project.menu.open",
            "Open Project Menu",
            "Projects",
            Some("RootMenu"),
            "secondary-p",
            Some("secondary-shift-p"),
            false,
        );

        for query in [
            "open project",
            "project.menu",
            "projects",
            "rootmenu",
            "project menu",
            "secondary-p",
            "secondary-shift-p",
        ] {
            assert!(shortcut_row_matches(&row, query), "query {query:?}");
        }
        assert!(!shortcut_row_matches(&row, "transcript"));
        assert!(shortcut_row_matches(
            &shortcut_row(
                "app.quit",
                "Quit",
                "Application",
                None,
                "secondary-q",
                None,
                false
            ),
            "global"
        ));
    }

    #[test]
    fn shortcut_list_cache_rebuilds_after_snapshot_changes() {
        let snapshot = ShortcutSnapshot {
            generation: 1,
            rows: vec![
                shortcut_row(
                    "chat.search",
                    "Search tasks",
                    "Chat",
                    Some("Chat"),
                    "secondary-k",
                    Some("secondary-k"),
                    false,
                ),
                shortcut_row(
                    "composer.send",
                    "Send message",
                    "Composer",
                    Some("TextInput"),
                    "enter",
                    Some("secondary-enter"),
                    true,
                ),
            ],
            last_error: None,
            compatibility_warning: None,
        };
        assert_eq!(
            ShortcutListCache::rebuild(&snapshot, "secondary-enter"),
            ShortcutListCache {
                visible_indices: vec![1],
                modified_count: 1,
            }
        );

        let replacement = ShortcutSnapshot {
            generation: 2,
            rows: vec![
                shortcut_row(
                    "composer.send",
                    "Send message",
                    "Composer",
                    Some("TextInput"),
                    "enter",
                    Some("enter"),
                    false,
                ),
                shortcut_row(
                    "chat.search",
                    "Search tasks",
                    "Chat",
                    Some("Chat"),
                    "secondary-k",
                    Some("secondary-enter"),
                    true,
                ),
            ],
            last_error: None,
            compatibility_warning: None,
        };
        assert_eq!(
            ShortcutListCache::rebuild(&replacement, "secondary-enter"),
            ShortcutListCache {
                visible_indices: vec![1],
                modified_count: 1,
            }
        );
    }
}
