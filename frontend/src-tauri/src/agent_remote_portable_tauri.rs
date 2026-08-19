//! Mobile-only, fail-closed Tauri boundary for portable Agent reads.
//!
//! This module deliberately has no production dependency installer. The app
//! manages [`AgentPortableTauriState::disabled`] until a native authentication
//! owner, released verifier, secure store, and peer factory land together.
#![allow(
    dead_code,
    reason = "native lifecycle hooks remain unwired while portable production composition is disabled"
)]

use std::{
    collections::HashMap,
    io::{self, Write},
    sync::{Arc, Mutex},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{
    issue_opaque_identifier, validate_opaque_identifier, validate_uuid,
    AgentPortableRemoteController, AgentPortableRemoteError, PortableFuture, PortableHistoryPage,
    PortablePageRequest, PortableRecordsPageRequest, PortableRuntimeStatus, PortableSessionPage,
    PortableTargetDescriptor, PortableTargetHandle, PortableTargetLease, MAX_CURRENT_TARGETS,
    MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER, MAX_PORTABLE_HISTORY_RECORD_BYTES,
};

const PORTABLE_WIRE_SCHEMA_VERSION: u16 = 1;
const MAX_PORTABLE_PAGE_JSON_BYTES: usize = 8 * 1024 * 1024;
const PORTABLE_HISTORY_PAGE_LEDGER_BASE_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(crate) enum AgentPortableWireError {
    Unavailable,
    Unauthenticated,
    PairingUnavailable,
    UnknownTarget,
    Busy,
    Cancelled,
    StaleRuntime,
    StaleLease,
    InvalidRequest,
    InvalidResponse,
    PeerUnavailable,
    CleanupFailed,
}

impl From<AgentPortableRemoteError> for AgentPortableWireError {
    fn from(error: AgentPortableRemoteError) -> Self {
        match error {
            AgentPortableRemoteError::Unavailable | AgentPortableRemoteError::Internal => {
                Self::Unavailable
            }
            AgentPortableRemoteError::UnsupportedStoredVersion
            | AgentPortableRemoteError::CorruptStoredRegistry
            | AgentPortableRemoteError::InvalidStoredRegistry
            | AgentPortableRemoteError::StoredRegistryRollback
            | AgentPortableRemoteError::StoredRegistryInterrupted
            | AgentPortableRemoteError::StoredRegistryEquivocation
            | AgentPortableRemoteError::DuplicateStoredTarget
            | AgentPortableRemoteError::Revoked
            | AgentPortableRemoteError::VerificationFailed => Self::PairingUnavailable,
            AgentPortableRemoteError::StoredRegistryConflict | AgentPortableRemoteError::Busy => {
                Self::Busy
            }
            AgentPortableRemoteError::Unauthenticated
            | AgentPortableRemoteError::AccountMismatch => Self::Unauthenticated,
            AgentPortableRemoteError::UnknownTarget => Self::UnknownTarget,
            AgentPortableRemoteError::Cancelled => Self::Cancelled,
            AgentPortableRemoteError::StaleLease => Self::StaleLease,
            AgentPortableRemoteError::InvalidRequest => Self::InvalidRequest,
            AgentPortableRemoteError::InvalidResponse => Self::InvalidResponse,
            AgentPortableRemoteError::PeerUnavailable => Self::PeerUnavailable,
            AgentPortableRemoteError::CleanupFailed => Self::CleanupFailed,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RefreshTargetsRequest {
    account_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareTargetRequest {
    account_id: String,
    runtime_id: String,
    target_handle: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AgentPortableWireLease {
    lease_handle: String,
    target_handle: String,
    host_epoch: String,
    connection_generation: u64,
}

impl std::fmt::Debug for AgentPortableWireLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentPortableWireLease")
            .field("lease_handle", &"<opaque>")
            .field("target_handle", &"<opaque>")
            .field("host_epoch", &self.host_epoch)
            .field("connection_generation", &self.connection_generation)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadRequest {
    account_id: String,
    runtime_id: String,
    lease: AgentPortableWireLease,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionsPageCommandRequest {
    account_id: String,
    runtime_id: String,
    lease: AgentPortableWireLease,
    page: PortablePageRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordsPageCommandRequest {
    account_id: String,
    runtime_id: String,
    lease: AgentPortableWireLease,
    page: PortableRecordsPageRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentPortableReadCapabilities {
    runtime_status: bool,
    session_summaries_page: bool,
    persisted_records_page: bool,
    synchronized_live_tail: bool,
    mutations: bool,
}

impl AgentPortableReadCapabilities {
    const READ_ONLY: Self = Self {
        runtime_status: true,
        session_summaries_page: true,
        persisted_records_page: true,
        synchronized_live_tail: false,
        mutations: false,
    };
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RefreshTargetsResponse {
    schema_version: u16,
    runtime_id: String,
    capabilities: AgentPortableReadCapabilities,
    items: Vec<PortableTargetDescriptor>,
}

impl std::fmt::Debug for RefreshTargetsResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RefreshTargetsResponse")
            .field("schema_version", &self.schema_version)
            .field("runtime_id", &"<opaque>")
            .field("capabilities", &self.capabilities)
            .field("target_count", &self.items.len())
            .finish()
    }
}

trait PortableControllerApi: Send + Sync {
    fn refresh_targets(
        &self,
        expected_account_id: String,
    ) -> PortableFuture<'_, Result<Vec<PortableTargetDescriptor>, AgentPortableRemoteError>>;

    fn prepare_target(
        &self,
        handle: PortableTargetHandle,
    ) -> PortableFuture<'_, Result<PortableTargetLease, AgentPortableRemoteError>>;

    fn runtime_status(
        &self,
        lease: PortableTargetLease,
    ) -> PortableFuture<'_, Result<PortableRuntimeStatus, AgentPortableRemoteError>>;

    fn sessions_page(
        &self,
        lease: PortableTargetLease,
        request: PortablePageRequest,
    ) -> PortableFuture<'_, Result<PortableSessionPage, AgentPortableRemoteError>>;

    fn records_page(
        &self,
        lease: PortableTargetLease,
        request: PortableRecordsPageRequest,
    ) -> PortableFuture<'_, Result<PortableHistoryPage, AgentPortableRemoteError>>;

    fn network_changed(
        &self,
        lease: PortableTargetLease,
    ) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>>;

    fn native_credentials_invalidated(
        &self,
    ) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>>;

    fn dispose(&self) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>>;
}

impl PortableControllerApi for AgentPortableRemoteController {
    fn refresh_targets(
        &self,
        expected_account_id: String,
    ) -> PortableFuture<'_, Result<Vec<PortableTargetDescriptor>, AgentPortableRemoteError>> {
        Box::pin(async move { self.refresh_targets_for_account(&expected_account_id).await })
    }

    fn prepare_target(
        &self,
        handle: PortableTargetHandle,
    ) -> PortableFuture<'_, Result<PortableTargetLease, AgentPortableRemoteError>> {
        Box::pin(async move { self.prepare_target(&handle).await })
    }

    fn runtime_status(
        &self,
        lease: PortableTargetLease,
    ) -> PortableFuture<'_, Result<PortableRuntimeStatus, AgentPortableRemoteError>> {
        Box::pin(async move { self.runtime_status(&lease).await })
    }

    fn sessions_page(
        &self,
        lease: PortableTargetLease,
        request: PortablePageRequest,
    ) -> PortableFuture<'_, Result<PortableSessionPage, AgentPortableRemoteError>> {
        Box::pin(async move { self.sessions_page(&lease, request).await })
    }

    fn records_page(
        &self,
        lease: PortableTargetLease,
        request: PortableRecordsPageRequest,
    ) -> PortableFuture<'_, Result<PortableHistoryPage, AgentPortableRemoteError>> {
        Box::pin(async move { self.records_page(&lease, request).await })
    }

    fn network_changed(
        &self,
        lease: PortableTargetLease,
    ) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>> {
        Box::pin(async move { self.network_changed(&lease).await })
    }

    fn native_credentials_invalidated(
        &self,
    ) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>> {
        Box::pin(async move { self.native_credentials_invalidated().await })
    }

    fn dispose(&self) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>> {
        Box::pin(async move { self.dispose().await })
    }
}

struct RuntimeBinding {
    account_id: String,
    runtime_id: String,
    targets: HashMap<String, PortableTargetHandle>,
    lease: Option<LeaseBinding>,
}

struct LeaseBinding {
    wire: AgentPortableWireLease,
    native: PortableTargetLease,
}

#[derive(Default)]
struct AgentPortableTauriInner {
    fence_epoch: u64,
    runtime: Option<RuntimeBinding>,
}

/// Mobile-managed portable command state. Production construction is inert.
pub(crate) struct AgentPortableTauriState {
    controller: Option<Arc<dyn PortableControllerApi>>,
    inner: Mutex<AgentPortableTauriInner>,
}

impl std::fmt::Debug for AgentPortableTauriState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock().map_err(|_| std::fmt::Error)?;
        formatter
            .debug_struct("AgentPortableTauriState")
            .field("enabled", &self.controller.is_some())
            .field("fence_epoch", &inner.fence_epoch)
            .field("has_runtime", &inner.runtime.is_some())
            .field(
                "has_lease",
                &inner
                    .runtime
                    .as_ref()
                    .is_some_and(|runtime| runtime.lease.is_some()),
            )
            .finish()
    }
}

impl AgentPortableTauriState {
    /// The sole production constructor in this slice. No provider, verifier,
    /// credential, store, or network factory is installed implicitly.
    pub(crate) fn disabled() -> Self {
        Self {
            controller: None,
            inner: Mutex::new(AgentPortableTauriInner::default()),
        }
    }

    #[cfg(test)]
    fn with_controller(controller: Arc<dyn PortableControllerApi>) -> Self {
        Self {
            controller: Some(controller),
            inner: Mutex::new(AgentPortableTauriInner::default()),
        }
    }

    fn controller(&self) -> Result<Arc<dyn PortableControllerApi>, AgentPortableWireError> {
        self.controller
            .clone()
            .ok_or(AgentPortableWireError::Unavailable)
    }

    async fn refresh_targets(
        &self,
        request: RefreshTargetsRequest,
    ) -> Result<RefreshTargetsResponse, AgentPortableWireError> {
        validate_account_id(&request.account_id)?;
        let controller = self.controller()?;
        let fence_epoch = {
            let mut inner = self.lock_inner()?;
            if inner
                .runtime
                .as_ref()
                .is_some_and(|runtime| runtime.account_id != request.account_id)
            {
                return Err(AgentPortableWireError::StaleRuntime);
            }
            inner.fence_epoch = next_epoch(inner.fence_epoch)?;
            inner.runtime = None;
            inner.fence_epoch
        };

        let targets = controller
            .refresh_targets(request.account_id.clone())
            .await
            .map_err(AgentPortableWireError::from)?;
        if targets.len() > MAX_CURRENT_TARGETS {
            return self
                .retire_invalid_response(controller, fence_epoch, None)
                .await;
        }
        let mut target_map = HashMap::with_capacity(targets.len());
        for descriptor in &targets {
            if descriptor.validate().is_err() {
                return self
                    .retire_invalid_response(controller, fence_epoch, None)
                    .await;
            }
            let handle = descriptor.handle.0.clone();
            if target_map
                .insert(handle, descriptor.handle.clone())
                .is_some()
            {
                return self
                    .retire_invalid_response(controller, fence_epoch, None)
                    .await;
            }
        }
        let runtime_id = match issue_opaque_identifier("runtime") {
            Ok(runtime_id) => runtime_id,
            Err(error) => {
                return self
                    .finish_failed_operation(controller, fence_epoch, None, error)
                    .await;
            }
        };
        let response = RefreshTargetsResponse {
            schema_version: PORTABLE_WIRE_SCHEMA_VERSION,
            runtime_id: runtime_id.clone(),
            capabilities: AgentPortableReadCapabilities::READ_ONLY,
            items: targets,
        };
        let mut inner = self.lock_inner()?;
        if inner.fence_epoch != fence_epoch || inner.runtime.is_some() {
            return Err(AgentPortableWireError::Cancelled);
        }
        inner.runtime = Some(RuntimeBinding {
            account_id: request.account_id,
            runtime_id,
            targets: target_map,
            lease: None,
        });
        Ok(response)
    }

    async fn prepare_target(
        &self,
        request: PrepareTargetRequest,
    ) -> Result<AgentPortableWireLease, AgentPortableWireError> {
        validate_account_id(&request.account_id)?;
        validate_runtime_id(&request.runtime_id)?;
        validate_target_handle(&request.target_handle)?;
        let controller = self.controller()?;
        let (fence_epoch, native_handle) = {
            let mut inner = self.lock_inner()?;
            let runtime =
                require_runtime_mut(&mut inner, &request.account_id, &request.runtime_id)?;
            let handle = runtime
                .targets
                .get(&request.target_handle)
                .cloned()
                .ok_or(AgentPortableWireError::UnknownTarget)?;
            runtime.lease = None;
            inner.fence_epoch = next_epoch(inner.fence_epoch)?;
            (inner.fence_epoch, handle)
        };

        let native_lease = match controller.prepare_target(native_handle).await {
            Ok(lease) => lease,
            Err(error) => {
                return self
                    .finish_failed_operation(
                        controller,
                        fence_epoch,
                        Some((&request.account_id, &request.runtime_id, None)),
                        error,
                    )
                    .await;
            }
        };
        if native_lease.validate().is_err() {
            return self
                .retire_invalid_response(
                    controller,
                    fence_epoch,
                    Some((&request.account_id, &request.runtime_id, None)),
                )
                .await;
        }
        let wire = AgentPortableWireLease {
            lease_handle: native_lease.target_id.clone(),
            target_handle: request.target_handle,
            host_epoch: native_lease.host_epoch.to_string(),
            connection_generation: native_lease.connection_generation,
        };
        if validate_wire_lease(&wire).is_err() {
            return self
                .retire_invalid_response(
                    controller,
                    fence_epoch,
                    Some((&request.account_id, &request.runtime_id, None)),
                )
                .await;
        }
        let mut inner = self.lock_inner()?;
        if inner.fence_epoch != fence_epoch {
            return Err(AgentPortableWireError::Cancelled);
        }
        let runtime = require_runtime_mut(&mut inner, &request.account_id, &request.runtime_id)?;
        runtime.lease = Some(LeaseBinding {
            wire: wire.clone(),
            native: native_lease,
        });
        Ok(wire)
    }

    async fn runtime_status(
        &self,
        request: ReadRequest,
    ) -> Result<PortableRuntimeStatus, AgentPortableWireError> {
        let (controller, fence_epoch, native_lease) =
            self.begin_read(&request.account_id, &request.runtime_id, &request.lease)?;
        let response = match controller.runtime_status(native_lease).await {
            Ok(response) => response,
            Err(error) => {
                return self
                    .finish_failed_operation(
                        controller,
                        fence_epoch,
                        Some((
                            &request.account_id,
                            &request.runtime_id,
                            Some(&request.lease),
                        )),
                        error,
                    )
                    .await;
            }
        };
        if response.validate().is_err() {
            return self
                .retire_invalid_response(
                    controller,
                    fence_epoch,
                    Some((
                        &request.account_id,
                        &request.runtime_id,
                        Some(&request.lease),
                    )),
                )
                .await;
        }
        self.require_read_current(
            fence_epoch,
            &request.account_id,
            &request.runtime_id,
            &request.lease,
        )?;
        Ok(response)
    }

    async fn sessions_page(
        &self,
        request: SessionsPageCommandRequest,
    ) -> Result<PortableSessionPage, AgentPortableWireError> {
        request
            .page
            .validate()
            .map_err(|_| AgentPortableWireError::InvalidRequest)?;
        let (controller, fence_epoch, native_lease) =
            self.begin_read(&request.account_id, &request.runtime_id, &request.lease)?;
        let response = match controller
            .sessions_page(native_lease, request.page.clone())
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return self
                    .finish_failed_operation(
                        controller,
                        fence_epoch,
                        Some((
                            &request.account_id,
                            &request.runtime_id,
                            Some(&request.lease),
                        )),
                        error,
                    )
                    .await;
            }
        };
        if response.validate_for(&request.page).is_err()
            || serialized_size_within_limit(&response, MAX_PORTABLE_PAGE_JSON_BYTES).is_err()
        {
            return self
                .retire_invalid_response(
                    controller,
                    fence_epoch,
                    Some((
                        &request.account_id,
                        &request.runtime_id,
                        Some(&request.lease),
                    )),
                )
                .await;
        }
        self.require_read_current(
            fence_epoch,
            &request.account_id,
            &request.runtime_id,
            &request.lease,
        )?;
        Ok(response)
    }

    async fn records_page(
        &self,
        request: RecordsPageCommandRequest,
    ) -> Result<PortableHistoryPage, AgentPortableWireError> {
        request
            .page
            .validate()
            .map_err(|_| AgentPortableWireError::InvalidRequest)?;
        let (controller, fence_epoch, native_lease) =
            self.begin_read(&request.account_id, &request.runtime_id, &request.lease)?;
        let response = match controller
            .records_page(native_lease, request.page.clone())
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return self
                    .finish_failed_operation(
                        controller,
                        fence_epoch,
                        Some((
                            &request.account_id,
                            &request.runtime_id,
                            Some(&request.lease),
                        )),
                        error,
                    )
                    .await;
            }
        };
        if response.validate_for(&request.page).is_err()
            || serialized_history_page_within_limits(&response).is_err()
        {
            return self
                .retire_invalid_response(
                    controller,
                    fence_epoch,
                    Some((
                        &request.account_id,
                        &request.runtime_id,
                        Some(&request.lease),
                    )),
                )
                .await;
        }
        self.require_read_current(
            fence_epoch,
            &request.account_id,
            &request.runtime_id,
            &request.lease,
        )?;
        Ok(response)
    }

    /// Native-only network hint. This is intentionally not a Tauri command.
    pub(crate) async fn native_network_changed(&self) -> Result<(), AgentPortableWireError> {
        let controller = self.controller()?;
        let (fence_epoch, account_id, runtime_id, wire, native) = {
            let inner = self.lock_inner()?;
            let runtime = inner
                .runtime
                .as_ref()
                .ok_or(AgentPortableWireError::StaleRuntime)?;
            let lease = runtime
                .lease
                .as_ref()
                .ok_or(AgentPortableWireError::StaleLease)?;
            (
                inner.fence_epoch,
                runtime.account_id.clone(),
                runtime.runtime_id.clone(),
                lease.wire.clone(),
                lease.native.clone(),
            )
        };
        match controller.network_changed(native).await {
            Ok(()) => {
                self.require_read_current(fence_epoch, &account_id, &runtime_id, &wire)?;
                Ok(())
            }
            Err(error) => {
                self.finish_failed_operation(
                    controller,
                    fence_epoch,
                    Some((&account_id, &runtime_id, Some(&wire))),
                    error,
                )
                .await
            }
        }
    }

    /// Native auth owner hook. The mapping is cleared before cleanup starts.
    pub(crate) async fn native_credentials_invalidated(
        &self,
    ) -> Result<(), AgentPortableWireError> {
        let Some(controller) = self.controller.clone() else {
            return Ok(());
        };
        self.clear_all()?;
        controller
            .native_credentials_invalidated()
            .await
            .map_err(AgentPortableWireError::from)
    }

    /// Native lifecycle hook. The mapping is cleared before cleanup starts.
    pub(crate) async fn native_dispose(&self) -> Result<(), AgentPortableWireError> {
        let Some(controller) = self.controller.clone() else {
            return Ok(());
        };
        self.clear_all()?;
        controller
            .dispose()
            .await
            .map_err(AgentPortableWireError::from)
    }

    fn begin_read(
        &self,
        account_id: &str,
        runtime_id: &str,
        lease: &AgentPortableWireLease,
    ) -> Result<ReadContext, AgentPortableWireError> {
        validate_account_id(account_id)?;
        validate_runtime_id(runtime_id)?;
        validate_wire_lease(lease)?;
        let controller = self.controller()?;
        let inner = self.lock_inner()?;
        let runtime = require_runtime(&inner, account_id, runtime_id)?;
        let binding = runtime
            .lease
            .as_ref()
            .ok_or(AgentPortableWireError::StaleLease)?;
        if binding.wire != *lease {
            return Err(AgentPortableWireError::StaleLease);
        }
        Ok((controller, inner.fence_epoch, binding.native.clone()))
    }

    fn require_read_current(
        &self,
        fence_epoch: u64,
        account_id: &str,
        runtime_id: &str,
        lease: &AgentPortableWireLease,
    ) -> Result<(), AgentPortableWireError> {
        let inner = self.lock_inner()?;
        let runtime = require_runtime(&inner, account_id, runtime_id)?;
        if inner.fence_epoch != fence_epoch {
            return Err(AgentPortableWireError::StaleLease);
        }
        if runtime
            .lease
            .as_ref()
            .is_none_or(|binding| binding.wire != *lease)
        {
            return Err(AgentPortableWireError::StaleLease);
        }
        Ok(())
    }

    async fn finish_failed_operation<T>(
        &self,
        controller: Arc<dyn PortableControllerApi>,
        fence_epoch: u64,
        binding: BindingExpectation<'_>,
        error: AgentPortableRemoteError,
    ) -> Result<T, AgentPortableWireError> {
        if should_retire_after(error)
            && self.clear_matching(fence_epoch, binding)?
            && controller.dispose().await.is_err()
        {
            return Err(AgentPortableWireError::CleanupFailed);
        }
        Err(error.into())
    }

    async fn retire_invalid_response<T>(
        &self,
        controller: Arc<dyn PortableControllerApi>,
        fence_epoch: u64,
        binding: BindingExpectation<'_>,
    ) -> Result<T, AgentPortableWireError> {
        if self.clear_matching(fence_epoch, binding)? && controller.dispose().await.is_err() {
            return Err(AgentPortableWireError::CleanupFailed);
        }
        Err(AgentPortableWireError::InvalidResponse)
    }

    fn clear_matching(
        &self,
        fence_epoch: u64,
        binding: BindingExpectation<'_>,
    ) -> Result<bool, AgentPortableWireError> {
        let mut inner = self.lock_inner()?;
        if inner.fence_epoch != fence_epoch {
            return Ok(false);
        }
        if let Some((account_id, runtime_id, expected_lease)) = binding {
            let Some(runtime) = inner.runtime.as_ref() else {
                return Ok(false);
            };
            if runtime.account_id != account_id || runtime.runtime_id != runtime_id {
                return Ok(false);
            }
            if expected_lease.is_some_and(|expected| {
                runtime
                    .lease
                    .as_ref()
                    .is_none_or(|lease| lease.wire != *expected)
            }) {
                return Ok(false);
            }
        }
        inner.fence_epoch = next_epoch(inner.fence_epoch)?;
        inner.runtime = None;
        Ok(true)
    }

    fn clear_all(&self) -> Result<(), AgentPortableWireError> {
        let mut inner = self.lock_inner()?;
        inner.fence_epoch = next_epoch(inner.fence_epoch)?;
        inner.runtime = None;
        Ok(())
    }

    fn lock_inner(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, AgentPortableTauriInner>, AgentPortableWireError> {
        self.inner
            .lock()
            .map_err(|_| AgentPortableWireError::Unavailable)
    }
}

