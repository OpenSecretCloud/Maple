//! Local-only device setup. Codes and account settings never traverse the remote
//! task protocol; a paired computer is selected explicitly before opening tasks.
use crate::{
    backend::AgentBackend,
    remote::manager::{PairingWindow, Status},
    ui::{task, text_input::TextInput, theme, widgets},
};
use gpui::{Context, Div, Entity, EventEmitter, Focusable, Render, Window, div, prelude::*, px};
use maple_sdk::remote::RemoteDevice;
use std::{
    cell::RefCell,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

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
    devices: Option<Vec<RemoteDevice>>,
    selected_device: Option<String>,
    code_input: Entity<TextInput>,
    names: std::collections::HashMap<String, String>,
    renaming: Option<(String, Entity<TextInput>)>,
    local_code: Option<(String, Instant)>,
    error: Option<String>,
    notice: Option<String>,
    busy: Option<&'static str>,
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
        let weak = cx.entity().downgrade();
        let code_input = cx.new(|cx| {
            TextInput::new("Eight-character pairing code", cx)
                .pairing_code()
                .on_enter(move |_, _, cx| {
                    let weak = weak.clone();
                    cx.defer(move |cx| {
                        weak.update(cx, |screen: &mut Self, cx| screen.pair_selected(cx))
                            .ok();
                    });
                })
        });
        let mut screen = Self {
            backend,
            status: None,
            devices: None,
            selected_device: None,
            code_input,
            names,
            renaming: None,
            local_code: None,
            error: None,
            notice: None,
            busy: None,
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
                            screen.notice = Some(
                                "Pairing code expired. Show a new code when you are ready.".into(),
                            );
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

    fn clear_local_code(&mut self) {
        if let Some((mut code, _)) = self.local_code.take() {
            zeroize::Zeroize::zeroize(&mut code);
        }
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected_device = None;
        self.code_input.update(cx, |input, cx| input.clear(cx));
    }

    /// Fed by the desktop's existing local-status refresh, including pairing
    /// completed by the other computer. No new discovery polling is needed.
    pub fn update_status(&mut self, status: Status, cx: &mut Context<Self>) {
        if self.status.as_ref() == Some(&status) {
            return;
        }
        let paired_controller = self.local_code.is_some()
            && status.peers.iter().any(|(key, peer)| {
                peer.controls_this_computer
                    && self
                        .status
                        .as_ref()
                        .and_then(|old| old.peers.get(key))
                        .is_none_or(|old| !old.controls_this_computer || old.grant != peer.grant)
            });
        if paired_controller || !status.enabled {
            self.clear_local_code();
        }
        if paired_controller {
            self.notice =
                Some("Computer paired. It can now use this computer’s projects and tools.".into());
        }
        if self
            .selected_device
            .as_ref()
            .is_some_and(|key| status.peers.get(key).is_some_and(|peer| peer.can_connect))
        {
            self.clear_selection(cx);
        }
        self.status = Some(status);
        cx.notify();
    }

    fn can_connect(&self, endpoint: &str) -> bool {
        self.status
            .as_ref()
            .and_then(|s| s.peers.get(endpoint))
            .is_some_and(|peer| peer.can_connect)
    }

    fn select_device(&mut self, endpoint: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.clear_selection(cx);
        self.selected_device = Some(endpoint);
        self.error = None;
        self.notice = None;
        window.focus(&self.code_input.focus_handle(cx), cx);
        cx.notify();
    }

    fn pair_selected(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(endpoint) = self.selected_device.clone() else {
            return;
        };
        let code =
            zeroize::Zeroizing::new(self.code_input.read(cx).text().trim().to_ascii_uppercase());
        if code.len() != 8 {
            self.error = Some("Enter all eight characters shown on the selected computer.".into());
            cx.notify();
            return;
        }
        self.code_input.update(cx, |input, cx| input.clear(cx));
        self.action(
            "remote_pair",
            serde_json::json!([endpoint, code.as_str()]),
            cx,
        );
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
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(if discover {
            "Finding computers…"
        } else {
            "Updating remote settings…"
        });
        if discover {
            self.clear_selection(cx);
            self.notice = None;
        }
        self.error = None;
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
                let window: Option<PairingWindow> =
                    backend.remote_setting("remote_pairing_window", ()).await?;
                Ok((status, devices, window))
            },
            cx,
            move |screen, result, cx| {
                screen.busy = None;
                match result {
                    Ok((status, devices, window)) => {
                        screen.clear_local_code();
                        if let Some(window) = window {
                            screen.local_code = Some((
                                window.code,
                                Instant::now() + Duration::from_secs(window.expires_in),
                            ));
                        }
                        cx.emit(ComputersChanged(status.clone()));
                        screen.update_status(status, cx);
                        if discover {
                            screen.devices = Some(devices);
                        }
                        screen.error = None;
                    }
                    Err(error) => screen.error = Some(error),
                }
                cx.notify();
            },
        );
        task::retain(&self.bridges, bridge);
        cx.notify();
    }
    fn action(&mut self, method: &'static str, params: serde_json::Value, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(match method {
            "remote_begin_pairing" => "Preparing pairing code…",
            "remote_pair" | "remote_reconcile" => "Pairing computer…",
            "remote_revoke" => "Unpairing computer…",
            _ => "Updating remote settings…",
        });
        self.error = None;
        self.notice = None;
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
                screen.busy = None;
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
                                    screen.error = Some("Could not read the pairing window".into());
                                    cx.notify();
                                    return;
                                }
                            }
                        } else if method == "remote_cancel_pairing" || method == "remote_enable" {
                            screen.clear_local_code();
                        } else if method == "remote_pair" || method == "remote_reconcile" {
                            screen.clear_selection(cx);
                            screen.notice = Some(
                                "Computer paired. Its projects are now available in the sidebar."
                                    .into(),
                            );
                        } else if method == "remote_revoke" {
                            screen.notice = Some("Computer unpaired.".into());
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
        self.clear_local_code();
    }
}
impl Render for RemoteScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("remote-devices")
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_lg().child("Remote computers"))
            .child(
                div().text_color(gpui::rgb(theme::text_secondary())).child(
                    "Work on another computer, or allow a paired computer to work on this one.",
                ),
            )
            .when_some(self.busy, |view, message| {
                view.child(div().text_sm().child(message))
            })
            .when_some(self.error.as_ref(), |view, error| {
                view.child(
                    widgets::card_row()
                        .border_color(gpui::rgb(theme::status_error()))
                        .child(error.clone()),
                )
            })
            .when_some(self.notice.as_ref(), |view, notice| {
                view.child(widgets::card_row().child(notice.clone()))
            })
            .when_some(
                self.status.as_ref().and_then(|s| s.error.as_ref()),
                |view, error| view.child(widgets::card_row().child(error.clone())),
            )
            .child(self.render_this_computer(cx))
            .child(self.render_discovery(cx))
            .when(
                self.status.as_ref().is_some_and(|s| !s.peers.is_empty()),
                |view| view.child(self.render_paired(cx)),
            )
    }
}

