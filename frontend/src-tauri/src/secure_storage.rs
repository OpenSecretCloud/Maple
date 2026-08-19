//! Fail-closed storage for Maple's installation identity.
//!
//! Production implementations must use platform secure storage. This module
//! deliberately has no filesystem implementation and no plaintext fallback.
#![allow(
    dead_code,
    reason = "bounded foundation is wired in later vertical slices"
)]

use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, MutexGuard, TryLockError},
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

use crate::durable_host_epoch::{HostEpochRecordKind, HostEpochStorageKey};

const DEVICE_SECRET_LEN: usize = 32;
const PURPOSE: &str = "remote-agent-installation-identity-v1";
const STORAGE_ENVELOPE_VERSION: u8 = 1;
const STORAGE_ACCOUNT: &str = "installation-identity";
#[cfg(target_os = "macos")]
const INITIALIZATION_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
const INITIALIZATION_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(target_os = "macos")]
static MACOS_IDENTITY_INIT_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretStoreError {
    Unavailable(String),
    Corrupt(String),
    Backend(String),
}

impl std::fmt::Display for SecretStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(formatter, "secure storage unavailable: {message}")
            }
            Self::Corrupt(message) => {
                write!(formatter, "secure storage data is corrupt: {message}")
            }
            Self::Backend(message) => write!(formatter, "secure storage failed: {message}"),
        }
    }
}

impl std::error::Error for SecretStoreError {}

/// Opaque storage interface. Implementations store one versioned credential at
/// a stable app/purpose slot; install marker and generation live inside that
/// credential so resetting never leaves addressable old identity slots behind.
/// Account-scoped registration remains above this interface.
pub trait DeviceSecretStore: Send + Sync {
    fn load(&self, slot: &DeviceSecretSlot)
        -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError>;
    fn store(&self, slot: &DeviceSecretSlot, secret: &[u8]) -> Result<(), SecretStoreError>;
    fn delete(&self, slot: &DeviceSecretSlot) -> Result<(), SecretStoreError>;

    /// Read one installation-local host epoch record. Implementations must
    /// keep the state and lineage-guard accounts distinct and must not provide
    /// a plaintext fallback when secure storage is unavailable.
    fn load_host_epoch_record(
        &self,
        _key: &HostEpochStorageKey,
        _kind: HostEpochRecordKind,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        Err(SecretStoreError::Unavailable(
            "secure storage backend has no durable host epoch records".into(),
        ))
    }

