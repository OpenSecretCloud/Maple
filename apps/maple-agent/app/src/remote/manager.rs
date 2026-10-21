//! Per-account networking owned by the local Maple daemon. Enrollment is local
//! control only; cloud metadata never substitutes for a persisted peer grant.
use super::{
    network, pairing,
    store::{Peer, Store},
    wire,
};
use crate::{
    backend::LocalBackend,
    host::{protocol, receipts::Receipts},
};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMap, RelayMode, RelayUrl, SecretKey};
use maple_agent::{agent::AgentServiceEvent, maple_api::MapleApiSession};
use maple_sdk::remote::*;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore, broadcast};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Manager {
    node: AsyncMutex<Option<Arc<Node>>>,
    initializing: AsyncMutex<()>,
    backend: Arc<LocalBackend>,
    events: broadcast::Sender<Arc<AgentServiceEvent>>,
    receipts: Arc<Receipts>,
    resident: AtomicBool,
    generation: AtomicU64,
    resuming: AtomicBool,
    last_error: Mutex<Option<String>>,
}

pub struct Node {
    pub(super) backend: Arc<LocalBackend>,
    pub(super) events: broadcast::Sender<Arc<AgentServiceEvent>>,
    pub(super) receipts: Arc<Receipts>,
    pub(super) store: Mutex<Store>,
    pub(super) session: Arc<MapleApiSession>,
    pub(super) endpoint: Endpoint,
    addresses: Mutex<std::collections::HashMap<EndpointId, (EndpointAddr, RemoteDevice)>>,
    pub(super) account: String,
    pub(super) project: String,
    pub(super) deployment: String,
    window: AsyncMutex<Option<pairing::Window>>,
    pairing_generation: AtomicU64,
    registration: AsyncMutex<()>,
    pub(super) stopped: CancellationToken,
    pub(super) revoked: broadcast::Sender<EndpointId>,
    // Remote connections never occupy the local IPC/control capacity.
    pub(super) admissions: Arc<Semaphore>,
    pub(super) controllers: Arc<Semaphore>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub name: String,
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub ready: bool,
    pub error: Option<String>,
    pub peers: std::collections::BTreeMap<String, Peer>,
}

#[derive(Serialize, Deserialize)]
pub struct PairingWindow {
    pub code: String,
    pub window: Uuid,
    pub expires_in: u64,
}

impl Manager {
    pub fn new(
        backend: Arc<LocalBackend>,
        events: broadcast::Sender<Arc<AgentServiceEvent>>,
        receipts: Arc<Receipts>,
    ) -> Arc<Self> {
        Arc::new(Self {
            node: AsyncMutex::new(None),
            initializing: AsyncMutex::new(()),
            backend,
            events,
            receipts,
            resident: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            resuming: AtomicBool::new(false),
            last_error: Mutex::new(None),
        })
    }
    pub fn stays_running(&self) -> bool {
        self.resident.load(Ordering::Acquire)
    }

    pub async fn shutdown(&self) {
        let node = {
            let mut current = self.node.lock().await;
            self.generation.fetch_add(1, Ordering::SeqCst);
            self.resident.store(false, Ordering::Release);
            current.take()
        };
        if let Some(node) = node {
            node.stopped.cancel();
            node.endpoint.close().await;
        }
    }

    fn store(&self, account: &str) -> Result<Store, String> {
        Store::open(
            &self.backend.remote_storage_root(),
            self.backend.remote_api_url(),
            &crate::backend::local::configured_client_id().to_string(),
            crate::profile::PROFILE,
            account,
        )
    }

