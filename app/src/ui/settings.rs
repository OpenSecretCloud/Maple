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

pub struct OpenSettings;

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
                .multiline(16);
            input.set_text(&prompt_text, cx);
            input
        });
        let this = Self {
            backend,
            user_id,
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
        let task = self.backend.spawn(future);
        cx.spawn(async move |this, cx| {
            let result = task
                .await
                .unwrap_or_else(|_| Err("The settings task was cancelled".to_string()));
            this.update(cx, |this, cx| then(this, result, cx)).ok();
        })
        .detach();
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

    fn toggle_permission_default(&mut self, cx: &mut Context<Self>) {
        self.settings.default_permission_mode = if self.settings.default_permission_mode == "auto" {
            "smart_approve".to_string()
        } else {
            "auto".to_string()
        };
        settings::save_settings_in_background(self.settings.clone());
        cx.notify();
    }

    fn toggle_web_default(&mut self, cx: &mut Context<Self>) {
        self.settings.default_web_enabled = !self.settings.default_web_enabled;
        settings::save_settings_in_background(self.settings.clone());
        cx.notify();
    }

    fn toggle_tool_details(&mut self, cx: &mut Context<Self>) {
        self.settings.tool_details = !self.settings.tool_details;
        settings::save_settings_in_background(self.settings.clone());
        cx.notify();
    }

    fn toggle_desktop_notifications(&mut self, cx: &mut Context<Self>) {
        self.settings.desktop_notifications = !self.settings.desktop_notifications;
        settings::save_settings_in_background(self.settings.clone());
        if self.settings.desktop_notifications {
            // Fire a test notification so enabling gives immediate feedback
            // and delivery problems surface right away.
            let enabled_at = chrono::Local::now().format("%H:%M").to_string();
            crate::notify::notify_desktop(
                "Desktop notifications on",
                &format!("You will see alerts like this at {enabled_at}."),
            );
        }
        cx.notify();
    }

    fn toggle_tool_summaries(&mut self, cx: &mut Context<Self>) {
        self.settings.tool_summaries = !self.settings.tool_summaries;
        settings::save_settings_in_background(self.settings.clone());
        cx.notify();
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
        settings::save_settings_in_background(self.settings.clone());
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
            .bg(gpui::rgb(theme::BG_APP))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(gpui::rgb(theme::BORDER))
                    .child(
                        div()
                            .id("settings-back")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                            .hover(|style| {
                                style
                                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                    .cursor_pointer()
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.close(cx);
                            }))
                            .child("← Back"),
                    )
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                            .text_color(gpui::rgb(theme::TEXT_MUTED))
                            .hover(|style| {
                                style
                                    .text_color(gpui::rgb(theme::STATUS_ERROR))
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
            .border_color(gpui::rgb(theme::BORDER))
            .bg(gpui::rgb(theme::BG_SIDEBAR))
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
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_SECONDARY
                    }))
                    .bg(gpui::rgb(if selected {
                        theme::BG_ELEVATED
                    } else {
                        theme::BG_SIDEBAR
                    }))
                    .hover(|style| style.bg(gpui::rgb(theme::BG_ELEVATED)).cursor_pointer())
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
                    .child(setting_row(
                        "Bypass permission prompts by default",
                        "New sessions approve every tool call without asking. \
                         Each session can still switch modes from its composer.",
                        if self.settings.default_permission_mode == "auto" {
                            "On"
                        } else {
                            "Off"
                        },
                        cx.listener(|this, _event, _window, cx| {
                            this.toggle_permission_default(cx);
                        }),
                    ))
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
                                    .text_color(gpui::rgb(theme::ACCENT))
                                    .child(plan.plan_label.clone()),
                            )
                        }),
                );
                pane = pane.child(match &self.plan {
                    Some(plan) => plan_card(plan),
                    None => div()
                        .text_sm()
                        .text_color(gpui::rgb(theme::TEXT_FAINT))
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
                            .text_color(gpui::rgb(theme::TEXT_FAINT))
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
                    .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                    .bg(gpui::rgb(theme::BG_INPUT))
                    .border_1()
                    .border_color(gpui::rgb(theme::BORDER))
                    .text_sm()
                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                    .text_color(gpui::rgb(theme::TEXT_SECONDARY))
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
                    div()
                        .id("mcp-add")
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .bg(gpui::rgb(theme::ACCENT))
                        .text_sm()
                        .text_color(gpui::rgb(theme::BG_APP))
                        .hover(|style| style.bg(gpui::rgb(theme::ACCENT_HOVER)).cursor_pointer())
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.open_mcp_editor(None, cx);
                        }))
                        .child(icon("plus", px(14.), theme::BG_APP))
                        .child("Add server"),
                ),
        );
        pane = pane.child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_MUTED))
                .child(
                    "Servers configured here are available to every task. \
                     Turn them on or off per task from the composer.",
                ),
        );
        if let Some(notice) = &self.mcp_notice {
            pane = pane.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(gpui::rgb(theme::STATUS_WARNING))
                    .text_sm()
                    .text_color(gpui::rgb(theme::BG_APP))
                    .child(notice.clone()),
            );
        }
        if let Some(editor) = &self.mcp_editor {
            pane = pane.child(self.render_mcp_editor(editor, cx));
        }
        match &self.mcp_servers {
            None => {
                pane = pane.child(
                    div()
                        .text_color(gpui::rgb(theme::TEXT_FAINT))
                        .child("Loading MCP servers…"),
                );
            }
            Some(servers) if servers.is_empty() => {
                pane = pane.child(
                    div()
                        .text_sm()
                        .text_color(gpui::rgb(theme::TEXT_FAINT))
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
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_3()
                        .rounded_lg()
                        .bg(gpui::rgb(theme::BG_ELEVATED))
                        .border_1()
                        .border_color(gpui::rgb(theme::BORDER_SUBTLE))
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
                                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                        .child(server.name.clone()),
                                )
                                .when(!server.description.is_empty(), |col| {
                                    col.child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                                            .child(server.description.clone()),
                                    )
                                })
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                            div()
                                .id(gpui::SharedString::from(format!("mcp-edit-{name}")))
                                .size_7()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .hover(|style| {
                                    style
                                        .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
                                        .cursor_pointer()
                                })
                                .on_click(cx.listener({
                                    let server = server.clone();
                                    move |this, _event, _window, cx| {
                                        this.open_mcp_editor(Some(server.clone()), cx)
                                    }
                                }))
                                .child(icon("pencil", px(14.), theme::TEXT_SECONDARY)),
                        )
                        .child(
                            div()
                                .id(gpui::SharedString::from(format!("mcp-remove-{name}")))
                                .size_7()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .hover(|style| {
                                    style
                                        .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
                                        .cursor_pointer()
                                })
                                .on_click(cx.listener({
                                    move |this, _event, _window, cx| {
                                        this.remove_mcp_server(&name, cx)
                                    }
                                }))
                                .child(icon("trash-2", px(14.), theme::STATUS_ERROR)),
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
                        .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                        .child(label),
                )
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(gpui::rgb(theme::BG_INPUT))
                        .border_1()
                        .border_color(gpui::rgb(theme::BORDER))
                        .text_sm()
                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                        .child(input),
                )
                .when(!hint.is_empty(), |col| {
                    col.child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                }))
                .when(active, |el| el.bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER)))
                .hover(|style| style.cursor_pointer())
                .child(label)
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_lg()
            .bg(gpui::rgb(theme::BG_ELEVATED))
            .border_1()
            .border_color(gpui::rgb(theme::BORDER))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                            .child("Transport"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .p_1()
                            .rounded_md()
                            .bg(gpui::rgb(theme::BG_SIDEBAR_CHROME))
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
                        div()
                            .id("mcp-save")
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .bg(gpui::rgb(theme::ACCENT))
                            .text_sm()
                            .text_color(gpui::rgb(theme::BG_APP))
                            .when(self.mcp_saving, |el| el.opacity(0.5))
                            .hover(|style| {
                                style.bg(gpui::rgb(theme::ACCENT_HOVER)).cursor_pointer()
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.submit_mcp_editor(cx);
                            }))
                            .child("Save"),
                    )
                    .child(
                        div()
                            .id("mcp-cancel")
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                            .hover(|style| {
                                style
                                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                    .cursor_pointer()
                            })
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
        .bg(gpui::rgb(theme::BG_SIDEBAR_CARD))
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_base()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                        .whitespace_nowrap()
                        .child(format!("{}% used", plan.percent_used)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                .bg(gpui::rgb(theme::BG_SIDEBAR_PILL))
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(fraction.clamp(0., 1.)))
                        .rounded_full()
                        .bg(gpui::rgb(theme::ACCENT)),
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
            theme::ACCENT
        } else {
            theme::BG_SIDEBAR_CARD
        }))
        .text_color(gpui::rgb(if on {
            theme::BG_APP
        } else {
            theme::TEXT_SECONDARY
        }))
        .hover(|style| style.cursor_pointer())
        .on_click(handler)
        .child(label)
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
        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
        .child(label.to_string())
}