    /// Atomically replace one host epoch record and make it durable before
    /// returning success. A backend error may occur after the write became
    /// durable; reservation recovery treats that value as consumed.
    fn store_host_epoch_record(
        &self,
        _key: &HostEpochStorageKey,
        _kind: HostEpochRecordKind,
        _record: &[u8],
    ) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unavailable(
            "secure storage backend has no durable host epoch records".into(),
        ))
    }

    /// Execute a host epoch reservation under one bounded lock shared by all
    /// Maple processes for this installation. The callback re-reads both
    /// records while the lock is held and performs the ordered durable writes.
    fn with_host_epoch_lock(
        &self,
        _key: &HostEpochStorageKey,
        _operation: &mut dyn FnMut() -> Result<u64, SecretStoreError>,
    ) -> Result<u64, SecretStoreError> {
        Err(SecretStoreError::Unavailable(
            "secure storage backend has no atomic host epoch boundary".into(),
        ))
    }

    /// Execute identity initialization under the strongest atomic boundary the
    /// backend can provide. Test stores use a shared mutex; macOS uses bounded
    /// process and advisory file locks before re-reading Keychain. No private
    /// material is written to the lock file.
    fn with_initialization_lock(
        &self,
        _slot: &DeviceSecretSlot,
        _operation: &mut dyn FnMut() -> Result<DeviceIdentity, SecretStoreError>,
    ) -> Result<DeviceIdentity, SecretStoreError> {
        // Initializing without a backend-wide create/re-read boundary can
        // return two different identities to racing processes. A new backend
        // must therefore opt in by implementing an atomic boundary; there is
        // deliberately no process-local-only fallback here.
        Err(SecretStoreError::Unavailable(
            "secure storage backend has no atomic identity initialization boundary".into(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceSecretSlot {
    app_identifier: String,
    install_marker: String,
    generation: u64,
}

impl DeviceSecretSlot {
    pub fn new(
        app_identifier: impl Into<String>,
        install_marker: impl Into<String>,
        generation: u64,
    ) -> Result<Self, SecretStoreError> {
        let slot = Self {
            app_identifier: app_identifier.into(),
            install_marker: install_marker.into(),
            generation,
        };
        if slot.generation == 0
            || !is_safe_component(&slot.app_identifier)
            || !is_safe_component(&slot.install_marker)
        {
            return Err(SecretStoreError::Corrupt(
                "app identifier, install marker, or generation has an invalid shape".into(),
            ));
        }
        Ok(slot)
    }

    fn service(&self) -> String {
        format!("{}.{}", self.app_identifier, PURPOSE)
    }

    fn account(&self) -> String {
        STORAGE_ACCOUNT.into()
    }
}

#[derive(Clone)]
pub struct DeviceIdentity {
    secret: Arc<Zeroizing<Vec<u8>>>,
    public_id: String,
    host_epoch_storage_key: HostEpochStorageKey,
}

impl std::fmt::Debug for DeviceIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceIdentity")
            .field("public_id", &self.public_id)
            .finish_non_exhaustive()
    }
}

impl DeviceIdentity {
    pub fn load_or_create(
        store: &dyn DeviceSecretStore,
        slot: &DeviceSecretSlot,
    ) -> Result<Self, SecretStoreError> {
        // The backend boundary must serialize threads and processes and then
        // re-read secure storage. A process-global mutex here would be both
        // insufficient across processes and an unbounded wait around OS I/O.
        let mut initialize = || load_generate_and_store(store, slot);
        store.with_initialization_lock(slot, &mut initialize)
    }

    pub fn public_id(&self) -> &str {
        &self.public_id
    }

    /// This stays crate-private: the private identity may be consumed by the
    /// native transport, but may never be returned by a Tauri command.
    pub(crate) fn iroh_secret_key(&self) -> Result<iroh::SecretKey, SecretStoreError> {
        secret_key_from_bytes(self.secret.as_slice())
    }

    pub(crate) fn host_epoch_storage_key(&self) -> &HostEpochStorageKey {
        &self.host_epoch_storage_key
    }
}

fn load_generate_and_store(
    store: &dyn DeviceSecretStore,
    slot: &DeviceSecretSlot,
) -> Result<DeviceIdentity, SecretStoreError> {
    // Always load again while holding the backend boundary. This is required
    // after waiting for a different Maple process to initialize Keychain.
    let mut secret = match store.load(slot)? {
        Some(envelope) => match decode_envelope(&envelope, slot)? {
            Some(secret) => secret,
            // A changed external install marker or explicit generation is an
            // identity reset. Overwrite the same secure-store slot.
            None => create_and_store_secret(store, slot)?,
        },
        None => create_and_store_secret(store, slot)?,
    };
    if secret.len() != DEVICE_SECRET_LEN {
        secret.zeroize();
        return Err(SecretStoreError::Corrupt(format!(
            "expected {DEVICE_SECRET_LEN} identity bytes"
        )));
    }
    let iroh_secret = secret_key_from_bytes(&secret)?;
    let public_id = iroh_secret.public().to_string();
    Ok(DeviceIdentity {
        secret: Arc::new(secret),
        public_id,
        host_epoch_storage_key: HostEpochStorageKey::new(
            slot.app_identifier.clone(),
            slot.install_marker.clone(),
            slot.generation,
        )?,
    })
}

fn secret_key_from_bytes(bytes: &[u8]) -> Result<iroh::SecretKey, SecretStoreError> {
    let bytes = Zeroizing::new(
        <[u8; DEVICE_SECRET_LEN]>::try_from(bytes)
            .map_err(|_| SecretStoreError::Corrupt("identity secret length is invalid".into()))?,
    );
    Ok(iroh::SecretKey::from_bytes(&bytes))
}

fn create_and_store_secret(
    store: &dyn DeviceSecretStore,
    slot: &DeviceSecretSlot,
) -> Result<Zeroizing<Vec<u8>>, SecretStoreError> {
    // Maple owns the seed buffer from the instant it is filled by the OS CSPRNG;
    // no temporary key object or unprotected byte array exists on this path.
    let mut generated = Zeroizing::new([0_u8; DEVICE_SECRET_LEN]);
    getrandom::fill(generated.as_mut()).map_err(|_| {
        SecretStoreError::Backend("operating-system random source is unavailable".into())
    })?;
    let envelope = encode_envelope(slot, &generated)?;
    store.store(slot, &envelope)?;
    Ok(Zeroizing::new(generated.to_vec()))
}

fn encode_envelope(
    slot: &DeviceSecretSlot,
    secret: &[u8; DEVICE_SECRET_LEN],
) -> Result<Zeroizing<Vec<u8>>, SecretStoreError> {
    let marker_len = u16::try_from(slot.install_marker.len()).map_err(|_| {
        SecretStoreError::Corrupt("install marker does not fit credential envelope".into())
    })?;
    let mut envelope = Zeroizing::new(Vec::with_capacity(
        1 + 8 + 2 + usize::from(marker_len) + DEVICE_SECRET_LEN,
    ));
    envelope.push(STORAGE_ENVELOPE_VERSION);
    envelope.extend_from_slice(&slot.generation.to_be_bytes());
    envelope.extend_from_slice(&marker_len.to_be_bytes());
    envelope.extend_from_slice(slot.install_marker.as_bytes());
    envelope.extend_from_slice(secret);
    Ok(envelope)
}

fn decode_envelope(
    envelope: &[u8],
    slot: &DeviceSecretSlot,
) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
    const HEADER_LEN: usize = 1 + 8 + 2;
    if envelope.len() < HEADER_LEN + DEVICE_SECRET_LEN {
        return Err(SecretStoreError::Corrupt(
            "identity credential envelope is truncated".into(),
        ));
    }
    if envelope[0] != STORAGE_ENVELOPE_VERSION {
        return Err(SecretStoreError::Corrupt(format!(
            "unsupported identity credential version {}",
            envelope[0]
        )));
    }
    let generation = u64::from_be_bytes(
        envelope[1..9]
            .try_into()
            .map_err(|_| SecretStoreError::Corrupt("invalid identity generation".into()))?,
    );
    let marker_len =
        usize::from(u16::from_be_bytes(envelope[9..11].try_into().map_err(
            |_| SecretStoreError::Corrupt("invalid install marker length".into()),
        )?));
    let expected_len = HEADER_LEN + marker_len + DEVICE_SECRET_LEN;
    if envelope.len() != expected_len {
        return Err(SecretStoreError::Corrupt(
            "identity credential envelope has an invalid length".into(),
        ));
    }
    let marker = &envelope[HEADER_LEN..HEADER_LEN + marker_len];
    if generation != slot.generation || marker != slot.install_marker.as_bytes() {
        return Ok(None);
    }
    Ok(Some(Zeroizing::new(
        envelope[HEADER_LEN + marker_len..].to_vec(),
    )))
}

fn is_safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

/// Return the secure backend for the current platform. Unsupported production
/// platforms receive an explicit error; callers must never substitute a file.
pub fn platform_store() -> Result<Box<dyn DeviceSecretStore>, SecretStoreError> {
    Err(SecretStoreError::Unavailable(
        "Maple device identity storage is not enabled on this platform; macOS Data Protection Keychain also requires signed-app entitlement/profile validation before activation".into(),
    ))
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
struct MacOsKeychainStore;

#[cfg(target_os = "macos")]
impl MacOsKeychainStore {
    fn query_options(slot: &DeviceSecretSlot) -> security_framework::passwords::PasswordOptions {
        let mut options = security_framework::passwords::PasswordOptions::new_generic_password(
            &slot.service(),
            &slot.account(),
        );
        // The installation seed is local-only and lives in the modern data-
        // protection Keychain. Iroh needs the raw 32-byte seed in-process, so
        // this is not a Secure Enclave key handle.
        options.set_access_synchronized(Some(false));
        options.use_protected_keychain();
        options
    }

    fn create_options(
        slot: &DeviceSecretSlot,
    ) -> Result<security_framework::passwords::PasswordOptions, SecretStoreError> {
        use security_framework::access_control::{ProtectionMode, SecAccessControl};

        let access_control = SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly),
            0,
        )
        .map_err(|error| {
            SecretStoreError::Backend(format!(
                "Keychain access-control creation returned OSStatus {}",
                error.code()
            ))
        })?;
        let mut options = Self::query_options(slot);
        options.set_access_control(access_control);
        Ok(options)
    }

    fn update_search(slot: &DeviceSecretSlot) -> security_framework::item::ItemSearchOptions {
        use security_framework::item::{ItemClass, ItemSearchOptions};

        let mut search = ItemSearchOptions::new();
        search
            .ignore_legacy_keychains()
            .class(ItemClass::generic_password())
            .service(&slot.service())
            .account(&slot.account())
            .cloud_sync(Some(false));
        search
    }
}

