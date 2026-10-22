//! Agent-only update orchestration. All requests and filesystem operations run
//! on the backend runtime; views receive precomputed, truthful snapshots.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use maple_agent_updater::{
    CheckOutcome, Config, PreparedUpdate, Revalidation, Target, Updater, VerifiedRelease,
};
use tokio::sync::{Mutex, OnceCell, watch};
use tokio_util::sync::CancellationToken;

gpui::actions!(maple_updates, [AboutAgent, CheckForUpdates]);

pub struct UpdateRequested(pub UpdateAction);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Disabled,
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Ready,
    Installing,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAction {
    Check,
    Download,
    ManualDownload,
    Restart,
    Later,
    AutomaticDownloads(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub state: State,
    pub label: String,
    pub version: Option<String>,
    pub primary: Option<UpdateAction>,
    pub primary_label: &'static str,
    pub manual_download_url: Option<String>,
    pub show_banner: bool,
}

impl Snapshot {
    fn new(state: State, label: impl Into<String>) -> Self {
        Self {
            state,
            label: label.into(),
            version: None,
            primary: None,
            primary_label: "",
            manual_download_url: None,
            show_banner: false,
        }
    }
    pub fn initial() -> Self {
        match disabled_reason(
            crate::profile::PROFILE,
            crate::profile::UPDATE_PUBLIC_KEYS,
            crate::env::env_flag("MAPLE_DISABLE_UPDATE_CHECK"),
        ) {
            Some(reason) => Self::new(State::Disabled, reason),
            None => Self::new(State::Idle, "Not checked yet"),
        }
    }
    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            State::Checking | State::Downloading | State::Installing
        )
    }
}

fn disabled_reason(profile: &str, keys: &[&str], hard_off: bool) -> Option<&'static str> {
    if hard_off {
        Some("Update checks disabled")
    } else if profile != "prod" {
        Some("Updates are available in packaged Prod builds")
    } else if keys.is_empty() {
        Some("Updates not configured")
    } else {
        None
    }
}

struct Service {
    updater: Arc<Updater>,
    installed_path: PathBuf,
    manual_only: bool,
}
#[derive(Default)]
struct ReleaseState {
    release: Option<VerifiedRelease>,
    prepared: Option<PreparedUpdate>,
    installation: Option<maple_agent_updater::installer::InstallPreparation>,
    install_lock: Option<maple_agent_updater::installer::InstallLock>,
    deferred: Option<String>,
}
struct Inner {
    service: OnceCell<Service>,
    operation: Mutex<()>,
    releases: Mutex<ReleaseState>,
    snapshots: watch::Sender<Snapshot>,
    automatic: AtomicBool,
    cancel: CancellationToken,
}

/// One controller per desktop process. The operation mutex covers each complete
/// check/download/install; repeated clicks cannot launch competing transactions.
#[derive(Clone)]
pub struct Controller(Arc<Inner>);

