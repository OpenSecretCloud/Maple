use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use tauri::AppHandle;
#[cfg(desktop)]
use tauri::Manager;

const TTS_MODELS_DIR_NAME: &str = "tts_models";
const TTS_MODELS_DIR_ENV: &str = "MAPLE_TTS_MODELS_DIR";
const MAPLE_BUNDLE_ID: &str = "cloud.opensecret.maple";

pub fn schedule(app: &AppHandle) {
    let roots = configured_roots(app);
    if roots.is_empty() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let cleanup = tokio::task::spawn_blocking(move || {
            roots
                .into_iter()
                .map(|root| {
                    let result = remove_root(&root);
                    (root, result)
                })
                .collect::<Vec<_>>()
        })
        .await;

        match cleanup {
            Ok(results) => {
                for (root, result) in results {
                    match result {
                        Ok(true) => {
                            log::info!("Removed legacy local TTS models from {}", root.display())
                        }
                        Ok(false) => {}
                        Err(error) => log::warn!(
                            "Could not remove legacy local TTS models from {}: {error}",
                            root.display()
                        ),
                    }
                }
            }
            Err(error) => {
                log::warn!("Legacy local TTS model cleanup task stopped unexpectedly: {error}")
            }
        }
    });
}

fn configured_roots(app: &AppHandle) -> Vec<PathBuf> {
    let override_root = std::env::var_os(TTS_MODELS_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    // The application handle is used only by the desktop default-path branch.
    #[cfg(target_os = "ios")]
    let _ = app;

    select_roots(override_root, || {
        let mut candidates = Vec::new();

        #[cfg(desktop)]
        match app.path().local_data_dir() {
            Ok(local_data) => candidates.push(("desktop local data", desktop_root(&local_data))),
            Err(error) => {
                log::warn!(
                    "Could not resolve Maple's local data directory for TTS cleanup: {error}"
                )
            }
        }

        #[cfg(target_os = "ios")]
        match std::env::var_os("HOME").filter(|value| !value.is_empty()) {
            Some(home) => {
                for root in ios_roots(Path::new(&home)) {
                    candidates.push(("iOS application home", root));
                }
            }
            None => log::warn!("Could not resolve the iOS application home for TTS cleanup"),
        }

        candidates
    })
}

fn select_roots<F>(override_root: Option<PathBuf>, defaults: F) -> Vec<PathBuf>
where
    F: FnOnce() -> Vec<(&'static str, PathBuf)>,
{
    match override_root {
        Some(root) => validated_roots([(TTS_MODELS_DIR_ENV, root)]),
        None => validated_roots(defaults()),
    }
}

#[cfg(any(desktop, test))]
fn desktop_root(local_data: &Path) -> PathBuf {
    local_data.join(MAPLE_BUNDLE_ID).join(TTS_MODELS_DIR_NAME)
}

#[cfg(any(target_os = "ios", test))]
fn ios_roots(home: &Path) -> [PathBuf; 2] {
    [
        home.join("Library")
            .join("Caches")
            .join(MAPLE_BUNDLE_ID)
            .join(TTS_MODELS_DIR_NAME),
        home.join("Documents").join(TTS_MODELS_DIR_NAME),
    ]
}

fn validated_roots<I, S>(candidates: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = (S, PathBuf)>,
    S: AsRef<str>,
{
    let mut roots = Vec::new();
    for (source, root) in candidates {
        match validate_root(&root) {
            Ok(()) => {
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
            Err(error) => log::warn!(
                "Ignoring unsafe legacy TTS model path from {}: {error}",
                source.as_ref()
            ),
        }
    }
    roots
}

fn validate_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("path must be absolute".to_string());
    }
    if root
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err("path must not contain parent-directory components".to_string());
    }
    if root.file_name() != Some(OsStr::new(TTS_MODELS_DIR_NAME)) {
        return Err(format!("path must end in {TTS_MODELS_DIR_NAME}"));
    }

    let parent = root
        .parent()
        .ok_or_else(|| "path has no parent directory".to_string())?;
    if parent.parent().is_none() {
        return Err("path is too broad".to_string());
    }

    Ok(())
}

fn remove_root(root: &Path) -> Result<bool, String> {
    validate_root(root)?;

    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("could not inspect path: {error}")),
    };

    if metadata.file_type().is_symlink() {
        return Err("refusing to remove a symlink path".to_string());
    }
    if !metadata.is_dir() {
        return Err("path exists but is not a directory".to_string());
    }

    match fs::remove_dir_all(root) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("directory removal failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_root_uses_the_historical_fixed_bundle_directory() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let local_data = temp.path().join("local-data");
        let alternate_app_data = local_data.join("cloud.opensecret.maple.workspace");

        assert_eq!(
            desktop_root(&local_data),
            local_data.join(MAPLE_BUNDLE_ID).join(TTS_MODELS_DIR_NAME)
        );
        assert_ne!(
            desktop_root(&local_data),
            alternate_app_data.join(TTS_MODELS_DIR_NAME)
        );
    }

    #[test]
    fn ios_roots_cover_current_cache_and_legacy_documents_locations() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let home = temp.path().join("application-home");

        assert_eq!(
            ios_roots(&home),
            [
                home.join("Library")
                    .join("Caches")
                    .join(MAPLE_BUNDLE_ID)
                    .join(TTS_MODELS_DIR_NAME),
                home.join("Documents").join(TTS_MODELS_DIR_NAME),
            ]
        );
    }

    #[test]
    fn configured_override_is_kept_and_duplicate_roots_are_removed() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let override_root = temp.path().join("custom").join(TTS_MODELS_DIR_NAME);

        let roots = validated_roots([
            (TTS_MODELS_DIR_ENV, override_root.clone()),
            ("duplicate", override_root.clone()),
        ]);

        assert_eq!(roots, vec![override_root]);
    }

    #[test]
    fn configured_override_takes_precedence_over_platform_defaults() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let override_root = temp.path().join("custom").join(TTS_MODELS_DIR_NAME);

        let roots = select_roots(Some(override_root.clone()), || {
            panic!("platform defaults must not be resolved when an override is set")
        });

        assert_eq!(roots, vec![override_root]);
    }

    #[test]
    fn validation_rejects_relative_broad_parent_and_wrong_leaf_paths() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let broad = PathBuf::from(format!(
            "{}{}",
            std::path::MAIN_SEPARATOR,
            TTS_MODELS_DIR_NAME
        ));

        assert!(validate_root(Path::new(TTS_MODELS_DIR_NAME)).is_err());
        assert!(validate_root(&broad).is_err());
        assert!(validate_root(&temp.path().join("voice_models")).is_err());
        assert!(validate_root(
            &temp
                .path()
                .join("nested")
                .join("..")
                .join(TTS_MODELS_DIR_NAME)
        )
        .is_err());
    }

    #[test]
    fn cleanup_removes_only_the_validated_tts_models_directory() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let local_data = temp.path().join("local-data");
        let app_data = local_data.join(MAPLE_BUNDLE_ID);
        let root = desktop_root(&local_data);
        let sibling = app_data.join("keep");
        fs::create_dir_all(root.join("partial-download")).expect("create model directory");
        fs::write(root.join("partial-download").join("model.part"), b"partial")
            .expect("write model fixture");
        fs::create_dir_all(&sibling).expect("create sibling");

        assert!(remove_root(&root).expect("cleanup should succeed"));
        assert!(!root.exists());
        assert!(sibling.is_dir());
        assert!(!remove_root(&root).expect("missing root is a no-op"));
    }

    #[test]
    fn cleanup_rejects_a_file_at_the_models_path() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = temp.path().join("app").join(TTS_MODELS_DIR_NAME);
        fs::create_dir_all(root.parent().expect("root parent")).expect("create parent");
        fs::write(&root, b"not a directory").expect("write fixture");

        assert!(remove_root(&root).is_err());
        assert!(root.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_rejects_a_symlink_at_the_models_root() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temporary directory");
        let target = temp.path().join("real-models");
        let root = temp.path().join("app").join(TTS_MODELS_DIR_NAME);
        fs::create_dir_all(&target).expect("create target");
        fs::write(target.join("keep"), b"model").expect("write target fixture");
        fs::create_dir_all(root.parent().expect("root parent")).expect("create parent");
        symlink(&target, &root).expect("create symlink");

        assert!(remove_root(&root).is_err());
        assert!(root.is_symlink());
        assert!(target.join("keep").is_file());
    }
}