#[cfg(target_os = "macos")]
impl DeviceSecretStore for MacOsKeychainStore {
    fn load(
        &self,
        slot: &DeviceSecretSlot,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        match security_framework::passwords::generic_password(Self::query_options(slot)) {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(error) if error.code() == -25300 => Ok(None), // errSecItemNotFound
            Err(error) => Err(SecretStoreError::Backend(format!(
                "Keychain read returned OSStatus {}",
                error.code()
            ))),
        }
    }

    fn store(&self, slot: &DeviceSecretSlot, secret: &[u8]) -> Result<(), SecretStoreError> {
        use core_foundation::data::CFData;
        use security_framework::item::{update_item, ItemUpdateOptions, ItemUpdateValue};

        // Update data separately from creation attributes. In particular,
        // kSecAttrAccessControl belongs on SecItemAdd, not in a match query.
        // If the protected item is absent, create it with the non-migrating
        // accessibility class. The initialization lock prevents a duplicate-
        // add race between Maple processes.
        let mut update = ItemUpdateOptions::new();
        update.set_value(ItemUpdateValue::Data(CFData::from_buffer(secret)));
        match update_item(&Self::update_search(slot), &update) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => {
                security_framework::passwords::set_generic_password_options(
                    secret,
                    Self::create_options(slot)?,
                )
                .map_err(|error| {
                    SecretStoreError::Backend(format!(
                        "Keychain create returned OSStatus {}",
                        error.code()
                    ))
                })
            }
            Err(error) => Err(SecretStoreError::Backend(format!(
                "Keychain update returned OSStatus {}",
                error.code()
            ))),
        }
    }

    fn delete(&self, slot: &DeviceSecretSlot) -> Result<(), SecretStoreError> {
        match security_framework::passwords::delete_generic_password_options(Self::query_options(
            slot,
        )) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()), // errSecItemNotFound
            Err(error) => Err(SecretStoreError::Backend(format!(
                "Keychain delete returned OSStatus {}",
                error.code()
            ))),
        }
    }

    fn with_initialization_lock(
        &self,
        slot: &DeviceSecretSlot,
        operation: &mut dyn FnMut() -> Result<DeviceIdentity, SecretStoreError>,
    ) -> Result<DeviceIdentity, SecretStoreError> {
        let deadline = Instant::now() + INITIALIZATION_LOCK_TIMEOUT;
        // macOS flock ownership is process-associated, so a bounded native
        // mutex is also required to serialize independently opened descriptors
        // from threads in this process. It is backend-local and has no
        // unbounded lock() call.
        let _process_guard = acquire_macos_process_lock(deadline)?;
        let lock_path = macos_identity_lock_path(slot)?;
        let lock_file = open_private_lock_file(&lock_path)?;
        acquire_initialization_lock(&lock_file, deadline)?;
        // load_generate_and_store executes here and therefore re-reads
        // Keychain after any other Maple process releases this lock.
        let result = operation();
        let unlock_result = fs2::FileExt::unlock(&lock_file).map_err(|error| {
            SecretStoreError::Backend(format!(
                "could not release installation identity lock: {error}"
            ))
        });
        match (result, unlock_result) {
            (Ok(identity), Ok(())) => Ok(identity),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_identity_lock_path(slot: &DeviceSecretSlot) -> Result<PathBuf, SecretStoreError> {
    let application_support = dirs::data_local_dir().ok_or_else(|| {
        SecretStoreError::Unavailable("macOS Application Support directory is unavailable".into())
    })?;
    // This dedicated, non-cache directory is intentionally not purgeable. It
    // contains no private key material, only a zero-length advisory lock.
    let lock_dir = application_support.join(format!("{}.{}-locks", slot.app_identifier, PURPOSE));
    create_private_directory(&lock_dir)?;
    Ok(lock_dir.join("initialization.lock"))
}

#[cfg(target_os = "macos")]
fn current_effective_uid() -> libc::uid_t {
    // SAFETY: geteuid has no arguments or caller-side safety preconditions.
    unsafe { libc::geteuid() }
}

#[cfg(target_os = "macos")]
fn create_private_directory(path: &Path) -> Result<(), SecretStoreError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(SecretStoreError::Unavailable(format!(
                "could not create installation identity lock directory: {error}"
            )))
        }
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        SecretStoreError::Unavailable(format!(
            "could not inspect installation identity lock directory: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != current_effective_uid()
    {
        return Err(SecretStoreError::Unavailable(
            "installation identity lock directory is not a private owned directory".into(),
        ));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(
            |error| {
                SecretStoreError::Unavailable(format!(
                    "could not protect installation identity lock directory: {error}"
                ))
            },
        )?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_private_lock_file(path: &Path) -> Result<std::fs::File, SecretStoreError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SecretStoreError::Unavailable(
                "installation identity lock path is not a regular file".into(),
            ));
        }
    }
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            SecretStoreError::Unavailable(format!(
                "could not open installation identity lock: {error}"
            ))
        })?;
    let metadata = lock_file.metadata().map_err(|error| {
        SecretStoreError::Unavailable(format!(
            "could not inspect installation identity lock: {error}"
        ))
    })?;
    if !metadata.is_file() || metadata.uid() != current_effective_uid() || metadata.nlink() != 1 {
        return Err(SecretStoreError::Unavailable(
            "installation identity lock is not a private owned file".into(),
        ));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        lock_file
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|error| {
                SecretStoreError::Unavailable(format!(
                    "could not protect installation identity lock: {error}"
                ))
            })?;
    }
    Ok(lock_file)
}

