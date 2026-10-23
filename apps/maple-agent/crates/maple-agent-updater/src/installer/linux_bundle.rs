//! User-owned Linux directory bundles. No downloaded executable is run while
//! preparing or authenticating an update. macOS keeps its own bundle verifier.
use super::*;
use crate::{Url, manifest};

const PROD_ROOT: &str = "maple-agent.app";
const DEV_ROOT: &str = "maple-agent-dev.app";
const DESCRIPTOR_LIMIT: u64 = 16 * 1024;
const RECEIPT_LIMIT: u64 = (manifest::MAX_ENVELOPE_BYTES * 6 + 16 * 1024) as u64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: String,
    product: String,
    profile: String,
    version: String,
    build: String,
    source_sha: String,
    bundle_id: String,
    entrypoint: String,
    executable: String,
}

impl Bundle {
    fn parse(bytes: &[u8], profile: &str) -> Result<Self> {
        if bytes.len() as u64 > DESCRIPTOR_LIMIT {
            return Err(InstallError::Invalid("bundle descriptor is too large"));
        }
        let bundle: Self = serde_json::from_slice(bytes)
            .map_err(|_| InstallError::Invalid("invalid Linux bundle descriptor"))?;
        let identity = match profile {
            "prod" => "cloud.opensecret.maple.agent",
            "dev" => "cloud.opensecret.maple.agent.dev",
            _ => return Err(InstallError::Invalid("invalid Linux bundle profile")),
        };
        if bundle.format != "maple-agent-linux-bundle-v1"
            || bundle.product != "maple-agent"
            || bundle.profile != profile
            || bundle.bundle_id != identity
            || bundle.entrypoint != "AppRun"
            || bundle.executable != "usr/bin/maple-agent"
            || bundle.build.is_empty()
            || bundle.build.len() > 18
            || bundle.build.starts_with('0')
            || !bundle.build.bytes().all(|b| b.is_ascii_digit())
            || bundle.source_sha.len() != 40
            || !bundle
                .source_sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || manifest::stable_version(&bundle.version).is_err()
        {
            return Err(InstallError::Invalid("Linux bundle identity changed"));
        }
        Ok(bundle)
    }

    fn matches_release(&self, release: &crate::VerifiedRelease) -> Result<()> {
        if self.version != release.manifest.version
            || self.build != release.manifest.build
            || self.source_sha != release.manifest.source_sha
        {
            return Err(InstallError::Invalid(
                "bundle differs from signed release identity",
            ));
        }
        Ok(())
    }
}

fn root_name(profile: &str) -> Result<&'static str> {
    match profile {
        "prod" => Ok(PROD_ROOT),
        "dev" => Ok(DEV_ROOT),
        _ => Err(InstallError::Invalid("invalid Linux bundle profile")),
    }
}

pub(super) fn is_bundle_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == PROD_ROOT || name == DEV_ROOT)
}

fn archive(
    payload: &Path,
    output: Option<&Path>,
    profile: &str,
) -> Result<(BTreeMap<String, Node>, Bundle)> {
    let root = root_name(profile)?;
    let (nodes, descriptor) = scan_archive_root(payload, output, root)?;
    let bundle = Bundle::parse(
        &descriptor.ok_or(InstallError::Invalid("missing bundle descriptor"))?,
        profile,
    )?;
    for executable in [
        "AppRun",
        "usr/bin/maple-agent",
        "usr/lib/ld-linux-x86-64.so.2",
    ] {
        if !matches!(
            nodes.get(&format!("{root}/{executable}")),
            Some(Node::File { mode: 0o755, .. })
        ) {
            return Err(InstallError::Invalid(
                "missing ordinary Linux bundle executable",
            ));
        }
    }
    if !matches!(
        nodes.get(&format!("{root}/bundle.json")),
        Some(Node::File { mode: 0o644, .. })
    ) {
        return Err(InstallError::Invalid(
            "invalid bundle descriptor permissions",
        ));
    }
    Ok((nodes, bundle))
}

