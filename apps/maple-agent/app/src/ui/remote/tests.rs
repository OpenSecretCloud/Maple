use super::*;
use gpui::TestAppContext;

fn screen(cx: &mut TestAppContext) -> Entity<RemoteScreen> {
    cx.executor().allow_parking();
    let backend =
        Arc::new(AgentBackend::for_test("http://127.0.0.1:9".into(), String::new()).unwrap());
    // No status request or timer: drive state transitions without connecting
    // to a developer's host or changing pairing authority.
    cx.new(|cx| RemoteScreen {
        backend,
        status: Some(Status {
            enabled: true,
            endpoint: Some("local".into()),
            peers: Default::default(),
        }),
        devices: None,
        selected_device: None,
        code_input: cx.new(|cx| TextInput::new("Pairing code", cx).pairing_code()),
        names: Default::default(),
        renaming: None,
        local_code: None,
        error: None,
        notice: None,
        busy: None,
        bridges: RefCell::new(Vec::new()),
    })
}

fn device(key: &str) -> RemoteDevice {
    RemoteDevice {
        channel: "test".into(),
        endpoint: key.into(),
        name: key.into(),
        addresses: Default::default(),
        accepting_connections: true,
        window: Some(maple_sdk::remote::RemoteWindow {
            id: uuid::Uuid::new_v4(),
            expires_at: i64::MAX,
        }),
        expires_at: i64::MAX,
    }
}

#[gpui::test]
fn code_entry_requires_selection_and_does_not_follow_a_different_device(cx: &mut TestAppContext) {
    struct Host(Entity<RemoteScreen>);
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(950.)).h(px(850.)).child(self.0.clone())
        }
    }
    let screen = screen(cx);
    let (_, cx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        crate::desktop::register_key_bindings(cx);
        Host(screen.clone())
    });
    cx.simulate_resize(gpui::size(px(950.), px(850.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("pairing-code-entry").is_none());

    screen.update(cx, |screen, cx| {
        screen.devices = Some(vec![device("a"), device("b")]);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("pairing-code-entry").is_none(),
        "discovery alone never opens code entry"
    );
    let choose = cx.debug_bounds("select-a").unwrap();
    cx.simulate_click(choose.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(cx.debug_bounds("pairing-code-entry").is_some());
    screen.update(cx, |screen, cx| {
        assert_eq!(screen.selected_device.as_deref(), Some("a"));
        screen
            .code_input
            .update(cx, |input, cx| input.set_text("ABCD-EFGH", cx));
    });
    cx.run_until_parked();
    let choose = cx.debug_bounds("select-b").unwrap();
    cx.simulate_click(choose.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    screen.update(cx, |screen, cx| {
        assert_eq!(screen.selected_device.as_deref(), Some("b"));
        assert!(screen.code_input.read(cx).text().is_empty());
        screen.pair_selected(cx);
        assert!(
            screen.busy.is_none(),
            "empty submission must not start a pairing attempt"
        );
        assert!(screen.error.is_some());
    });
    cx.run_until_parked();
    let cancel = cx.debug_bounds("cancel-code").unwrap();
    cx.simulate_click(cancel.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(cx.debug_bounds("pairing-code-entry").is_none());
    screen.update(cx, |screen, cx| {
        screen.pair_selected(cx);
        assert!(
            screen.busy.is_none(),
            "no selection must not start a pairing attempt"
        );
    });
}

#[gpui::test]
fn incoming_pairing_closes_the_code_but_an_outgoing_grant_does_not(cx: &mut TestAppContext) {
    let screen = screen(cx);
    screen.update(cx, |screen, cx| {
        screen.local_code = Some((
            "ABCD-EFGH".into(),
            Instant::now() + Duration::from_secs(300),
        ));
        let mut status = screen.status.clone().unwrap();
        status.peers.insert(
            "peer".into(),
            serde_json::from_value(serde_json::json!({
                "name": "Peer", "grant": vec![1; 16], "can_connect": true,
                "controls_this_computer": false, "pending_confirmation": null
            }))
            .unwrap(),
        );
        screen.update_status(status.clone(), cx);
        assert!(
            screen.local_code.is_some(),
            "outgoing access is not a new controller"
        );
        status.peers.get_mut("peer").unwrap().controls_this_computer = true;
        screen.update_status(status, cx);
        assert!(screen.local_code.is_none());
        assert!(screen.notice.as_ref().unwrap().contains("Computer paired"));
        assert!(screen.status.as_ref().unwrap().peers["peer"].controls_this_computer);
    });
}
