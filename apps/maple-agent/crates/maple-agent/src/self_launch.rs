//! Resolve the application entry point, including Linux's bundled loader.
//!
//! AppRun executes the loader explicitly, so `current_exe()` names that
//! loader. Re-enter the persistent bundle's AppRun for host startup and restart.
//! Legacy AppImages re-enter their outer image to retain an independent mount.

use std::{path::PathBuf, process::Command};

pub fn command() -> Result<Command, String> {
    executable().map(Command::new)
}

/// Re-execute a bounded child within this process's package mount. This uses
/// the loader directly, without an AppImage runtime or shell between parent
/// and child, so private stdin and parent-death/cancellation ownership survive.
/// The caller may clear the child's environment after resolving this command.
pub fn child_command() -> Result<Command, String> {
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    #[cfg(target_os = "linux")]
    {
        linux_child_command(
            current,
            std::env::args_os().next().as_deref(),
            std::env::var_os("APPDIR").as_deref(),
            std::env::var_os("APPIMAGE").as_deref(),
            std::env::var_os("APPIMAGE_EXTRACT_AND_RUN").is_some(),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(Command::new(current))
    }
}

pub fn executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    #[cfg(target_os = "linux")]
    {
        linux_executable(
            current,
            std::env::args_os().next().as_deref(),
            std::env::var_os("APPDIR").as_deref(),
            std::env::var_os("APPIMAGE").as_deref(),
            std::env::var_os("APPIMAGE_EXTRACT_AND_RUN").is_some(),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(current)
    }
}

#[cfg(any(target_os = "linux", all(test, unix)))]
const LOADER: &str = "ld-linux-x86-64.so.2";

#[cfg(any(target_os = "linux", all(test, unix)))]
fn linux_executable(
    current: PathBuf,
    argv0: Option<&std::ffi::OsStr>,
    appdir: Option<&std::ffi::OsStr>,
    appimage: Option<&std::ffi::OsStr>,
    extract_and_run: bool,
) -> Result<PathBuf, String> {
    linux_launch(current, argv0, appdir, appimage, extract_and_run).map(|launch| launch.executable)
}

#[cfg(any(target_os = "linux", all(test, unix)))]
struct LinuxPackage {
    directory: PathBuf,
    loader: PathBuf,
    payload: PathBuf,
}

#[cfg(any(target_os = "linux", all(test, unix)))]
struct LinuxLaunch {
    executable: PathBuf,
    package: Option<LinuxPackage>,
}

#[cfg(any(target_os = "linux", all(test, unix)))]
fn linux_child_command(
    current: PathBuf,
    argv0: Option<&std::ffi::OsStr>,
    appdir: Option<&std::ffi::OsStr>,
    appimage: Option<&std::ffi::OsStr>,
    extract_and_run: bool,
) -> Result<Command, String> {
    let launch = linux_launch(current, argv0, appdir, appimage, extract_and_run)?;
    let Some(package) = launch.package else {
        return Ok(Command::new(launch.executable));
    };
    let mut library_path = package.directory.join("usr/lib").into_os_string();
    // Match AppRun's explicit loader path without relying on inherited linker
    // variables. The checker clears its environment before spawning.
    library_path
        .push(":/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu:/lib64:/usr/lib64:/lib:/usr/lib");
    let mut command = Command::new(package.loader);
    command
        .args(["--inhibit-cache", "--library-path"])
        .arg(library_path)
        .arg(package.payload);
    Ok(command)
}

#[cfg(any(target_os = "linux", all(test, unix)))]
fn linux_launch(
    current: PathBuf,
    argv0: Option<&std::ffi::OsStr>,
    appdir: Option<&std::ffi::OsStr>,
    appimage: Option<&std::ffi::OsStr>,
    extract_and_run: bool,
) -> Result<LinuxLaunch, String> {
    use std::{io::Read, path::Path};

    // Ignore another AppImage's inherited variables for ordinary binaries.
    // A loader or executable inside APPDIR is a recognized package context:
    // inconsistent inputs there must never fall back to executing the loader.
    let in_appdir = appdir.is_some_and(|path| current.starts_with(Path::new(path)));
    if current.file_name() != Some(LOADER.as_ref()) && !in_appdir {
        return Ok(LinuxLaunch {
            executable: current,
            package: None,
        });
    }
    let invalid = || {
        "Cannot relaunch this Linux bundle: invalid APPDIR, loader or payload. Launch the intact installed maple-agent.app/AppRun, or run the original legacy .AppImage.".to_string()
    };
    let appdir = Path::new(appdir.ok_or_else(invalid)?);
    if !appdir.is_absolute() {
        return Err(invalid());
    }
    let appdir = appdir.canonicalize().map_err(|_| invalid())?;
    let loader = appdir.join("usr/lib").join(LOADER);
    let payload = appdir.join("usr/bin/maple-agent");
    let launcher = appdir.join("AppRun");
    let canonical_loader = loader.canonicalize().map_err(|_| invalid())?;
    let canonical_payload = payload.canonicalize().map_err(|_| invalid())?;
    let canonical_launcher = launcher.canonicalize().map_err(|_| invalid())?;
    let argv0 = Path::new(argv0.ok_or_else(invalid)?);
    if !argv0.is_absolute()
        || !canonical_loader.starts_with(&appdir)
        || !canonical_payload.starts_with(&appdir)
        || !canonical_launcher.starts_with(&appdir)
        || current.canonicalize().map_err(|_| invalid())? != canonical_loader
        || argv0.canonicalize().map_err(|_| invalid())? != canonical_payload
        || !is_executable_file(&loader)
        || !is_executable_file(&payload)
        || !is_executable_file(&launcher)
    {
        return Err(invalid());
    }

    // The pinned runtime shares this extraction directory across invocations
    // and deletes it when *either* wrapper exits. A detached host would lose
    // its resources. Reject both the environment and command-line forms.
    if extract_and_run
        || appdir
            .file_name()
            .is_some_and(|name| name.as_encoded_bytes().starts_with(b"appimage_extracted_"))
    {
        return Err("Maple host startup and restart do not support temporary --appimage-extract-and-run mode. Install FUSE and run the .AppImage normally, or extract it once with --appimage-extract and run squashfs-root/AppRun.".into());
    }
    let package = Some(LinuxPackage {
        directory: appdir.clone(),
        loader: canonical_loader,
        payload: canonical_payload,
    });
    let Some(appimage) = appimage else {
        // Official tar bundles and persistent extractions need no runtime.
        return Ok(LinuxLaunch {
            executable: canonical_launcher,
            package,
        });
    };
    let appimage = Path::new(appimage);
    if !appimage.is_absolute() || !is_executable_file(appimage) {
        return Err(invalid());
    }
    let appimage = appimage.canonicalize().map_err(|_| invalid())?;
    if appimage.starts_with(&appdir) {
        return Err(invalid());
    }
    let mut header = [0; 11];
    std::fs::File::open(&appimage)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| invalid())?;
    if &header[..4] != b"\x7fELF" || &header[8..] != b"AI\x02" {
        return Err(invalid());
    }
    Ok(LinuxLaunch {
        executable: appimage,
        package,
    })
}

