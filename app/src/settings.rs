//! App settings persisted to ~/.config/maple-gpui/settings.json and local
//! usage aggregation read from the goose usage ledger.

use rusqlite::Connection;
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppSettings {
    /// Default permission policy for new sessions: "smart_approve" or
    /// "auto" (bypass).
    #[serde(default = "default_permission_mode")]
    pub default_permission_mode: String,
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
    /// Whether run completion, permissions, and questions raise desktop
    /// notifications while the window is not focused.
    #[serde(default = "default_desktop_notifications")]
    pub desktop_notifications: bool,
}

fn default_web_enabled() -> bool {
    true
}

fn default_permission_mode() -> String {
    "smart_approve".to_string()
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
            default_permission_mode: default_permission_mode(),
            tool_details: default_tool_details(),
            default_web_enabled: default_web_enabled(),
            tool_summaries: default_tool_summaries(),
            pinned_roots: Vec::new(),
            desktop_notifications: default_desktop_notifications(),
        }
    }
}

fn settings_file() -> PathBuf {
    crate::backend::app_config_root().join("settings.json")
}

pub fn load_settings() -> AppSettings {
    std::fs::read_to_string(settings_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &AppSettings) {
    let path = settings_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(&path, text);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

/// Write settings from a background thread so a toggle never blocks the UI
/// thread on disk I/O. Writes are sequenced: a later snapshot always wins
/// over an earlier one that finishes late.
pub fn save_settings_in_background(settings: AppSettings) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    static LAST_WRITTEN: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::thread::spawn(move || {
        let mut last_written = LAST_WRITTEN
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if sequence < *last_written {
            return;
        }
        *last_written = sequence;
        save_settings(&settings);
    });
}

/// One aggregated usage row: per session or per model.
#[derive(Debug, Clone, Default)]
pub struct UsageRow {
    pub label: String,
    pub sessions: u64,
    pub turns: u64,
    pub input_tokens: i64,
    pub output_tokens: i64,
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
    let Ok(mut conn) = Connection::open(&db) else {
        return UsageSummary::default();
    };
    let mut summary = UsageSummary::default();

    if let Ok(mut stmt) = conn.prepare(
        "SELECT COUNT(*), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), \
         COALESCE(SUM(total_tokens),0), COALESCE(SUM(cost),0) FROM usage_ledger",
    ) {
        if let Ok(row) = stmt.query_row([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, f64>(4)?,
            ))
        }) {
            summary.totals = UsageRow {
                label: "All activity".to_string(),
                sessions: 0,
                turns: row.0.max(0) as u64,
                input_tokens: row.1,
                output_tokens: row.2,
                total_tokens: row.3,
                cost: row.4,
            };
        }
    }

    if let Ok(mut stmt) = conn.prepare(
        "SELECT model, COUNT(DISTINCT session_id), COUNT(*), \
         COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), \
         COALESCE(SUM(total_tokens),0), COALESCE(SUM(cost),0) \
         FROM usage_ledger GROUP BY model ORDER BY SUM(total_tokens) DESC",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(UsageRow {
                label: row
                    .get::<_, Option<String>>(0)?
                    .unwrap_or_else(|| "unknown".into()),
                sessions: row.get::<_, i64>(1)?.max(0) as u64,
                turns: row.get::<_, i64>(2)?.max(0) as u64,
                input_tokens: row.get::<_, i64>(3)?,
                output_tokens: row.get::<_, i64>(4)?,
                total_tokens: row.get::<_, i64>(5)?,
                cost: row.get::<_, f64>(6)?,
            })
        }) {
            for row in rows.flatten() {
                summary.totals.sessions += row.sessions;
                summary.by_model.push(row);
            }
        }
    }

    if let Ok(mut stmt) = conn.prepare(
        "SELECT s.name, u.session_id, COUNT(*), \
         COALESCE(SUM(u.input_tokens),0), COALESCE(SUM(u.output_tokens),0), \
         COALESCE(SUM(u.total_tokens),0), COALESCE(SUM(u.cost),0) \
         FROM usage_ledger u JOIN sessions s ON s.id = u.session_id \
         GROUP BY u.session_id ORDER BY MAX(u.created_timestamp) DESC LIMIT 20",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(UsageRow {
                label: {
                    let name: String = row.get::<_, Option<String>>(0)?.unwrap_or_default();
                    let id: String = row.get(1)?;
                    if name.trim().is_empty() { id } else { name }
                },
                sessions: 1,
                turns: row.get::<_, i64>(2)?.max(0) as u64,
                input_tokens: row.get::<_, i64>(3)?,
                output_tokens: row.get::<_, i64>(4)?,
                total_tokens: row.get::<_, i64>(5)?,
                cost: row.get::<_, f64>(6)?,
            })
        }) {
            for row in rows.flatten() {
                summary.by_session.push(row);
            }
        }
    }

    summary
}
