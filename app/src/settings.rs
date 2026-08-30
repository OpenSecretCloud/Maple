//! App settings persisted to ~/.config/maple-gpui/settings.json and local
//! usage aggregation read from the goose usage ledger.

// This module is the desktop frontend's boundary. A headless build (no
// `desktop` feature) uses only a few entry points, so the rest is unused
// there by design.
#![cfg_attr(not(feature = "desktop"), allow(dead_code))]

use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppSettings {
    /// Default permission policy for new sessions; see [`PermissionMode`].
    #[serde(default)]
    pub default_permission_mode: PermissionMode,
    /// Whether tool cards show input/output payloads by default.
    #[serde(default = "default_tool_details")]
    pub tool_details: bool,
    /// Whether new tasks can use the web tools.
    #[serde(default = "default_web_enabled")]
    pub default_web_enabled: bool,
    /// Whether completed tool calls get a one-line model summary.
    #[serde(default = "default_tool_summaries")]
    pub tool_summaries: bool,
    #[serde(default)]
    pub pinned_roots: Vec<String>,
    /// Display names for project roots, keyed by absolute path.
    #[serde(default)]
    pub project_names: std::collections::HashMap<String, String>,
    /// Whether run completion, permissions, and questions raise desktop
    /// notifications while the window is not focused.
    #[serde(default = "default_desktop_notifications")]
    pub desktop_notifications: bool,
    /// Opening system prompt text for agents this app hosts. Empty means
    /// [`DEFAULT_HARNESS_INSTRUCTIONS`].
    #[serde(default)]
    pub harness_instructions: String,
    /// Window size and state from the last run.
    #[serde(default)]
    pub window: Option<WindowState>,
    /// Color theme: "system", "dark", or "light".
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Text-to-speech voice id; see [`TTS_VOICES`].
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    /// Text-to-speech speed multiplier; see [`TTS_SPEEDS`].
    #[serde(default = "default_tts_speed")]
    pub tts_speed: f32,
}

/// Voxtral voice ids with their labels, in the order the settings row
/// cycles through them. Mirrors the Maple web app.
pub const TTS_VOICES: [(&str, &str); 20] = [
    ("neutral_female", "Neutral — Female"),
    ("neutral_male", "Neutral — Male"),
    ("casual_female", "Casual — Female"),
    ("casual_male", "Casual — Male"),
    ("cheerful_female", "Cheerful — Female"),
    ("ar_male", "Arabic-accented — Male"),
    ("de_female", "German-accented — Female"),
    ("de_male", "German-accented — Male"),
    ("es_female", "Spanish-accented — Female"),
    ("es_male", "Spanish-accented — Male"),
    ("fr_female", "French-accented — Female"),
    ("fr_male", "French-accented — Male"),
    ("hi_female", "Hindi-accented — Female"),
    ("hi_male", "Hindi-accented — Male"),
    ("it_female", "Italian-accented — Female"),
    ("it_male", "Italian-accented — Male"),
    ("nl_female", "Dutch-accented — Female"),
    ("nl_male", "Dutch-accented — Male"),
    ("pt_female", "Portuguese-accented — Female"),
    ("pt_male", "Portuguese-accented — Male"),
];

/// Speech speeds the settings row cycles through.
pub const TTS_SPEEDS: [f32; 6] = [0.8, 1.0, 1.2, 1.5, 1.8, 2.0];

const DEFAULT_TTS_VOICE: &str = "casual_female";
const DEFAULT_TTS_SPEED: f32 = 1.0;

/// Label for a voice id; the id itself when it is unknown.
pub fn tts_voice_label(voice: &str) -> &str {
    TTS_VOICES
        .iter()
        .find(|(id, _)| *id == voice)
        .map(|(_, label)| *label)
        .unwrap_or(voice)
}

fn default_tts_voice() -> String {
    DEFAULT_TTS_VOICE.to_string()
}

fn default_tts_speed() -> f32 {
    DEFAULT_TTS_SPEED
}

/// Permission policy for a session: whether a gated tool call needs a
/// decision from the user. Modelled on [`crate::ui::theme::Preference`],
/// including the same infallible `parse` so an unknown value on disk
/// degrades to the safer mode instead of failing the whole settings load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionMode {
    /// Confirm each gated tool call.
    #[default]
    SmartApprove,
    /// Approve every tool call without asking.
    Auto,
}

impl PermissionMode {
    /// Anything unknown reads as the safer mode.
    pub fn parse(value: &str) -> Self {
        match value {
            "auto" => Self::Auto,
            _ => Self::SmartApprove,
        }
    }

