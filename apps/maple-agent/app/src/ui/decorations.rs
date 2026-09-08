//! Who draws the window title bar.
//!
//! macOS, Windows, and X11 window managers draw one. Wayland compositors
//! only draw one if they support the `xdg-decoration` protocol; GNOME
//! (mutter) does not, and leaves every window undecorated. The app draws
//! its own bar (`super::titlebar`) exactly when the system draws none.

use gpui::{Decorations, Window};

/// True when the window already has a system title bar.
pub fn system_draws_titlebar(window: &Window) -> bool {
    #[cfg(target_os = "linux")]
    if gpui::guess_compositor() == "Wayland" && !wayland::has_decoration_manager() {
        // gpui reports `Server` whenever server-side decorations were
        // requested, even on a compositor that never answered the request.
        // On GNOME that answer never comes, so ask the compositor instead.
        return false;
    }

    matches!(window.window_decorations(), Decorations::Server)
}

#[cfg(target_os = "linux")]
mod wayland {
    use std::sync::OnceLock;

    use wayland_client::protocol::wl_registry;
    use wayland_client::{Connection, Dispatch, QueueHandle};

    /// Whether the compositor advertises `zxdg_decoration_manager_v1`, which
    /// it does only if it can draw the decorations itself. The answer cannot
    /// change while the app runs, so it is probed once.
    pub fn has_decoration_manager() -> bool {
        static FOUND: OnceLock<bool> = OnceLock::new();
        // A failed probe reports "no manager", which leaves the app drawing
        // its own bar. A spare title bar is a far smaller fault than none.
        *FOUND.get_or_init(|| {
            let found = probe().unwrap_or(false);
            log::debug!("wayland compositor draws window decorations: {found}");
            found
        })
    }

    fn probe() -> Option<bool> {
        let connection = Connection::connect_to_env().ok()?;
        let mut queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        let mut globals = Globals { found: false };
        queue.roundtrip(&mut globals).ok()?;
        Some(globals.found)
    }

    struct Globals {
        found: bool,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
        fn event(
            state: &mut Self,
            _registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _data: &(),
            _connection: &Connection,
            _queue: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global { interface, .. } = event
                && interface == "zxdg_decoration_manager_v1"
            {
                state.found = true;
            }
        }
    }
}
