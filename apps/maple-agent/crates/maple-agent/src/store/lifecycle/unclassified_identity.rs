//! Filesystem qualification for an explicitly discarded, unreadable store.
//!
//! A damaged or missing generation record cannot qualify its own database ID.
//! Instead, the reset journal records the actual private directory and its
//! bounded set of files. Renaming that directory preserves this identity;
//! adopting a different directory or a replacement file never does.

use std::{
    fs::{self, File},
    io,
    path::Path,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

use crate::store::{
    StoreError, StoreResult,
    paths::{sync_directory, verify_private_path},
};

const MAX_FILES: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum FileIdentity {
    Unix { device: u64, inode: u64 },
    Windows { volume: u32, file_index: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestEntry {
    name: String,
    identity: FileIdentity,
    length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectoryManifest {
    directory: FileIdentity,
    entries: Vec<ManifestEntry>,
}

impl DirectoryManifest {
    /// Capture metadata only. Even a zero-length or otherwise unreadable
    /// database is eligible; no database or generation bytes are interpreted.
    pub(super) fn capture(path: &Path) -> StoreResult<Self> {
        let directory = capture_path(path, true)?.0;
        let entries = capture_entries(path)?;
        let manifest = Self { directory, entries };
        manifest.validate()?;
        // Directory replacement during enumeration must not qualify the new
        // path using entries observed through the previous one.
        manifest.verify_directory(path)?;
        manifest.verify(path, false)?;
        Ok(manifest)
    }

    /// A partial erase may have removed any subset of the original files.
    /// Surviving files must still have their original identity and length.
    pub(super) fn verify(&self, path: &Path, allow_missing_files: bool) -> StoreResult<()> {
        self.validate()?;
        self.verify_directory(path)?;
        let observed = capture_entries(path)?;
        for entry in &observed {
            let expected = self
                .entries
                .binary_search_by(|expected| expected.name.cmp(&entry.name))
                .ok()
                .map(|index| &self.entries[index])
                .ok_or(StoreError::UnsafePath)?;
            if expected != entry {
                return Err(StoreError::UnsafePath);
            }
        }
        if !allow_missing_files && observed.len() != self.entries.len() {
            return Err(StoreError::UnsafePath);
        }
        self.verify_directory(path)
    }

    pub(super) fn verify_directory(&self, path: &Path) -> StoreResult<()> {
        if self.matches_directory(path)? {
            Ok(())
        } else {
            Err(StoreError::UnsafePath)
        }
    }

    /// Missing paths are an ordinary nonmatch. Existing unsafe paths remain
    /// errors, rather than making a link or foreign directory look absent.
    pub(super) fn matches_directory(&self, path: &Path) -> StoreResult<bool> {
        match capture_path(path, true) {
            Ok((identity, _)) => Ok(identity == self.directory),
            Err(StoreError::Io {
                kind: io::ErrorKind::NotFound,
            }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Delete only captured files after preflighting every survivor. The
    /// generation record, if present, is removed after all other contents and
    /// a directory sync. A crash at any step can resume against the same
    /// manifest with an arbitrary subset missing.
    pub(super) fn delete_remaining(&self, path: &Path) -> StoreResult<()> {
        self.validate()?;
        let parent = path.parent().ok_or(StoreError::UnsafePath)?;
        match fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Resume a crash after the final rmdir but before its parent
                // directory sync. Never create a directory to finish cleanup.
                sync_directory(parent)?;
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
        self.verify(path, true)?;
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.name != "generation.json")
        {
            self.remove_captured_file(path, entry)?;
        }
        self.verify(path, true)?;
        sync_directory(path)?;
        if let Some(generation) = self
            .entries
            .iter()
            .find(|entry| entry.name == "generation.json")
        {
            self.remove_captured_file(path, generation)?;
        }
        self.verify(path, true)?;
        sync_directory(path)?;
        self.verify_directory(path)?;
        fs::remove_dir(path)?;
        sync_directory(parent)?;
        Ok(())
    }

    fn remove_captured_file(&self, directory: &Path, entry: &ManifestEntry) -> StoreResult<()> {
        self.verify_directory(directory)?;
        let path = directory.join(&entry.name);
        let (identity, length) = match capture_path(&path, false) {
            Ok(snapshot) => snapshot,
            Err(StoreError::Io {
                kind: io::ErrorKind::NotFound,
            }) => return Ok(()),
            Err(error) => return Err(error),
        };
        if identity != entry.identity || length != entry.length {
            return Err(StoreError::UnsafePath);
        }
        self.verify_directory(directory)?;
        fs::remove_file(path)?;
        Ok(())
    }

    pub(super) fn validate(&self) -> StoreResult<()> {
        if self.entries.is_empty()
            || self.entries.len() > MAX_FILES
            || !self.entries.iter().any(|entry| entry.name == "maple.db")
            || self.entries.iter().any(|entry| !allowed_name(&entry.name))
            || self
                .entries
                .windows(2)
                .any(|entries| entries[0].name >= entries[1].name)
        {
            return Err(StoreError::UnsafePath);
        }
        Ok(())
    }
}

fn capture_entries(path: &Path) -> StoreResult<Vec<ManifestEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        if entries.len() >= MAX_FILES {
            return Err(StoreError::UnsafePath);
        }
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::UnsafePath)?;
        if !allowed_name(&name) {
            return Err(StoreError::UnsafePath);
        }
        let (identity, length) = capture_path(&entry.path(), false)?;
        entries.push(ManifestEntry {
            name,
            identity,
            length,
        });
    }
    entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

pub(super) fn allowed_name(name: &str) -> bool {
    if matches!(
        name,
        "maple.db"
            | "maple.db-wal"
            | "maple.db-shm"
            | "maple.db-journal"
            | "keys.json"
            | "keys.backup.json"
            | "generation.json"
    ) {
        return true;
    }
    if let Some(identifier) = name
        .strip_prefix(".control-")
        .and_then(|name| name.strip_suffix(".tmp"))
    {
        return identifier.len() == 32 && identifier.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    if let Some(identifier) = name
        .strip_prefix(".keys-")
        .and_then(|name| name.strip_suffix(".tmp"))
    {
        // Accept only complete random identifiers, including the current key
        // writer's canonical base64url encoding of exactly 16 random bytes.
        return (identifier.len() == 32 && identifier.bytes().all(|byte| byte.is_ascii_hexdigit()))
            || (identifier.len() == 22
                && URL_SAFE_NO_PAD.decode(identifier).is_ok_and(|bytes| {
                    bytes.len() == 16 && URL_SAFE_NO_PAD.encode(bytes) == identifier
                }));
    }
    false
}

pub(super) fn capture_path(path: &Path, directory: bool) -> StoreResult<(FileIdentity, u64)> {
    verify_private_path(path, directory)?;
    let opened = open_no_follow(path, directory)?;
    let snapshot = opened_identity(&opened, directory)?;
    // Recheck the named path and the actual opened object. On Windows the
    // shared permission helper also verifies the protected owner-only ACL.
    verify_private_path(path, directory)?;
    let named = open_no_follow(path, directory)?;
    if snapshot != opened_identity(&named, directory)? {
        return Err(StoreError::UnsafePath);
    }
    Ok(snapshot)
}

#[cfg(unix)]
fn open_no_follow(path: &Path, directory: bool) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let flags = libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
}

#[cfg(unix)]
fn opened_identity(file: &File, directory: bool) -> StoreResult<(FileIdentity, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || (!directory && (metadata.mode() & 0o7777 != 0o600 || metadata.nlink() != 1))
    {
        return Err(StoreError::UnsafePath);
    }
    Ok((
        FileIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        metadata.len(),
    ))
}

#[cfg(windows)]
fn open_no_follow(path: &Path, directory: bool) -> io::Result<File> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING, READ_CONTROL,
        },
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_handle(handle) })
    }
}

