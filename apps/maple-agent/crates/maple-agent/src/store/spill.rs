//! Transient output owned by one canonical store, never by a profile name alone.
//!
//! Cleanup callers must hold this store's `owner.lock`. Construction does not scan
//! temporary files or clean anything. No previous spill name or directory enters here.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::paths::{private_directory, verify_private_path};
use super::{StoreError, StorePaths, StoreResult};

#[derive(Clone, Debug)]
pub struct SpillRoot {
    container: PathBuf,
    root: PathBuf,
}

impl SpillRoot {
    /// Resolve only the new store's exact namespace. Its canonical path must
    /// already exist; no legacy state or temporary-file inventory is consulted.
    pub fn new(paths: &StorePaths) -> StoreResult<Self> {
        Self::new_in(paths, &temporary_root(), &user_identity()?)
    }

    fn new_in(paths: &StorePaths, temp: &Path, user: &str) -> StoreResult<Self> {
        if !temp.is_absolute()
            || user.is_empty()
            || !user
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(StoreError::UnsafePath);
        }
        verify_private_path(paths.root(), true)?;
        let canonical = fs::canonicalize(paths.root())?;
        let digest = canonical_hash(&canonical);
        let container = temp.join(format!("maple-store-v1-{user}-{digest}"));
        Ok(Self {
            root: container.join("spill"),
            container,
        })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// A canonical task id is the whole final path component. Reject separators
    /// and alternate spellings before inspecting or creating anything.
    pub fn task_path(&self, task: &str) -> StoreResult<PathBuf> {
        if !valid_task_id(task) {
            return Err(StoreError::InvalidInput);
        }
        Ok(self.root.join(task))
    }

    /// Create or verify every private component on each task setup. Failures are
    /// typed; the caller must not turn them into a fallback to the OS temp root.
    pub fn task_dir(&self, task: &str) -> StoreResult<PathBuf> {
        let task = self.task_path(task)?;
        private_directory(&self.container)?;
        private_directory(&self.root)?;
        private_directory(&task)?;
        Ok(task)
    }

    pub fn output_dir(&self, task: &str) -> StoreResult<PathBuf> {
        self.task_dir(task)
    }

    /// Remove only this store's spill subtree. Run during startup Maintenance
    /// before tasks are admitted, and again after all tools stop on host exit.
    pub fn cleanup(&self) -> StoreResult<()> {
        if !self.existing_private_roots()? {
            return Ok(());
        }
        // std's remove_dir_all does not follow symbolic links. The enclosing
        // directory is checked first so another OS user cannot replace entries.
        remove_directory_if_present(&self.root)
    }

