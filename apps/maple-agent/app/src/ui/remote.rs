//! Local-only device setup. Codes and account settings never traverse the remote
//! task protocol; a paired computer is selected explicitly before opening tasks.
use crate::{
    backend::AgentBackend,
    remote::manager::{PairingWindow, Status},
    ui::{task, text_input::TextInput, theme, widgets},
};
use gpui::{Context, Div, Entity, EventEmitter, Render, Window, div, prelude::*, px};
use maple_sdk::remote::RemoteDevice;
use std::{
    cell::RefCell,
    sync::Arc,
    time::{Duration, Instant},
};

pub struct ConnectComputer(pub String);
pub struct ComputersChanged(pub Status);
pub struct ComputerNamed {
    pub key: String,
    pub name: String,
}

pub(crate) fn computer_name(key: &str, advertised: &str, alias: &str) -> String {
    if !alias.trim().is_empty() {
        alias.to_string()
    } else if advertised == "Maple computer" {
        format!("Computer {}", key.chars().take(8).collect::<String>())
    } else {
        advertised.to_string()
    }
}

pub struct RemoteScreen {
    backend: Arc<AgentBackend>,
    status: Option<Status>,
    devices: Vec<RemoteDevice>,
    code_input: Entity<TextInput>,
    names: std::collections::HashMap<String, String>,
    renaming: Option<(String, Entity<TextInput>)>,
    local_code: Option<(String, Instant)>,
    error: Option<String>,
    busy: bool,
    bridges: RefCell<Vec<gpui::Task<()>>>,
}
impl EventEmitter<ConnectComputer> for RemoteScreen {}
impl EventEmitter<ComputersChanged> for RemoteScreen {}
impl EventEmitter<ComputerNamed> for RemoteScreen {}

