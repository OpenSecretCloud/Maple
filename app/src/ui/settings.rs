//! Settings screen: left navigation with content panes, following Maple's
//! settings layout. Sections: General (defaults), System prompt, MCP
//! servers, Usage, About.

use std::sync::Arc;

use gpui::{App, Context, Div, Entity, EventEmitter, Render, Window, div, prelude::*, px};

use maple_agent::agent::{AgentMcpKeyValue, AgentMcpServer, AgentMcpTransport};

use crate::ui::icons::icon;
use crate::ui::text_input::TextInput;

use crate::backend::AgentBackend;
use crate::settings::{self, AppSettings, UsageSummary};
use crate::ui::theme;
use crate::ui::widgets;

/// Emitted when the user leaves settings.
pub struct SettingsClosed(pub AppSettings);

/// Emitted when the user clicks Sign out in the settings header.
pub struct SignOutRequested;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Prompt,
    Mcp,
    Usage,
    About,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Prompt => "System prompt",
            Self::Mcp => "MCP servers",
            Self::Usage => "Usage",
            Self::About => "About",
        }
    }

    const ALL: [Self; 5] = [
        Self::General,
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

impl SettingsScreen {
    pub fn new(
        backend: Arc<AgentBackend>,
        user_id: String,
        settings: AppSettings,
        section: Section,
        cx: &mut Context<Self>,
    ) -> Self {
        let prompt_text = settings.effective_harness_instructions();
        let prompt_editor = cx.new(|cx| {
            let mut input = TextInput::new("You are …", cx)
                .with_tab_index(1)
                .multiline(16)
                .spell_check();
            input.set_text(&prompt_text, cx);
            input
        });
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
        let mut field = |placeholder: &str, value: &str, index: isize| {
            let value = value.to_string();
            cx.new(|cx| {
                let mut input = TextInput::new(placeholder, cx).with_tab_index(index);
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

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(SettingsClosed(self.settings.clone()));
    }
}

impl Render for SettingsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
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
                            .rounded_md()
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
                div()
                    .id(gpui::SharedString::from(format!(
                        "settings-nav-{}",
                        section.label()
                    )))
                    .px_3()
                    .py_2()
                    .rounded_md()
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
                    .hover(|style| style.bg(gpui::rgb(theme::bg_elevated())).cursor_pointer())
                    .on_click({
                        let section = *section;
                        cx.listener(move |this, _event, _window, cx| {
                            this.section = section;
                            cx.notify();
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
            .overflow_y_scroll();
        match self.section {
            Section::General => {
                pane = pane
                    .child(section_title("Defaults"))
                    .child({
                        let mode = self.settings.default_permission_mode;
                        setting_row(
                            "Default permission mode",
                            mode.note(),
                            mode.label(),
                            cx.listener(|this, _event, _window, cx| {
                                this.toggle_permission_default(cx);
                            }),
                        )
                    })
                    .child(setting_row(
                        "New tasks can use the web",
                        "Offers web_search and open_url to the model. Each task can \
                         switch web access on or off from its composer.",
                        if self.settings.default_web_enabled {
                            "On"
                        } else {
                            "Off"
                        },
                        cx.listener(|this, _event, _window, cx| {
                            this.toggle_web_default(cx);
                        }),
                    ))
                    .child(setting_row(
                        "Appearance",
                        "Follow the system theme, or force dark or light.",
                        self.theme.label(),
                        cx.listener(|this, _event, _window, cx| {
                            this.cycle_theme(cx);
                        }),
                    ))
                    .child(setting_row(
                        "Show tool call details",
                        "Tool cards include their input and output payloads.",
                        if self.settings.tool_details {
                            "On"
                        } else {
                            "Off"
                        },
                        cx.listener(|this, _event, _window, cx| {
                            this.toggle_tool_details(cx);
                        }),
                    ))
                    .child(setting_row(
                        "Desktop notifications",
                        "Notify when a task finishes or needs your input while \
                         the window is not focused.",
                        if self.settings.desktop_notifications {
                            "On"
                        } else {
                            "Off"
                        },
                        cx.listener(|this, _event, _window, cx| {
                            this.toggle_desktop_notifications(cx);
                        }),
                    ))
                    .child(setting_row(
                        "Summarize tool calls",
                        "Completed tool calls with long output get a one-line \
                         summary from the title model on their cards.",
                        if self.settings.tool_summaries {
                            "On"
                        } else {
                            "Off"
                        },
                        cx.listener(|this, _event, _window, cx| {
                            this.toggle_tool_summaries(cx);
                        }),
                    ))
                    .child(section_title("Voice"))
                    .child(setting_row(
                        "Speech voice",
                        "The voice that reads messages aloud. Click to move to \
                         the next voice.",
                        settings::tts_voice_label(&self.settings.tts_voice),
                        cx.listener(|this, _event, _window, cx| {
                            this.cycle_tts_voice(cx);
                        }),
                    ))
                    .child(setting_row(
                        "Speech speed",
                        "How fast messages are read aloud.",
                        &format!("{:.1}×", self.settings.tts_speed),
                        cx.listener(|this, _event, _window, cx| {
                            this.cycle_tts_speed(cx);
                        }),
                    ));
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
                div()
                    .p_3()
                    .rounded_md()
                    .bg(gpui::rgb(theme::bg_input()))
                    .border_1()
                    .border_color(gpui::rgb(theme::border()))
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_primary()))
                    .child(self.prompt_editor.clone()),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(pill_button(
                        "prompt-save".to_string(),
                        "Save",
                        true,
                        cx.listener(|this, _event, _window, cx| this.save_prompt(cx)),
                    ))
                    .child(pill_button(
                        "prompt-reset".to_string(),
                        "Reset to default",
                        false,
                        cx.listener(|this, _event, _window, cx| this.reset_prompt(cx)),
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
                    widgets::primary_button("mcp-add")
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.open_mcp_editor(None, cx);
                        }))
                        .child(icon("plus", widgets::ROW_ICON, theme::bg_app()))
                        .child("Add server"),
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
                    widgets::card_row()
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
                        )
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
                .rounded_md()
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
            .rounded_lg()
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
                            .rounded_md()
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
        .rounded_lg()
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
            theme::bg_app()
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

fn section_title(label: &str) -> Div {
    div()
        .text_lg()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(gpui::rgb(theme::text_primary()))
        .child(label.to_string())
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
                .rounded_md()
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
                .rounded_md()
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
}
