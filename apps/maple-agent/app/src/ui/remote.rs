//! Local-only device setup. Codes and account settings never traverse the remote
//! task protocol; a paired computer is selected explicitly before opening tasks.
use crate::{
    backend::AgentBackend,
    remote::manager::{PairingWindow, Status},
    ui::{task, text_input::TextInput, theme, widgets},
};
use gpui::{Context, Entity, EventEmitter, Render, Window, div, prelude::*};
use maple_sdk::remote::RemoteDevice;
use std::{
    cell::RefCell,
    sync::Arc,
    time::{Duration, Instant},
};

pub struct ConnectComputer(pub String);
pub struct CloseDevices;
pub struct RemoteEnabled(pub bool);

pub struct RemoteScreen {
    backend: Arc<AgentBackend>,
    status: Option<Status>,
    devices: Vec<RemoteDevice>,
    code_input: Entity<TextInput>,
    local_code: Option<(String, Instant)>,
    error: Option<String>,
    busy: bool,
    bridges: RefCell<Vec<gpui::Task<()>>>,
}
impl EventEmitter<ConnectComputer> for RemoteScreen {}
impl EventEmitter<CloseDevices> for RemoteScreen {}
impl EventEmitter<RemoteEnabled> for RemoteScreen {}

impl RemoteScreen {
    pub fn new(backend: Arc<AgentBackend>, cx: &mut Context<Self>) -> Self {
        let mut screen = Self {
            backend,
            status: None,
            devices: Vec::new(),
            code_input: cx
                .new(|cx| TextInput::new("Eight-character code from the other computer", cx)),
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
                        if screen
                            .local_code
                            .as_ref()
                            .is_some_and(|(_, deadline)| *deadline <= Instant::now())
                            && let Some((mut code, _)) = screen.local_code.take()
                        {
                            zeroize::Zeroize::zeroize(&mut code);
                        }
                        cx.notify();
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
                        cx.emit(RemoteEnabled(status.enabled));
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
        let peers = self
            .status
            .as_ref()
            .map(|s| s.peers.clone())
            .unwrap_or_default();
        div().id("remote-devices").flex_1().min_h_0().overflow_y_scroll().bg(gpui::rgb(theme::bg_app())).p_6().flex().flex_col().gap_4()
            .child(div().flex().items_center().justify_between().child(div().text_xl().child("Computers"))
                .child(widgets::secondary_button("remote-back").child("Back to tasks").on_click(cx.listener(|_, _, _, cx| cx.emit(CloseDevices)))))
            .child(div().child("Remote tasks run on the selected computer, with its tools and files. Account and desktop preferences stay here."))
            .child(div().flex().gap_3().items_center().child(if enabled { "This computer accepts paired controllers" } else { "Remote access to this computer is off" })
                .child(widgets::secondary_button("remote-enable").child(if enabled { "Turn off" } else { "Enable remote access" }).on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_enable", serde_json::json!([!enabled]), cx)))))
            .when(enabled, |view| view.child(div().flex().gap_3()
                .child(widgets::primary_button("pairing-window").child("Pair another computer").on_click(cx.listener(|screen, _, _, cx| screen.action("remote_begin_pairing", serde_json::json!([]), cx))))))
            .when_some(self.local_code.as_ref(), |view, (code, deadline)| view.child(div().flex().flex_col().gap_2()
                .child(div().text_xl().child(code.clone()))
                .child(format!("One attempt · expires in {} seconds. Enter this code only on the computer you want to pair.", deadline.saturating_duration_since(Instant::now()).as_secs()))
                .child(widgets::secondary_button("cancel-window").child("Close pairing window").on_click(cx.listener(|screen, _, _, cx| screen.action("remote_cancel_pairing", serde_json::json!([]), cx))))))
            .child(div().text_lg().child("Paired computers"))
            .children(peers.into_iter().map(|(key, peer)| {
                let connect = key.clone(); let revoke = key.clone(); let confirm = key.clone();
                div().flex().gap_3().items_center().child(format!("{} · {}", peer.name, &key[..key.len().min(10)]))
                    .when(peer.pending_confirmation.is_some(), |row| row.child(widgets::secondary_button(format!("confirm-{key}")).child("Confirm previous pairing").on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_reconcile", serde_json::json!([confirm]), cx)))))
                    .when(peer.can_connect, |row| row.child(widgets::primary_button(format!("connect-{key}")).child("Open tasks").on_click(cx.listener(move |_, _, _, cx| cx.emit(ConnectComputer(connect.clone()))))))
                    .child(widgets::danger_button(format!("revoke-{key}")).child("Unpair").on_click(cx.listener(move |screen, _, _, cx| screen.action("remote_revoke", serde_json::json!([revoke]), cx))))
            }))
            .child(div().flex().gap_3().items_center().child(div().text_lg().child("Find another computer"))
                .child(widgets::secondary_button("discover").child("Refresh devices").on_click(cx.listener(|screen, _, _, cx| screen.refresh(true, cx)))))
            .child(self.code_input.clone())
            .children(self.devices.iter().map(|device| {
                let endpoint = device.endpoint.clone();
                div().flex().gap_3().items_center().child(format!("{} · {}", device.name, &endpoint[..endpoint.len().min(10)]))
                    .when(device.window.is_some(), |row| row.child(widgets::primary_button(format!("pair-{endpoint}")).child("Pair using code").on_click(cx.listener(move |screen, _, _, cx| {
                        let code = screen.code_input.read(cx).text().trim().to_ascii_uppercase();
                        screen.code_input.update(cx, |input, cx| input.set_text("", cx));
                        screen.action("remote_pair", serde_json::json!([endpoint, code]), cx);
                    }))))
            }))
            .when(self.busy, |view| view.child("Connecting…"))
            .when_some(self.error.as_ref(), |view, error| view.child(div().child(error.clone())))
    }
}
