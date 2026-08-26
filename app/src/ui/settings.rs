//! Settings screen: left navigation with content panes, following Maple's
//! settings layout. Sections: General (defaults), Usage, About.

use std::sync::Arc;

use gpui::{App, Context, Div, EventEmitter, Render, Window, div, prelude::*};

use crate::backend::AgentBackend;
use crate::settings::{self, AppSettings, UsageSummary};
use crate::ui::theme;

/// Emitted when the user leaves settings.
pub struct SettingsClosed(pub AppSettings);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    Usage,
    About,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Usage => "Usage",
            Self::About => "About",
        }
    }

    const ALL: [Self; 3] = [Self::General, Self::Usage, Self::About];
}

pub struct SettingsScreen {
    backend: Arc<AgentBackend>,
    user_id: String,
    settings: AppSettings,
    section: Section,
    usage: Option<UsageSummary>,
}

pub struct OpenSettings;

impl EventEmitter<SettingsClosed> for SettingsScreen {}

impl SettingsScreen {
    pub fn new(
        backend: Arc<AgentBackend>,
        user_id: String,
        settings: AppSettings,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = Self {
            backend,
            user_id,
            settings,
            section: Section::General,
            usage: None,
        };
        this.load_usage(cx);
        this
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
        settings::save_settings(&self.settings);
        cx.notify();
    }

    fn toggle_tool_details(&mut self, cx: &mut Context<Self>) {
        self.settings.tool_details = !self.settings.tool_details;
        settings::save_settings(&self.settings);
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
                         Each session can still switch modes from its header.",
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
                    ));
            }
            Section::Usage => {
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