fn setting_row(
    title: &str,
    description: &str,
    value: &str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(gpui::SharedString::from(format!(
            "setting-{}",
            title.to_lowercase()
        )))
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .p_4()
        .rounded_lg()
        .bg(gpui::rgb(theme::BG_ELEVATED))
        .border_1()
        .border_color(gpui::rgb(theme::BORDER_SUBTLE))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                .bg(gpui::rgb(theme::BG_INPUT))
                .border_1()
                .border_color(gpui::rgb(theme::BORDER))
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .hover(|style| style.cursor_pointer())
                .on_click(on_click)
                .child(value.to_string()),
        )
}

fn info_row(label: &str, value: String) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .p_4()
        .rounded_lg()
        .bg(gpui::rgb(theme::BG_ELEVATED))
        .border_1()
        .border_color(gpui::rgb(theme::BORDER_SUBTLE))
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                .text_color(gpui::rgb(theme::TEXT_MUTED))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                .child(value),
        )
}

fn usage_table(title: &str, rows: &[crate::settings::UsageRow]) -> Div {
    let mut table = div().flex().flex_col().gap_2().child(
        div()
            .text_sm()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                .bg(gpui::rgb(theme::BG_ELEVATED))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                        .line_clamp(1)
                        .child(row.label.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                        .child(format!("{} turns", row.turns)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                        .child(format_tokens(row.total_tokens)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::TEXT_MUTED))
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
