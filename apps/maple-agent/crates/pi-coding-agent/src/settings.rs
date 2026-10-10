use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pi_agent_core::{QueueMode, ToolExecutionMode};
use pi_ai::ThinkingLevel;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::compaction::CompactionSettings;
use crate::tools::resolve_to_cwd;
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
    /// Skill files or folders the settings list, the project's before the user's.
    /// [`Settings::load`] resolves each against its own file's folder.
    pub skills: Vec<PathBuf>,
    /// Prompt template files or folders the settings list, kept as `skills` are.
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

/// The skill and prompt template paths one settings file lists.
#[derive(Default)]
struct ListedPaths {
    skills: Vec<PathBuf>,
    prompts: Vec<PathBuf>,
}

/// `merged` with the settings file at `path` over it, the settings they give, and the
/// paths the file lists, which do not replace the other file's.
fn overlay(
    merged: &Value,
    path: &Path,
    is_project: bool,
) -> io::Result<Option<(Value, Settings, ListedPaths)>> {
    let Some(mut value) = read_json(path)? else {
        return Ok(None);
    };
    let mut listed = ListedPaths::default();
    if let Some(object) = value.as_object_mut() {
        if is_project {
            // A project cannot decide how far it is trusted.
            object.remove("defaultProjectTrust");
        }
        let folder = path.parent().unwrap_or(Path::new(""));
        listed.skills = take_listed(object, "skills", folder)?;
        listed.prompts = take_listed(object, "prompts", folder)?;
    }
    let mut next = merged.clone();
    merge_json(&mut next, value);
    let settings = Settings::deserialize(&next)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Some((next, settings, listed)))
}

/// The paths a settings file lists under `key`, taken out of it and resolved against
/// `folder`, the file's own.
fn take_listed(
    object: &mut Map<String, Value>,
    key: &str,
    folder: &Path,
) -> io::Result<Vec<PathBuf>> {
    let Some(value) = object.remove(key) else {
        return Ok(Vec::new());
    };
    let entries: Vec<String> = serde_json::from_value(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("{key}: {error}")))?;
    Ok(entries
        .iter()
        .map(|entry| resolve_to_cwd(entry.trim(), folder))
        .collect())
}

impl Settings {
    /// Load user settings overlaid with project settings. Missing and empty files are
    /// skipped. A file that cannot be read, does not parse or holds a value of the wrong
    /// type is left out and returned with its error.
    ///
    /// The skills and prompt templates the files list are kept from both, the
    /// project's first, as Pi's resource loader takes them. Each path is resolved as
    /// Pi resolves it: `~` is the home folder, and a relative path is relative to the
    /// folder of the file that lists it.
    pub fn load(user: Option<&Path>, project: Option<&Path>) -> (Self, Vec<SettingsError>) {
        let mut settings = Settings::default();
        let mut merged = serde_json::to_value(&settings).expect("settings serialize");
        let mut errors = Vec::new();
        let mut listed = Vec::new();
        for (path, is_project) in [(user, false), (project, true)] {
            let Some(path) = path else { continue };
            match overlay(&merged, path, is_project) {
                Ok(Some((next, loaded, paths))) => {
                    (merged, settings) = (next, loaded);
                    listed.push(paths);
                }
                Ok(None) => {}
                Err(error) => errors.push(SettingsError {
                    path: path.to_path_buf(),
                    error,
                }),
            }
        }
        for paths in listed.into_iter().rev() {
            settings.skills.extend(paths.skills);
            settings.prompts.extend(paths.prompts);
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
    fn queue_modes_are_spelled_as_in_pi() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.json");
        fs::write(
            &user,
            json!({ "steeringMode": "one-at-a-time", "followUpMode": "all" }).to_string(),
        )
        .unwrap();
        let (settings, errors) = Settings::load(Some(&user), None);
        assert!(errors.is_empty());
        assert_eq!(settings.steering_mode, QueueMode::OneAtATime);
        assert_eq!(settings.follow_up_mode, QueueMode::All);
        let saved = serde_json::to_value(&settings).unwrap();
        assert_eq!(saved["steeringMode"], "one-at-a-time");
        assert_eq!(saved["followUpMode"], "all");
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
    fn listed_paths_resolve_against_their_own_file_and_both_files_count() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("agent/settings.json");
        let project = dir.path().join("repo/.maple/settings.json");
        fs::create_dir_all(user.parent().unwrap()).unwrap();
        fs::create_dir_all(project.parent().unwrap()).unwrap();
        fs::write(
            &user,
            json!({ "skills": ["~/shared-skills", "mine"], "prompts": ["prompts/../more"] })
                .to_string(),
        )
        .unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::write(
            &project,
            json!({ "skills": [" team ", elsewhere] }).to_string(),
        )
        .unwrap();
        let (settings, errors) = Settings::load(Some(&user), Some(&project));
        assert!(errors.is_empty());
        let home = std::env::home_dir().unwrap();
        assert_eq!(
            settings.skills,
            [
                dir.path().join("repo/.maple/team"),
                elsewhere,
                home.join("shared-skills"),
                dir.path().join("agent/mine"),
            ]
        );
        assert_eq!(settings.prompts, [dir.path().join("agent/more")]);

        // A list that is not paths leaves out its file, as any wrong type does.
        fs::write(&project, json!({ "prompts": [5] }).to_string()).unwrap();
        let (settings, errors) = Settings::load(Some(&user), Some(&project));
        assert_eq!(errors.len(), 1);
        assert_eq!(settings.prompts, [dir.path().join("agent/more")]);
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