#[cfg(target_os = "macos")]
fn acquire_macos_process_lock(
    deadline: Instant,
) -> Result<MutexGuard<'static, ()>, SecretStoreError> {
    loop {
        match MACOS_IDENTITY_INIT_LOCK.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => wait_for_initialization_lock(deadline)?,
            Err(TryLockError::Poisoned(_)) => {
                return Err(SecretStoreError::Backend(
                    "installation identity process lock is poisoned".into(),
                ))
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn acquire_initialization_lock(
    lock_file: &std::fs::File,
    deadline: Instant,
) -> Result<(), SecretStoreError> {
    let contended_code = fs2::lock_contended_error().raw_os_error();
    loop {
        match fs2::FileExt::try_lock_exclusive(lock_file) {
            Ok(()) => return Ok(()),
            Err(error) if error.raw_os_error() == contended_code => {
                wait_for_initialization_lock(deadline)?;
            }
            Err(error) => {
                return Err(SecretStoreError::Unavailable(format!(
                    "could not acquire installation identity lock: {error}"
                )))
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn wait_for_initialization_lock(deadline: Instant) -> Result<(), SecretStoreError> {
    let now = Instant::now();
    if now >= deadline {
        return Err(SecretStoreError::Unavailable(
            "timed out acquiring installation identity lock".into(),
        ));
    }
    std::thread::sleep(
        INITIALIZATION_LOCK_POLL_INTERVAL.min(deadline.saturating_duration_since(now)),
    );
    Ok(())
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Debug, Clone, Default)]
    pub struct InMemorySecretStore {
        values: Arc<Mutex<HashMap<(String, String), Vec<u8>>>>,
        initialization: Arc<Mutex<()>>,
    }

    impl InMemorySecretStore {
        pub fn entry_count(&self) -> usize {
            self.values.lock().unwrap().len()
        }
    }

    impl DeviceSecretStore for InMemorySecretStore {
        fn load(
            &self,
            slot: &DeviceSecretSlot,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
            Ok(self
                .values
                .lock()
                .map_err(|_| SecretStoreError::Backend("test store lock poisoned".into()))?
                .get(&(slot.service(), slot.account()))
                .cloned()
                .map(Zeroizing::new))
        }

        fn store(&self, slot: &DeviceSecretSlot, secret: &[u8]) -> Result<(), SecretStoreError> {
            self.values
                .lock()
                .map_err(|_| SecretStoreError::Backend("test store lock poisoned".into()))?
                .insert((slot.service(), slot.account()), secret.to_vec());
            Ok(())
        }

        fn delete(&self, slot: &DeviceSecretSlot) -> Result<(), SecretStoreError> {
            self.values
                .lock()
                .map_err(|_| SecretStoreError::Backend("test store lock poisoned".into()))?
                .remove(&(slot.service(), slot.account()));
            Ok(())
        }

        fn load_host_epoch_record(
            &self,
            key: &HostEpochStorageKey,
            kind: HostEpochRecordKind,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
            Ok(self
                .values
                .lock()
                .map_err(|_| SecretStoreError::Backend("test store lock poisoned".into()))?
                .get(&(key.service(), key.account(kind).into()))
                .cloned()
                .map(Zeroizing::new))
        }

        fn store_host_epoch_record(
            &self,
            key: &HostEpochStorageKey,
            kind: HostEpochRecordKind,
            record: &[u8],
        ) -> Result<(), SecretStoreError> {
            self.values
                .lock()
                .map_err(|_| SecretStoreError::Backend("test store lock poisoned".into()))?
                .insert((key.service(), key.account(kind).into()), record.to_vec());
            Ok(())
        }

        fn with_host_epoch_lock(
            &self,
            _key: &HostEpochStorageKey,
            operation: &mut dyn FnMut() -> Result<u64, SecretStoreError>,
        ) -> Result<u64, SecretStoreError> {
            let _guard = self.initialization.lock().map_err(|_| {
                SecretStoreError::Backend("test initialization lock poisoned".into())
            })?;
            operation()
        }

        fn with_initialization_lock(
            &self,
            _slot: &DeviceSecretSlot,
            operation: &mut dyn FnMut() -> Result<DeviceIdentity, SecretStoreError>,
        ) -> Result<DeviceIdentity, SecretStoreError> {
            let _guard = self.initialization.lock().map_err(|_| {
                SecretStoreError::Backend("test initialization lock poisoned".into())
            })?;
            operation()
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub struct UnavailableSecretStore;

    impl DeviceSecretStore for UnavailableSecretStore {
        fn load(
            &self,
            _slot: &DeviceSecretSlot,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
            Err(SecretStoreError::Unavailable("test backend offline".into()))
        }

        fn store(&self, _slot: &DeviceSecretSlot, _secret: &[u8]) -> Result<(), SecretStoreError> {
            Err(SecretStoreError::Unavailable("test backend offline".into()))
        }

        fn delete(&self, _slot: &DeviceSecretSlot) -> Result<(), SecretStoreError> {
            Err(SecretStoreError::Unavailable("test backend offline".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::*, *};
    use std::thread;

    fn slot(install: &str, generation: u64) -> DeviceSecretSlot {
        DeviceSecretSlot::new("cloud.opensecret.maple.test", install, generation).unwrap()
    }

    struct FailingWriteStore;

    impl DeviceSecretStore for FailingWriteStore {
        fn load(
            &self,
            _slot: &DeviceSecretSlot,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
            Ok(None)
        }

        fn store(&self, _slot: &DeviceSecretSlot, _secret: &[u8]) -> Result<(), SecretStoreError> {
            Err(SecretStoreError::Backend("synthetic write failure".into()))
        }

        fn delete(&self, _slot: &DeviceSecretSlot) -> Result<(), SecretStoreError> {
            Ok(())
        }

        fn with_initialization_lock(
            &self,
            _slot: &DeviceSecretSlot,
            operation: &mut dyn FnMut() -> Result<DeviceIdentity, SecretStoreError>,
        ) -> Result<DeviceIdentity, SecretStoreError> {
            operation()
        }
    }

    #[test]
    fn identity_is_stable_for_one_install_slot() {
        let store = InMemorySecretStore::default();
        let first = DeviceIdentity::load_or_create(&store, &slot("install-a", 1)).unwrap();
        let second = DeviceIdentity::load_or_create(&store, &slot("install-a", 1)).unwrap();
        assert_eq!(first.public_id(), second.public_id());
    }

    #[test]
    fn concurrent_initialization_returns_one_identity() {
        let store = Arc::new(InMemorySecretStore::default());
        let slot = Arc::new(slot("install-concurrent", 1));
        let workers = (0..16)
            .map(|_| {
                let store = store.clone();
                let slot = slot.clone();
                thread::spawn(move || {
                    DeviceIdentity::load_or_create(store.as_ref(), slot.as_ref())
                        .unwrap()
                        .public_id()
                        .to_owned()
                })
            })
            .collect::<Vec<_>>();
        let identities = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert!(identities.iter().all(|identity| identity == &identities[0]));
        assert_eq!(store.entry_count(), 1);
    }

    #[test]
    fn backend_atomic_boundary_models_independent_process_initializers() {
        let store = Arc::new(InMemorySecretStore::default());
        let slot = Arc::new(slot("install-backend-atomic", 1));
        let workers = (0..16)
            .map(|_| {
                let store = store.clone();
                let slot = slot.clone();
                thread::spawn(move || {
                    // Each thread represents an independent process relying
                    // only on the backend atomic boundary and its mandatory
                    // re-read.
                    let mut operation = || load_generate_and_store(store.as_ref(), slot.as_ref());
                    store
                        .with_initialization_lock(slot.as_ref(), &mut operation)
                        .unwrap()
                        .public_id()
                        .to_owned()
                })
            })
            .collect::<Vec<_>>();
        let identities = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert!(identities.iter().all(|identity| identity == &identities[0]));
        assert_eq!(store.entry_count(), 1);
    }

    #[test]
    fn installations_and_generations_are_isolated() {
        let store = InMemorySecretStore::default();
        let first = DeviceIdentity::load_or_create(&store, &slot("install-a", 1)).unwrap();
        let other_install = DeviceIdentity::load_or_create(&store, &slot("install-b", 1)).unwrap();
        let next_generation =
            DeviceIdentity::load_or_create(&store, &slot("install-a", 2)).unwrap();
        assert_ne!(first.public_id(), other_install.public_id());
        assert_ne!(first.public_id(), next_generation.public_id());
        assert_eq!(store.entry_count(), 1, "resets overwrite the stable slot");
    }

    #[test]
    fn unavailable_storage_never_generates_an_ephemeral_identity() {
        assert!(matches!(
            DeviceIdentity::load_or_create(&UnavailableSecretStore, &slot("install-a", 1)),
            Err(SecretStoreError::Unavailable(_))
        ));
    }

    #[test]
    fn failed_secure_store_write_never_returns_generated_identity() {
        assert!(matches!(
            DeviceIdentity::load_or_create(&FailingWriteStore, &slot("install-write-fails", 1)),
            Err(SecretStoreError::Backend(message)) if message == "synthetic write failure"
        ));
    }

    #[test]
    fn private_key_is_not_debuggable() {
        let store = InMemorySecretStore::default();
        let identity = DeviceIdentity::load_or_create(&store, &slot("install-a", 1)).unwrap();
        let debug = format!("{identity:?}");
        assert!(debug.contains(identity.public_id()));
        assert!(!debug.contains("secret"));
    }
}