/// Release CI uses the same bounded parser and metadata/layout checks as install.
/// This inspects data only, without extraction or execution.
pub fn validate_linux_bundle_archive(payload: &Path, profile: &str) -> Result<()> {
    archive(payload, None, profile).map(|_| ())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = open_regular(path, false, false)?;
    let metadata = file.metadata()?;
    #[cfg(unix)]
    if metadata.nlink() != 1 {
        return Err(InstallError::Invalid("metadata file is hard-linked"));
    }
    if metadata.len() > limit {
        return Err(InstallError::Invalid("metadata file is too large"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(InstallError::Invalid("metadata file grew"));
    }
    Ok(bytes)
}

fn installed_bundle(path: &Path) -> Result<Bundle> {
    let bundle = Bundle::parse(
        &read_bounded(&path.join("bundle.json"), DESCRIPTOR_LIMIT)?,
        "prod",
    )?;
    for directory in ["usr", "usr/bin", "usr/lib"] {
        let metadata = fs::symlink_metadata(path.join(directory))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(InstallError::Invalid("Linux bundle layout changed"));
        }
    }
    for executable in [
        "AppRun",
        "usr/bin/maple-agent",
        "usr/lib/ld-linux-x86-64.so.2",
    ] {
        let metadata = fs::symlink_metadata(path.join(executable))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(InstallError::Invalid("Linux bundle executable changed"));
        }
        #[cfg(unix)]
        if metadata.mode() & 0o7777 != 0o755 || metadata.nlink() != 1 {
            return Err(InstallError::Invalid(
                "unsafe Linux bundle executable permissions",
            ));
        }
    }
    Ok(bundle)
}

pub(super) fn validate_destination(installed: &Path) -> Result<()> {
    if !installed.is_absolute()
        || installed.file_name().is_none_or(|name| name != PROD_ROOT)
        || installed
            .to_str()
            .is_none_or(|value| value.chars().any(char::is_control))
        || installed.components().any(|part| {
            matches!(part, Component::CurDir | Component::ParentDir)
                || part
                    .as_os_str()
                    .to_str()
                    .is_some_and(|value| value.starts_with(STAGING_PREFIX))
        })
    {
        return Err(InstallError::NotifyOnly(
            "run a user-owned installed Maple Agent Linux bundle",
        ));
    }
    let metadata = fs::symlink_metadata(installed)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || installed.canonicalize()? != installed
    {
        return Err(InstallError::Invalid(
            "installation is not a canonical Linux bundle directory",
        ));
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o7022 != 0 {
        return Err(InstallError::NotifyOnly(
            "this Linux bundle is not safely owned by this account",
        ));
    }
    Ok(())
}

pub(super) fn preflight(installed: &Path) -> Result<()> {
    validate_destination(installed)?;
    installed_bundle(installed)?;
    let _probe = tempfile::Builder::new()
        .prefix(".maple-agent-write-probe-")
        .tempfile_in(installed.parent().unwrap())
        .map_err(destination_error)?;
    let _lock = open_install_lock(installed)?;
    Ok(())
}

/// `resolved_apprun` comes from the running bundle's validated launcher context,
/// not an environment path. The compiled identity prevents adopting another copy.
pub fn discover_linux_installation(
    resolved_apprun: &Path,
    running_version: &str,
    running_source_sha: &str,
) -> Result<PathBuf> {
    if resolved_apprun
        .file_name()
        .is_none_or(|name| name != "AppRun")
        || resolved_apprun.canonicalize()? != resolved_apprun
    {
        return Err(InstallError::NotifyOnly(
            "run the installed Linux AppRun launcher",
        ));
    }
    let installed = resolved_apprun
        .parent()
        .ok_or(InstallError::Invalid("missing bundle parent"))?;
    preflight(installed)?;
    let bundle = installed_bundle(installed)?;
    if bundle.version != running_version || bundle.source_sha != running_source_sha {
        return Err(InstallError::Invalid(
            "installed Linux bundle differs from running app",
        ));
    }
    Ok(installed.to_owned())
}

fn stage_prefix(installed: &Path) -> String {
    let digest = hex::encode(Sha256::digest(installed.as_os_str().as_encoded_bytes()));
    format!("{STAGING_PREFIX}{digest}-")
}

fn validate_stage(installed: &Path, stage: &Path) -> Result<()> {
    let prefix = stage_prefix(installed);
    if stage.parent() != installed.parent()
        || !stage
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.strip_prefix(&prefix).is_some_and(|tail| {
                    tail.len() == STAGING_RANDOM_LENGTH
                        && tail.bytes().all(|b| b.is_ascii_alphanumeric())
                })
            })
        || stage.canonicalize()? != stage
    {
        return Err(InstallError::Invalid(
            "Linux staging does not belong to this installation",
        ));
    }
    let metadata = fs::symlink_metadata(stage)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(InstallError::Invalid("Linux staging directory changed"));
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != 0o700
        || metadata.dev() != fs::metadata(installed)?.dev()
    {
        return Err(InstallError::Invalid(
            "Linux staging ownership, permissions or volume changed",
        ));
    }
    Ok(())
}