type ReadContext = (Arc<dyn PortableControllerApi>, u64, PortableTargetLease);
type BindingExpectation<'a> = Option<(&'a str, &'a str, Option<&'a AgentPortableWireLease>)>;

fn require_runtime<'a>(
    inner: &'a AgentPortableTauriInner,
    account_id: &str,
    runtime_id: &str,
) -> Result<&'a RuntimeBinding, AgentPortableWireError> {
    let runtime = inner
        .runtime
        .as_ref()
        .ok_or(AgentPortableWireError::StaleRuntime)?;
    if runtime.account_id != account_id || runtime.runtime_id != runtime_id {
        return Err(AgentPortableWireError::StaleRuntime);
    }
    Ok(runtime)
}

fn require_runtime_mut<'a>(
    inner: &'a mut AgentPortableTauriInner,
    account_id: &str,
    runtime_id: &str,
) -> Result<&'a mut RuntimeBinding, AgentPortableWireError> {
    let runtime = inner
        .runtime
        .as_mut()
        .ok_or(AgentPortableWireError::StaleRuntime)?;
    if runtime.account_id != account_id || runtime.runtime_id != runtime_id {
        return Err(AgentPortableWireError::StaleRuntime);
    }
    Ok(runtime)
}

fn validate_account_id(value: &str) -> Result<(), AgentPortableWireError> {
    validate_uuid("portable account", value).map_err(|_| AgentPortableWireError::InvalidRequest)
}