    /// The mode named by `value`, or `None` when it names no mode. Use
    /// this where an unknown value must fall back to a saved default
    /// rather than to the safer mode.
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "smart_approve" => Some(Self::SmartApprove),
            _ => None,
        }
    }

    /// Icon name: a bolt for allow all, a shield for ask first.
    pub fn icon(self) -> &'static str {
        match self {
            Self::SmartApprove => "shield-check",
            Self::Auto => "zap",
        }
    }

    /// The value written to disk and handed to the agent runtime.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SmartApprove => "smart_approve",
            Self::Auto => "auto",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SmartApprove => "Ask first",
            Self::Auto => "Allow all",
        }
    }

    /// One line of explanation under the label.
    pub fn note(self) -> &'static str {
        match self {
            Self::SmartApprove => "Confirm each gated tool call",
            Self::Auto => "Approve every tool call without asking",
        }
    }

    /// The next choice in the settings cycle.
    pub fn next(self) -> Self {
        match self {
            Self::SmartApprove => Self::Auto,
            Self::Auto => Self::SmartApprove,
        }
    }
}

// Serialized as the bare string it has always been, so settings.json and
// the runtime's mode field keep their format.
impl serde::Serialize for PermissionMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for PermissionMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Ok(Self::parse(&value))
    }
}

/// Persisted window geometry. Position is left to the window manager:
/// Wayland does not expose it, and a stale position can open the window
/// off-screen after a monitor change.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowState {
    /// Keep a saved size inside a sane range so a corrupt file cannot
    /// open a window too small to use.
    pub fn clamped(self) -> Self {
        Self {
            width: self.width.clamp(640., 8192.),
            height: self.height.clamp(480., 8192.),
            maximized: self.maximized,
        }
    }
}

/// Opening system prompt for agents this app hosts: the agent is Maple.
/// The runtime appends its tool and runtime guidance after this text.
pub const DEFAULT_HARNESS_INSTRUCTIONS: &str =
    "You are a general-purpose AI agent called Maple, created by Maple AI.
You run in the Maple app's Agent Mode; users know you simply as Maple.";

impl AppSettings {
    /// The harness instructions to hand the runtime: the saved text, or the
    /// default when nothing is saved.
    pub fn effective_harness_instructions(&self) -> String {
        let saved = self.harness_instructions.trim();
        if saved.is_empty() {
            DEFAULT_HARNESS_INSTRUCTIONS.to_string()
        } else {
            saved.to_string()
        }
    }
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_web_enabled() -> bool {
    true
}

fn default_tool_details() -> bool {
    true
}

fn default_desktop_notifications() -> bool {
    true
}

fn default_tool_summaries() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_permission_mode: PermissionMode::default(),
            tool_details: default_tool_details(),
            default_web_enabled: default_web_enabled(),
            tool_summaries: default_tool_summaries(),
            pinned_roots: Vec::new(),
            project_names: std::collections::HashMap::new(),
            desktop_notifications: default_desktop_notifications(),
            harness_instructions: String::new(),
            window: None,
            theme: default_theme(),
            tts_voice: default_tts_voice(),
            tts_speed: default_tts_speed(),
        }
    }
}

fn settings_file() -> PathBuf {
    crate::backend::app_config_root().join("settings.json")
}

pub fn load_settings() -> AppSettings {
    let path = settings_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("Cannot read settings at {}: {error}", path.display());
            }
            return AppSettings::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|error| {
        log::warn!(
            "Settings at {} are not valid; using defaults: {error}",
            path.display()
        );
        AppSettings::default()
    })
}

/// Record the window state for the next launch. Runs on the UI thread at
/// quit and waits for the write, so updates queued earlier also land.
pub fn save_window_state(state: WindowState) {
    update_settings_and_wait(move |settings| settings.window = Some(state));
}

fn save_settings(settings: &AppSettings) {
    let path = settings_file();
    if let Err(error) = maple_agent::private_file::write_private_json(&path, settings) {
        log::error!("Cannot save settings to {}: {error}", path.display());
    }
}

type SettingsUpdate = Box<dyn FnOnce(&mut AppSettings) + Send>;

/// One queued change and, optionally, a channel to signal once it is on disk.
struct SettingsWrite {
    update: SettingsUpdate,
    done: Option<std::sync::mpsc::Sender<()>>,
}

/// The single writer thread. Every change goes through it in call order
/// as a read-modify-write of the file, so two callers that change
/// different fields both land and a later change to one field always
/// wins over an earlier one.
fn settings_writer() -> &'static std::sync::mpsc::Sender<SettingsWrite> {
    static WRITER: std::sync::OnceLock<std::sync::mpsc::Sender<SettingsWrite>> =
        std::sync::OnceLock::new();
    WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<SettingsWrite>();
        std::thread::Builder::new()
            .name("settings-writer".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    // Coalesce a burst of changes into one write.
                    let mut batch = vec![first];
                    while let Ok(next) = rx.try_recv() {
                        batch.push(next);
                    }
                    let mut settings = load_settings();
                    let mut acks = Vec::new();
                    for write in batch {
                        (write.update)(&mut settings);
                        acks.extend(write.done);
                    }
                    save_settings(&settings);
                    for ack in acks {
                        let _ = ack.send(());
                    }
                }
            })
            .expect("spawn settings writer");
        tx
    })
}

fn queue_settings_write(write: SettingsWrite) {
    if settings_writer().send(write).is_err() {
        log::error!("Settings writer is gone; change not saved");
    }
}

