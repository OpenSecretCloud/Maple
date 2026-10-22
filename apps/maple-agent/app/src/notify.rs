//! Desktop notifications. gpui posts them through the platform's own
//! notification center, which gives them the app's icon, action buttons,
//! replacement by tag, and click-to-focus. That path needs an app bundle
//! on macOS: `cargo run` is not one, so an unbundled binary falls back to
//! the platform's command-line tool, run on a separate thread so a D-Bus
//! or script launch never blocks the UI thread.

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::io::Write;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, SharedString, SystemNotification, SystemNotificationAction};

/// Whether the running binary can use the platform notification center.
/// macOS delivers notifications only for a bundled app; gpui's path
/// silently drops them otherwise.
pub fn native_available() -> bool {
    if cfg!(target_os = "macos") {
        std::env::current_exe()
            .map(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"))
            .unwrap_or(false)
    } else {
        true
    }
}

/// Show a notification. `tag` identifies it: a later notification with
/// the same tag replaces the earlier one, and a click reports the tag
/// back through [`gpui::App::on_system_notification_response`].
/// `actions` are `(id, label)` buttons where the platform shows them.
pub fn notify(
    cx: &App,
    tag: impl Into<SharedString>,
    title: &str,
    body: &str,
    actions: &[(&str, &str)],
) {
    if native_available() {
        cx.show_system_notification(SystemNotification {
            tag: tag.into(),
            title: title.into(),
            body: body.replace('\n', " ").into(),
            actions: actions
                .iter()
                .map(|(id, label)| SystemNotificationAction {
                    id: SharedString::from(id.to_string()),
                    label: SharedString::from(label.to_string()),
                })
                .collect(),
        });
    } else {
        notify_desktop(title, body);
    }
}

/// Set after the first failed launch so a missing tool logs one warning,
/// not one per notification.
static REPORTED_FAILURE: AtomicBool = AtomicBool::new(false);

/// Show a desktop notification with `title` and `body`. Returns at once;
/// delivery happens on a background thread. Failures are logged once.
pub fn notify_desktop(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.replace('\n', " ");
    std::thread::spawn(move || {
        if let Err(error) = send(&title, &body) {
            if !REPORTED_FAILURE.swap(true, Ordering::Relaxed) {
                log::warn!("desktop notification failed (further failures are silent): {error}");
            } else {
                log::debug!("desktop notification failed: {error}");
            }
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_script(mut command: Command, script: &str) -> Result<(), String> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let write = child
        .stdin
        .take()
        .ok_or("Notification input unavailable")?
        .write_all(script.as_bytes());
    if write.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Could not submit notification".into());
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exited with {status}"))
    }
}

#[cfg(target_os = "linux")]
fn send(title: &str, body: &str) -> Result<(), String> {
    let connection =
        zbus::blocking::Connection::session().map_err(|_| "Notification bus unavailable")?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .map_err(|_| "Notification service unavailable")?;
    let _: u32 = proxy
        .call(
            "Notify",
            &(
                crate::profile::DISPLAY_NAME,
                0u32,
                "",
                title,
                body,
                Vec::<String>::new(),
                std::collections::HashMap::<String, zbus::zvariant::OwnedValue>::new(),
                8000i32,
            ),
        )
        .map_err(|_| "Notification delivery failed")?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn send(title: &str, body: &str) -> Result<(), String> {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        escape_applescript(body),
        escape_applescript(title)
    );
    let mut command = Command::new("osascript");
    command.arg("-");
    run_script(command, &script)
}

#[cfg(target_os = "windows")]
fn send(title: &str, body: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = format!(
        concat!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null;",
            "$xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent(",
            "[Windows.UI.Notifications.ToastTemplateType]::ToastText02);",
            "$text = $xml.GetElementsByTagName('text');",
            "$text.Item(0).AppendChild($xml.CreateTextNode('{}')) | Out-Null;",
            "$text.Item(1).AppendChild($xml.CreateTextNode('{}')) | Out-Null;",
            "$toast = [Windows.UI.Notifications.ToastNotification]::new($xml);",
            "[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Maple').Show($toast)"
        ),
        escape_powershell(title),
        escape_powershell(body)
    );
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", "-"])
        .creation_flags(CREATE_NO_WINDOW);
    run_script(command, &script)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn send(title: &str, _body: &str) -> Result<(), String> {
    log::debug!("desktop notifications are not supported on this platform: {title}");
    Ok(())
}

/// Escape text for an AppleScript double-quoted string literal.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn escape_applescript(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape text for a PowerShell single-quoted string literal.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn escape_powershell(text: &str) -> String {
    text.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_applescript_quotes() {
        assert_eq!(escape_applescript(r#"a "b" \c"#), r#"a \"b\" \\c"#);
    }

    #[test]
    fn escapes_powershell_quotes() {
        assert_eq!(escape_powershell("it's"), "it''s");
    }
}