fn expected_artifact(update: &PreparedUpdate) -> Result<&Artifact> {
    manifest::validate_manifest(&update.release.manifest)
        .map_err(|_| InstallError::Invalid("manifest fields changed"))?;
    update
        .release
        .artifact(Target::LinuxX86_64)
        .map_err(|_| InstallError::Invalid("missing Linux v2 bundle payload"))
}

fn upgrade_destination(update: &PreparedUpdate) -> Result<()> {
    validate_destination(&update.installed_path)?;
    let current = installed_bundle(&update.installed_path)?;
    let current_version = manifest::stable_version(&current.version)
        .map_err(|_| InstallError::Invalid("invalid installed Linux version"))?;
    let proposed = manifest::stable_version(&update.release.manifest.version)
        .map_err(|_| InstallError::Invalid("invalid proposed Linux version"))?;
    if proposed <= current_version {
        return Err(InstallError::Invalid(
            "installation already has this version or a newer version",
        ));
    }
    Ok(())
}

pub(super) fn prepare(update: &PreparedUpdate) -> Result<InstallPreparation> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(InstallError::NotifyOnly(
            "automatic Linux installation requires x86_64 Linux",
        ));
    }
    preflight(&update.installed_path)?;
    upgrade_destination(update)?;
    let artifact = expected_artifact(update)?;
    let temporary = tempfile::Builder::new()
        .prefix(&stage_prefix(&update.installed_path))
        .rand_bytes(STAGING_RANDOM_LENGTH)
        .tempdir_in(update.installed_path.parent().unwrap())
        .map_err(destination_error)?;
    set_mode(temporary.path(), 0o700)?;
    let payload_path = temporary.path().join("payload");
    copy_verified_payload(&update.payload_path, &payload_path, artifact)?;
    let (expected, bundle) = archive(&payload_path, Some(temporary.path()), "prod")?;
    bundle.matches_release(&update.release)?;
    let staged_path = temporary.path().join(PROD_ROOT);
    if scan_tree_root(&staged_path, PROD_ROOT)? != expected {
        return Err(InstallError::Invalid(
            "staged Linux bundle differs from signed archive",
        ));
    }
    sync_directory(temporary.path())?;
    let prepared = InstallPreparation {
        installed_path: update.installed_path.clone(),
        staging_dir: temporary.path().to_owned(),
        payload_path,
        staged_path,
        version: bundle.version,
        build: bundle.build,
        size: artifact.size,
        sha256: artifact.sha256.clone(),
    };
    let _ = temporary.keep();
    Ok(prepared)
}