impl RemoteScreen {
    pub fn new(
        backend: Arc<AgentBackend>,
        names: std::collections::HashMap<String, String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut screen = Self {
            backend,
            status: None,
            devices: Vec::new(),
            code_input: cx.new(|cx| TextInput::new("Pairing code (ABCD-EFGH)", cx)),
            names,
            renaming: None,
            local_code: None,
            error: None,
            busy: false,
            bridges: RefCell::new(Vec::new()),
        };
        screen.refresh(false, cx);
        let timer = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this
                    .update(cx, |screen, cx| {
                        let counting = screen.local_code.is_some();
                        if screen
                            .local_code
                            .as_ref()
                            .is_some_and(|(_, deadline)| *deadline <= Instant::now())
                            && let Some((mut code, _)) = screen.local_code.take()
                        {
                            zeroize::Zeroize::zeroize(&mut code);
                        }
                        if counting {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        screen.bridges.borrow_mut().push(timer);
        screen
    }

    fn rename(&mut self, key: String, current: String, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Computer name", cx).on_enter(move |_, _, cx| {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |screen, cx| screen.save_name(cx)).ok();
                });
            });
            input.set_text(&current, cx);
            input
        });
        self.renaming = Some((key, input));
        cx.notify();
    }

    fn save_name(&mut self, cx: &mut Context<Self>) {
        let Some((key, input)) = &self.renaming else {
            return;
        };
        let name = input.read(cx).text().trim().to_string();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            self.error = Some("Use a computer name between 1 and 64 characters.".into());
            cx.notify();
            return;
        }
        let key = key.clone();
        self.names.insert(key.clone(), name.clone());
        self.renaming = None;
        self.error = None;
        let saved_key = key.clone();
        let saved_name = name.clone();
        crate::settings::update_settings_in_background(move |settings| {
            settings
                .remote_sidebars
                .entry(saved_key)
                .or_default()
                .display_name = saved_name;
        });
        cx.emit(ComputerNamed { key, name });
        cx.notify();
    }
    fn refresh(&mut self, discover: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let backend = self.backend.clone();
        let bridge = task::call(
            &self.backend,
            async move {
                let status: Status = backend.remote_setting("remote_status", ()).await?;
                let devices = if discover {
                    backend.remote_setting("remote_discover", ()).await?
                } else {
                    Vec::new()
                };
                Ok((status, devices))
            },
            cx,
            move |screen, result, cx| {
                screen.busy = false;
                match result {
                    Ok((status, devices)) => {
                        cx.emit(ComputersChanged(status.clone()));
                        screen.status = Some(status);
                        if discover {
                            screen.devices = devices;
                        }
                        screen.error = None;
                    }
                    Err(error) => screen.error = Some(error),
                }
                cx.notify();
            },
        );
        task::retain(&self.bridges, bridge);
    }
    fn action(&mut self, method: &'static str, params: serde_json::Value, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let backend = self.backend.clone();
        let bridge = task::call(
            &self.backend,
            async move {
                backend
                    .remote_setting::<serde_json::Value>(method, params)
                    .await
            },
            cx,
            move |screen, result, cx| {
                screen.busy = false;
                match result {
                    Ok(value) => {
                        if method == "remote_begin_pairing" {
                            match serde_json::from_value::<PairingWindow>(value) {
                                Ok(window) => {
                                    screen.local_code = Some((
                                        window.code,
                                        Instant::now()
                                            + Duration::from_secs(window.expires_in.min(300)),
                                    ))
                                }
                                Err(_) => {
                                    screen.error = Some("Could not read the pairing window".into())
                                }
                            }
                        } else if (method == "remote_cancel_pairing" || method == "remote_enable")
                            && let Some((mut code, _)) = screen.local_code.take()
                        {
                            zeroize::Zeroize::zeroize(&mut code);
                        }
                        screen.refresh(false, cx);
                    }
                    Err(error) => screen.error = Some(error),
                }
                cx.notify();
            },
        );
        task::retain(&self.bridges, bridge);
        cx.notify();
    }
}
impl Drop for RemoteScreen {
    fn drop(&mut self) {
        if let Some((code, _)) = &mut self.local_code {
            zeroize::Zeroize::zeroize(code);
        }
    }
}
impl Render for RemoteScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.status.as_ref().is_some_and(|s| s.enabled);
        div().id("remote-devices").w_full().flex().flex_col().gap_4()
            .child(div().text_lg().child("Remote computers"))
            .child(div().text_color(gpui::rgb(theme::text_secondary()))
                .child("Work in another computer’s projects from this window. Tasks use that computer’s tools and files; your account and desktop preferences stay here."))
            .child(div().flex().gap_3().items_center().justify_between()
                .child(div().flex_1().child(if enabled { "This computer accepts paired controllers" } else { "Remote access to this computer is off" }))
                .child(widgets::secondary_button("remote-enable")
                    .aria_label(if enabled { "Turn off remote access" } else { "Enable remote access" }).child(if enabled { "Turn off" } else { "Enable remote access" })
                    .on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_enable", serde_json::json!([!enabled]), cx)))))
            .when(enabled, |view| view.child(div().child(
                widgets::primary_button("pairing-window").aria_label("Pair another computer").child("Pair another computer")
                    .on_click(cx.listener(|screen, _, _, cx| screen.action("remote_begin_pairing", serde_json::json!([]), cx))))))
            .when_some(self.local_code.as_ref(), |view, (code, deadline)| view.child(widgets::card_row().flex().flex_col().gap_3()
                .child(div().text_sm().child("Enter this code on the other computer"))
                .child(pairing_code_boxes(code))
                .child(format!("One attempt · expires in {} seconds. Enter this code only on the computer you want to pair.", deadline.saturating_duration_since(Instant::now()).as_secs()))
                .child(widgets::secondary_button("cancel-window").aria_label("Close pairing window").child("Close pairing window")
                    .on_click(cx.listener(|screen, _, _, cx| screen.action("remote_cancel_pairing", serde_json::json!([]), cx))))))
            .child(div().pt_4().text_lg().child("Paired computers"))
            .when(self.status.as_ref().is_some_and(|s| s.peers.is_empty()), |view| view.child(
                div().text_color(gpui::rgb(theme::text_muted())).child("No paired computers yet.")))
            .children(self.status.iter().flat_map(|s| s.peers.iter()).map(|(key, peer)| {
                let connect = key.clone();
                let revoke = key.clone();
                let confirm = key.clone();
                let rename = key.clone();
                let name = computer_name(key, &peer.name, self.names.get(key).map_or("", String::as_str));
                let editing = self.renaming.as_ref().filter(|(target, _)| target == key);
                div().flex().flex_col().gap_2().p_3().border_1()
                    .border_color(gpui::rgb(theme::border())).rounded(theme::RADIUS_MD)
                    .child(div().flex().gap_3().items_center()
                        .child(div().flex_1().min_w_0().flex().flex_col().gap_1()
                            .child(name.clone())
                            .child(div().text_xs().text_color(gpui::rgb(theme::text_muted()))
                                .child(if peer.can_connect { "Projects available in the sidebar" } else { "Can control this computer" })))
                        .child(widgets::ghost_button(format!("rename-{key}")).aria_label("Rename").child("Rename")
                            .on_click(cx.listener(move |screen, _, _, cx| screen.rename(rename.clone(), name.clone(), cx))))
                        .when(peer.can_connect, |row| row.child(widgets::secondary_button(format!("connect-{key}")).aria_label("Open projects").child("Open projects")
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(ConnectComputer(connect.clone()))))))
                        .child(widgets::ghost_button(format!("revoke-{key}")).aria_label("Unpair").child("Unpair")
                            .on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_revoke", serde_json::json!([revoke]), cx)))))
                    .when_some(editing, |row, (_, input)| row.child(div().flex().gap_2().items_center()
                        .child(div().flex_1().child(input.clone()))
                        .child(widgets::secondary_button(format!("save-name-{key}")).aria_label("Save name").child("Save name")
                            .on_click(cx.listener(|screen, _, _, cx| screen.save_name(cx))))))
                    .when(peer.pending_confirmation.is_some(), |row| row.child(
                        widgets::secondary_button(format!("confirm-{key}")).aria_label("Confirm previous pairing").child("Confirm previous pairing")
                            .on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_reconcile", serde_json::json!([confirm]), cx)))))
            }))
            .child(div().pt_4().flex().gap_3().items_center().justify_between()
                .child(div().text_lg().child("Find another computer"))
                .child(widgets::secondary_button("discover").aria_label("Refresh devices").child("Refresh devices")
                    .on_click(cx.listener(|screen, _, _, cx| screen.refresh(true, cx)))))
            .child(div().text_color(gpui::rgb(theme::text_secondary()))
                .child("Open Remote computers in Settings on the other device and start pairing there. Then refresh and enter its code here."))
            .child(div().flex().flex_col().gap_2()
                .child(div().text_sm().child("Pairing code"))
                .child(widgets::input_frame().w(px(320.)).max_w_full().text_xl()
                    .font_family(crate::assets::FONT_MONO).child(self.code_input.clone()))
                .child(div().text_sm().text_color(gpui::rgb(theme::text_muted()))
                    .child("Type or paste the code from the other computer. The dash is optional.")))
            .children(self.devices.iter().map(|device| {
                let endpoint = device.endpoint.clone();
                div().flex().gap_3().items_center().justify_between()
                    .child(computer_name(&endpoint, &device.name, self.names.get(&endpoint).map_or("", String::as_str)))
                    .when(device.window.is_some(), |row| row.child(widgets::primary_button(format!("pair-{endpoint}")).aria_label("Pair using code").child("Pair using code")
                        .on_click(cx.listener(move |screen, _, _, cx| {
                            let code = screen.code_input.read(cx).text().trim().to_ascii_uppercase();
                            screen.code_input.update(cx, |input, cx| input.set_text("", cx));
                            screen.action("remote_pair", serde_json::json!([endpoint, code]), cx);
                        }))))
            }))
            .when(self.busy, |view| view.child("Connecting…"))
            .when_some(self.error.as_ref(), |view, error| view.child(div().child(error.clone())))
    }
}

fn pairing_code_boxes(code: &str) -> Div {
    div()
        .flex()
        .gap_2()
        .items_center()
        .children(code.chars().map(|character| {
            let cell = div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(44.))
                .text_xl()
                .font_family(crate::assets::FONT_MONO)
                .child(character.to_string());
            if character == '-' {
                cell.w(px(12.)).text_color(gpui::rgb(theme::text_muted()))
            } else {
                cell.w(px(34.))
                    .rounded(theme::RADIUS_MD)
                    .bg(gpui::rgb(theme::bg_input()))
                    .border_1()
                    .border_color(gpui::rgb(theme::border()))
            }
        }))
}