fn validate_runtime_id(value: &str) -> Result<(), AgentPortableWireError> {
    validate_opaque_identifier(value, "runtime").map_err(|_| AgentPortableWireError::InvalidRequest)
}

fn validate_target_handle(value: &str) -> Result<(), AgentPortableWireError> {
    validate_opaque_identifier(value, "target").map_err(|_| AgentPortableWireError::InvalidRequest)
}

fn validate_wire_lease(lease: &AgentPortableWireLease) -> Result<(), AgentPortableWireError> {
    validate_opaque_identifier(&lease.lease_handle, "lease")
        .map_err(|_| AgentPortableWireError::InvalidRequest)?;
    validate_target_handle(&lease.target_handle)?;
    let host_epoch = lease
        .host_epoch
        .parse::<u64>()
        .map_err(|_| AgentPortableWireError::InvalidRequest)?;
    if host_epoch == 0
        || host_epoch.to_string() != lease.host_epoch
        || lease.connection_generation == 0
        || lease.connection_generation > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER
    {
        return Err(AgentPortableWireError::InvalidRequest);
    }
    Ok(())
}

fn next_epoch(current: u64) -> Result<u64, AgentPortableWireError> {
    current
        .checked_add(1)
        .ok_or(AgentPortableWireError::Unavailable)
}