pub(super) fn revalidate(prepared: &InstallPreparation, update: &PreparedUpdate) -> Result<()> {
    preflight(&update.installed_path)?;
    upgrade_destination(update)?;
    validate_stage(&update.installed_path, &prepared.staging_dir)?;
    let artifact = expected_artifact(update)?;
    if prepared.installed_path != update.installed_path
        || prepared.payload_path != prepared.staging_dir.join("payload")
        || prepared.staged_path != prepared.staging_dir.join(PROD_ROOT)
        || prepared.version != update.release.manifest.version
        || prepared.build != update.release.manifest.build
        || prepared.size != artifact.size
        || prepared.sha256 != artifact.sha256
        || fs::symlink_metadata(prepared.staging_dir.join(RECEIPT_NAME)).is_ok()
    {
        return Err(InstallError::Invalid("persisted Linux preparation changed"));
    }
    verify_payload(&prepared.payload_path, artifact)?;
    let (expected, bundle) = archive(&prepared.payload_path, None, "prod")?;
    bundle.matches_release(&update.release)?;
    if scan_tree_root(&prepared.staged_path, PROD_ROOT)? != expected {
        return Err(InstallError::Invalid(
            "staged Linux content differs from signed archive",
        ));
    }
    Ok(())
}

pub(super) fn discard(prepared: &InstallPreparation) -> Result<()> {
    validate_destination(&prepared.installed_path)?;
    validate_stage(&prepared.installed_path, &prepared.staging_dir)?;
    if prepared.payload_path != prepared.staging_dir.join("payload")
        || prepared.staged_path != prepared.staging_dir.join(PROD_ROOT)
        || fs::symlink_metadata(prepared.staging_dir.join(RECEIPT_NAME)).is_ok()
    {
        return Err(InstallError::Invalid(
            "cannot discard a changed Linux preparation or retained backup",
        ));
    }
    fs::remove_dir_all(&prepared.staging_dir)?;
    sync_directory(prepared.installed_path.parent().unwrap())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    format: String,
    installed: InstalledUpdate,
    envelope: String,
}

pub(super) fn replace(
    prepared: &InstallPreparation,
    update: &PreparedUpdate,
) -> Result<InstalledUpdate> {
    replace_with_synchronizer(prepared, update, Synchronizer::Native)
}

fn replace_with_synchronizer(
    prepared: &InstallPreparation,
    update: &PreparedUpdate,
    synchronizer: Synchronizer,
) -> Result<InstalledUpdate> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(InstallError::NotifyOnly(
            "automatic Linux installation requires x86_64 Linux",
        ));
    }
    revalidate(prepared, update)?;
    let mut installed = InstalledUpdate {
        installed_path: prepared.installed_path.clone(),
        staging_dir: prepared.staging_dir.clone(),
        backup_path: prepared.staged_path.clone(),
        version: prepared.version.clone(),
        build: prepared.build.clone(),
        durability_warning: false,
    };
    let receipt = Receipt {
        format: "maple-agent-linux-install-v1".into(),
        installed: installed.clone(),
        envelope: update.release.envelope.clone(),
    };
    let bytes = serde_json::to_vec(&receipt)
        .map_err(|_| InstallError::Invalid("cannot serialize Linux receipt"))?;
    if bytes.len() as u64 > RECEIPT_LIMIT {
        return Err(InstallError::Invalid("Linux receipt is too large"));
    }
    let mut file = tempfile::NamedTempFile::new_in(&prepared.staging_dir)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(prepared.staging_dir.join(RECEIPT_NAME))
        .map_err(|error| error.error)?;
    sync_directory(&prepared.staging_dir)?;
    // Persist the private stage's parent entry before it can retain the old app.
    sync_directory(prepared.installed_path.parent().unwrap())?;
    if let Err(error) = exchange(&prepared.installed_path, &prepared.staged_path) {
        let _ = fs::remove_file(prepared.staging_dir.join(RECEIPT_NAME));
        return Err(error);
    }
    // Successful exchange is the commit point, including later fsync errors.
    let stage_synced = synchronizer.sync(&prepared.staging_dir).is_ok();
    let parent_synced = synchronizer
        .sync(prepared.installed_path.parent().unwrap())
        .is_ok();
    installed.durability_warning = !stage_synced || !parent_synced;
    Ok(installed)
}