    /// Run after the task is durably hidden and all of its tools have stopped.
    pub fn remove_task(&self, task: &str) -> StoreResult<()> {
        let task = self.task_path(task)?;
        if !self.existing_private_roots()? {
            return Ok(());
        }
        match fs::symlink_metadata(&task) {
            Ok(_) => verify_private_path(&task, true)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        remove_directory_if_present(&task)
    }

    fn existing_private_roots(&self) -> StoreResult<bool> {
        for path in [&self.container, &self.root] {
            match fs::symlink_metadata(path) {
                Ok(_) => verify_private_path(path, true)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(true)
    }
}

fn remove_directory_if_present(path: &Path) -> StoreResult<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn valid_task_id(task: &str) -> bool {
    task.len() == 36
        && task.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn canonical_hash(path: &Path) -> String {
    #[cfg(unix)]
    let digest = {
        use std::os::unix::ffi::OsStrExt;
        Sha256::digest(path.as_os_str().as_bytes())
    };
    #[cfg(windows)]
    let digest = {
        use std::os::windows::ffi::OsStrExt;
        let bytes: Vec<u8> = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        Sha256::digest(bytes)
    };
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn temporary_root() -> PathBuf {
    #[cfg(target_os = "linux")]
    for variable in ["XDG_RUNTIME_DIR", "TMPDIR"] {
        if let Some(path) = std::env::var_os(variable).filter(|path| !path.is_empty()) {
            return path.into();
        }
    }
    std::env::temp_dir()
}

#[cfg(unix)]
fn user_identity() -> StoreResult<String> {
    Ok(unsafe { libc::geteuid() }.to_string())
}

#[cfg(windows)]
fn user_identity() -> StoreResult<String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut length = 0;
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut length) };
    let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    };
    let error = std::io::Error::last_os_error();
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return Err(error.into());
    }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut length = 0;
    while unsafe { *sid.add(length) } != 0 {
        length += 1;
    }
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(sid, length) });
    unsafe { LocalFree(sid.cast()) };
    text.map_err(|_| StoreError::InvalidData)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "01234567-89ab-7def-8123-456789abcdef";
    const SECOND: &str = "11234567-89ab-7def-8123-456789abcdef";

    fn fixture(temp: &Path, name: &str) -> SpillRoot {
        let paths = StorePaths::new(temp.join(name)).prepare().unwrap();
        SpillRoot::new_in(&paths, temp, &user_identity().unwrap()).unwrap()
    }

    #[test]
    fn spill_dirs_are_per_task_and_private() {
        let temp = tempfile::tempdir().unwrap();
        let spills = fixture(temp.path(), "profile");
        let first = spills.task_dir(FIRST).unwrap();
        let second = spills.task_dir(SECOND).unwrap();
        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(spills.path()));
        for path in [&spills.container, &spills.root, &first, &second] {
            verify_private_path(path, true).unwrap();
        }
        let output =
            pi_coding_agent::tools::write_output_file_in(&first, "test", ".log", b"private")
                .unwrap();
        verify_private_path(&output, false).unwrap();
    }

    #[test]
    fn spill_dir_is_deleted_on_task_delete() {
        let temp = tempfile::tempdir().unwrap();
        let spills = fixture(temp.path(), "profile");
        let first = spills.task_dir(FIRST).unwrap();
        let second = spills.task_dir(SECOND).unwrap();
        fs::write(first.join("output"), b"first").unwrap();
        spills.remove_task(FIRST).unwrap();
        assert!(!first.exists());
        assert!(second.exists());
        spills.remove_task(FIRST).unwrap();
    }

    #[test]
    fn canonical_stores_never_clean_each_others_spills_or_previous_files() {
        let temp = tempfile::tempdir().unwrap();
        let first = fixture(temp.path(), "mde-one");
        let second = fixture(temp.path(), "mde-two");
        assert_ne!(first.path(), second.path());
        let first_task = first.task_dir(FIRST).unwrap();
        let second_task = second.task_dir(FIRST).unwrap();
        fs::write(first_task.join("output"), b"one").unwrap();
        fs::write(second_task.join("output"), b"two").unwrap();
        let previous = temp.path().join("maple-bash-previous.log");
        fs::write(&previous, b"previous sentinel").unwrap();
        first.cleanup().unwrap();
        assert!(!first.path().exists());
        assert_eq!(fs::read(second_task.join("output")).unwrap(), b"two");
        assert_eq!(fs::read(previous).unwrap(), b"previous sentinel");
        assert!(second_task.exists());
        first.cleanup().unwrap();
        let restarted = first.task_dir(FIRST).unwrap();
        assert!(!restarted.join("output").exists());
    }

    #[test]
    fn task_names_cannot_escape_the_exact_spill_root() {
        let temp = tempfile::tempdir().unwrap();
        let spills = fixture(temp.path(), "profile");
        for name in [
            "../previous",
            "/tmp",
            ".",
            "",
            "01234567-89AB-7def-8123-456789abcdef",
        ] {
            assert_eq!(spills.task_dir(name), Err(StoreError::InvalidInput));
            assert_eq!(spills.remove_task(name), Err(StoreError::InvalidInput));
        }
        assert!(!spills.path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn precreated_symlink_and_shared_directories_are_refused_without_adoption() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let temp = tempfile::tempdir().unwrap();
        let spills = fixture(temp.path(), "profile");
        let previous = temp.path().join("previous-sentinel");
        fs::create_dir(&previous).unwrap();
        fs::write(previous.join("keep"), b"untouched").unwrap();
        symlink(&previous, &spills.container).unwrap();
        assert_eq!(spills.task_dir(FIRST), Err(StoreError::UnsafePath));
        assert_eq!(spills.cleanup(), Err(StoreError::UnsafePath));
        assert_eq!(fs::read(previous.join("keep")).unwrap(), b"untouched");
        fs::remove_file(&spills.container).unwrap();
        fs::create_dir(&spills.container).unwrap();
        fs::set_permissions(&spills.container, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(spills.task_dir(FIRST), Err(StoreError::UnsafePath));
        assert_eq!(
            fs::metadata(&spills.container)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_never_follows_links_inside_its_owned_root() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let spills = fixture(temp.path(), "profile");
        let task = spills.task_dir(FIRST).unwrap();
        let previous = temp.path().join("previous-sentinel");
        fs::create_dir(&previous).unwrap();
        fs::write(previous.join("keep"), b"untouched").unwrap();
        symlink(&previous, task.join("link")).unwrap();
        spills.cleanup().unwrap();
        assert_eq!(fs::read(previous.join("keep")).unwrap(), b"untouched");
    }
}