fn should_retire_after(error: AgentPortableRemoteError) -> bool {
    matches!(
        error,
        AgentPortableRemoteError::Unavailable
            | AgentPortableRemoteError::UnsupportedStoredVersion
            | AgentPortableRemoteError::CorruptStoredRegistry
            | AgentPortableRemoteError::InvalidStoredRegistry
            | AgentPortableRemoteError::StoredRegistryRollback
            | AgentPortableRemoteError::StoredRegistryInterrupted
            | AgentPortableRemoteError::StoredRegistryEquivocation
            | AgentPortableRemoteError::StoredRegistryConflict
            | AgentPortableRemoteError::DuplicateStoredTarget
            | AgentPortableRemoteError::Unauthenticated
            | AgentPortableRemoteError::AccountMismatch
            | AgentPortableRemoteError::Revoked
            | AgentPortableRemoteError::VerificationFailed
            | AgentPortableRemoteError::UnknownTarget
            | AgentPortableRemoteError::StaleLease
            | AgentPortableRemoteError::InvalidResponse
            | AgentPortableRemoteError::PeerUnavailable
            | AgentPortableRemoteError::CleanupFailed
            | AgentPortableRemoteError::Internal
    )
}

fn decode_request<T: DeserializeOwned>(
    request: Option<serde_json::Value>,
) -> Result<T, AgentPortableWireError> {
    request
        .ok_or(AgentPortableWireError::InvalidRequest)
        .and_then(|request| {
            serde_json::from_value(request).map_err(|_| AgentPortableWireError::InvalidRequest)
        })
}

struct BoundedJsonWriter {
    written: usize,
    limit: usize,
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .written
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "portable page too large"))?;
        if next > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "portable page too large",
            ));
        }
        self.written = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialized_size_within_limit<T: Serialize>(
    value: &T,
    limit: usize,
) -> Result<usize, AgentPortableWireError> {
    let mut writer = BoundedJsonWriter { written: 0, limit };
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| AgentPortableWireError::InvalidResponse)?;
    Ok(writer.written)
}

fn serialized_history_page_within_limits(
    page: &PortableHistoryPage,
) -> Result<usize, AgentPortableWireError> {
    conservative_history_page_ledger_within_limit(page, MAX_PORTABLE_PAGE_JSON_BYTES)?;
    serialized_size_within_limit(page, MAX_PORTABLE_PAGE_JSON_BYTES)
}

fn conservative_history_page_ledger_within_limit(
    page: &PortableHistoryPage,
    limit: usize,
) -> Result<usize, AgentPortableWireError> {
    let mut total = PORTABLE_HISTORY_PAGE_LEDGER_BASE_BYTES
        .checked_add(page.history_revision.len())
        .and_then(|total| total.checked_add(page.next_cursor.as_deref().unwrap_or("").len()))
        .ok_or(AgentPortableWireError::InvalidResponse)?;
    if total > limit {
        return Err(AgentPortableWireError::InvalidResponse);
    }
    for record in &page.items {
        let record_bytes = serialized_size_within_limit(record, MAX_PORTABLE_HISTORY_RECORD_BYTES)?;
        total = total
            .checked_add(record_bytes)
            .and_then(|total| total.checked_add(1))
            .ok_or(AgentPortableWireError::InvalidResponse)?;
        if total > limit {
            return Err(AgentPortableWireError::InvalidResponse);
        }
    }
    Ok(total)
}

#[tauri::command]
pub(crate) async fn agent_portable_refresh_targets(
    state: tauri::State<'_, AgentPortableTauriState>,
    request: Option<serde_json::Value>,
) -> Result<RefreshTargetsResponse, AgentPortableWireError> {
    state.refresh_targets(decode_request(request)?).await
}

#[tauri::command]
pub(crate) async fn agent_portable_prepare_target(
    state: tauri::State<'_, AgentPortableTauriState>,
    request: Option<serde_json::Value>,
) -> Result<AgentPortableWireLease, AgentPortableWireError> {
    state.prepare_target(decode_request(request)?).await
}

#[tauri::command]
pub(crate) async fn agent_portable_get_runtime_status(
    state: tauri::State<'_, AgentPortableTauriState>,
    request: Option<serde_json::Value>,
) -> Result<PortableRuntimeStatus, AgentPortableWireError> {
    state.runtime_status(decode_request(request)?).await
}

#[tauri::command]
pub(crate) async fn agent_portable_list_sessions_page(
    state: tauri::State<'_, AgentPortableTauriState>,
    request: Option<serde_json::Value>,
) -> Result<PortableSessionPage, AgentPortableWireError> {
    state.sessions_page(decode_request(request)?).await
}

