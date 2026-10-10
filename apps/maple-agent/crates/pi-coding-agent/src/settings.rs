use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pi_agent_core::{QueueMode, ToolExecutionMode};
use pi_ai::ThinkingLevel;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::compaction::CompactionSettings;
use crate::trust::DefaultProjectTrust;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetrySettings {
    pub enabled: bool,
    pub max_retries: u32,
    /// The first delay; each retry doubles it.
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            base_delay_ms: 2_000,
            max_delay_ms: 60_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageSettings {
    /// Resize images the `read` tool sends so they fit inline image limits.
    pub auto_resize: bool,
}

impl Default for ImageSettings {
    fn default() -> Self {
        Self { auto_resize: true }
    }
}

/// Session behaviour. User settings are overlaid by project settings, key by key.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub compaction: CompactionSettings,
    pub retry: RetrySettings,
    pub steering_mode: QueueMode,
    pub follow_up_mode: QueueMode,
    pub tool_execution: ToolExecutionMode,
    pub default_thinking_level: Option<ThinkingLevel>,
    /// Extra skill files or folders.
    pub skills: Vec<PathBuf>,
    /// Extra prompt template files or folders.
    pub prompts: Vec<PathBuf>,
    /// Text added to the system prompt, in place of `APPEND_SYSTEM.md`.
    pub append_system_prompt: Option<String>,
    /// The shell `bash` runs instead of the one it finds; a leading `~` is the home
    /// folder.
    pub shell_path: Option<PathBuf>,
    /// Run before every `bash` command, for example `shopt -s expand_aliases`.
    pub shell_command_prefix: Option<String>,
    pub images: ImageSettings,
    /// What to do with a project that has resources of its own and no decision yet.
    /// Only the user's settings can set it.
    pub default_project_trust: DefaultProjectTrust,
}

/// Overlay `overlay` onto `base`: objects merge key by key, anything else replaces.
pub fn merge_json(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

/// A settings file that could not be loaded. Loading goes on without it, as in Pi, so
/// the other file still applies and the host can show the problem.
#[derive(Debug)]
pub struct SettingsError {
    pub path: PathBuf,
    pub error: io::Error,
}

/// A missing or empty file has no settings. A leading byte order mark is skipped.
fn read_json(path: &Path) -> io::Result<Option<Value>> {
    match fs::read_to_string(path) {
        Ok(text) if text.is_empty() => Ok(None),
        Ok(text) => serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text))
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// `merged` with the settings file at `path` over it, and the settings they give.
fn overlay(merged: &Value, path: &Path, is_project: bool) -> io::Result<Option<(Value, Settings)>> {
    let Some(mut value) = read_json(path)? else {
        return Ok(None);
    };
    if is_project && let Some(object) = value.as_object_mut() {
        // A project cannot decide how far it is trusted.
        object.remove("defaultProjectTrust");
    }
    let mut next = merged.clone();
    merge_json(&mut next, value);
    let settings = Settings::deserialize(&next)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Some((next, settings)))
}

impl Settings {
    /// Load user settings overlaid with project settings. Missing and empty files are
    /// skipped. A file that cannot be read, does not parse or holds a value of the wrong
    /// type is left out and returned with its error.
    pub fn load(user: Option<&Path>, project: Option<&Path>) -> (Self, Vec<SettingsError>) {
        let mut settings = Settings::default();
        let mut merged = serde_json::to_value(&settings).expect("settings serialize");
        let mut errors = Vec::new();
        for (path, is_project) in [(user, false), (project, true)] {
            let Some(path) = path else { continue };
            match overlay(&merged, path, is_project) {
                Ok(Some((next, loaded))) => (merged, settings) = (next, loaded),
                Ok(None) => {}
                Err(error) => errors.push(SettingsError {
                    path: path.to_path_buf(),
                    error,
                }),
            }
        }
        (settings, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_settings_overlay_user_settings_key_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.json");
        let project = dir.path().join("project.json");
        fs::write(&user, json!({ "compaction": { "reserveTokens": 1000, "keepRecentTokens": 5 }, "steeringMode": "all" }).to_string()).unwrap();
        fs::write(
            &project,
            json!({ "compaction": { "keepRecentTokens": 9 }, "defaultThinkingLevel": "high" })
                .to_string(),
        )
        .unwrap();

        let (settings, errors) = Settings::load(Some(&user), Some(&project));
        assert!(errors.is_empty());
        assert_eq!(settings.compaction.reserve_tokens, 1000);
        assert_eq!(settings.compaction.keep_recent_tokens, 9);
        assert!(settings.compaction.enabled);
        assert_eq!(settings.steering_mode, QueueMode::All);
        assert_eq!(settings.default_thinking_level, Some(ThinkingLevel::High));
        assert_eq!(settings.retry, RetrySettings::default());
    }

    #[test]
    fn only_the_user_sets_the_default_project_trust() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.json");
        let project = dir.path().join("project.json");
        fs::write(&user, json!({ "defaultProjectTrust": "never" }).to_string()).unwrap();
        fs::write(
            &project,
            json!({ "defaultProjectTrust": "always" }).to_string(),
        )
        .unwrap();
        let (settings, _) = Settings::load(Some(&user), Some(&project));
        assert_eq!(settings.default_project_trust, DefaultProjectTrust::Never);
        let (settings, _) = Settings::load(None, Some(&project));
        assert_eq!(settings.default_project_trust, DefaultProjectTrust::Ask);
    }

    #[test]
    fn missing_and_empty_files_give_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.json");
        fs::write(&empty, "").unwrap();
        let (settings, errors) = Settings::load(Some(&dir.path().join("none.json")), Some(&empty));
        assert_eq!(settings, Settings::default());
        assert!(errors.is_empty());
    }

    #[test]
    fn a_byte_order_mark_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.json");
        let project = dir.path().join("project.json");
        fs::write(
            &user,
            format!("\u{feff}{}", json!({ "steeringMode": "all" })),
        )
        .unwrap();
        fs::write(
            &project,
            format!("\u{feff}{}", json!({ "defaultThinkingLevel": "high" })),
        )
        .unwrap();
        let (settings, errors) = Settings::load(Some(&user), Some(&project));
        assert!(errors.is_empty());
        assert_eq!(settings.steering_mode, QueueMode::All);
        assert_eq!(settings.default_thinking_level, Some(ThinkingLevel::High));
    }

    #[test]
    fn a_broken_file_is_reported_and_the_other_still_applies() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.json");
        let project = dir.path().join("project.json");
        fs::write(&user, "{ invalid user json").unwrap();
        fs::write(&project, "{ invalid project json").unwrap();
        let (settings, errors) = Settings::load(Some(&user), Some(&project));
        assert_eq!(settings, Settings::default());
        let paths: Vec<&Path> = errors.iter().map(|error| error.path.as_path()).collect();
        assert_eq!(paths, [user.as_path(), project.as_path()]);
        assert!(
            errors
                .iter()
                .all(|error| error.error.kind() == io::ErrorKind::InvalidData)
        );

        fs::write(&user, json!({ "steeringMode": "all" }).to_string()).unwrap();
        let (settings, errors) = Settings::load(Some(&user), Some(&project));
        assert_eq!(settings.steering_mode, QueueMode::All);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, project);

        // A value of the wrong type spoils its own file only.
        fs::write(
            &project,
            json!({ "retry": { "maxRetries": "three" } }).to_string(),
        )
        .unwrap();
        let (settings, errors) = Settings::load(Some(&user), Some(&project));
        assert_eq!(settings.steering_mode, QueueMode::All);
        assert_eq!(settings.retry, RetrySettings::default());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, project);
    }
}