impl RemoteScreen {
    fn render_this_computer(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.status.as_ref().is_some_and(|s| s.enabled);
        widgets::card_row().flex().flex_col().gap_4()
            .child(div().flex().gap_4().items_center().justify_between()
                .child(div().flex_1().flex().flex_col().gap_1()
                    .child(div().text_lg().child("This computer"))
                    .child(div().text_sm().text_color(gpui::rgb(theme::text_secondary()))
                        .child(if self.status.is_none() { "Loading remote access status…" }
                            else if enabled && !self.status.as_ref().is_some_and(|s| s.ready) { "Starting remote access. Waiting for a connection…" }
                            else if enabled { "Remote access is on. Only paired computers can connect." }
                            else { "Remote access is off. Other computers cannot connect here." })))
                .when(self.status.is_some(), |row| row.child(widgets::secondary_button("remote-enable")
                    .aria_label(if enabled { "Turn off remote access" } else { "Enable remote access" })
                    .child(if enabled { "Turn off" } else { "Enable remote access" })
                    .on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_enable", serde_json::json!([!enabled]), cx))))))
            .when(enabled && self.local_code.is_none(), |view| view.child(div().flex().gap_4().items_center().justify_between()
                .child(div().flex_1().text_sm().text_color(gpui::rgb(theme::text_secondary()))
                    .child("To pair a new computer, show a one-time code here and enter it there."))
                .child(widgets::primary_button("pairing-window").aria_label("Show pairing code").child("Show pairing code")
                    .on_click(cx.listener(|screen, _, _, cx| screen.action("remote_begin_pairing", serde_json::json!([]), cx))))))
            .when_some(self.local_code.as_ref(), |view, (code, deadline)| view.child(div().flex().flex_col().gap_3()
                .child(div().text_sm().child("On the other computer, choose Find computers, select this computer, and enter this code."))
                .child(pairing_code_boxes(code))
                .child(div().flex().items_center().gap_4().justify_between()
                    .child(div().text_sm().text_color(gpui::rgb(theme::text_secondary()))
                        .child(format!("One attempt · expires in {}:{:02}",
                            deadline.saturating_duration_since(Instant::now()).as_secs() / 60,
                            deadline.saturating_duration_since(Instant::now()).as_secs() % 60)))
                    .child(widgets::secondary_button("cancel-window").aria_label("Cancel pairing").child("Cancel pairing")
                        .on_click(cx.listener(|screen, _, _, cx| screen.action("remote_cancel_pairing", serde_json::json!([]), cx)))))))
    }

    fn render_discovery(&self, cx: &mut Context<Self>) -> Div {
        let searched = self.devices.is_some();
        let no_devices = self.devices.as_ref().is_some_and(|devices| {
            devices
                .iter()
                .all(|device| self.can_connect(&device.endpoint))
        });
        widgets::card_row().flex().flex_col().gap_3()
            .child(div().flex().items_center().gap_4().justify_between()
                .child(div().text_lg().child("Connect to another computer"))
                .child(widgets::secondary_button("discover")
                    .aria_label(if searched { "Refresh devices" } else { "Find computers" })
                    .child(if searched { "Refresh devices" } else { "Find computers" })
                    .on_click(cx.listener(|screen, _, _, cx| screen.refresh(true, cx)))))
            .child(div().text_sm().text_color(gpui::rgb(theme::text_secondary()))
                .child("Find computers signed into your Maple account. On the other computer, enable remote access and choose Show pairing code."))
            .when(no_devices, |view| view.child(div().text_sm()
                .child("No new computers found. Check the other computer is online and signed in, then refresh.")))
            .children(self.devices.iter().flatten().filter(|device| !self.can_connect(&device.endpoint))
                .map(|device| self.render_device(device, cx)))
    }

    fn render_device(&self, device: &RemoteDevice, cx: &mut Context<Self>) -> Div {
        let endpoint = device.endpoint.clone();
        let selected = self.selected_device.as_ref() == Some(&endpoint);
        let ready = device.window.is_some();
        div()
            .p_3()
            .border_1()
            .border_color(gpui::rgb(theme::border()))
            .rounded(theme::RADIUS_MD)
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .justify_between()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(computer_name(
                                &endpoint,
                                &device.name,
                                self.names.get(&endpoint).map_or("", String::as_str),
                            ))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(gpui::rgb(theme::text_secondary()))
                                    .child(if ready {
                                        "Ready to pair"
                                    } else {
                                        "Show a pairing code on this computer, then refresh."
                                    }),
                            ),
                    )
                    .when(ready && !selected, |row| {
                        row.child(
                            widgets::secondary_button(format!("select-{endpoint}"))
                                .debug_selector({
                                    let endpoint = endpoint.clone();
                                    move || format!("select-{endpoint}")
                                })
                                .aria_label("Enter code")
                                .child("Enter code")
                                .on_click(cx.listener(move |screen, _, window, cx| {
                                    screen.select_device(endpoint.clone(), window, cx)
                                })),
                        )
                    })
                    .when(selected, |row| {
                        row.child(
                            widgets::ghost_button("cancel-code")
                                .debug_selector(|| "cancel-code".to_string())
                                .aria_label("Cancel code entry")
                                .child("Cancel")
                                .on_click(cx.listener(|screen, _, _, cx| {
                                    if screen.busy.is_none() {
                                        screen.clear_selection(cx);
                                        screen.error = None;
                                        cx.notify();
                                    }
                                })),
                        )
                    }),
            )
            .when(selected && ready, |view| {
                view.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_sm().child("Pairing code from this computer"))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .id("pairing-code-entry")
                                        .debug_selector(|| "pairing-code-entry".to_string())
                                        .w(px(384.))
                                        .child(self.code_input.clone()),
                                )
                                .child(
                                    widgets::primary_button("pair-selected")
                                        .aria_label("Pair computer")
                                        .child("Pair computer")
                                        .on_click(
                                            cx.listener(|screen, _, _, cx| {
                                                screen.pair_selected(cx)
                                            }),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(gpui::rgb(theme::text_muted()))
                                .child(
                                    "Type or paste its eight-character code. The dash is optional.",
                                ),
                        ),
                )
            })
    }

    fn render_paired(&self, cx: &mut Context<Self>) -> Div {
        widgets::card_row().flex().flex_col().gap_3()
            .child(div().text_lg().child("Paired computers"))
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
                            .child(div().text_sm().text_color(gpui::rgb(theme::text_secondary()))
                                .child(match (peer.can_connect, peer.controls_this_computer) {
                                    (true, true) => "Can connect in both directions",
                                    (true, false) => "Its projects are available in your sidebar",
                                    (false, true) => "Can use this computer’s projects and tools",
                                    (false, false) => "Pairing needs confirmation",
                                })))
                        .child(widgets::ghost_button(format!("rename-{key}")).aria_label("Rename").child("Rename")
                            .on_click(cx.listener(move |screen, _, _, cx| screen.rename(rename.clone(), name.clone(), cx))))
                        .when(peer.can_connect, |row| row.child(widgets::secondary_button(format!("connect-{key}")).aria_label("Open projects").child("Open projects")
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(ConnectComputer(connect.clone()))))))
                        .child(widgets::ghost_button(format!("revoke-{key}")).aria_label("Unpair").child("Unpair")
                            .on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_revoke", serde_json::json!([revoke]), cx)))))
                    .when_some(editing, |row, (_, input)| row.child(div().flex().gap_2().items_center()
                        .child(widgets::input_frame().flex_1().child(input.clone()))
                        .child(widgets::secondary_button(format!("save-name-{key}")).aria_label("Save name").child("Save name")
                            .on_click(cx.listener(|screen, _, _, cx| screen.save_name(cx))))))
                    .when(peer.pending_confirmation.is_some(), |row| row.child(
                        widgets::secondary_button(format!("confirm-{key}")).aria_label("Confirm previous pairing").child("Confirm previous pairing")
                            .on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_reconcile", serde_json::json!([confirm]), cx)))))
            }))
    }
}

fn pairing_code_boxes(code: &str) -> Div {
    div()
        .flex()
        .gap(px(6.))
        .items_center()
        .children(code.chars().map(|character| {
            let cell = div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(48.))
                .text_xl()
                .font_family(crate::assets::FONT_MONO)
                .child(character.to_string());
            if character == '-' {
                cell.w(px(16.)).text_color(gpui::rgb(theme::text_muted()))
            } else {
                cell.w(px(40.))
                    .rounded(theme::RADIUS_MD)
                    .bg(gpui::rgb(theme::bg_input()))
                    .border_1()
                    .border_color(gpui::rgb(theme::border()))
            }
        }))
}