#[cfg(windows)]
fn opened_identity(file: &File, directory: bool) -> StoreResult<(FileIdentity, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        GetFileInformationByHandle,
    };
    let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let information = unsafe { information.assume_init() };
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && information.nNumberOfLinks != 1)
    {
        return Err(StoreError::UnsafePath);
    }
    Ok((
        FileIdentity::Windows {
            volume: information.dwVolumeSerialNumber,
            file_index: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        },
        (u64::from(information.nFileSizeHigh) << 32) | u64::from(information.nFileSizeLow),
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::store::paths::{private_directory, private_new_file};

    fn private_file(path: &Path, content: &[u8]) {
        let mut file = private_new_file(path).unwrap();
        file.write_all(content).unwrap();
        file.sync_all().unwrap();
    }

    fn fixture(root: &Path) -> std::path::PathBuf {
        let directory = root.join("db");
        private_directory(&directory).unwrap();
        private_file(&directory.join("maple.db"), b"corrupt");
        private_file(&directory.join("keys.json"), b"corrupt metadata");
        private_file(&directory.join("generation.json"), b"not a generation");
        directory
    }

    #[test]
    fn manifest_survives_rename_and_serialization_without_reading_generation() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = fixture(temporary.path());
        let manifest = DirectoryManifest::capture(&directory).unwrap();
        let serialized = serde_json::to_vec(&manifest).unwrap();
        let manifest: DirectoryManifest = serde_json::from_slice(&serialized).unwrap();
        let archive = temporary.path().join("db.unclassified");
        fs::rename(&directory, &archive).unwrap();
        assert!(!manifest.matches_directory(&directory).unwrap());
        manifest.verify(&archive, false).unwrap();
        assert_eq!(fs::read(archive.join("maple.db")).unwrap(), b"corrupt");
        assert!(!String::from_utf8(serialized).unwrap().contains("corrupt"));
    }

    #[test]
    fn empty_database_and_missing_generation_are_qualified_by_filesystem_only() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("db");
        private_directory(&directory).unwrap();
        private_file(&directory.join("maple.db"), b"");
        let manifest = DirectoryManifest::capture(&directory).unwrap();
        manifest.verify(&directory, false).unwrap();
        manifest.delete_remaining(&directory).unwrap();
        assert!(!directory.exists());
    }

    #[test]
    fn partial_deletion_resumes_and_completed_deletion_is_idempotent() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = fixture(temporary.path());
        let manifest = DirectoryManifest::capture(&directory).unwrap();
        fs::remove_file(directory.join("maple.db")).unwrap();
        assert_eq!(
            manifest.verify(&directory, false),
            Err(StoreError::UnsafePath)
        );
        manifest.verify(&directory, true).unwrap();
        manifest.delete_remaining(&directory).unwrap();
        manifest.delete_remaining(&directory).unwrap();
        assert!(!directory.exists());
    }

    #[test]
    fn replaced_file_or_directory_is_rejected_before_other_contents_are_deleted() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = fixture(temporary.path());
        let manifest = DirectoryManifest::capture(&directory).unwrap();
        let saved_keys = temporary.path().join("saved-keys");
        fs::rename(directory.join("keys.json"), &saved_keys).unwrap();
        private_file(&directory.join("keys.json"), b"corrupt metadata");
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(fs::read(directory.join("maple.db")).unwrap(), b"corrupt");
        let archive = temporary.path().join("archive");
        fs::rename(&directory, &archive).unwrap();
        fixture(temporary.path());
        assert!(!manifest.matches_directory(&directory).unwrap());
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(fs::read(directory.join("maple.db")).unwrap(), b"corrupt");
        assert_eq!(fs::read(&saved_keys).unwrap(), b"corrupt metadata");
    }

    #[test]
    fn unknown_files_and_changed_lengths_are_rejected_before_deletion() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = fixture(temporary.path());
        let manifest = DirectoryManifest::capture(&directory).unwrap();
        private_file(&directory.join("foreign-content"), b"keep me");
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(fs::read(directory.join("maple.db")).unwrap(), b"corrupt");
        fs::remove_file(directory.join("foreign-content")).unwrap();
        let mut keys = fs::OpenOptions::new()
            .append(true)
            .open(directory.join("keys.json"))
            .unwrap();
        keys.write_all(b" changed").unwrap();
        drop(keys);
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(fs::read(directory.join("maple.db")).unwrap(), b"corrupt");
    }

    #[test]
    fn temp_names_are_exact_and_canonical() {
        assert!(allowed_name(
            ".control-0123456789abcdef0123456789abcdef.tmp"
        ));
        assert!(allowed_name(".keys-0123456789abcdef0123456789abcdef.tmp"));
        assert!(allowed_name(".keys-AAAAAAAAAAAAAAAAAAAAAA.tmp"));
        for invalid in [
            ".control-0123456789abcdef.tmp",
            ".control-0123456789abcdef0123456789abcdef.tmp.extra",
            ".keys-AAAAAAAAAAAAAAAAAAAAAB.tmp",
            ".keys-AAAAAAAAAAAAAAAAAAAAAA=.tmp",
            ".keys-../../outside.tmp",
            "generation.json.old",
            "maple.db/child",
        ] {
            assert!(!allowed_name(invalid), "{invalid}");
        }
    }

    #[test]
    fn malformed_serialized_manifest_never_authorizes_deletion() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = fixture(temporary.path());
        let mut manifest = DirectoryManifest::capture(&directory).unwrap();
        manifest.entries.push(manifest.entries[0].clone());
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(fs::read(directory.join("maple.db")).unwrap(), b"corrupt");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_hardlinks_directories_and_broad_modes_preserve_external_canary() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temporary = tempfile::tempdir().unwrap();
        let directory = fixture(temporary.path());
        let manifest = DirectoryManifest::capture(&directory).unwrap();
        let external = temporary.path().join("external-canary");
        private_file(&external, b"untouched");
        let keys = directory.join("keys.json");
        fs::remove_file(&keys).unwrap();
        symlink(&external, &keys).unwrap();
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(
            DirectoryManifest::capture(&directory),
            Err(StoreError::UnsafePath)
        );
        fs::remove_file(&keys).unwrap();
        fs::hard_link(&external, &keys).unwrap();
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(
            DirectoryManifest::capture(&directory),
            Err(StoreError::UnsafePath)
        );
        fs::remove_file(&keys).unwrap();
        private_directory(&keys).unwrap();
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(
            DirectoryManifest::capture(&directory),
            Err(StoreError::UnsafePath)
        );
        fs::remove_dir(&keys).unwrap();
        private_file(&keys, b"corrupt metadata");
        fs::set_permissions(&keys, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            manifest.delete_remaining(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(
            DirectoryManifest::capture(&directory),
            Err(StoreError::UnsafePath)
        );
        assert_eq!(fs::read(&external).unwrap(), b"untouched");
        assert_eq!(fs::read(directory.join("maple.db")).unwrap(), b"corrupt");
    }
}