    pub async fn restore(self: &Arc<Self>) {
        let current = self.node.lock().await;
        let enabled = self.backend.saved_user_id().is_some_and(|account| {
            Store::exists(
                &self.backend.remote_storage_root(),
                self.backend.remote_api_url(),
                &crate::backend::local::configured_client_id().to_string(),
                crate::profile::PROFILE,
                &account,
            ) && self.store(&account).is_ok_and(|s| s.enabled())
        });
        if !enabled {
            return;
        }
        self.resident.store(true, Ordering::Release);
        drop(current);
        // Invalid configuration is actionable, not a network outage to retry.
        if let Err(error) = configuration() {
            *self.last_error.lock().unwrap() = Some(error);
            return;
        }
        if self.resuming.swap(true, Ordering::AcqRel) {
            return;
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut delay = Duration::from_secs(1);
            loop {
                let Some(manager) = weak.upgrade() else {
                    return;
                };
                if !manager.stays_running() {
                    manager.resuming.store(false, Ordering::Release);
                    return;
                }
                match manager.active().await {
                    Ok(_) => {
                        *manager.last_error.lock().unwrap() = None;
                        manager.resuming.store(false, Ordering::Release);
                        return;
                    }
                    Err(error) if manager.stays_running() => {
                        *manager.last_error.lock().unwrap() = Some(error)
                    }
                    Err(_) => {}
                }
                drop(manager);
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(30));
            }
        });
    }

    pub async fn active(self: &Arc<Self>) -> Result<Arc<Node>, String> {
        let account = self
            .backend
            .saved_user_id()
            .ok_or("Sign in before using remote access")?;
        let _initializing = self.initializing.lock().await;
        let (generation, current) = {
            let current = self.node.lock().await;
            (self.generation.load(Ordering::SeqCst), current.clone())
        };
        if let Some(node) = current {
            if node.account == account && node.valid().await {
                return Ok(node);
            }
            node.stopped.cancel();
            node.endpoint.close().await;
            let mut current = self.node.lock().await;
            if current.as_ref().is_some_and(|n| Arc::ptr_eq(n, &node)) {
                current.take();
            }
        }
        let session = tokio::time::timeout(
            Duration::from_secs(15),
            self.backend.remote_session(&account),
        )
        .await
        .map_err(|_| "Could not validate remote account; try again when connected")??;
        let store = self.store(&account)?;
        let key = store.key();
        let project = crate::backend::local::configured_client_id().to_string();
        let deployment = if self.backend.remote_api_url().starts_with("http://") {
            "local"
        } else if crate::profile::PCR_ENVIRONMENT == "development" {
            "dev"
        } else {
            "prod"
        }
        .to_string();
        let (relays, relay_only) = configuration()?;
        // Register possession before asking an authenticated Maple relay to
        // accept this key. Addresses are refreshed after binding.
        register(
            &session,
            &key,
            (&account, &project, &deployment),
            RemoteAddresses::default(),
            store.name().to_owned(),
            store.enabled(),
            None,
        )
        .await?;
        if self.generation.load(Ordering::SeqCst) != generation
            || !session.remote_session_active().await
            || self.backend.saved_user_id().as_deref() != Some(&account)
        {
            return Err("The signed-in account changed".into());
        }
        let endpoint = network::bind(key, relays, relay_only).await?;
        if self.generation.load(Ordering::SeqCst) != generation
            || !session.remote_session_active().await
            || self.backend.saved_user_id().as_deref() != Some(&account)
        {
            endpoint.close().await;
            return Err("The signed-in account changed".into());
        }
        // Settings mutations advance generation under node's installation lock.
        // Any older initialization is discarded, even after this disk read.
        let store = self.store(&account)?;
        let (revoked, _) = broadcast::channel(64);
        let node = Arc::new(Node {
            backend: self.backend.clone(),
            events: self.events.clone(),
            receipts: self.receipts.clone(),
            store: Mutex::new(store),
            session,
            endpoint,
            addresses: Mutex::new(std::collections::HashMap::new()),
            account,
            project,
            deployment,
            window: AsyncMutex::new(None),
            pairing_generation: AtomicU64::new(0),
            registration: AsyncMutex::new(()),
            stopped: CancellationToken::new(),
            revoked,
            admissions: Arc::new(Semaphore::new(4)),
            controllers: Arc::new(Semaphore::new(8)),
        });
        {
            let mut current = self.node.lock().await;
            if self.generation.load(Ordering::SeqCst) != generation {
                drop(current);
                node.endpoint.close().await;
                return Err("Remote access was stopped while connecting".into());
            }
            self.resident
                .store(node.store.lock().unwrap().enabled(), Ordering::Release);
            *current = Some(node.clone());
        }
        super::control::listen(node.clone());
        let weak = Arc::downgrade(&node);
        let manager = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                let Some(node) = weak.upgrade() else {
                    break;
                };
                if !node.valid().await {
                    node.stopped.cancel();
                    node.endpoint.close().await;
                    if let Some(manager) = manager.upgrade() {
                        let current = manager.node.lock().await;
                        if current.as_ref().is_some_and(|n| Arc::ptr_eq(n, &node)) {
                            manager.resident.store(false, Ordering::Release);
                        }
                    }
                    break;
                }
                // Discovery is only a reachability hint. Existing paired
                // control survives an enrollment-service outage.
                if let Err(error) = node.publish().await {
                    log::debug!("Remote discovery refresh unavailable: {error}");
                }
                let stopped = node.stopped.clone();
                drop(node);
                tokio::select! { _ = stopped.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(60)) => {} }
            }
        });
        Ok(node)
    }

    pub async fn dispatch(
        self: &Arc<Self>,
        request: protocol::Request,
    ) -> Result<serde_json::Value, String> {
        let encode = |value| {
            serde_json::to_value(value).map_err(|_| "Could not encode remote settings".to_string())
        };
        if request.method == "remote_pairing_window" {
            let current = self.node.lock().await.clone();
            let window = match current {
                Some(node) if node.valid().await => node.current_pairing().await,
                _ => None,
            };
            return serde_json::to_value(window)
                .map_err(|_| "Could not display pairing window".into());
        }
        if request.method == "remote_status" {
            let account = self
                .backend
                .saved_user_id()
                .ok_or("Sign in before using remote access")?;
            if !Store::exists(
                &self.backend.remote_storage_root(),
                self.backend.remote_api_url(),
                &crate::backend::local::configured_client_id().to_string(),
                crate::profile::PROFILE,
                &account,
            ) {
                return encode(Status {
                    name: super::store::default_name().into(),
                    enabled: false,
                    endpoint: None,
                    ready: false,
                    error: None,
                    peers: Default::default(),
                });
            }
            let current = self.node.lock().await;
            let store = self.store(&account)?;
            return encode(Status {
                name: store.name().into(),
                enabled: store.enabled(),
                endpoint: Some(store.key().public().to_string()),
                ready: current
                    .as_ref()
                    .is_some_and(|n| n.account == account && !n.stopped.is_cancelled()),
                error: self.last_error.lock().unwrap().clone(),
                peers: store.peers().clone(),
            });
        }
        if request.method == "remote_set_name" {
            let (name,): (String,) =
                serde_json::from_value(request.params).map_err(|_| "Invalid computer name")?;
            super::validate_name(&name)?;
            let account = self
                .backend
                .saved_user_id()
                .ok_or("Sign in before naming this computer")?;
            // Like enable/revoke, serialize local writes with node installation.
            // Naming an offline/disabled computer must not start networking.
            let current = self.node.lock().await;
            self.generation.fetch_add(1, Ordering::SeqCst);
            let node = current.as_ref().filter(|n| n.account == account).cloned();
            if let Some(node) = &node {
                node.store.lock().unwrap().set_name(&name)?;
            } else {
                self.store(&account)?.set_name(&name)?;
            }
            drop(current);
            if let Some(node) = node {
                tokio::spawn(async move {
                    let _ = node.publish().await;
                });
            }
            return Ok(serde_json::Value::Null);
        }
        if matches!(
            request.method.as_str(),
            "remote_revoke" | "remote_cancel_pairing" | "remote_enable"
        ) {
            let account = self
                .backend
                .saved_user_id()
                .ok_or("No local account is selected")?;
            let enable = if request.method == "remote_enable" {
                let (enabled,): (bool,) = serde_json::from_value(request.params.clone())
                    .map_err(|_| "Invalid remote setting")?;
                Some(enabled)
            } else {
                None
            };
            // This short local lock orders settings writes against node install.
            // No credential validation or network operation holds it.
            let current = self.node.lock().await;
            self.generation.fetch_add(1, Ordering::SeqCst);
            let node = current.as_ref().filter(|n| n.account == account).cloned();
            if (request.method != "remote_enable" || enable == Some(false))
                && let Some(node) = &node
            {
                let mut window = node.window.lock().await;
                node.pairing_generation.fetch_add(1, Ordering::SeqCst);
                *window = None;
            }
            if request.method != "remote_cancel_pairing" {
                let mut disk;
                let mut memory;
                let store = if let Some(node) = &node {
                    memory = node.store.lock().unwrap();
                    &mut *memory
                } else {
                    disk = self.store(&account)?;
                    &mut disk
                };
                if let Some(enabled) = enable {
                    store.set_enabled(enabled)?;
                    self.resident.store(enabled, Ordering::Release);
                    *self.last_error.lock().unwrap() = None;
                    if !enabled && let Some(node) = &node {
                        let _ = node.revoked.send(node.endpoint.id());
                    }
                } else {
                    let (peer,): (String,) =
                        serde_json::from_value(request.params).map_err(|_| "Invalid device")?;
                    let peer = peer.parse().map_err(|_| "Invalid device")?;
                    store.revoke(peer)?;
                    if let Some(node) = &node {
                        let _ = node.revoked.send(peer);
                    }
                }
            }
            drop(current);
            if enable == Some(true) {
                self.restore().await;
            }
            // Publish best effort without making local disable/cancel wait on a service.
            if let Some(node) = node {
                tokio::spawn(async move {
                    let _ = node.publish().await;
                });
            }
            return Ok(serde_json::Value::Null);
        }
        let node = self.active().await?;
        match request.method.as_str() {
            "remote_begin_pairing" => serde_json::to_value(node.begin_pairing().await?)
                .map_err(|_| "Could not display pairing window".into()),
            "remote_discover" => {
                node.publish().await?;
                let list = node.session.list_remote_devices().await?;
                validate_devices(&list.devices)?;
                node.ensure_valid().await?;
                node.remember_peer_names(&list.devices);
                serde_json::to_value(
                    list.devices
                        .into_iter()
                        .filter(|d| {
                            d.channel == crate::profile::PROFILE
                                && d.endpoint != node.endpoint.id().to_string()
                        })
                        .collect::<Vec<_>>(),
                )
                .map_err(|_| "Invalid device list".into())
            }
            "remote_pair" => {
                let (peer, mut code): (String, String) = serde_json::from_value(request.params)
                    .map_err(|_| "Invalid pairing request")?;
                let parsed = pairing::Code::parse(&code);
                zeroize::Zeroize::zeroize(&mut code);
                node.pair(
                    peer.parse().map_err(|_| "Invalid remote identity")?,
                    parsed?,
                )
                .await?;
                Ok(serde_json::Value::Null)
            }
            "remote_reconcile" => {
                let (peer,): (String,) =
                    serde_json::from_value(request.params).map_err(|_| "Invalid device")?;
                node.reconcile(peer.parse().map_err(|_| "Invalid device")?)
                    .await?;
                Ok(serde_json::Value::Null)
            }
            _ => Err("Unknown local remote setting".into()),
        }
    }
}