#[tauri::command]
pub(crate) async fn agent_portable_list_records_page(
    state: tauri::State<'_, AgentPortableTauriState>,
    request: Option<serde_json::Value>,
) -> Result<PortableHistoryPage, AgentPortableWireError> {
    state.records_page(decode_request(request)?).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use serde_json::{json, Value};
    use tokio::sync::Notify;

    use super::*;
    use crate::agent_remote_portable::{
        PortableHistoryRecord, PortableSessionSummary, PortableTimelineItem,
    };

    const ACCOUNT_ID: &str = "00000000-0000-4000-8000-000000000001";

    #[derive(Default)]
    struct TestGate {
        open: AtomicBool,
        notify: Notify,
    }

    impl TestGate {
        fn open(&self) {
            self.open.store(true, Ordering::Release);
            self.notify.notify_waiters();
        }

        async fn wait(&self) {
            loop {
                let notified = self.notify.notified();
                if self.open.load(Ordering::Acquire) {
                    return;
                }
                notified.await;
            }
        }
    }

    struct TestController {
        targets: Mutex<Result<Vec<PortableTargetDescriptor>, AgentPortableRemoteError>>,
        prepared_lease: PortableTargetLease,
        status: Mutex<Result<PortableRuntimeStatus, AgentPortableRemoteError>>,
        sessions: Mutex<Result<PortableSessionPage, AgentPortableRemoteError>>,
        records: Mutex<Result<PortableHistoryPage, AgentPortableRemoteError>>,
        prepare_gate: Mutex<Option<Arc<TestGate>>>,
        status_gate: Mutex<Option<Arc<TestGate>>>,
        dispose_gate: Mutex<Option<Arc<TestGate>>>,
        refresh_calls: AtomicU64,
        prepare_calls: AtomicU64,
        status_calls: AtomicU64,
        sessions_calls: AtomicU64,
        records_calls: AtomicU64,
        network_calls: AtomicU64,
        invalidation_calls: AtomicU64,
        dispose_calls: AtomicU64,
    }

    impl TestController {
        fn valid() -> Self {
            Self {
                targets: Mutex::new(Ok(vec![PortableTargetDescriptor {
                    handle: PortableTargetHandle(opaque("target", '1')),
                    label: "Paired Mac".to_string(),
                }])),
                prepared_lease: PortableTargetLease {
                    target_id: opaque("lease", '2'),
                    host_epoch: 7,
                    connection_generation: 9,
                },
                status: Mutex::new(Ok(PortableRuntimeStatus {
                    running: true,
                    active_run_count: 1,
                })),
                sessions: Mutex::new(Ok(sample_sessions_page())),
                records: Mutex::new(Ok(sample_records_page())),
                prepare_gate: Mutex::new(None),
                status_gate: Mutex::new(None),
                dispose_gate: Mutex::new(None),
                refresh_calls: AtomicU64::new(0),
                prepare_calls: AtomicU64::new(0),
                status_calls: AtomicU64::new(0),
                sessions_calls: AtomicU64::new(0),
                records_calls: AtomicU64::new(0),
                network_calls: AtomicU64::new(0),
                invalidation_calls: AtomicU64::new(0),
                dispose_calls: AtomicU64::new(0),
            }
        }
    }

    impl PortableControllerApi for TestController {
        fn refresh_targets(
            &self,
            _expected_account_id: String,
        ) -> PortableFuture<'_, Result<Vec<PortableTargetDescriptor>, AgentPortableRemoteError>>
        {
            self.refresh_calls.fetch_add(1, Ordering::AcqRel);
            let result = self.targets.lock().unwrap().clone();
            Box::pin(async move { result })
        }

        fn prepare_target(
            &self,
            _handle: PortableTargetHandle,
        ) -> PortableFuture<'_, Result<PortableTargetLease, AgentPortableRemoteError>> {
            self.prepare_calls.fetch_add(1, Ordering::AcqRel);
            let result = self.prepared_lease.clone();
            let gate = self.prepare_gate.lock().unwrap().clone();
            Box::pin(async move {
                if let Some(gate) = gate {
                    gate.wait().await;
                }
                Ok(result)
            })
        }

        fn runtime_status(
            &self,
            _lease: PortableTargetLease,
        ) -> PortableFuture<'_, Result<PortableRuntimeStatus, AgentPortableRemoteError>> {
            self.status_calls.fetch_add(1, Ordering::AcqRel);
            let result = self.status.lock().unwrap().clone();
            let gate = self.status_gate.lock().unwrap().clone();
            Box::pin(async move {
                if let Some(gate) = gate {
                    gate.wait().await;
                }
                result
            })
        }

        fn sessions_page(
            &self,
            _lease: PortableTargetLease,
            _request: PortablePageRequest,
        ) -> PortableFuture<'_, Result<PortableSessionPage, AgentPortableRemoteError>> {
            self.sessions_calls.fetch_add(1, Ordering::AcqRel);
            let result = self.sessions.lock().unwrap().clone();
            Box::pin(async move { result })
        }

        fn records_page(
            &self,
            _lease: PortableTargetLease,
            _request: PortableRecordsPageRequest,
        ) -> PortableFuture<'_, Result<PortableHistoryPage, AgentPortableRemoteError>> {
            self.records_calls.fetch_add(1, Ordering::AcqRel);
            let result = self.records.lock().unwrap().clone();
            Box::pin(async move { result })
        }

        fn network_changed(
            &self,
            _lease: PortableTargetLease,
        ) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>> {
            self.network_calls.fetch_add(1, Ordering::AcqRel);
            Box::pin(async { Ok(()) })
        }

        fn native_credentials_invalidated(
            &self,
        ) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>> {
            self.invalidation_calls.fetch_add(1, Ordering::AcqRel);
            Box::pin(async { Ok(()) })
        }

        fn dispose(&self) -> PortableFuture<'_, Result<(), AgentPortableRemoteError>> {
            self.dispose_calls.fetch_add(1, Ordering::AcqRel);
            let gate = self.dispose_gate.lock().unwrap().clone();
            Box::pin(async move {
                if let Some(gate) = gate {
                    gate.wait().await;
                }
                Ok(())
            })
        }
    }

    fn opaque(prefix: &str, digit: char) -> String {
        format!("{prefix}_{}", digit.to_string().repeat(48))
    }

    fn sample_sessions_page() -> PortableSessionPage {
        PortableSessionPage {
            items: vec![PortableSessionSummary {
                id: "session-1".to_string(),
                title: "Portable session".to_string(),
                created_ms: 1,
                updated_ms: 2,
                page_sort_ms: 2,
                message_count: 3,
            }],
            next_cursor: Some("sessions:next".to_string()),
        }
    }

    fn sample_timeline_item(text: String) -> PortableTimelineItem {
        PortableTimelineItem {
            id: "item-1".to_string(),
            item_type: "message".to_string(),
            role: Some("user".to_string()),
            title: Some("Message".to_string()),
            text: Some(text),
            status: Some("completed".to_string()),
            created_ms: 3,
            merge: "append".to_string(),
        }
    }

    fn sample_records_page() -> PortableHistoryPage {
        PortableHistoryPage {
            items: vec![PortableHistoryRecord {
                record_id: "record:1".to_string(),
                role: "user".to_string(),
                created_ms: 3,
                items: vec![sample_timeline_item("hello".to_string())],
            }],
            history_revision: "history:1".to_string(),
            next_cursor: Some("records:next".to_string()),
        }
    }

    fn escape_heavy_record_with_json_size(target_bytes: usize) -> PortableHistoryRecord {
        escape_heavy_record_with_json_size_and_suffix(target_bytes, "escape-heavy")
    }

    fn escape_heavy_record_with_json_size_and_suffix(
        target_bytes: usize,
        suffix: &str,
    ) -> PortableHistoryRecord {
        const ITEM_COUNT: usize = 4;
        const MAX_NEWLINES_PER_ITEM: usize = 150_000;

        let mut record = PortableHistoryRecord {
            record_id: format!("record:{suffix}"),
            role: "assistant".to_string(),
            created_ms: 3,
            items: (0..ITEM_COUNT)
                .map(|index| PortableTimelineItem {
                    id: format!("item-{suffix}-{index}"),
                    item_type: "message".to_string(),
                    role: Some("assistant".to_string()),
                    title: None,
                    text: Some(String::new()),
                    status: None,
                    created_ms: 3,
                    merge: "append".to_string(),
                })
                .collect(),
        };
        let base_bytes = serialized_size_within_limit(&record, usize::MAX).unwrap();
        assert!(base_bytes <= target_bytes);
        let mut remaining = target_bytes - base_bytes;
        for item in &mut record.items {
            let newline_count = (remaining / 2).min(MAX_NEWLINES_PER_ITEM);
            item.text
                .as_mut()
                .unwrap()
                .push_str(&"\n".repeat(newline_count));
            remaining -= newline_count * 2;
        }
        if remaining == 1 {
            record
                .items
                .last_mut()
                .unwrap()
                .text
                .as_mut()
                .unwrap()
                .push('x');
            remaining = 0;
        }
        assert_eq!(remaining, 0);
        assert_eq!(
            serialized_size_within_limit(&record, usize::MAX).unwrap(),
            target_bytes
        );
        record
    }

    fn history_page_at_conservative_ledger_limit() -> PortableHistoryPage {
        const FIXED_RECORDS: usize = 8;
        const FIXED_RECORD_JSON_BYTES: usize = 930_000;

        let history_revision = "history:ledger-boundary".to_string();
        let next_cursor = Some("records:ledger-next".to_string());
        let metadata_bytes = PORTABLE_HISTORY_PAGE_LEDGER_BASE_BYTES
            + history_revision.len()
            + next_cursor.as_deref().unwrap().len();
        let final_record_json_bytes = MAX_PORTABLE_PAGE_JSON_BYTES
            - metadata_bytes
            - FIXED_RECORDS * (FIXED_RECORD_JSON_BYTES + 1)
            - 1;
        assert!(final_record_json_bytes <= MAX_PORTABLE_HISTORY_RECORD_BYTES);

        let mut items = (0..FIXED_RECORDS)
            .map(|index| {
                escape_heavy_record_with_json_size_and_suffix(
                    FIXED_RECORD_JSON_BYTES,
                    &format!("ledger-{index}"),
                )
            })
            .collect::<Vec<_>>();
        items.push(escape_heavy_record_with_json_size_and_suffix(
            final_record_json_bytes,
            "ledger-final",
        ));
        PortableHistoryPage {
            items,
            history_revision,
            next_cursor,
        }
    }

    fn increment_last_record_json_byte(page: &mut PortableHistoryPage) {
        page.items
            .last_mut()
            .unwrap()
            .items
            .last_mut()
            .unwrap()
            .text
            .as_mut()
            .unwrap()
            .push('x');
    }

    fn refresh_request() -> RefreshTargetsRequest {
        RefreshTargetsRequest {
            account_id: ACCOUNT_ID.to_string(),
        }
    }

    async fn bootstrap(
        state: &AgentPortableTauriState,
    ) -> (RefreshTargetsResponse, AgentPortableWireLease) {
        let refresh = state.refresh_targets(refresh_request()).await.unwrap();
        let lease = state
            .prepare_target(PrepareTargetRequest {
                account_id: ACCOUNT_ID.to_string(),
                runtime_id: refresh.runtime_id.clone(),
                target_handle: refresh.items[0].handle.0.clone(),
            })
            .await
            .unwrap();
        (refresh, lease)
    }

    async fn wait_for_counter(counter: &AtomicU64, expected: u64) {
        for _ in 0..10_000 {
            if counter.load(Ordering::Acquire) >= expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("counter did not reach {expected}");
    }

    fn serialized_keys(value: &impl Serialize) -> Vec<String> {
        let mut keys = serde_json::to_value(value)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    #[test]
    fn command_roster_is_mobile_only_and_exactly_five() {
        let source = include_str!("lib.rs");
        for command in [
            "agent_remote_portable::tauri::agent_portable_refresh_targets",
            "agent_remote_portable::tauri::agent_portable_prepare_target",
            "agent_remote_portable::tauri::agent_portable_get_runtime_status",
            "agent_remote_portable::tauri::agent_portable_list_sessions_page",
            "agent_remote_portable::tauri::agent_portable_list_records_page",
        ] {
            assert_eq!(source.matches(command).count(), 2, "{command}");
        }
        for forbidden in [
            "agent_remote_portable::tauri::agent_portable_network_changed",
            "agent_remote_portable::tauri::agent_portable_release",
            "agent_remote_portable::tauri::agent_portable_dispose",
            "agent_remote_portable::tauri::agent_portable_native_credentials_invalidated",
        ] {
            assert_eq!(source.matches(forbidden).count(), 0, "{forbidden}");
        }

        let desktop = source
            .split("// Mobile (iOS and Android) configuration")
            .next()
            .unwrap();
        assert!(!desktop.contains("agent_remote_portable::tauri::agent_portable_"));
    }

    #[tokio::test]
    async fn disabled_state_fails_before_any_runtime_or_controller_activity() {
        let state = AgentPortableTauriState::disabled();
        assert!(state.controller.is_none());
        assert_eq!(
            state.refresh_targets(refresh_request()).await.unwrap_err(),
            AgentPortableWireError::Unavailable
        );
        let inner = state.inner.lock().unwrap();
        assert_eq!(inner.fence_epoch, 0);
        assert!(inner.runtime.is_none());
    }

    #[test]
    fn request_decoding_and_error_wire_are_closed() {
        assert_eq!(
            decode_request::<RefreshTargetsRequest>(Some(json!({
                "accountId": ACCOUNT_ID,
                "unexpected": true,
            })))
            .err()
            .unwrap(),
            AgentPortableWireError::InvalidRequest
        );
        assert_eq!(
            decode_request::<RefreshTargetsRequest>(None).err().unwrap(),
            AgentPortableWireError::InvalidRequest
        );
        let noncanonical_epoch = AgentPortableWireLease {
            lease_handle: opaque("lease", '1'),
            target_handle: opaque("target", '2'),
            host_epoch: "07".to_string(),
            connection_generation: 1,
        };
        assert_eq!(
            validate_wire_lease(&noncanonical_epoch).unwrap_err(),
            AgentPortableWireError::InvalidRequest
        );
        let full_epoch = AgentPortableWireLease {
            lease_handle: opaque("lease", '1'),
            target_handle: opaque("target", '2'),
            host_epoch: u64::MAX.to_string(),
            connection_generation: 1,
        };
        validate_wire_lease(&full_epoch).unwrap();
        let full_epoch_json = serde_json::to_value(&full_epoch).unwrap();
        assert_eq!(full_epoch_json["hostEpoch"], json!(u64::MAX.to_string()));
        assert!(full_epoch_json["hostEpoch"].is_string());
        assert!(full_epoch_json["connectionGeneration"].is_number());

        let mut overflow_epoch = full_epoch;
        overflow_epoch.host_epoch = "18446744073709551616".to_string();
        assert_eq!(
            validate_wire_lease(&overflow_epoch).unwrap_err(),
            AgentPortableWireError::InvalidRequest
        );
        assert_eq!(
            decode_request::<ReadRequest>(Some(json!({
                "accountId": ACCOUNT_ID,
                "runtimeId": opaque("runtime", '3'),
                "lease": {
                    "leaseHandle": opaque("lease", '1'),
                    "targetHandle": opaque("target", '2'),
                    "hostEpoch": "7",
                    "connectionGeneration": 1,
                    "unexpected": true,
                },
            })))
            .err()
            .unwrap(),
            AgentPortableWireError::InvalidRequest
        );

        let errors = [
            (AgentPortableWireError::Unavailable, "unavailable"),
            (AgentPortableWireError::Unauthenticated, "unauthenticated"),
            (
                AgentPortableWireError::PairingUnavailable,
                "pairing_unavailable",
            ),
            (AgentPortableWireError::UnknownTarget, "unknown_target"),
            (AgentPortableWireError::Busy, "busy"),
            (AgentPortableWireError::Cancelled, "cancelled"),
            (AgentPortableWireError::StaleRuntime, "stale_runtime"),
            (AgentPortableWireError::StaleLease, "stale_lease"),
            (AgentPortableWireError::InvalidRequest, "invalid_request"),
            (AgentPortableWireError::InvalidResponse, "invalid_response"),
            (AgentPortableWireError::PeerUnavailable, "peer_unavailable"),
            (AgentPortableWireError::CleanupFailed, "cleanup_failed"),
        ];
        for (error, code) in errors {
            assert_eq!(
                serde_json::to_value(error).unwrap(),
                json!({ "code": code })
            );
        }
    }

    #[test]
    fn serialized_page_budget_is_inclusive_and_exact() {
        let exact_payload = "x".repeat(MAX_PORTABLE_PAGE_JSON_BYTES - 2);
        assert_eq!(
            serialized_size_within_limit(&exact_payload, MAX_PORTABLE_PAGE_JSON_BYTES).unwrap(),
            MAX_PORTABLE_PAGE_JSON_BYTES
        );
        let oversized_payload = format!("{exact_payload}x");
        assert_eq!(
            serialized_size_within_limit(&oversized_payload, MAX_PORTABLE_PAGE_JSON_BYTES)
                .unwrap_err(),
            AgentPortableWireError::InvalidResponse
        );
    }

    #[test]
    fn escape_heavy_record_json_budget_is_inclusive_and_exact() {
        let exact = escape_heavy_record_with_json_size(MAX_PORTABLE_HISTORY_RECORD_BYTES);
        exact.validate().unwrap();
        let exact_page = PortableHistoryPage {
            items: vec![exact.clone()],
            history_revision: "history:escape-heavy".to_string(),
            next_cursor: None,
        };
        assert!(serialized_history_page_within_limits(&exact_page).is_ok());

        let mut oversized = exact;
        oversized
            .items
            .last_mut()
            .unwrap()
            .text
            .as_mut()
            .unwrap()
            .push('x');
        assert_eq!(
            serialized_size_within_limit(&oversized, usize::MAX).unwrap(),
            MAX_PORTABLE_HISTORY_RECORD_BYTES + 1
        );
        // The retained native CBOR presentation limit still accepts this
        // escape-heavy record; the Tauri JSON limit must reject it.
        oversized.validate().unwrap();
        let oversized_page = PortableHistoryPage {
            items: vec![oversized],
            history_revision: "history:escape-heavy".to_string(),
            next_cursor: None,
        };
        assert!(
            serialized_size_within_limit(&oversized_page, MAX_PORTABLE_PAGE_JSON_BYTES).is_ok()
        );
        assert_eq!(
            serialized_history_page_within_limits(&oversized_page).unwrap_err(),
            AgentPortableWireError::InvalidResponse
        );
    }

    #[test]
    fn conservative_history_page_ledger_is_inclusive_and_exact() {
        let exact = history_page_at_conservative_ledger_limit();
        let request = PortableRecordsPageRequest {
            session_id: "session-1".to_string(),
            cursor: None,
            limit: u16::try_from(exact.items.len()).unwrap(),
        };
        exact.validate_for(&request).unwrap();
        assert!(exact.items.iter().all(|record| {
            serialized_size_within_limit(record, MAX_PORTABLE_HISTORY_RECORD_BYTES).is_ok()
        }));
        let exact_page_json =
            serialized_size_within_limit(&exact, MAX_PORTABLE_PAGE_JSON_BYTES).unwrap();
        assert!(exact_page_json < MAX_PORTABLE_PAGE_JSON_BYTES);
        assert_eq!(
            conservative_history_page_ledger_within_limit(&exact, usize::MAX).unwrap(),
            MAX_PORTABLE_PAGE_JSON_BYTES
        );
        assert_eq!(
            serialized_history_page_within_limits(&exact).unwrap(),
            exact_page_json
        );

        let mut oversized = exact;
        increment_last_record_json_byte(&mut oversized);
        oversized.validate_for(&request).unwrap();
        assert!(oversized.items.iter().all(|record| {
            serialized_size_within_limit(record, MAX_PORTABLE_HISTORY_RECORD_BYTES).is_ok()
        }));
        assert!(serialized_size_within_limit(&oversized, MAX_PORTABLE_PAGE_JSON_BYTES).is_ok());
        assert_eq!(
            conservative_history_page_ledger_within_limit(&oversized, usize::MAX).unwrap(),
            MAX_PORTABLE_PAGE_JSON_BYTES + 1
        );
        assert_eq!(
            serialized_history_page_within_limits(&oversized).unwrap_err(),
            AgentPortableWireError::InvalidResponse
        );
    }

    #[tokio::test]
    async fn refresh_and_prepare_issue_distinct_strict_runtime_and_lease_handles() {
        let controller = Arc::new(TestController::valid());
        let state = AgentPortableTauriState::with_controller(controller.clone());
        let (refresh, lease) = bootstrap(&state).await;

        assert_eq!(refresh.schema_version, 1);
        assert_eq!(
            refresh.capabilities,
            AgentPortableReadCapabilities::READ_ONLY
        );
        assert_eq!(refresh.items.len(), 1);
        assert!(refresh.runtime_id.starts_with("runtime_"));
        assert!(lease.lease_handle.starts_with("lease_"));
        assert!(lease.target_handle.starts_with("target_"));
        assert_ne!(lease.lease_handle, lease.target_handle);
        assert_eq!(lease.host_epoch, "7");
        assert_eq!(lease.connection_generation, 9);
        assert_eq!(controller.refresh_calls.load(Ordering::Acquire), 1);
        assert_eq!(controller.prepare_calls.load(Ordering::Acquire), 1);

        let value = serde_json::to_value((&refresh, &lease)).unwrap();
        let serialized = value.to_string();
        for forbidden in [
            "accountId",
            "projectId",
            "projectRoot",
            "endpointId",
            "model",
            "mode",
            "runId",
            "targetId",
        ] {
            assert!(!serialized.contains(forbidden), "{forbidden}");
        }
        let Value::Array(values) = value else {
            panic!("tuple should serialize as an array");
        };
        assert_eq!(
            serialized_keys(&values[0]),
            ["capabilities", "items", "runtimeId", "schemaVersion",]
        );
        assert_eq!(
            serialized_keys(&values[0]["capabilities"]),
            [
                "mutations",
                "persistedRecordsPage",
                "runtimeStatus",
                "sessionSummariesPage",
                "synchronizedLiveTail",
            ]
        );
        assert_eq!(serialized_keys(&values[0]["items"][0]), ["handle", "label"]);
        assert_eq!(
            serialized_keys(&values[1]),
            [
                "connectionGeneration",
                "hostEpoch",
                "leaseHandle",
                "targetHandle",
            ]
        );
    }

    #[tokio::test]
    async fn prepare_projects_full_u64_host_epoch_only_as_a_decimal_string() {
        let mut controller = TestController::valid();
        controller.prepared_lease.host_epoch = u64::MAX;
        let controller = Arc::new(controller);
        let state = AgentPortableTauriState::with_controller(controller);
        let (_, lease) = bootstrap(&state).await;

        assert_eq!(lease.host_epoch, u64::MAX.to_string());
        validate_wire_lease(&lease).unwrap();
        let value = serde_json::to_value(&lease).unwrap();
        assert_eq!(value["hostEpoch"], json!(u64::MAX.to_string()));
        assert!(value["hostEpoch"].is_string());
        assert!(value["connectionGeneration"].is_number());

        let native_host_epoch = state
            .inner
            .lock()
            .unwrap()
            .runtime
            .as_ref()
            .unwrap()
            .lease
            .as_ref()
            .unwrap()
            .native
            .host_epoch;
        assert_eq!(native_host_epoch, u64::MAX);
    }

    #[tokio::test]
    async fn forged_runtime_target_and_lease_fail_before_controller_calls() {
        let controller = Arc::new(TestController::valid());
        let state = AgentPortableTauriState::with_controller(controller.clone());
        let (refresh, lease) = bootstrap(&state).await;

        let prepare_calls = controller.prepare_calls.load(Ordering::Acquire);
        assert_eq!(
            state
                .prepare_target(PrepareTargetRequest {
                    account_id: ACCOUNT_ID.to_string(),
                    runtime_id: opaque("runtime", 'f'),
                    target_handle: refresh.items[0].handle.0.clone(),
                })
                .await
                .unwrap_err(),
            AgentPortableWireError::StaleRuntime
        );
        assert_eq!(
            state
                .prepare_target(PrepareTargetRequest {
                    account_id: ACCOUNT_ID.to_string(),
                    runtime_id: refresh.runtime_id.clone(),
                    target_handle: opaque("target", 'f'),
                })
                .await
                .unwrap_err(),
            AgentPortableWireError::UnknownTarget
        );
        assert_eq!(
            controller.prepare_calls.load(Ordering::Acquire),
            prepare_calls
        );

        let mut forged_lease = lease;
        forged_lease.connection_generation += 1;
        assert_eq!(
            state
                .runtime_status(ReadRequest {
                    account_id: ACCOUNT_ID.to_string(),
                    runtime_id: refresh.runtime_id,
                    lease: forged_lease,
                })
                .await
                .unwrap_err(),
            AgentPortableWireError::StaleLease
        );
        assert_eq!(controller.status_calls.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn reads_return_only_sanitized_pages_with_exact_outer_keys() {
        let controller = Arc::new(TestController::valid());
        let state = AgentPortableTauriState::with_controller(controller.clone());
        let (refresh, lease) = bootstrap(&state).await;
        let status = state
            .runtime_status(ReadRequest {
                account_id: ACCOUNT_ID.to_string(),
                runtime_id: refresh.runtime_id.clone(),
                lease: lease.clone(),
            })
            .await
            .unwrap();
        let sessions = state
            .sessions_page(SessionsPageCommandRequest {
                account_id: ACCOUNT_ID.to_string(),
                runtime_id: refresh.runtime_id.clone(),
                lease: lease.clone(),
                page: PortablePageRequest {
                    cursor: None,
                    limit: 10,
                },
            })
            .await
            .unwrap();
        let records = state
            .records_page(RecordsPageCommandRequest {
                account_id: ACCOUNT_ID.to_string(),
                runtime_id: refresh.runtime_id,
                lease,
                page: PortableRecordsPageRequest {
                    session_id: "session-1".to_string(),
                    cursor: None,
                    limit: 10,
                },
            })
            .await
            .unwrap();
        assert!(status.running);
        assert_eq!(controller.status_calls.load(Ordering::Acquire), 1);
        assert_eq!(controller.sessions_calls.load(Ordering::Acquire), 1);
        assert_eq!(controller.records_calls.load(Ordering::Acquire), 1);
        assert_eq!(serialized_keys(&status), ["activeRunCount", "running"]);
        assert_eq!(serialized_keys(&sessions), ["items", "nextCursor"]);
        assert_eq!(
            serialized_keys(&sessions.items[0]),
            [
                "createdMs",
                "id",
                "messageCount",
                "pageSortMs",
                "title",
                "updatedMs",
            ]
        );
        assert_eq!(
            serialized_keys(&records),
            ["historyRevision", "items", "nextCursor"]
        );
        assert_eq!(
            serialized_keys(&records.items[0]),
            ["createdMs", "items", "recordId", "role"]
        );
        assert_eq!(
            serialized_keys(&records.items[0].items[0]),
            [
                "createdMs",
                "id",
                "itemType",
                "merge",
                "role",
                "status",
                "text",
                "title",
            ]
        );
    }

    #[tokio::test]
    async fn oversized_records_page_is_rejected_and_cleanup_is_acknowledged() {
        let controller = Arc::new(TestController::valid());
        let large_item = sample_timeline_item("x".repeat(190_000));
        let record_items = vec![large_item; 5];
        let records = (0..9)
            .map(|index| PortableHistoryRecord {
                record_id: format!("record:{index}"),
                role: "assistant".to_string(),
                created_ms: index,
                items: record_items.clone(),
            })
            .collect();
        *controller.records.lock().unwrap() = Ok(PortableHistoryPage {
            items: records,
            history_revision: "history:large".to_string(),
            next_cursor: None,
        });
        let state = AgentPortableTauriState::with_controller(controller.clone());
        let (refresh, lease) = bootstrap(&state).await;
        assert_eq!(
            state
                .records_page(RecordsPageCommandRequest {
                    account_id: ACCOUNT_ID.to_string(),
                    runtime_id: refresh.runtime_id,
                    lease,
                    page: PortableRecordsPageRequest {
                        session_id: "session-1".to_string(),
                        cursor: None,
                        limit: 10,
                    },
                })
                .await
                .unwrap_err(),
            AgentPortableWireError::InvalidResponse
        );
        assert_eq!(controller.records_calls.load(Ordering::Acquire), 1);
        assert_eq!(controller.dispose_calls.load(Ordering::Acquire), 1);
        assert!(state.inner.lock().unwrap().runtime.is_none());
    }

    #[tokio::test]
    async fn json_oversized_record_is_rejected_and_cleanup_is_acknowledged() {
        let controller = Arc::new(TestController::valid());
        let dispose_gate = Arc::new(TestGate::default());
        *controller.dispose_gate.lock().unwrap() = Some(dispose_gate.clone());
        let mut oversized = escape_heavy_record_with_json_size(MAX_PORTABLE_HISTORY_RECORD_BYTES);
        oversized
            .items
            .last_mut()
            .unwrap()
            .text
            .as_mut()
            .unwrap()
            .push('x');
        oversized.validate().unwrap();
        let response = PortableHistoryPage {
            items: vec![oversized],
            history_revision: "history:escape-heavy".to_string(),
            next_cursor: None,
        };
        assert!(serialized_size_within_limit(&response, MAX_PORTABLE_PAGE_JSON_BYTES).is_ok());
        *controller.records.lock().unwrap() = Ok(response);

        let state = Arc::new(AgentPortableTauriState::with_controller(controller.clone()));
        let (refresh, lease) = bootstrap(&state).await;
        let task_state = state.clone();
        let records = tokio::spawn(async move {
            task_state
                .records_page(RecordsPageCommandRequest {
                    account_id: ACCOUNT_ID.to_string(),
                    runtime_id: refresh.runtime_id,
                    lease,
                    page: PortableRecordsPageRequest {
                        session_id: "session-1".to_string(),
                        cursor: None,
                        limit: 1,
                    },
                })
                .await
        });
        wait_for_counter(&controller.dispose_calls, 1).await;
        assert_eq!(controller.records_calls.load(Ordering::Acquire), 1);
        assert!(state.inner.lock().unwrap().runtime.is_none());
        assert!(!records.is_finished());
        dispose_gate.open();
        assert_eq!(
            records.await.unwrap().unwrap_err(),
            AgentPortableWireError::InvalidResponse
        );
        assert_eq!(controller.dispose_calls.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn conservative_ledger_overflow_is_rejected_and_cleanup_is_acknowledged() {
        let controller = Arc::new(TestController::valid());
        let dispose_gate = Arc::new(TestGate::default());
        *controller.dispose_gate.lock().unwrap() = Some(dispose_gate.clone());
        let mut response = history_page_at_conservative_ledger_limit();
        increment_last_record_json_byte(&mut response);
        let page_limit = u16::try_from(response.items.len()).unwrap();
        let request = PortableRecordsPageRequest {
            session_id: "session-1".to_string(),
            cursor: None,
            limit: page_limit,
        };
        response.validate_for(&request).unwrap();
        assert!(response.items.iter().all(|record| {
            serialized_size_within_limit(record, MAX_PORTABLE_HISTORY_RECORD_BYTES).is_ok()
        }));
        assert!(serialized_size_within_limit(&response, MAX_PORTABLE_PAGE_JSON_BYTES).is_ok());
        assert_eq!(
            conservative_history_page_ledger_within_limit(&response, usize::MAX).unwrap(),
            MAX_PORTABLE_PAGE_JSON_BYTES + 1
        );
        *controller.records.lock().unwrap() = Ok(response);

        let state = Arc::new(AgentPortableTauriState::with_controller(controller.clone()));
        let (refresh, lease) = bootstrap(&state).await;
        let task_state = state.clone();
        let records = tokio::spawn(async move {
            task_state
                .records_page(RecordsPageCommandRequest {
                    account_id: ACCOUNT_ID.to_string(),
                    runtime_id: refresh.runtime_id,
                    lease,
                    page: request,
                })
                .await
        });
        wait_for_counter(&controller.dispose_calls, 1).await;
        assert_eq!(controller.records_calls.load(Ordering::Acquire), 1);
        assert!(state.inner.lock().unwrap().runtime.is_none());
        assert!(!records.is_finished());
        dispose_gate.open();
        assert_eq!(
            records.await.unwrap().unwrap_err(),
            AgentPortableWireError::InvalidResponse
        );
        assert_eq!(controller.dispose_calls.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn native_invalidation_fences_inflight_prepare_before_publication() {
        let controller = Arc::new(TestController::valid());
        let prepare_gate = Arc::new(TestGate::default());
        *controller.prepare_gate.lock().unwrap() = Some(prepare_gate.clone());
        let state = Arc::new(AgentPortableTauriState::with_controller(controller.clone()));
        let refresh = state.refresh_targets(refresh_request()).await.unwrap();
        let task_state = state.clone();
        let runtime_id = refresh.runtime_id.clone();
        let target_handle = refresh.items[0].handle.0.clone();
        let prepare = tokio::spawn(async move {
            task_state
                .prepare_target(PrepareTargetRequest {
                    account_id: ACCOUNT_ID.to_string(),
                    runtime_id,
                    target_handle,
                })
                .await
        });
        wait_for_counter(&controller.prepare_calls, 1).await;
        state.native_credentials_invalidated().await.unwrap();
        assert!(state.inner.lock().unwrap().runtime.is_none());
        prepare_gate.open();
        assert_eq!(
            prepare.await.unwrap().unwrap_err(),
            AgentPortableWireError::Cancelled
        );
        assert_eq!(controller.invalidation_calls.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn native_invalidation_suppresses_inflight_read_response() {
        let controller = Arc::new(TestController::valid());
        let status_gate = Arc::new(TestGate::default());
        *controller.status_gate.lock().unwrap() = Some(status_gate.clone());
        let state = Arc::new(AgentPortableTauriState::with_controller(controller.clone()));
        let (refresh, lease) = bootstrap(&state).await;
        let task_state = state.clone();
        let status = tokio::spawn(async move {
            task_state
                .runtime_status(ReadRequest {
                    account_id: ACCOUNT_ID.to_string(),
                    runtime_id: refresh.runtime_id,
                    lease,
                })
                .await
        });
        wait_for_counter(&controller.status_calls, 1).await;
        state.native_credentials_invalidated().await.unwrap();
        status_gate.open();
        assert_eq!(
            status.await.unwrap().unwrap_err(),
            AgentPortableWireError::StaleRuntime
        );
    }

    #[tokio::test]
    async fn native_dispose_clears_mapping_before_awaiting_acknowledgement() {
        let controller = Arc::new(TestController::valid());
        let dispose_gate = Arc::new(TestGate::default());
        *controller.dispose_gate.lock().unwrap() = Some(dispose_gate.clone());
        let state = Arc::new(AgentPortableTauriState::with_controller(controller.clone()));
        bootstrap(&state).await;
        let task_state = state.clone();
        let dispose = tokio::spawn(async move { task_state.native_dispose().await });
        wait_for_counter(&controller.dispose_calls, 1).await;
        assert!(state.inner.lock().unwrap().runtime.is_none());
        assert!(!dispose.is_finished());
        dispose_gate.open();
        dispose.await.unwrap().unwrap();
    }
}
