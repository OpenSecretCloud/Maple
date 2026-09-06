//! The Account section: who is signed in, email verification, and
//! sign-out. Later sections (password, deletion) build on this state.

use gpui::{Context, Div, div, prelude::*, px};

use super::{SettingsScreen, SettingsTarget, SignOutRequested, info_row, section_title};
use crate::backend::{MapleAccount, MapleLoginMethod};
use crate::ui::icons::icon;
use crate::ui::theme;
use crate::ui::widgets;

/// Rows in the Account pane that Application Vim can land on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AccountTarget {
    ResendVerification,
    SignOut,
}

pub(super) struct AccountState {
    /// The profile; None until loaded or when loading failed.
    pub(super) info: Option<MapleAccount>,
    pub(super) load_error: Option<String>,
    pub(super) verification_notice: Option<String>,
    pub(super) verification_busy: bool,
}

impl AccountState {
    pub(super) fn new() -> Self {
        Self {
            info: None,
            load_error: None,
            verification_notice: None,
            verification_busy: false,
        }
    }

    /// Whether the Resend control is on screen.
    fn can_resend_verification(&self) -> bool {
        self.info
            .as_ref()
            .is_some_and(|info| info.email.is_some() && !info.email_verified)
    }
}

impl SettingsScreen {
    pub(super) fn load_account(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.account(&user_id).await },
            cx,
            |this, result, cx| {
                match result {
                    Ok(info) => {
                        this.account.info = Some(info);
                        this.account.load_error = None;
                    }
                    Err(message) => this.account.load_error = Some(message),
                }
                if this.settings.application_vim_enabled {
                    this.reconcile_application_vim_target();
                }
                cx.notify();
            },
        );
    }

    pub(super) fn account_targets(&self) -> Vec<SettingsTarget> {
        let mut targets = Vec::new();
        if self.account.can_resend_verification() {
            targets.push(SettingsTarget::Account(AccountTarget::ResendVerification));
        }
        targets.push(SettingsTarget::Account(AccountTarget::SignOut));
        targets
    }

    pub(super) fn activate_account_target(
        &mut self,
        target: AccountTarget,
        cx: &mut Context<Self>,
    ) {
        match target {
            AccountTarget::ResendVerification => self.resend_verification(cx),
            AccountTarget::SignOut => cx.emit(SignOutRequested),
        }
    }

    pub(super) fn resend_verification(&mut self, cx: &mut Context<Self>) {
        if self.account.verification_busy || !self.account.can_resend_verification() {
            return;
        }
        self.account.verification_busy = true;
        self.account.verification_notice = None;
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.request_verification_email(&user_id).await },
            cx,
            |this, result, cx| {
                this.account.verification_busy = false;
                this.account.verification_notice = Some(match result {
                    Ok(()) => "Verification email sent. Check your inbox.".to_string(),
                    Err(message) => message,
                });
                cx.notify();
            },
        );
    }

    pub(super) fn render_account_pane(&self, cx: &mut Context<Self>) -> Div {
        let mut pane = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(section_title("Account"));
        let Some(info) = self.account.info.as_ref() else {
            return pane.child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_faint()))
                    .child(match &self.account.load_error {
                        Some(message) => message.clone(),
                        None => "Loading account…".to_string(),
                    }),
            );
        };

        let is_guest = info.login_method == MapleLoginMethod::Guest;
        pane = pane.child(match &info.email {
            Some(email) => self.render_email_row(email, info.email_verified, cx),
            None if is_guest => guest_id_row(&info.user_id, cx),
            None => info_row("Email", "No email on this account".to_string()).into_any_element(),
        });
        if let Some(notice) = &self.account.verification_notice {
            pane = pane.child(
                widgets::banner(theme::bg_sidebar_pill())
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .child(notice.clone()),
            );
        }
        pane = pane
            .child(info_row(
                "Sign-in method",
                info.login_method.label().to_string(),
            ))
            .child(info_row("Member since", member_since(&info.created_at)))
            .child(
                self.application_target(
                    || SettingsTarget::Account(AccountTarget::SignOut),
                    widgets::card_row()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .child(super::setting_copy(
                            "Sign out",
                            "Ends this session on this device. Your tasks stay in your \
                         account.",
                        ))
                        .child(
                            widgets::secondary_button("account-sign-out")
                                .flex_none()
                                .py_1p5()
                                .on_click(cx.listener(|_this, _event, _window, cx| {
                                    cx.emit(SignOutRequested);
                                }))
                                .child("Sign out"),
                        ),
                ),
            );
        pane
    }

    fn render_email_row(
        &self,
        email: &str,
        verified: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let busy = self.account.verification_busy;
        let mut value = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_primary()))
                    .child(email.to_string()),
            )
            .child(if verified {
                status_badge("Verified", theme::status_success())
            } else {
                status_badge("Unverified", theme::status_warning())
            });
        if !verified {
            value = value.child(
                widgets::secondary_button("account-resend-verification")
                    .flex_none()
                    .py_1p5()
                    .when(busy, |el| el.opacity(0.6))
                    .when(!busy, |el| {
                        el.on_click(cx.listener(|this, _event, _window, cx| {
                            this.resend_verification(cx);
                        }))
                    })
                    .child(if busy { "Sending…" } else { "Resend email" }),
            );
        }
        let row = widgets::card_row()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .child("Email"),
            )
            .child(value);
        if verified {
            row.into_any_element()
        } else {
            self.application_target(
                || SettingsTarget::Account(AccountTarget::ResendVerification),
                row,
            )
        }
    }
}