impl Node {
    pub(super) async fn valid(&self) -> bool {
        !self.stopped.is_cancelled()
            && self.backend.saved_user_id().as_deref() == Some(&self.account)
            && self.session.remote_session_active().await
    }
    pub(super) async fn ensure_valid(&self) -> Result<(), String> {
        if self.valid().await {
            Ok(())
        } else {
            Err("The remote account is no longer active".into())
        }
    }
    pub(super) fn hello(&self, mode: wire::Mode) -> wire::Hello {
        wire::Hello {
            version: wire::VERSION,
            minimum_version: wire::VERSION,
            account: self.account.clone(),
            project: self.project.clone(),
            deployment: self.deployment.clone(),
            channel: crate::profile::PROFILE.into(),
            mode,
        }
    }
    fn context<'a>(
        &'a self,
        controller: &'a EndpointId,
        execution: &'a EndpointId,
        window: &'a [u8; 16],
    ) -> pairing::Context<'a> {
        pairing::Context {
            controller: controller.as_bytes(),
            execution: execution.as_bytes(),
            account: &self.account,
            project: &self.project,
            deployment: &self.deployment,
            channel: crate::profile::PROFILE,
            window,
        }
    }
    pub(super) async fn publish(&self) -> Result<(), String> {
        // A stale readiness refresh must not overwrite a newly registered W.
        // Local cancel/disable never wait on this network serialization gate.
        let _registration = self.registration.lock().await;
        self.ensure_valid().await?;
        let key = self.store.lock().unwrap().key();
        let enabled = self.store.lock().unwrap().enabled();
        let name = self.store.lock().unwrap().name().to_owned();
        let window = self
            .window
            .lock()
            .await
            .as_ref()
            .filter(|w| w.is_open(&w.id))
            .map(|w| RemoteWindowInit {
                id: Uuid::from_bytes(w.id),
                remaining_seconds: w.remaining().as_secs().min(300) as u32,
            });
        let address = self.endpoint.addr();
        register(
            &self.session,
            &key,
            (&self.account, &self.project, &self.deployment),
            RemoteAddresses {
                relays: address.relay_urls().map(ToString::to_string).collect(),
                direct: address.ip_addrs().map(ToString::to_string).collect(),
            },
            name,
            enabled,
            window,
        )
        .await?;
        self.ensure_valid().await
    }
    async fn current_pairing(&self) -> Option<PairingWindow> {
        self.window
            .lock()
            .await
            .as_ref()
            .filter(|w| w.is_open(&w.id))
            .and_then(|w| {
                Some(PairingWindow {
                    code: w.code()?,
                    window: Uuid::from_bytes(w.id),
                    expires_in: w.remaining().as_secs(),
                })
            })
    }
    async fn begin_pairing(&self) -> Result<PairingWindow, String> {
        let _starting = self.registration.lock().await;
        if !self.store.lock().unwrap().enabled() {
            return Err("Enable remote access on this computer first".into());
        }
        if let Some(window) = self.current_pairing().await {
            return Ok(window);
        }
        let generation = self.pairing_generation.load(Ordering::SeqCst);
        let next = pairing::Window::new();
        let key = self.store.lock().unwrap().key();
        let name = self.store.lock().unwrap().name().to_owned();
        let address = self.endpoint.addr();
        let registration = register(
            &self.session,
            &key,
            (&self.account, &self.project, &self.deployment),
            RemoteAddresses {
                relays: address.relay_urls().map(ToString::to_string).collect(),
                direct: address.ip_addrs().map(ToString::to_string).collect(),
            },
            name,
            true,
            Some(RemoteWindowInit {
                id: Uuid::from_bytes(next.id),
                remaining_seconds: 300,
            }),
        )
        .await?;
        self.ensure_valid().await?;
        let mut window = self.window.lock().await;
        if self.pairing_generation.load(Ordering::SeqCst) != generation
            || !self.store.lock().unwrap().enabled()
        {
            return Err("Pairing was cancelled locally".into());
        }
        if registration.window.as_ref().map(|w| w.id) != Some(Uuid::from_bytes(next.id))
            || next.remaining().is_zero()
        {
            return Err("Pairing registration was not acknowledged. Start a new window.".into());
        }
        let result = PairingWindow {
            code: next.code().ok_or("Pairing window expired")?,
            window: Uuid::from_bytes(next.id),
            expires_in: next.remaining().as_secs(),
        };
        *window = Some(next);
        Ok(result)
    }
    pub(super) async fn destination(
        &self,
        peer: EndpointId,
    ) -> Result<(EndpointAddr, RemoteDevice), String> {
        let available =
            tokio::time::timeout(Duration::from_secs(3), self.session.list_remote_devices()).await;
        self.ensure_valid().await?;
        let list = match available {
            Ok(Ok(list)) => list,
            _ => {
                return self
                    .addresses
                    .lock()
                    .unwrap()
                    .get(&peer)
                    .cloned()
                    .ok_or_else(|| {
                        "Remote discovery is unavailable and no previous address is known".into()
                    });
            }
        };
        validate_devices(&list.devices)?;
        self.remember_peer_names(&list.devices);
        let device = list
            .devices
            .into_iter()
            .find(|d| d.channel == crate::profile::PROFILE && d.endpoint == peer.to_string())
            .ok_or("The other computer is not currently discoverable")?;
        let mut address = EndpointAddr::new(peer);
        if device.addresses.relays.len() > 4 || device.addresses.direct.len() > 16 {
            return Err("Invalid remote address list".into());
        }
        for relay in &device.addresses.relays {
            address = address.with_relay_url(relay.parse().map_err(|_| "Invalid remote relay")?);
        }
        for ip in &device.addresses.direct {
            address = address.with_ip_addr(ip.parse().map_err(|_| "Invalid remote address")?);
        }
        {
            let mut addresses = self.addresses.lock().unwrap();
            if addresses.len() < 64 || addresses.contains_key(&peer) {
                addresses.insert(peer, (address.clone(), device.clone()));
            }
        }
        Ok((address, device))
    }
    fn remember_peer_names(&self, devices: &[RemoteDevice]) {
        let names = devices
            .iter()
            .filter(|d| d.channel == crate::profile::PROFILE)
            .map(|d| (d.endpoint.as_str(), d.name.as_str()));
        if self
            .store
            .lock()
            .unwrap()
            .refresh_peer_names(names)
            .is_err()
        {
            log::warn!("Could not save updated remote computer names");
        }
    }

    /// Pairing authority is already persisted. Resolve the controller's label
    /// separately, so optional discovery metadata cannot delay or fail pairing.
    pub(super) async fn refresh_peer_names(&self) {
        if let Ok(Ok(list)) =
            tokio::time::timeout(Duration::from_secs(3), self.session.list_remote_devices()).await
            && validate_devices(&list.devices).is_ok()
            && self.valid().await
        {
            self.remember_peer_names(&list.devices);
        }
    }
    async fn pair(&self, peer: EndpointId, code: pairing::Code) -> Result<(), String> {
        let generation = self.pairing_generation.load(Ordering::SeqCst);
        self.publish().await?;
        let (address, device) = self.destination(peer).await?;
        let window = device
            .window
            .ok_or("Open a pairing window on the other computer first")?
            .id;
        let admission = self
            .session
            .request_remote_admission(RemoteAdmissionRequest {
                controller: self.endpoint.id().to_string(),
                execution: peer.to_string(),
                window,
            })
            .await?;
        self.ensure_valid().await?;
        let controller = self.endpoint.id();
        let proof =
            pairing::Controller::new(code, &self.context(&controller, &peer, window.as_bytes()))?;
        let exchange = async {
            let connection = self
                .endpoint
                .connect(address, wire::ALPN)
                .await
                .map_err(|_| "Could not connect to the other computer")?;
            if connection.remote_id() != peer {
                return Err("The remote identity changed".into());
            }
            let (mut tx, mut rx) = connection
                .open_bi()
                .await
                .map_err(|_| "Pairing connection closed")?;
            wire::write(
                &mut tx,
                &self.hello(wire::Mode::Pair {
                    window: *window.as_bytes(),
                    admission: admission.handle,
                }),
                wire::MAX_PREFACE,
            )
            .await
            .map_err(|e| e.to_string())?;
            // Reserve immediately before the first code-derived byte. A failed
            // dial does not burn a window, but a partial proof can never retry.
            self.ensure_valid().await?;
            {
                let mut store = self.store.lock().unwrap();
                if self.pairing_generation.load(Ordering::SeqCst) != generation {
                    return Err("Pairing was cancelled locally".into());
                }
                store.reserve_attempt(peer, *window.as_bytes())?;
            }
            wire::write(&mut tx, &proof.message(), wire::MAX_PREFACE)
                .await
                .map_err(|e| e.to_string())?;
            let answer: Result<(Vec<u8>, Vec<u8>), wire::Refusal> =
                wire::read(&mut rx, wire::MAX_PREFACE)
                    .await
                    .map_err(|e| e.to_string())?;
            let (message, host_tag) = answer.map_err(|e| e.message)?;
            let tag = proof.confirm(message, &host_tag)?;
            {
                let _account = self.backend.account_gate.read().await;
                self.ensure_valid().await?;
                let mut store = self.store.lock().unwrap();
                if self.pairing_generation.load(Ordering::SeqCst) != generation {
                    return Err("Pairing was cancelled locally".into());
                }
                store.remember_host_proof(peer, *window.as_bytes(), device.name)?;
            }
            wire::write(&mut tx, &tag.as_slice(), wire::MAX_PREFACE)
                .await
                .map_err(|e| e.to_string())?;
            let grant: Result<[u8; 16], wire::Refusal> = wire::read(&mut rx, wire::MAX_PREFACE)
                .await
                .map_err(|e| e.to_string())?;
            let grant = grant.map_err(|e| e.message)?;
            if grant != *window.as_bytes() {
                return Err("Invalid pairing acknowledgement".into());
            }
            {
                let _account = self.backend.account_gate.read().await;
                self.ensure_valid().await?;
                let mut store = self.store.lock().unwrap();
                if self.pairing_generation.load(Ordering::SeqCst) != generation {
                    return Err("Pairing was cancelled locally".into());
                }
                store.confirm_host(peer, grant)?;
            }
            tx.finish().map_err(|_| "Pairing connection closed")?;
            Ok::<_, String>(())
        };
        match tokio::time::timeout(Duration::from_secs(20), exchange).await {
            Ok(Ok(())) => Ok(()),
            result => {
                if self
                    .store
                    .lock()
                    .unwrap()
                    .peers()
                    .get(&peer.to_string())
                    .is_some_and(|p| p.pending_confirmation == Some(*window.as_bytes()))
                {
                    return self.reconcile(peer).await;
                }
                result.map_err(|_| "Pairing timed out; open a new window before retrying")?
            }
        }
    }
    pub(super) async fn reconcile(&self, peer: EndpointId) -> Result<(), String> {
        let generation = self.pairing_generation.load(Ordering::SeqCst);
        let window = self
            .store
            .lock()
            .unwrap()
            .peers()
            .get(&peer.to_string())
            .and_then(|p| p.pending_confirmation)
            .ok_or("No host confirmation is pending")?;
        let (address, _) = self.destination(peer).await?;
        let confirm = async {
            let connection = self
                .endpoint
                .connect(address, wire::ALPN)
                .await
                .map_err(|_| "Could not reconnect to confirm pairing")?;
            if connection.remote_id() != peer {
                return Err("Remote identity changed".to_string());
            }
            let (mut tx, mut rx) = connection
                .open_bi()
                .await
                .map_err(|_| "Remote connection closed")?;
            wire::write(
                &mut tx,
                &self.hello(wire::Mode::ConfirmGrant { window }),
                wire::MAX_PREFACE,
            )
            .await
            .map_err(|e| e.to_string())?;
            let confirmed: Result<[u8; 16], wire::Refusal> = wire::read(&mut rx, wire::MAX_PREFACE)
                .await
                .map_err(|e| e.to_string())?;
            if confirmed.map_err(|e| e.message)? != window {
                return Err("Invalid grant acknowledgement".into());
            }
            let _account = self.backend.account_gate.read().await;
            self.ensure_valid().await?;
            let mut store = self.store.lock().unwrap();
            if self.pairing_generation.load(Ordering::SeqCst) != generation {
                return Err("Pairing was cancelled locally".into());
            }
            store.confirm_host(peer, window)
        };
        tokio::time::timeout(Duration::from_secs(15), confirm)
            .await
            .map_err(|_| "Pairing acknowledgement is still pending; reconnect to confirm it")?
    }
    pub(super) async fn accept_pairing(
        &self,
        peer: EndpointId,
        window: [u8; 16],
        admission: String,
        tx: &mut iroh::endpoint::SendStream,
        rx: &mut iroh::endpoint::RecvStream,
    ) -> Result<(), String> {
        if !self.store.lock().unwrap().enabled() {
            return Err("Remote access is disabled".into());
        }
        if !self
            .window
            .lock()
            .await
            .as_ref()
            .is_some_and(|w| w.is_open(&window))
        {
            return Err("Pairing window is closed".into());
        }
        let admitted = tokio::time::timeout(
            Duration::from_secs(8),
            self.session.check_remote_admission(CheckRemoteAdmission {
                handle: admission,
                controller: peer.to_string(),
                execution: self.endpoint.id().to_string(),
                window: Uuid::from_bytes(window),
            }),
        )
        .await
        .map_err(|_| "Pairing admission check timed out; try again")??;
        self.ensure_valid().await?;
        if !admitted.valid {
            return Err("Pairing admission was rejected".into());
        }
        let code = self
            .window
            .lock()
            .await
            .as_mut()
            .ok_or("Pairing window is closed")?
            .take_attempt(&window)?;
        let message = wire::read(rx, wire::MAX_PREFACE)
            .await
            .map_err(|e| e.to_string())?;
        let execution = self.endpoint.id();
        let proof =
            pairing::Execution::new(code, &self.context(&peer, &execution, &window), message)?;
        wire::write(
            tx,
            &Ok::<_, wire::Refusal>((proof.message(), proof.confirmation().as_slice())),
            wire::MAX_PREFACE,
        )
        .await
        .map_err(|e| e.to_string())?;
        let tag: Vec<u8> = wire::read(rx, wire::MAX_PREFACE)
            .await
            .map_err(|e| e.to_string())?;
        proof.verify(&tag)?;
        let _account = self.backend.account_gate.read().await;
        self.ensure_valid().await?;
        let pending_window = self.window.lock().await;
        if !pending_window
            .as_ref()
            .is_some_and(|w| w.id == window && !w.remaining().is_zero())
        {
            return Err("Pairing window was closed or expired".into());
        }
        {
            let mut store = self.store.lock().unwrap();
            if !store.enabled() {
                return Err("Remote access is disabled".into());
            }
            store.grant(
                peer,
                Peer {
                    name: "Paired controller".into(),
                    grant: window,
                    controls_this_computer: true,
                    can_connect: false,
                    pending_confirmation: None,
                },
            )?;
        }
        drop(pending_window);
        wire::write(tx, &Ok::<_, wire::Refusal>(window), wire::MAX_PREFACE)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

async fn register(
    session: &MapleApiSession,
    key: &SecretKey,
    (account, project, deployment): (&str, &str, &str),
    addresses: RemoteAddresses,
    name: String,
    enabled: bool,
    window: Option<RemoteWindowInit>,
) -> Result<RemoteDevice, String> {
    let registration = RemoteRegistration {
        account: account.parse().map_err(|_| "Invalid account")?,
        project: project.parse().map_err(|_| "Invalid project")?,
        deployment: deployment.into(),
        channel: crate::profile::PROFILE.into(),
        endpoint: key.public().to_string(),
        name,
        addresses,
        accepting_connections: enabled,
        window,
    };
    let signature = key
        .sign(
            &registration
                .proof_message()
                .map_err(|_| "Invalid registration")?,
        )
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    tokio::time::timeout(
        Duration::from_secs(10),
        session.register_remote_device(RegisterRemoteDevice {
            registration,
            signature,
        }),
    )
    .await
    .map_err(|_| "Remote registration timed out")?
}

fn configuration() -> Result<(RelayMode, bool), String> {
    let configured = crate::env::env_string("MAPLE_REMOTE_RELAYS").unwrap_or_default();
    relay_configuration(
        crate::profile::PROFILE,
        &configured,
        crate::env::env_string("MAPLE_REMOTE_RELAY_ONLY").as_deref() == Some("1"),
    )
}

fn relay_configuration(
    profile: &str,
    configured: &str,
    relay_only: bool,
) -> Result<(RelayMode, bool), String> {
    // Packaged testing uses n0's public relays until Maple-operated relays are
    // ready. An explicit override never falls back to the public relay map.
    // This selects relays only; network::builder still disables public lookup.
    if configured.is_empty() && matches!(profile, "dev" | "prod") {
        return Ok((RelayMode::Default, false));
    }
    if configured == "public-test" && profile == "unpackaged" {
        return Ok((RelayMode::Default, relay_only));
    }
    if configured == "none" && profile == "unpackaged" {
        return Ok((RelayMode::Disabled, false));
    }
    let relays: Vec<RelayUrl> = configured
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().map_err(|_| "Invalid remote relay configuration"))
        .collect::<Result<_, _>>()?;
    if relays.is_empty() {
        return Err("Maple remote relay service is not configured for this build".into());
    }
    Ok((RelayMode::Custom(RelayMap::from_iter(relays)), false))
}