impl Controller {
    pub fn new(automatic_downloads: bool) -> Self {
        let (snapshots, _) = watch::channel(Snapshot::initial());
        Self(Arc::new(Inner {
            service: OnceCell::new(),
            operation: Mutex::new(()),
            releases: Mutex::new(ReleaseState::default()),
            snapshots,
            automatic: AtomicBool::new(automatic_downloads),
            cancel: CancellationToken::new(),
        }))
    }
    pub fn snapshot(&self) -> Snapshot {
        self.0.snapshots.borrow().clone()
    }
    #[cfg(test)]
    pub(crate) fn set_test_state(&self, state: State) {
        self.publish(Snapshot::new(state, "Test update state"));
    }
    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.0.snapshots.subscribe()
    }
    pub fn shutdown(&self) {
        self.0.cancel.cancel();
    }
    /// Best effort on graceful exit. Keep the authenticated downloaded payload
    /// for restoration, but remove the derived same-volume staging bundle.
    pub async fn cleanup_on_exit(&self) {
        let Ok(_operation) =
            tokio::time::timeout(Duration::from_secs(2), self.0.operation.lock()).await
        else {
            return;
        };
        let mut releases = self.0.releases.lock().await;
        let staging = releases.installation.take();
        let lock = releases.install_lock.take();
        drop(releases);
        if let Some(staging) = staging {
            discard_preparation_with_lock(staging, lock).await;
        }
    }
    fn publish(&self, snapshot: Snapshot) {
        self.0.snapshots.send_if_modified(|current| {
            if *current == snapshot {
                false
            } else {
                *current = snapshot;
                true
            }
        });
    }
    fn begin_check(&self) {
        let previous = self.snapshot();
        let mut checking = Snapshot::new(State::Checking, "Checking for updates…");
        checking.version = previous.version;
        checking.manual_download_url = previous.manual_download_url;
        self.publish(checking);
    }
    fn fail(&self, message: impl Into<String>) {
        let previous = self.snapshot();
        let mut snapshot = Snapshot::new(State::Error, message);
        snapshot.version = previous.version;
        snapshot.manual_download_url = previous.manual_download_url;
        snapshot.show_banner = snapshot.version.is_some();
        snapshot.primary = Some(UpdateAction::Check);
        snapshot.primary_label = "Retry";
        self.publish(snapshot);
    }
    async fn service(&self) -> Result<&Service, String> {
        self.0
            .service
            .get_or_try_init(|| async {
                if self.snapshot().state == State::Disabled {
                    return Err(self.snapshot().label);
                }
                tokio::task::spawn_blocking(build_service)
                    .await
                    .map_err(|_| "Could not initialize updates".to_string())?
            })
            .await
    }
    /// Owned by the desktop lifetime. Cancellation interrupts polling and HTTP
    /// work; a blocking atomic replacement, once started, finishes deliberately.
    pub async fn run(self) {
        if self.snapshot().state == State::Disabled || self.0.cancel.is_cancelled() {
            return;
        }
        let startup = async {
            if let Ok(service) = self.service().await {
                let installed = service.installed_path.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    maple_agent_updater::installer::cleanup_current_installation(
                        &installed,
                        env!("CARGO_PKG_VERSION"),
                    )
                })
                .await;
                match service.updater.restore_prepared().await {
                    Ok(Some(prepared)) => {
                        let mut releases = self.0.releases.lock().await;
                        releases.release = Some(prepared.release.clone());
                        releases.prepared = Some(prepared);
                    }
                    Ok(None) => {}
                    Err(_) => self.fail("Saved update could not be verified; checking again"),
                }
            }
            self.check().await;
        };
        tokio::select! { _ = self.0.cancel.cancelled() => return, _ = startup => {} }
        loop {
            let delay = Duration::from_secs(3300 + rand::random::<u64>() % 601);
            tokio::select! {
                _ = self.0.cancel.cancelled() => return,
                _ = tokio::time::sleep(delay) => {},
            }
            tokio::select! { _ = self.0.cancel.cancelled() => return, _ = self.check() => {} }
        }
    }
    pub async fn check(&self) {
        if self.0.cancel.is_cancelled() {
            return;
        }
        let Ok(_operation) = self.0.operation.try_lock() else {
            return;
        };
        self.check_inner().await;
    }
    async fn check_inner(&self) {
        if self.snapshot().state == State::Disabled {
            return;
        }
        self.begin_check();
        let service = match self.service().await {
            Ok(service) => service,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        match service.updater.check().await {
            Ok(CheckOutcome::Current | CheckOutcome::Withdrawn) => {
                let mut releases = self.0.releases.lock().await;
                releases.release = None;
                releases.prepared = None;
                let staging = releases.installation.take();
                let lock = releases.install_lock.take();
                drop(releases);
                if let Some(staging) = staging {
                    discard_preparation_with_lock(staging, lock).await;
                }
                let _ = service.updater.clear_prepared().await;
                let mut snapshot = Snapshot::new(State::Current, "Up to date");
                snapshot.primary = Some(UpdateAction::Check);
                snapshot.primary_label = "Check for Updates";
                self.publish(snapshot);
            }
            Ok(CheckOutcome::Available(release)) => self.accept_release(service, *release).await,
            Err(_) => {
                self.fail("Could not check for updates. Your current app is still available.")
            }
        }
    }
    async fn accept_release(&self, service: &Service, release: VerifiedRelease) {
        let (prepared, previous_staging, previous_lock) = {
            let mut releases = self.0.releases.lock().await;
            let prepared = releases
                .prepared
                .as_ref()
                .filter(|p| p.release.envelope == release.envelope)
                .cloned();
            let (previous_staging, previous_lock) = if prepared.is_none() {
                releases.prepared = None;
                (releases.installation.take(), releases.install_lock.take())
            } else {
                (None, None)
            };
            releases.release = Some(release.clone());
            (prepared, previous_staging, previous_lock)
        };
        if let Some(staging) = previous_staging {
            discard_preparation_with_lock(staging, previous_lock).await;
        }
        if let Some(prepared) = prepared.filter(|_| !service.manual_only) {
            match self.stage_prepared(prepared).await {
                Ok(()) => self.release_snapshot(service, &release, State::Ready).await,
                Err(error) => self.fail(error),
            }
        } else if self.0.automatic.load(Ordering::Relaxed) && !service.manual_only {
            self.download_inner(service, release).await;
        } else {
            self.release_snapshot(service, &release, State::Available)
                .await;
        }
    }
    async fn stage_prepared(&self, prepared: PreparedUpdate) -> Result<(), String> {
        // A restored Ready flag is never trusted. Extract/check the signed
        // package before offering Restart, then check it again at installation.
        let mut releases = self.0.releases.lock().await;
        let previous = releases.installation.take();
        let previous_lock = releases.install_lock.take();
        drop(releases);
        let (installation, lock) = tokio::task::spawn_blocking(move || {
            let lock = match previous_lock {
                Some(lock) => lock,
                None => {
                    maple_agent_updater::installer::acquire_install_lock(&prepared.installed_path)
                        .map_err(|e| e.to_string())?
                }
            };
            if let Some(previous) = previous {
                match maple_agent_updater::installer::revalidate(&previous, &prepared) {
                    Ok(()) => return Ok((previous, lock)),
                    Err(error) => {
                        let _ = maple_agent_updater::installer::discard_preparation(&previous);
                        return Err(error.to_string());
                    }
                }
            }
            let installation =
                maple_agent_updater::installer::prepare(&prepared).map_err(|e| e.to_string())?;
            Ok((installation, lock))
        })
        .await
        .map_err(|_| "Could not prepare the update".to_string())??;
        let mut releases = self.0.releases.lock().await;
        releases.installation = Some(installation);
        releases.install_lock = Some(lock);
        Ok(())
    }
    async fn release_snapshot(&self, service: &Service, release: &VerifiedRelease, state: State) {
        let version = release.manifest.version.clone();
        let mut snapshot = Snapshot::new(
            state,
            match state {
                State::Ready => format!("v{version} is ready to install"),
                State::Downloading => format!("Downloading v{version}…"),
                _ if service.manual_only => {
                    format!("v{version} is available for manual installation")
                }
                _ => format!("v{version} is available"),
            },
        );
        snapshot.version = Some(version.clone());
        snapshot.manual_download_url = manual_download_url(release);
        if state == State::Ready {
            snapshot.primary = Some(UpdateAction::Restart);
            snapshot.primary_label = "Restart to Update";
        } else if state == State::Available {
            snapshot.primary = Some(if service.manual_only {
                UpdateAction::ManualDownload
            } else {
                UpdateAction::Download
            });
            snapshot.primary_label = "Download";
        }
        snapshot.show_banner = state != State::Downloading
            && self.0.releases.lock().await.deferred.as_ref() != Some(&version);
        self.publish(snapshot);
    }
    pub async fn download(&self) {
        if self.0.cancel.is_cancelled() {
            return;
        }
        let Ok(_operation) = self.0.operation.try_lock() else {
            return;
        };
        let service = match self.service().await {
            Ok(service) => service,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let release = self.0.releases.lock().await.release.clone();
        if let Some(release) = release {
            self.download_inner(service, release).await;
        }
    }
    async fn download_inner(&self, service: &Service, release: VerifiedRelease) {
        if service.manual_only {
            self.release_snapshot(service, &release, State::Available)
                .await;
            return;
        }
        self.release_snapshot(service, &release, State::Downloading)
            .await;
        match service.updater.download(&release).await {
            Ok(prepared) => {
                self.0.releases.lock().await.prepared = Some(prepared.clone());
                match self.stage_prepared(prepared).await {
                    Ok(()) => self.release_snapshot(service, &release, State::Ready).await,
                    Err(error) => self.fail(error),
                }
            }
            Err(_) => {
                self.fail("Could not download or verify the update. Retry when you are ready.")
            }
        }
    }
    pub async fn set_auto_download(&self, enabled: bool) {
        self.0.automatic.store(enabled, Ordering::Relaxed);
        if enabled && self.snapshot().state == State::Available {
            self.download().await;
        }
    }
    pub async fn later(&self) {
        self.0.releases.lock().await.deferred = self.snapshot().version;
        let mut snapshot = self.snapshot();
        snapshot.show_banner = false;
        self.publish(snapshot);
    }
    /// Called only after the desktop's explicit task-stop confirmation. Returns
    /// the verified installed path for GPUI's deliberate restart handoff.
    pub async fn install(&self, api_url: String) -> Result<Option<PathBuf>, String> {
        if self.0.cancel.is_cancelled() {
            return Ok(None);
        }
        let Ok(_operation) = self.0.operation.try_lock() else {
            return Ok(None);
        };
        let result = self.install_inner(api_url).await;
        if let Err(error) = &result {
            self.fail(error.clone());
        }
        result
    }
    async fn install_inner(&self, api_url: String) -> Result<Option<PathBuf>, String> {
        let service = self.service().await?;
        let prepared = self
            .0
            .releases
            .lock()
            .await
            .prepared
            .clone()
            .ok_or("No verified update is ready")?;
        let mut installing = Snapshot::new(State::Installing, "Preparing to restart and update…");
        installing.version = Some(prepared.release.manifest.version.clone());
        installing.manual_download_url = manual_download_url(&prepared.release);
        self.publish(installing);
        let installed_path = service.installed_path.clone();
        let staging = prepared.clone();
        let mut releases = self.0.releases.lock().await;
        let saved = releases.installation.take();
        let saved_lock = releases.install_lock.take();
        drop(releases);
        let (lock, preparation) = tokio::task::spawn_blocking(move || {
            let lock = match saved_lock {
                Some(lock) => lock,
                None => maple_agent_updater::installer::acquire_install_lock(&installed_path)
                    .map_err(|e| e.to_string())?,
            };
            let preparation = match saved {
                Some(preparation) => {
                    if let Err(error) =
                        maple_agent_updater::installer::revalidate(&preparation, &staging)
                    {
                        let _ = maple_agent_updater::installer::discard_preparation(&preparation);
                        return Err(error.to_string());
                    }
                    preparation
                }
                None => {
                    maple_agent_updater::installer::prepare(&staging).map_err(|e| e.to_string())?
                }
            };
            Ok::<_, String>((lock, preparation))
        })
        .await
        .map_err(|_| "Could not prepare installation".to_string())??;
        let fresh = match service.updater.revalidate(&prepared).await {
            Ok(fresh) => fresh,
            Err(_) => {
                discard_preparation(preparation).await;
                return Err("Could not revalidate the update. Maple is still running.".into());
            }
        };
        match fresh {
            Revalidation::Ready(_) => {}
            Revalidation::Superseded(release) => {
                discard_preparation(preparation).await;
                drop(lock);
                self.0.releases.lock().await.prepared = None;
                service
                    .updater
                    .clear_prepared()
                    .await
                    .map_err(|e| e.to_string())?;
                self.accept_release(service, release).await;
                return Ok(None);
            }
            Revalidation::Withdrawn => {
                discard_preparation(preparation).await;
                drop(lock);
                self.0.releases.lock().await.prepared = None;
                self.0.releases.lock().await.release = None;
                service
                    .updater
                    .clear_prepared()
                    .await
                    .map_err(|e| e.to_string())?;
                self.publish(Snapshot::new(
                    State::Current,
                    "This update is no longer offered. Check again later.",
                ));
                return Ok(None);
            }
        }
        let recovery_api = api_url.clone();
        let updater = service.updater.clone();
        let result = tokio::task::spawn_blocking(move || {
            let _lock = lock;
            let result = crate::host::with_update_fence(&api_url, || {
                updater.with_commit_guard(&prepared, || {
                // replace performs the final payload/tree/codesign checks
                // while both process fences and the commit guard are held.
                let installed = maple_agent_updater::installer::replace(&preparation, &prepared).map_err(|e| e.to_string())?;
                if installed.durability_warning {
                    log::warn!("The update was installed; filesystem synchronization did not complete. Restarting the installed application.");
                }
                Ok(installed.installed_path)
                }).map_err(|error| error.to_string())
            });
            if result.is_err() {
                let _ = maple_agent_updater::installer::discard_preparation(&preparation);
            }
            result
        }).await.map_err(|_| "Installation did not complete; retry or reinstall Maple".to_string())?;
        match result {
            Ok(path) => Ok(Some(path)),
            Err(error) => {
                // An Err is guaranteed to precede the atomic swap. The host
                // fence and application lock have both dropped before recovery.
                if crate::host::recover_after_failed_update(&recovery_api)
                    .await
                    .is_err()
                {
                    return Err(format!(
                        "{error}. Quit and reopen Maple to restore its host."
                    ));
                }
                Err(error)
            }
        }
    }
}

async fn discard_preparation(preparation: maple_agent_updater::installer::InstallPreparation) {
    discard_preparation_with_lock(preparation, None).await;
}

async fn discard_preparation_with_lock(
    preparation: maple_agent_updater::installer::InstallPreparation,
    lock: Option<maple_agent_updater::installer::InstallLock>,
) {
    let _ = tokio::task::spawn_blocking(move || {
        let _lock = lock;
        maple_agent_updater::installer::discard_preparation(&preparation)
    })
    .await;
}

fn manual_download_url(release: &VerifiedRelease) -> Option<String> {
    let artifact = if cfg!(target_os = "linux") {
        release.manifest.downloads.linux_appimage.as_ref()?
    } else {
        &release.manifest.downloads.macos
    };
    release
        .origin
        .join(&artifact.path)
        .ok()
        .map(|url| url.to_string())
}

fn build_service() -> Result<Service, String> {
    let (installed_path, manual_only) = installation()?;
    let target = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Target::DarwinAarch64
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Target::LinuxX86_64
    } else {
        return Err("Updates are not supported on this platform".into());
    };
    let config = Config {
        feed_url: crate::profile::UPDATE_FEED_URL
            .ok_or("Updates not configured")?
            .parse()
            .map_err(|_| "Invalid packaged update configuration")?,
        public_keys: crate::profile::UPDATE_PUBLIC_KEYS
            .iter()
            .map(|key| (*key).to_owned())
            .collect(),
        current_version: env!("CARGO_PKG_VERSION")
            .parse()
            .map_err(|_| "Invalid app version")?,
        target,
        installed_path: installed_path.clone(),
        state_dir: crate::backend::local_data_root().join("updates"),
    };
    Ok(Service {
        updater: Arc::new(
            Updater::new(config).map_err(|_| "Invalid packaged update configuration")?,
        ),
        installed_path,
        manual_only,
    })
}

