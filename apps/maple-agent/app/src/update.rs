//! Agent-only update orchestration. All requests and filesystem operations run
//! on the backend runtime; views receive precomputed, truthful snapshots.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc, OnceLock,
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
    RestartRequired,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAction {
    Check,
    Download,
    ManualDownload,
    Restart,
    Relaunch,
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
    manual_only: AtomicBool,
}
impl Service {
    fn manual_only(&self) -> bool {
        self.manual_only.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
struct InstallationFailure {
    message: String,
    manual: bool,
    recover_host: bool,
}
impl From<maple_agent_updater::installer::InstallError> for InstallationFailure {
    fn from(error: maple_agent_updater::installer::InstallError) -> Self {
        Self {
            manual: error.requires_manual_installation(),
            message: error.to_string(),
            recover_host: false,
        }
    }
}
impl From<String> for InstallationFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            manual: false,
            recover_host: false,
        }
    }
}
impl From<&str> for InstallationFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
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
    started: AtomicBool,
    cancel: CancellationToken,
    installed: OnceLock<CommittedInstallation>,
}
struct CommittedInstallation {
    path: PathBuf,
    // The GPUI helper waits for this PID to exit. Keep old-client admission
    // fenced until OS exit closes the files, then the new process can start.
    _host_fence: Option<std::mem::ManuallyDrop<crate::host::HostUpdateFence>>,
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
            started: AtomicBool::new(false),
            cancel: CancellationToken::new(),
            installed: OnceLock::new(),
        }))
    }
    pub fn snapshot(&self) -> Snapshot {
        self.0.snapshots.borrow().clone()
    }
    #[cfg(test)]
    pub(crate) fn startup_policy_for_test(&self) -> (bool, bool) {
        (
            self.0.started.load(Ordering::Acquire),
            self.0.automatic.load(Ordering::Acquire),
        )
    }
    #[cfg(test)]
    pub(crate) fn set_test_state(&self, state: State) {
        self.publish(Snapshot::new(state, "Test update state"));
    }
    #[cfg(test)]
    pub(crate) fn set_test_installed(&self, path: PathBuf) {
        self.installed(path, "0.0.2", None);
    }
    #[cfg(test)]
    pub(crate) fn set_test_installed_with_fence(
        &self,
        path: PathBuf,
        fence: crate::host::HostUpdateFence,
    ) {
        self.installed(path, "0.0.2", Some(fence));
    }
    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.0.snapshots.subscribe()
    }
    pub fn shutdown(&self) {
        self.0.cancel.cancel();
    }
    /// Once replacement commits, this process can only finish its restart.
    /// GPUI's restart API cannot report helper-spawn failure, so retain an
    /// accurate retryable state even when the old window remains open.
    pub fn restart_path(&self) -> Option<PathBuf> {
        self.0
            .installed
            .get()
            .map(|installed| installed.path.clone())
    }
    fn installed(
        &self,
        path: PathBuf,
        version: &str,
        host_fence: Option<crate::host::HostUpdateFence>,
    ) {
        // Exactly one committed update can occur in this process. Deliberately
        // leave its two lock descriptors open until OS exit, even if GPUI drops
        // its final controller Arc during teardown before the PID disappears.
        // Pending old-client startup must remain fenced during that interval.
        if let Err(rejected) = self.0.installed.set(CommittedInstallation {
            path,
            _host_fence: host_fence.map(std::mem::ManuallyDrop::new),
        }) {
            if let Some(fence) = rejected._host_fence {
                drop(std::mem::ManuallyDrop::into_inner(fence));
            }
            return;
        }
        self.0.cancel.cancel();
        let mut snapshot = Snapshot::new(
            State::RestartRequired,
            format!("v{version} is installed. Restart Maple to finish, or quit and reopen it."),
        );
        snapshot.version = Some(version.to_owned());
        snapshot.primary = Some(UpdateAction::Relaunch);
        snapshot.primary_label = "Restart Maple";
        snapshot.show_banner = true;
        self.publish(snapshot);
    }
    /// Start best-effort cleanup before GPUI drops the windows and their backend
    /// owners. GPUI polls its quit future without a Tokio reactor, so both the
    /// lock timeout and blocking filesystem work must run on the backend runtime.
    pub fn cleanup_on_exit(
        &self,
        backend: Arc<crate::backend::AgentBackend>,
    ) -> tokio::task::JoinHandle<()> {
        let controller = self.clone();
        let cleanup_backend = backend.clone();
        backend.spawn(async move {
            // A Handle does not keep an owned runtime alive. Retain its owner
            // even if GPUI's 200 ms shutdown budget stops awaiting this task.
            let _backend = cleanup_backend;
            controller.cleanup_staging_on_exit().await;
        })
    }
    /// Keep the authenticated downloaded payload for restoration, but remove
    /// only the derived uncommitted same-volume staging bundle.
    async fn cleanup_staging_on_exit(&self) {
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
        if self.restart_path().is_some() && snapshot.state != State::RestartRequired {
            return;
        }
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
    fn recheck_failed(&self, previous: Snapshot) {
        if previous.state == State::Ready {
            let mut ready = previous;
            ready.label = "The downloaded update is ready. Could not check for updates; restarting will verify it again before installation.".into();
            self.publish(ready);
        } else {
            self.fail("Could not check for updates. Your current app is still available.");
        }
    }
    fn fail(&self, message: impl Into<String>) {
        if self.restart_path().is_some() {
            return;
        }
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
        if self.0.started.swap(true, Ordering::AcqRel) {
            return;
        }
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
            if self.restart_path().is_some() {
                return;
            }
            let delay = Duration::from_secs(3300 + rand::random::<u64>() % 601);
            tokio::select! {
                _ = self.0.cancel.cancelled() => return,
                _ = tokio::time::sleep(delay) => {},
            }
            tokio::select! { _ = self.0.cancel.cancelled() => return, _ = self.check() => {} }
        }
    }
    pub async fn check(&self) {
        if self.0.cancel.is_cancelled() || self.restart_path().is_some() {
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
        let previous = self.snapshot();
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
            Err(_) => self.recheck_failed(previous),
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
        if let Some(prepared) = prepared.filter(|_| !service.manual_only()) {
            match self.stage_prepared(prepared).await {
                Ok(()) => self.release_snapshot(service, &release, State::Ready).await,
                Err(error) => self.installation_failed(service, &release, error).await,
            }
        } else if self.0.automatic.load(Ordering::Relaxed) && !service.manual_only() {
            self.download_inner(service, release).await;
        } else {
            self.release_snapshot(service, &release, State::Available)
                .await;
        }
    }
    async fn stage_prepared(&self, prepared: PreparedUpdate) -> Result<(), InstallationFailure> {
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
                        .map_err(InstallationFailure::from)?
                }
            };
            if let Some(previous) = previous {
                match maple_agent_updater::installer::revalidate(&previous, &prepared) {
                    Ok(()) => return Ok((previous, lock)),
                    Err(error) => {
                        let _ = maple_agent_updater::installer::discard_preparation(&previous);
                        return Err(error.into());
                    }
                }
            }
            let installation = maple_agent_updater::installer::prepare(&prepared)
                .map_err(InstallationFailure::from)?;
            Ok::<_, InstallationFailure>((installation, lock))
        })
        .await
        .map_err(|_| "Could not prepare the update".to_string())??;
        let mut releases = self.0.releases.lock().await;
        releases.installation = Some(installation);
        releases.install_lock = Some(lock);
        Ok(())
    }
    async fn installation_failed(
        &self,
        service: &Service,
        release: &VerifiedRelease,
        error: InstallationFailure,
    ) {
        if error.manual {
            service.manual_only.store(true, Ordering::Release);
            self.release_snapshot(service, release, State::Available)
                .await;
            let mut snapshot = self.snapshot();
            snapshot.label = format!(
                "{} Download v{} to install manually.",
                error.message, release.manifest.version
            );
            self.publish(snapshot);
        } else {
            self.fail(error.message);
        }
    }
    async fn release_snapshot(&self, service: &Service, release: &VerifiedRelease, state: State) {
        let version = release.manifest.version.clone();
        let mut snapshot = Snapshot::new(
            state,
            match state {
                State::Ready => format!("v{version} is ready to install"),
                State::Downloading => format!("Downloading v{version}…"),
                _ if service.manual_only() => {
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
            snapshot.primary = Some(if service.manual_only() {
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
        if self.0.cancel.is_cancelled() || self.restart_path().is_some() {
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
        if service.manual_only() {
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
                    Err(error) => self.installation_failed(service, &release, error).await,
                }
            }
            Err(_) => {
                self.fail("Could not download or verify the update. Retry when you are ready.")
            }
        }
    }
    /// Apply on the UI thread accepting the current settings generation. A
    /// delayed runtime task must never overwrite a newer saved opt-out.
    pub fn set_auto_download(&self, enabled: bool) {
        self.0.automatic.store(enabled, Ordering::Relaxed);
    }
    pub async fn download_if_automatic(&self) {
        if self.0.automatic.load(Ordering::Relaxed) && self.snapshot().state == State::Available {
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
    pub async fn install(
        &self,
        backend: Arc<crate::backend::AgentBackend>,
    ) -> Result<Option<PathBuf>, String> {
        if let Some(path) = self.restart_path() {
            return Ok(Some(path));
        }
        if self.0.cancel.is_cancelled() {
            return Ok(None);
        }
        let Ok(_operation) = self.0.operation.try_lock() else {
            return Ok(None);
        };
        let result = self.install_inner(backend).await;
        if let Some(path) = self.restart_path() {
            return Ok(Some(path));
        }
        if let Err(error) = &result {
            if let Some(service) = self.0.service.get() {
                let release = self.0.releases.lock().await.release.clone();
                if let Some(release) = release {
                    self.installation_failed(service, &release, error.clone())
                        .await;
                } else {
                    self.fail(error.message.clone());
                }
            } else {
                self.fail(error.message.clone());
            }
        }
        result.map_err(|error| error.message)
    }
    async fn install_inner(
        &self,
        backend: Arc<crate::backend::AgentBackend>,
    ) -> Result<Option<PathBuf>, InstallationFailure> {
        let api_url = backend.api_url().to_owned();
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
                    .map_err(InstallationFailure::from)?,
            };
            let preparation = match saved {
                Some(preparation) => {
                    if let Err(error) =
                        maple_agent_updater::installer::revalidate(&preparation, &staging)
                    {
                        let _ = maple_agent_updater::installer::discard_preparation(&preparation);
                        return Err(error.into());
                    }
                    preparation
                }
                None => maple_agent_updater::installer::prepare(&staging)
                    .map_err(InstallationFailure::from)?,
            };
            Ok::<_, InstallationFailure>((lock, preparation))
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
        // Commit every pending frontend patch before asking the host to drain.
        // Failed saves leave the live store and installation untouched.
        if let Err(error) = crate::desktop::flush_frontend_state(&backend).await {
            discard_preparation(preparation).await;
            return Err(error.into());
        }
        let recovery_api = api_url.clone();
        let installed_version = prepared.release.manifest.version.clone();
        let controller = self.clone();
        let updater = service.updater.clone();
        let result = tokio::task::spawn_blocking(move || {
            let _lock = lock;
            // Refresh effective permissions after the asynchronous discovery
            // check, before stopping a healthy host or closing admission.
            if let Err(error) = maple_agent_updater::installer::preflight_installation(&prepared.installed_path) {
                let _ = maple_agent_updater::installer::discard_preparation(&preparation);
                return Err(InstallationFailure::from(error));
            }
            let manual_failure = std::cell::Cell::new(false);
            let mut host_fence = match crate::host::acquire_update_fence(&api_url) {
                Ok(fence) => Some(fence),
                Err(message) => {
                    let _ = maple_agent_updater::installer::discard_preparation(&preparation);
                    return Err(InstallationFailure { message, manual: false, recover_host: true });
                }
            };
            let result = updater.with_commit_guard(&prepared, || {
                // replace performs the final payload/tree/codesign checks
                // while both process fences and the commit guard are held.
                let installed = maple_agent_updater::installer::replace(&preparation, &prepared).map_err(|error| {
                    manual_failure.set(error.requires_manual_installation());
                    error.to_string()
                })?;
                if installed.durability_warning {
                    log::warn!("The update was installed; filesystem synchronization did not complete. Restarting the installed application.");
                }
                // Retire old-process RPC admission and retain host ownership
                // until this PID exits, including if the restart helper fails.
                backend.retire_after_update();
                controller.installed(installed.installed_path.clone(), &installed_version, host_fence.take());
                Ok(installed.installed_path)
            }).map_err(|error| error.to_string());
            drop(host_fence);
            if result.is_err() {
                let _ = maple_agent_updater::installer::discard_preparation(&preparation);
            }
            result.map_err(|message| InstallationFailure { message, manual: manual_failure.get(), recover_host: true })
        }).await.map_err(|_| "Installation did not complete; retry or reinstall Maple".to_string())?;
        match result {
            Ok(path) => Ok(Some(path)),
            Err(error) => {
                if !error.recover_host {
                    return Err(error);
                }
                // An Err is guaranteed to precede the atomic swap. The host
                // fence and application lock have both dropped before recovery.
                if crate::host::recover_after_failed_update(&recovery_api)
                    .await
                    .is_err()
                {
                    return Err(InstallationFailure {
                        message: format!(
                            "{}. Quit and reopen Maple to restore its host.",
                            error.message
                        ),
                        ..error
                    });
                }
                Err(InstallationFailure {
                    message: format!(
                        "{}. Quit and reopen Maple before continuing work.",
                        error.message
                    ),
                    ..error
                })
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
        state_dir: update_state_dir(),
    };
    Ok(Service {
        updater: Arc::new(
            Updater::new(config).map_err(|_| "Invalid packaged update configuration")?,
        ),
        installed_path,
        manual_only: AtomicBool::new(manual_only),
    })
}

fn update_state_dir() -> PathBuf {
    // Signed discovery/anti-rollback state survives local-store reset/repair.
    crate::backend::profile_data_root().join("updates")
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

    #[cfg(unix)]
    #[test]
    fn quit_cleanup_without_tokio_preserves_download_and_installation() {
        use std::os::unix::fs::PermissionsExt;

        assert!(tokio::runtime::Handle::try_current().is_err());
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let installed = root.join("Maple Agent.app");
        let staging = root.join(".maple-agent-update-abcdefghijklmnop");
        let staged = staging.join("Maple Agent.app");
        let cached = root.join("download.payload");
        std::fs::create_dir(&installed).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(&staged).unwrap();
        std::fs::write(installed.join("sentinel"), b"current installation").unwrap();
        std::fs::write(&cached, b"authenticated download remains").unwrap();
        let payload = staging.join("payload");
        std::fs::write(&payload, b"derived staging copy").unwrap();
        let lock = maple_agent_updater::installer::acquire_install_lock(&installed).unwrap();
        let controller = Controller::new(true);
        {
            let mut releases = controller.0.releases.try_lock().unwrap();
            releases.installation = Some(maple_agent_updater::installer::InstallPreparation {
                installed_path: installed.clone(),
                staging_dir: staging.clone(),
                payload_path: payload,
                staged_path: staged,
                version: "0.0.2".into(),
                build: "489".into(),
                size: 20,
                sha256: "0".repeat(64),
            });
            releases.install_lock = Some(lock);
        }
        let backend = Arc::new(
            crate::backend::AgentBackend::for_test("http://127.0.0.1:9".into(), String::new())
                .unwrap(),
        );
        // No other owner keeps this backend runtime alive. Hold the operation
        // gate until the handoff returns so cleanup cannot finish before then.
        let operation = controller.0.operation.try_lock().unwrap();
        controller.shutdown();
        let cleanup = controller.cleanup_on_exit(backend);
        drop(operation);
        assert!(tokio::runtime::Handle::try_current().is_err());
        // GPUI's test shutdown skips quit futures; poll the production handoff
        // directly on the same reactor-free kind of foreground executor.
        gpui::block_on(cleanup).unwrap();
        assert!(tokio::runtime::Handle::try_current().is_err());
        assert!(!staging.exists());
        assert_eq!(
            std::fs::read(installed.join("sentinel")).unwrap(),
            b"current installation"
        );
        assert_eq!(
            std::fs::read(cached).unwrap(),
            b"authenticated download remains"
        );
        maple_agent_updater::installer::acquire_install_lock(&installed).unwrap();
    }

    #[test]
    fn discovery_state_stays_outside_the_resettable_sql_store() {
        let profile = crate::backend::profile_data_root();
        let store = maple_agent::store::StorePaths::new(&profile);
        assert_eq!(update_state_dir(), profile.join("updates"));
        assert!(!update_state_dir().starts_with(store.root()));
    }

    #[tokio::test]
    async fn deferred_store_bootstrap_cannot_overwrite_a_newer_opt_out() {
        let controller = Controller::new(false);
        controller.set_test_state(State::Disabled);
        controller.set_auto_download(true);
        let older_bootstrap = controller.clone().run();
        controller.set_auto_download(false);
        older_bootstrap.await;
        assert_eq!(controller.startup_policy_for_test(), (true, false));
        controller.clone().run().await;
        assert_eq!(controller.startup_policy_for_test(), (true, false));
    }
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
    fn failed_recheck_retains_ready_restart_and_manual_offer() {
        let controller = Controller::new(false);
        let mut downloading = Snapshot::new(State::Ready, "v0.0.2 is ready to install");
        downloading.version = Some("0.0.2".into());
        downloading.manual_download_url =
            Some("https://updates.trymaple.ai/agent/stable/releases/0.0.2/Maple.dmg".into());
        downloading.primary = Some(UpdateAction::Restart);
        downloading.primary_label = "Restart to Update";
        downloading.show_banner = true;
        controller.publish(downloading.clone());
        controller.begin_check();
        assert_eq!(controller.snapshot().state, State::Checking);
        controller.recheck_failed(downloading);
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.state, State::Ready);
        assert_eq!(snapshot.primary, Some(UpdateAction::Restart));
        assert!(snapshot.label.contains("verify it again"));
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
                .install(Arc::new(
                    crate::backend::AgentBackend::for_test(
                        "http://127.0.0.1:9".into(),
                        String::new()
                    )
                    .unwrap()
                ))
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
    #[tokio::test]
    async fn committed_update_remains_restartable_without_reinstall_or_network() {
        let controller = Controller::new(true);
        let path = PathBuf::from("/Applications/Maple Agent.app");
        controller.set_test_installed(path.clone());
        assert_eq!(controller.snapshot().state, State::RestartRequired);
        assert_eq!(controller.snapshot().primary, Some(UpdateAction::Relaunch));
        assert!(!controller.snapshot().busy());
        controller.fail("A late pre-restart callback failed");
        controller.check().await;
        controller.download().await;
        controller.clone().run().await;
        let backend = Arc::new(
            crate::backend::AgentBackend::for_test("http://127.0.0.1:9".into(), String::new())
                .unwrap(),
        );
        assert_eq!(
            controller.install(backend).await.unwrap(),
            Some(path.clone())
        );
        assert_eq!(controller.restart_path(), Some(path));
        assert_eq!(controller.snapshot().state, State::RestartRequired);
        assert!(controller.0.service.get().is_none());
    }
    // This exercises macOS installation permissions with the macOS-only signed
    // fixture. Linux discovery requires its own artifact/download inventory.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn permission_failure_switches_to_signed_manual_download_before_host_stop() {
        let controller = Controller::new(true);
        let temp = tempfile::tempdir().unwrap();
        let installed = temp.path().join("Maple Agent.app");
        std::fs::create_dir_all(&installed).unwrap();
        let feed: maple_agent_updater::Url = "https://updates.trymaple.ai/agent/stable/latest.json"
            .parse()
            .unwrap();
        let key =
            include_str!("../../crates/maple-agent-updater/tests/fixtures/public-test-key.pub")
                .to_owned();
        let release = maple_agent_updater::manifest::verify_envelope(
            include_bytes!("../../crates/maple-agent-updater/tests/fixtures/signed-envelope.json"),
            &feed,
            std::slice::from_ref(&key),
        )
        .unwrap();
        let service = Service {
            updater: Arc::new(
                Updater::new(Config {
                    feed_url: feed,
                    public_keys: vec![key],
                    current_version: "0.0.1".parse().unwrap(),
                    target: Target::DarwinAarch64,
                    installed_path: installed.clone(),
                    state_dir: temp.path().join("updates"),
                })
                .unwrap(),
            ),
            installed_path: installed,
            manual_only: AtomicBool::new(false),
        };
        let error = InstallationFailure::from(maple_agent_updater::installer::InstallError::Io(
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        ));
        assert!(
            !error.recover_host,
            "known permission failures must precede host shutdown"
        );
        controller
            .installation_failed(&service, &release, error)
            .await;
        let snapshot = controller.snapshot();
        assert!(service.manual_only());
        assert_eq!(snapshot.state, State::Available);
        assert_eq!(snapshot.primary, Some(UpdateAction::ManualDownload));
        assert!(
            snapshot
                .manual_download_url
                .unwrap()
                .ends_with("/Maple-Agent.dmg")
        );
        assert!(snapshot.label.contains("install manually"));
        assert!(controller.restart_path().is_none());
    }
}
