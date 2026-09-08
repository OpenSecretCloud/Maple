use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const UPDATER_CONFIG_FILE: &str = "updater_config.json";
static UPDATER_PREFERENCES_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));
pub type UpdaterPreferencesGuard = tokio::sync::MutexGuard<'static, ()>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdaterPreferences {
    #[serde(default = "automatic_updates_default")]
    pub automatic_updates: bool,
}

const fn automatic_updates_default() -> bool {
    true
}

impl Default for UpdaterPreferences {
    fn default() -> Self {
        Self {
            automatic_updates: automatic_updates_default(),
        }
    }
}

async fn updater_config_path(app_handle: &AppHandle) -> Result<PathBuf> {
    let app_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|error| anyhow!("Failed to resolve app config dir: {error}"))?;
    tokio::fs::create_dir_all(&app_dir).await?;
    Ok(app_dir.join(UPDATER_CONFIG_FILE))
}

async fn load_from_path(path: &Path) -> Result<UpdaterPreferences> {
    let json = match tokio::fs::read_to_string(path).await {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UpdaterPreferences::default());
        }
        Err(error) => return Err(error.into()),
    };

    Ok(serde_json::from_str(&json)?)
}

async fn save_to_path(path: &Path, preferences: UpdaterPreferences) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Updater config path has no parent directory"))?;
    tokio::fs::create_dir_all(parent).await?;

    let json = serde_json::to_vec_pretty(&preferences)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }

    temporary.write_all(&json)?;
    temporary.as_file_mut().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| anyhow::Error::new(error.error))?;

    #[cfg(unix)]
    {
        finish_committed_save(
            std::fs::File::open(parent).and_then(|directory| directory.sync_all()),
        )?;
    }

    Ok(())
}

/// Once the atomic replacement succeeds, callers must observe the new value.
/// A directory fsync failure weakens crash durability but cannot be reported as
/// an uncommitted preference without making Settings disagree with the updater.
#[cfg(unix)]
fn finish_committed_save(directory_sync: std::io::Result<()>) -> Result<()> {
    if let Err(error) = directory_sync {
        log::warn!(
            "Updater preferences were committed, but the config directory could not be synced: {error}"
        );
    }
    Ok(())
}

pub async fn load(app_handle: &AppHandle) -> Result<UpdaterPreferences> {
    let path = updater_config_path(app_handle).await?;
    let (preferences, _guard) = load_and_lock_from_path(&path).await?;
    Ok(preferences)
}

/// Load the preference while retaining the same lock used by saves. Automatic
/// update actions hold this guard through their final install/stage boundary,
/// so a successful save(false) is ordered after any action already in flight
/// and before every later automatic action.
pub async fn load_and_lock(
    app_handle: &AppHandle,
) -> Result<(UpdaterPreferences, UpdaterPreferencesGuard)> {
    let path = updater_config_path(app_handle).await?;
    load_and_lock_from_path(&path).await
}

async fn load_and_lock_from_path(
    path: &Path,
) -> Result<(UpdaterPreferences, UpdaterPreferencesGuard)> {
    let guard = UPDATER_PREFERENCES_LOCK.lock().await;
    let preferences = load_from_path(path).await?;
    Ok((preferences, guard))
}

async fn save_with_lock_to_path(path: &Path, preferences: UpdaterPreferences) -> Result<()> {
    let _guard = UPDATER_PREFERENCES_LOCK.lock().await;
    save_to_path(path, preferences).await
}

#[tauri::command]
pub async fn load_updater_preferences(app_handle: AppHandle) -> Result<UpdaterPreferences, String> {
    load(&app_handle)
        .await
        .map_err(|error| format!("Failed to load updater preferences: {error}"))
}

#[tauri::command]
pub async fn save_updater_preferences(
    app_handle: AppHandle,
    preferences: UpdaterPreferences,
) -> Result<(), String> {
    let path = updater_config_path(&app_handle)
        .await
        .map_err(|error| format!("Failed to locate updater preferences: {error}"))?;
    save_with_lock_to_path(&path, preferences)
        .await
        .map_err(|error| format!("Failed to save updater preferences: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_preferences_keep_existing_automatic_behavior() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(UPDATER_CONFIG_FILE);

        assert_eq!(
            load_from_path(&path).await.unwrap(),
            UpdaterPreferences {
                automatic_updates: true
            }
        );
    }

    #[tokio::test]
    async fn older_config_without_the_preference_defaults_to_enabled() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(UPDATER_CONFIG_FILE);
        tokio::fs::write(&path, "{}").await.unwrap();

        assert!(load_from_path(&path).await.unwrap().automatic_updates);
    }

    #[tokio::test]
    async fn disabled_preference_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("nested").join(UPDATER_CONFIG_FILE);
        let preferences = UpdaterPreferences {
            automatic_updates: false,
        };

        save_to_path(&path, UpdaterPreferences::default())
            .await
            .unwrap();
        save_to_path(&path, preferences).await.unwrap();

        assert_eq!(load_from_path(&path).await.unwrap(), preferences);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                tokio::fs::metadata(&path)
                    .await
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn invalid_preferences_are_not_treated_as_consent() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(UPDATER_CONFIG_FILE);
        tokio::fs::write(&path, "not-json").await.unwrap();

        assert!(load_from_path(&path).await.is_err());
    }

    #[tokio::test]
    async fn disabling_waits_for_an_automatic_action_guard() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(UPDATER_CONFIG_FILE);
        save_to_path(&path, UpdaterPreferences::default())
            .await
            .unwrap();

        let (preferences, automatic_action_guard) = load_and_lock_from_path(&path).await.unwrap();
        assert!(preferences.automatic_updates);

        let save_started = Arc::new(tokio::sync::Notify::new());
        let save_completed = Arc::new(AtomicBool::new(false));
        let task_path = path.clone();
        let task_started = Arc::clone(&save_started);
        let task_completed = Arc::clone(&save_completed);
        let disable_task = tokio::spawn(async move {
            task_started.notify_one();
            save_with_lock_to_path(
                &task_path,
                UpdaterPreferences {
                    automatic_updates: false,
                },
            )
            .await
            .unwrap();
            task_completed.store(true, Ordering::SeqCst);
        });

        save_started.notified().await;
        tokio::task::yield_now().await;
        assert!(!save_completed.load(Ordering::SeqCst));

        // This is the linearization point used by the updater: work that was
        // already authorized finishes before save(false) can report success.
        drop(automatic_action_guard);
        disable_task.await.unwrap();

        assert!(save_completed.load(Ordering::SeqCst));
        assert!(!load_from_path(&path).await.unwrap().automatic_updates);
    }

    #[cfg(unix)]
    #[test]
    fn a_post_commit_directory_sync_failure_is_still_reported_as_saved() {
        let sync_failure = std::io::Error::other("simulated directory sync failure");
        assert!(finish_committed_save(Err(sync_failure)).is_ok());
    }
}