fn installation() -> Result<(PathBuf, bool), String> {
    let exe = std::env::current_exe().map_err(|_| "Could not locate the installed app")?;
    if cfg!(target_os = "macos") {
        let app = exe
            .ancestors()
            .nth(3)
            .filter(|path| path.extension().is_some_and(|e| e == "app"))
            .ok_or("Automatic updates require an installed Maple app")?;
        let app = std::fs::canonicalize(app).map_err(|_| "Could not locate the installed app")?;
        let manual = manual_macos_path(&app)
            || maple_agent_updater::installer::discover_installation(&exe, None).is_err();
        Ok((app, manual))
    } else {
        // Linux stays notify-only until packaged host/update/relaunch gates are
        // demonstrated. The launch helper fix is an independent prerequisite.
        Ok((exe, true))
    }
}
fn manual_macos_path(path: &Path) -> bool {
    path.starts_with("/Volumes")
        || path
            .components()
            .any(|component| component.as_os_str() == "AppTranslocation")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packaged_prod_needs_pins_and_dev_cannot_enable_updates() {
        assert_eq!(
            disabled_reason("prod", &[], false),
            Some("Updates not configured")
        );
        assert!(disabled_reason("prod", &["key"], false).is_none());
        for profile in ["dev", "unpackaged"] {
            assert!(disabled_reason(profile, &["key"], false).is_some());
        }
        assert_eq!(
            disabled_reason("prod", &["key"], true),
            Some("Update checks disabled")
        );
    }
    #[test]
    fn disk_image_and_translocated_apps_use_manual_installation() {
        assert!(manual_macos_path(Path::new(
            "/Volumes/Maple/Maple Agent.app"
        )));
        assert!(manual_macos_path(Path::new(
            "/private/var/folders/a/AppTranslocation/x/d/Maple Agent.app"
        )));
        assert!(!manual_macos_path(Path::new(
            "/Applications/Maple Agent.app"
        )));
    }
    #[tokio::test]
    async fn later_retains_verified_ready_action_without_a_banner() {
        let controller = Controller::new(true);
        let mut ready = Snapshot::new(State::Ready, "v0.0.2 is ready to install");
        ready.version = Some("0.0.2".into());
        ready.primary = Some(UpdateAction::Restart);
        ready.primary_label = "Restart to Update";
        ready.show_banner = true;
        controller.publish(ready);
        controller.later().await;
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.state, State::Ready);
        assert_eq!(snapshot.primary, Some(UpdateAction::Restart));
        assert!(!snapshot.show_banner);
        assert_eq!(
            controller.0.releases.lock().await.deferred.as_deref(),
            Some("0.0.2")
        );
    }
    #[test]
    fn failed_recheck_keeps_the_verified_manual_download_offer() {
        let controller = Controller::new(false);
        let mut downloading = Snapshot::new(State::Ready, "v0.0.2 is ready to install");
        downloading.version = Some("0.0.2".into());
        downloading.manual_download_url =
            Some("https://updates.trymaple.ai/agent/stable/releases/0.0.2/Maple.dmg".into());
        controller.publish(downloading);
        controller.begin_check();
        assert_eq!(controller.snapshot().state, State::Checking);
        controller.fail("Could not check for updates");
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.state, State::Error);
        assert_eq!(snapshot.primary, Some(UpdateAction::Check));
        assert!(snapshot.manual_download_url.is_some());
        assert!(snapshot.show_banner);
    }
    #[tokio::test]
    async fn repeated_checks_cannot_replace_an_active_transaction() {
        let controller = Controller::new(false);
        let _active = controller.0.operation.lock().await;
        controller.publish(Snapshot::new(State::Installing, "Installing…"));
        controller.check().await;
        controller.download().await;
        assert_eq!(controller.snapshot().state, State::Installing);
        assert!(controller.0.service.get().is_none());
    }
    #[tokio::test]
    async fn shutdown_prevents_new_network_or_install_transactions() {
        let controller = Controller::new(true);
        controller.publish(Snapshot::new(State::Idle, "Not checked yet"));
        controller.shutdown();
        controller.clone().run().await;
        controller.check().await;
        controller.download().await;
        assert!(
            controller
                .install("https://unused.invalid".into())
                .await
                .unwrap()
                .is_none()
        );
        assert!(controller.0.service.get().is_none());
        assert_eq!(controller.snapshot().state, State::Idle);
    }
    #[tokio::test]
    async fn disabled_controller_is_accurate_and_has_no_network_work() {
        let controller = Controller::new(true);
        controller.publish(Snapshot::new(State::Disabled, "Test disables updates"));
        assert_eq!(controller.snapshot().state, State::Disabled);
        controller.clone().run().await;
        controller.check().await;
        assert_eq!(controller.snapshot().state, State::Disabled);
        assert!(controller.0.service.get().is_none());
    }
}
