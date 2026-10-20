//! Stock status-icon backend; no service supervisor or custom native tray code.
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};

pub enum Action {
    Open,
    Quit,
}
pub struct StatusIcon(TrayIcon);
impl StatusIcon {
    pub fn new() -> Result<(Self, tokio::sync::mpsc::Receiver<Action>), String> {
        let menu = Menu::new();
        let open = MenuItem::with_id("maple-open", "Open Maple", true, None);
        let quit = MenuItem::with_id("maple-quit", "Quit Maple", true, None);
        menu.append_items(&[&open, &quit])
            .map_err(|_| "Could not create the Maple tray menu")?;
        let image = image::load_from_memory(include_bytes!("../packaging/maple-agent.png"))
            .map_err(|_| "Could not load the Maple tray icon")?
            .resize(32, 32, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        let icon =
            Icon::from_rgba(image.into_raw(), 32, 32).map_err(|_| "Invalid Maple tray icon")?;
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_tooltip("Maple — remote access off")
            .build()
            .map_err(|_| "The desktop tray is unavailable")?;
        tray.set_visible(false)
            .map_err(|_| "Could not configure the Maple tray")?;
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = match event.id.as_ref() {
                "maple-open" => Action::Open,
                "maple-quit" => Action::Quit,
                _ => return,
            };
            let _ = sender.try_send(action);
        }));
        Ok((Self(tray), receiver))
    }
    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.set_tooltip(Some(if enabled {
            "Maple — remote access on"
        } else {
            "Maple — remote access off"
        }));
        let _ = self.0.set_visible(enabled);
    }
}

/// Creating an SNI item does not mean the desktop actually displays a tray.
#[cfg(target_os = "linux")]
pub async fn usable() -> bool {
    let check = async {
        let connection = zbus::Connection::session().await.ok()?;
        let watcher = zbus::Proxy::new(
            &connection,
            "org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            "org.kde.StatusNotifierWatcher",
        )
        .await
        .ok()?;
        watcher
            .get_property::<bool>("IsStatusNotifierHostRegistered")
            .await
            .ok()
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), check)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}