#[cfg(any(target_os = "linux", all(test, unix)))]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{ffi::OsStr, os::unix::fs::PermissionsExt, path::Path};

    struct Package {
        temporary: PathBuf,
        appdir: PathBuf,
        loader: PathBuf,
        payload: PathBuf,
        image: PathBuf,
    }

    fn executable_file(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    impl Package {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let temporary = std::env::temp_dir().join(format!(
                "maple-self-launch-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            ));
            std::fs::create_dir(&temporary).unwrap();
            let temporary = temporary.canonicalize().unwrap();
            let appdir = temporary.join("install with spaces/maple-agent.app");
            let loader = appdir.join("usr/lib").join(LOADER);
            let payload = appdir.join("usr/bin/maple-agent");
            let image = temporary.join("Maple Agent.AppImage");
            executable_file(&loader, b"loader");
            executable_file(&payload, b"payload");
            executable_file(&appdir.join("AppRun"), b"#!/bin/sh\n");
            executable_file(&image, b"\x7fELF\x02\x01\x01\x00AI\x02");
            Self {
                temporary,
                appdir,
                loader,
                payload,
                image,
            }
        }

        fn resolve(&self, image: Option<&OsStr>) -> Result<PathBuf, String> {
            linux_executable(
                self.loader.clone(),
                Some(self.payload.as_os_str()),
                Some(self.appdir.as_os_str()),
                image,
                false,
            )
        }
    }

    impl Drop for Package {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.temporary);
        }
    }

    #[test]
    fn image_reentry_uses_absolute_image_with_spaces_instead_of_loader() {
        let package = Package::new();
        assert_eq!(
            package.resolve(Some(package.image.as_os_str())).unwrap(),
            package.image
        );
    }

    #[test]
    fn normal_binary_ignores_inherited_appimage_context() {
        let package = Package::new();
        let ordinary = package.temporary.join("normal-agent");
        assert_eq!(
            linux_executable(
                ordinary.clone(),
                None,
                Some(package.appdir.as_os_str()),
                Some(package.image.as_os_str()),
                true
            )
            .unwrap(),
            ordinary
        );
    }

    #[test]
    fn tar_bundle_without_appimage_reenters_apprun() {
        let package = Package::new();
        assert_eq!(
            package.resolve(None).unwrap(),
            package.appdir.join("AppRun")
        );
    }

    #[test]
    fn bounded_child_uses_direct_loader_with_no_environment_or_shell_dependency() {
        let package = Package::new();
        for image in [None, Some(package.image.as_os_str())] {
            let mut command = linux_child_command(
                package.loader.clone(),
                Some(package.payload.as_os_str()),
                Some(package.appdir.as_os_str()),
                image,
                false,
            )
            .unwrap();
            command.env("PRIVATE_LAUNCH_CANARY", "private").env_clear();
            command
                .arg("--internal-store-integrity-check")
                .arg("/private/store-v1");
            assert_eq!(command.get_program(), package.loader.as_os_str());
            let args = command.get_args().collect::<Vec<_>>();
            assert_eq!(args[0], "--inhibit-cache");
            assert_eq!(args[1], "--library-path");
            assert!(
                args[2]
                    .to_str()
                    .unwrap()
                    .starts_with(package.appdir.join("usr/lib").to_str().unwrap())
            );
            assert_eq!(args[3], package.payload.as_os_str());
            assert_eq!(args[4], "--internal-store-integrity-check");
            assert_eq!(args[5], "/private/store-v1");
            assert_eq!(args.len(), 6);
            assert_eq!(command.get_envs().count(), 0);
        }
    }

    #[test]
    fn invalid_recognized_context_never_executes_loader() {
        let package = Package::new();
        assert!(
            linux_executable(
                package.loader.clone(),
                Some(package.payload.as_os_str()),
                None,
                Some(package.image.as_os_str()),
                false
            )
            .is_err()
        );
        assert!(
            linux_executable(
                package.loader.clone(),
                Some(package.payload.as_os_str()),
                Some(OsStr::new("relative/AppDir")),
                Some(package.image.as_os_str()),
                false
            )
            .is_err()
        );
        assert!(
            linux_executable(
                package.loader.clone(),
                Some(package.loader.as_os_str()),
                Some(package.appdir.as_os_str()),
                Some(package.image.as_os_str()),
                false
            )
            .is_err()
        );
        assert!(
            package
                .resolve(Some(OsStr::new("relative.AppImage")))
                .is_err()
        );
        assert!(package.resolve(Some(package.payload.as_os_str())).is_err());
        std::fs::write(&package.image, b"#!/bin/sh\nexit 0\n").unwrap();
        assert!(package.resolve(Some(package.image.as_os_str())).is_err());
    }

    #[test]
    fn missing_nonexecutable_and_escaped_package_files_are_rejected() {
        let package = Package::new();
        std::fs::set_permissions(&package.image, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(package.resolve(Some(package.image.as_os_str())).is_err());
        std::fs::remove_file(&package.payload).unwrap();
        assert!(package.resolve(None).is_err());
        std::os::unix::fs::symlink(&package.image, &package.payload).unwrap();
        assert!(package.resolve(None).is_err());
    }

    #[test]
    fn both_temporary_extract_and_run_forms_fail_with_recovery_instructions() {
        let package = Package::new();
        let error = linux_executable(
            package.loader.clone(),
            Some(package.payload.as_os_str()),
            Some(package.appdir.as_os_str()),
            Some(package.image.as_os_str()),
            true,
        )
        .unwrap_err();
        assert!(error.contains("Install FUSE"));
        let temporary_appdir = package.appdir.with_file_name("appimage_extracted_fakehash");
        std::fs::rename(&package.appdir, &temporary_appdir).unwrap();
        let error = linux_executable(
            temporary_appdir.join("usr/lib").join(LOADER),
            Some(temporary_appdir.join("usr/bin/maple-agent").as_os_str()),
            Some(temporary_appdir.as_os_str()),
            Some(package.image.as_os_str()),
            false,
        )
        .unwrap_err();
        assert!(error.contains("--appimage-extract"));
    }
}