fn validate_devices(devices: &[RemoteDevice]) -> Result<(), String> {
    if devices.len() > 64 {
        return Err("Invalid remote discovery response".into());
    }
    for device in devices {
        let key = device
            .endpoint
            .parse::<EndpointId>()
            .map_err(|_| "Invalid remote discovery identity")?;
        if key.to_string() != device.endpoint
            || device.name.len() > 128
            || device.name.chars().any(char::is_control)
            || device.addresses.direct.len() > 16
            || device.addresses.relays.len() > 4
        {
            return Err("Invalid remote discovery metadata".into());
        }
        for relay in &device.addresses.relays {
            let url = url::Url::parse(relay).map_err(|_| "Invalid remote relay address")?;
            if relay.len() > 512
                || url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("Invalid remote relay address".into());
            }
        }
        for direct in &device.addresses.direct {
            direct
                .parse::<std::net::SocketAddr>()
                .map_err(|_| "Invalid remote direct address")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_profiles_default_to_public_relays_with_normal_path_selection() {
        for profile in ["dev", "prod"] {
            for forced in [false, true] {
                let (relays, relay_only) = relay_configuration(profile, "", forced).unwrap();
                assert!(matches!(relays, RelayMode::Default));
                assert!(!relay_only);
            }
        }
    }

    #[test]
    fn explicit_relays_override_packaged_defaults_and_development_modes() {
        let urls = ["https://relay-one.example/", "https://relay-two.example/"];
        for profile in ["dev", "prod", "unpackaged"] {
            let (relays, relay_only) = relay_configuration(profile, &urls.join(","), true).unwrap();
            let RelayMode::Custom(map) = relays else {
                panic!("explicit relay configuration was not retained");
            };
            assert_eq!(map.len(), urls.len());
            for url in urls {
                assert!(map.contains(&url.parse().unwrap()));
            }
            assert!(!relay_only);
        }
    }

    #[test]
    fn invalid_overrides_never_fall_back_to_public_relays() {
        for profile in ["dev", "prod", "unpackaged"] {
            for configured in [",,,", "not a URL", "https://relay.example/,not a URL"] {
                assert!(relay_configuration(profile, configured, false).is_err());
            }
        }
    }

    #[test]
    fn development_shortcuts_are_not_available_in_packaged_profiles() {
        for profile in ["dev", "prod"] {
            for configured in ["public-test", "none"] {
                assert!(relay_configuration(profile, configured, true).is_err());
            }
        }
    }

    #[test]
    fn unpackaged_networking_still_requires_explicit_configuration() {
        assert!(relay_configuration("unpackaged", "", false).is_err());
        assert!(relay_configuration("unknown", "", false).is_err());
        for forced in [false, true] {
            let (relays, relay_only) =
                relay_configuration("unpackaged", "public-test", forced).unwrap();
            assert!(matches!(relays, RelayMode::Default));
            assert_eq!(relay_only, forced);
            let (relays, relay_only) = relay_configuration("unpackaged", "none", forced).unwrap();
            assert!(matches!(relays, RelayMode::Disabled));
            assert!(!relay_only);
        }
    }
}