fn exchange(installed: &Path, staged: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let installed = CString::new(installed.as_os_str().as_bytes())
            .map_err(|_| InstallError::Invalid("invalid installation path"))?;
        let staged = CString::new(staged.as_os_str().as_bytes())
            .map_err(|_| InstallError::Invalid("invalid staging path"))?;
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                installed.as_ptr(),
                libc::AT_FDCWD,
                staged.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        } != 0
        {
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP)
            ) {
                return Err(InstallError::NotifyOnly(
                    "this filesystem does not support atomic Linux bundle replacement",
                ));
            }
            return Err(error.into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (installed, staged);
        Err(InstallError::NotifyOnly(
            "Linux replacement is unavailable on this platform",
        ))
    }
}

/// After SQL/frontend startup succeeds, authenticate retained receipts using
/// compiled key pins and the actual running identity before deleting old bytes.
pub fn cleanup_current_linux_bundle(
    installed: &Path,
    running_version: &str,
    running_source_sha: &str,
    compiled_origin: &Url,
    compiled_public_keys: &[String],
) -> Result<usize> {
    validate_destination(installed)?;
    let _lock = acquire_install_lock(installed)?;
    let current = installed_bundle(installed)?;
    if current.version != running_version || current.source_sha != running_source_sha {
        return Err(InstallError::Invalid(
            "installed Linux identity differs from running app",
        ));
    }
    let parent = installed.parent().unwrap();
    let prefix = stage_prefix(installed);
    let mut cleaned = 0;
    for entry in fs::read_dir(parent)?.take(4096) {
        let entry = entry?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix))
        {
            continue;
        }
        let stage = entry.path();
        if validate_stage(installed, &stage).is_err() {
            continue;
        }
        match fs::symlink_metadata(stage.join(RECEIPT_NAME)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::remove_dir_all(&stage)?;
                sync_directory(parent)?;
                cleaned += 1;
                continue;
            }
            Ok(_) => {}
            Err(_) => continue,
        }
        // An invalid receipt may still retain the previous install. Preserve it.
        let verified = (|| -> Result<()> {
            let receipt: Receipt =
                serde_json::from_slice(&read_bounded(&stage.join(RECEIPT_NAME), RECEIPT_LIMIT)?)
                    .map_err(|_| InstallError::Invalid("invalid Linux install receipt"))?;
            if receipt.format != "maple-agent-linux-install-v1"
                || receipt.installed.installed_path != installed
                || receipt.installed.staging_dir != stage
                || receipt.installed.backup_path != stage.join(PROD_ROOT)
                || receipt.installed.version != running_version
                || receipt.installed.build != current.build
            {
                return Err(InstallError::Invalid("Linux receipt identity changed"));
            }
            let release = manifest::verify_envelope(
                receipt.envelope.as_bytes(),
                compiled_origin,
                compiled_public_keys,
            )
            .map_err(|_| InstallError::Invalid("Linux receipt signature is invalid"))?;
            current.matches_release(&release)?;
            let artifact = release
                .artifact(Target::LinuxX86_64)
                .map_err(|_| InstallError::Invalid("Linux receipt has the wrong payload format"))?;
            let payload = stage.join("payload");
            verify_payload(&payload, artifact)?;
            let (expected, bundle) = archive(&payload, None, "prod")?;
            bundle.matches_release(&release)?;
            if scan_tree_root(installed, PROD_ROOT)? != expected {
                return Err(InstallError::Invalid(
                    "installed Linux tree differs from signed archive",
                ));
            }
            Ok(())
        })();
        if verified.is_ok() {
            fs::remove_dir_all(&stage)?;
            sync_directory(parent)?;
            cleaned += 1;
        }
    }
    Ok(cleaned)
}

#[cfg(all(test, unix))]
#[path = "linux_bundle_tests.rs"]
mod tests;