/// Apply `update` to the settings file from the writer thread so a
/// toggle never blocks the UI thread on disk I/O.
pub fn update_settings_in_background(update: impl FnOnce(&mut AppSettings) + Send + 'static) {
    queue_settings_write(SettingsWrite {
        update: Box::new(update),
        done: None,
    });
}

/// Apply `update` and block until it and every earlier update are on disk.
pub fn update_settings_and_wait(update: impl FnOnce(&mut AppSettings) + Send + 'static) {
    let (done, rx) = std::sync::mpsc::channel::<()>();
    queue_settings_write(SettingsWrite {
        update: Box::new(update),
        done: Some(done),
    });
    let _ = rx.recv();
}

/// One aggregated usage row: per session or per model.
#[derive(Debug, Clone, Default)]
pub struct UsageRow {
    pub label: String,
    pub sessions: u64,
    pub turns: u64,
    pub total_tokens: i64,
    pub cost: f64,
}

#[derive(Debug, Clone, Default)]
pub struct UsageSummary {
    pub totals: UsageRow,
    pub by_model: Vec<UsageRow>,
    pub by_session: Vec<UsageRow>,
}

/// Read usage totals from the goose usage ledger for one account scope.
pub fn load_usage(account_scope: &str) -> UsageSummary {
    let db = crate::backend::account_session_db(account_scope);
    let Some(conn) = crate::backend::open_session_db_read_only(&db) else {
        return UsageSummary::default();
    };
    let mut summary = UsageSummary::default();

    if let Ok(mut stmt) = conn.prepare(
        "SELECT COUNT(*), COALESCE(SUM(total_tokens),0), COALESCE(SUM(cost),0) \
         FROM usage_ledger",
    ) && let Ok(row) = stmt.query_row([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, f64>(2)?,
        ))
    }) {
        summary.totals = UsageRow {
            label: "All activity".to_string(),
            sessions: 0,
            turns: row.0.max(0) as u64,
            total_tokens: row.1,
            cost: row.2,
        };
    }

    if let Ok(mut stmt) = conn.prepare(
        "SELECT model, COUNT(DISTINCT session_id), COUNT(*), \
         COALESCE(SUM(total_tokens),0), COALESCE(SUM(cost),0) \
         FROM usage_ledger GROUP BY model ORDER BY SUM(total_tokens) DESC",
    ) && let Ok(rows) = stmt.query_map([], |row| {
        Ok(UsageRow {
            label: row
                .get::<_, Option<String>>(0)?
                .unwrap_or_else(|| "unknown".into()),
            sessions: row.get::<_, i64>(1)?.max(0) as u64,
            turns: row.get::<_, i64>(2)?.max(0) as u64,
            total_tokens: row.get::<_, i64>(3)?,
            cost: row.get::<_, f64>(4)?,
        })
    }) {
        for row in rows.flatten() {
            summary.totals.sessions += row.sessions;
            summary.by_model.push(row);
        }
    }

    if let Ok(mut stmt) = conn.prepare(
        "SELECT s.name, u.session_id, COUNT(*), \
         COALESCE(SUM(u.total_tokens),0), COALESCE(SUM(u.cost),0) \
         FROM usage_ledger u JOIN sessions s ON s.id = u.session_id \
         GROUP BY u.session_id ORDER BY MAX(u.created_timestamp) DESC LIMIT 20",
    ) && let Ok(rows) = stmt.query_map([], |row| {
        Ok(UsageRow {
            label: {
                let name: String = row.get::<_, Option<String>>(0)?.unwrap_or_default();
                let id: String = row.get(1)?;
                if name.trim().is_empty() { id } else { name }
            },
            sessions: 1,
            turns: row.get::<_, i64>(2)?.max(0) as u64,
            total_tokens: row.get::<_, i64>(3)?,
            cost: row.get::<_, f64>(4)?,
        })
    }) {
        for row in rows.flatten() {
            summary.by_session.push(row);
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_mode_round_trips_as_a_string() {
        for mode in [PermissionMode::SmartApprove, PermissionMode::Auto] {
            let json = serde_json::to_string(&mode).expect("serialize");
            assert_eq!(json, format!("\"{}\"", mode.as_str()));
            let back: PermissionMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn unknown_permission_mode_reads_as_the_safer_one() {
        assert_eq!(PermissionMode::parse("chat"), PermissionMode::SmartApprove);
        assert_eq!(PermissionMode::parse(""), PermissionMode::SmartApprove);
        assert_eq!(PermissionMode::default(), PermissionMode::SmartApprove);
    }

    #[test]
    fn permission_mode_cycles_between_both_choices() {
        let start = PermissionMode::default();
        assert_eq!(start.next(), PermissionMode::Auto);
        assert_eq!(start.next().next(), start);
    }

    #[test]
    fn default_settings_keep_the_on_disk_permission_string() {
        let json = serde_json::to_value(AppSettings::default()).expect("serialize");
        assert_eq!(json["default_permission_mode"], "smart_approve");
    }
}
