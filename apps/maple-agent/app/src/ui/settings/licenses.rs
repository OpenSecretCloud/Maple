//! Full notices live beside distributed binaries; developer builds need no generator.
use std::io::Write;
use std::path::{Path, PathBuf};

const DEVELOPMENT_NOTICES: &str = include_str!("../../../assets/licenses/development.html");

fn packaged_notices(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    [
        directory.join("../Resources/licenses/THIRD_PARTY_NOTICES.html"),
        directory.join("../share/maple-agent/licenses/THIRD_PARTY_NOTICES.html"),
        directory.join("../share/licenses/maple-agent/THIRD_PARTY_NOTICES.html"),
        directory.join("licenses/THIRD_PARTY_NOTICES.html"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

pub(super) fn open() -> Result<(), String> {
    if let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|exe| packaged_notices(&exe))
    {
        return open_path(&path);
    }
    // Public license text only. create_new avoids following a pre-existing
    // path; a random name lets the browser retain its own document lifetime.
    let path =
        std::env::temp_dir().join(format!("maple-agent-notices-{}.html", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "Could not open third-party notices.".to_owned())?;
    file.write_all(DEVELOPMENT_NOTICES.as_bytes())
        .map_err(|_| "Could not open third-party notices.".to_owned())?;
    drop(file);
    open_path(&path)
}

fn open_path(path: &Path) -> Result<(), String> {
    let url = url::Url::from_file_path(path)
        .map_err(|_| "Could not open third-party notices.".to_owned())?;
    webbrowser::open(url.as_str()).map_err(|_| "Could not open third-party notices.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_notices_in_each_native_package_layout() {
        for (executable, notices) in [
            (
                "Maple.app/Contents/MacOS/maple-agent",
                "Maple.app/Contents/Resources/licenses",
            ),
            (
                "AppDir/usr/bin/maple-agent",
                "AppDir/usr/share/maple-agent/licenses",
            ),
            ("nix/bin/maple-agent", "nix/share/licenses/maple-agent"),
            ("windows/maple-agent.exe", "windows/licenses"),
        ] {
            let temporary = tempfile::tempdir().unwrap();
            let executable = temporary.path().join(executable);
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            let notices = temporary
                .path()
                .join(notices)
                .join("THIRD_PARTY_NOTICES.html");
            std::fs::create_dir_all(notices.parent().unwrap()).unwrap();
            std::fs::write(&notices, "public notice fixture").unwrap();
            assert_eq!(
                packaged_notices(&executable)
                    .unwrap()
                    .canonicalize()
                    .unwrap(),
                notices.canonicalize().unwrap()
            );
        }
    }

    #[test]
    fn unpackaged_binary_needs_no_generated_notice_file() {
        let temporary = tempfile::tempdir().unwrap();
        assert!(packaged_notices(&temporary.path().join("maple-agent")).is_none());
        assert!(DEVELOPMENT_NOTICES.contains("https://github.com/MaplePrivacyLabs/Maple"));
    }
}