/// Guests sign in with their account id, so it is the thing to show and
/// copy where an email would be.
fn guest_id_row(user_id: &str, cx: &mut Context<SettingsScreen>) -> gpui::AnyElement {
    widgets::card_row()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(super::setting_copy(
            "Anonymous account",
            "This account has no email. Keep the account id and password \
             somewhere safe: they are the only way to sign in again.",
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_family(crate::assets::FONT_MONO)
                        .text_color(gpui::rgb(theme::text_primary()))
                        .child(user_id.to_string()),
                )
                .child(widgets::copy_button(
                    "account-copy-id",
                    gpui::SharedString::from(user_id.to_string()),
                    None,
                    Some(cx.entity_id()),
                )),
        )
        .into_any_element()
}

fn status_badge(label: &'static str, color: u32) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .px_1p5()
        .py_0p5()
        .rounded(theme::RADIUS_SM)
        .text_xs()
        .text_color(gpui::rgb(color))
        .bg(gpui::rgb(theme::bg_sidebar_pill()))
        .child(icon(
            if color == theme::status_success() {
                "check"
            } else {
                "x"
            },
            px(11.),
            color,
        ))
        .child(label)
}

/// "September 6, 2026" from an RFC 3339 timestamp; the raw text when it
/// does not parse.
pub(super) fn member_since(created_at: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(created_at) {
        Ok(time) => time
            .with_timezone(&chrono::Local)
            .format("%B %-d, %Y")
            .to_string(),
        Err(_) => created_at.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_since_formats_a_date_and_keeps_unparseable_text() {
        let formatted = member_since("2024-03-05T12:00:00Z");
        assert!(formatted.ends_with(", 2024"), "{formatted}");
        assert_eq!(member_since("soon"), "soon");
    }

    #[test]
    fn resend_is_offered_only_for_an_unverified_email() {
        let mut state = AccountState::new();
        assert!(!state.can_resend_verification());
        let mut info = MapleAccount {
            user_id: "u".into(),
            email: Some("a@b.c".into()),
            name: None,
            email_verified: false,
            login_method: MapleLoginMethod::Email,
            created_at: String::new(),
        };
        state.info = Some(info.clone());
        assert!(state.can_resend_verification());
        info.email_verified = true;
        state.info = Some(info.clone());
        assert!(!state.can_resend_verification());
        info.email = None;
        info.email_verified = false;
        state.info = Some(info);
        assert!(!state.can_resend_verification());
    }
}
