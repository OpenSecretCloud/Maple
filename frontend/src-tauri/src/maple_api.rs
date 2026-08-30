use crate::open_secret_config::{configured_pcr0_environment, normalize_api_url};
use opensecret::{
    InferenceRequest, InferenceResponse, OpenSecretClient, WebExtractRequest, WebExtractResponse,
    WebSearchRequest, WebSearchResponse,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, RwLock, RwLockReadGuard};
use tokio_util::sync::CancellationToken;

const AUTH_CHANGED_EVENT: &str = "maple-api-auth-changed";
const CREDENTIAL_VALIDATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapleApiAuthRequest {
    pub user_id: String,
    pub api_url: String,
    pub auth_bundle: String,
}

#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapleApiAuthSnapshot {
    pub user_id: String,
    pub auth_bundle: String,
    pub native_instance_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MapleApiAuthChanged {
    user_id: String,
    revision: u64,
    authenticated: bool,
}

trait MapleApiAuthEventSink: Send + Sync {
    fn auth_changed(&self, user_id: &str, revision: u64, authenticated: bool);
}

struct TauriAuthEventSink(AppHandle);

impl MapleApiAuthEventSink for TauriAuthEventSink {
    fn auth_changed(&self, user_id: &str, revision: u64, authenticated: bool) {
        if let Err(error) = self.0.emit(
            AUTH_CHANGED_EVENT,
            MapleApiAuthChanged {
                user_id: user_id.to_string(),
                revision,
                authenticated,
            },
        ) {
            log::warn!("Failed to notify Maple of refreshed API credentials: {error}");
        }
    }
}

struct CredentialedClient {
    generation: u64,
    api_url: String,
    client: Arc<OpenSecretClient>,
}

struct MapleApiSessionInner {
    active: bool,
    revision: u64,
    credentials: CredentialedClient,
}

/// A stable, account-scoped handle shared by Maple's native provider and tools.
///
/// Replacing browser credentials swaps the underlying client atomically. Calls
/// already in flight retain their client snapshot, while their late refreshes
/// are prevented from overwriting a newer generation.
pub(crate) struct MapleApiSession {
    user_id: String,
    account_scope: String,
    native_instance_id: String,
    event_sink: Arc<dyn MapleApiAuthEventSink>,
    inner: RwLock<MapleApiSessionInner>,
}

pub(crate) struct MapleApiAuthLease<'a> {
    _inner: RwLockReadGuard<'a, MapleApiSessionInner>,
}

struct ClientSnapshot {
    generation: u64,
    client: Arc<OpenSecretClient>,
    auth_bundle_before: String,
}

struct CancelOperationOnDrop(CancellationToken);

impl Drop for CancelOperationOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl MapleApiSession {
    fn new(
        event_sink: Arc<dyn MapleApiAuthEventSink>,
        user_id: String,
        account_scope: String,
        native_instance_id: String,
        api_url: String,
        client: Arc<OpenSecretClient>,
    ) -> Result<Self, String> {
        capture_auth_bundle(&client)?;
        Ok(Self {
            user_id,
            account_scope,
            native_instance_id,
            event_sink,
            inner: RwLock::new(MapleApiSessionInner {
                active: true,
                revision: 1,
                credentials: CredentialedClient {
                    generation: 1,
                    api_url,
                    client,
                },
            }),
        })
    }

    pub(crate) fn account_scope(&self) -> &str {
        &self.account_scope
    }

    async fn replace_client(
        &self,
        api_url: String,
        client: Arc<OpenSecretClient>,
    ) -> Result<MapleApiAuthSnapshot, String> {
        let replacement_bundle = capture_auth_bundle(&client)?;
        let mut inner = self.inner.write().await;
        if !inner.active {
            return Err("Maple API authentication is no longer active".to_string());
        }

        let current_bundle = capture_auth_bundle(&inner.credentials.client)?;
        if inner.credentials.api_url == api_url && current_bundle == replacement_bundle {
            return snapshot_from_inner(&self.user_id, &self.native_instance_id, &inner);
        }

        let generation = inner
            .credentials
            .generation
            .checked_add(1)
            .ok_or_else(|| "Maple API credential generation exhausted".to_string())?;
        inner.revision = inner
            .revision
            .checked_add(1)
            .ok_or_else(|| "Maple API authentication revision exhausted".to_string())?;
        inner.credentials = CredentialedClient {
            generation,
            api_url,
            client,
        };
        snapshot_from_inner(&self.user_id, &self.native_instance_id, &inner)
    }

    async fn invalidate(&self) {
        self.inner.write().await.active = false;
    }

    pub(crate) async fn active_lease(&self) -> Result<MapleApiAuthLease<'_>, String> {
        let inner = self.inner.read().await;
        if !inner.active {
            return Err("Maple API authentication is no longer active".to_string());
        }
        Ok(MapleApiAuthLease { _inner: inner })
    }

    #[cfg(test)]
    pub(crate) async fn invalidate_for_test(&self) {
        self.invalidate().await;
    }

    async fn client_snapshot(&self) -> Result<ClientSnapshot, String> {
        let inner = self.inner.read().await;
        if !inner.active {
            return Err("Maple API authentication is no longer active".to_string());
        }
        let client = Arc::clone(&inner.credentials.client);
        Ok(ClientSnapshot {
            generation: inner.credentials.generation,
            auth_bundle_before: capture_auth_bundle(&client)?,
            client,
        })
    }

    async fn record_refresh(&self, snapshot: &ClientSnapshot) -> Result<(), String> {
        self.record_auth_bundle_change(snapshot, capture_auth_bundle(&snapshot.client))
            .await
    }

    async fn record_auth_bundle_change(
        &self,
        snapshot: &ClientSnapshot,
        auth_bundle_after: Result<String, String>,
    ) -> Result<(), String> {
        let auth_bundle_after = match auth_bundle_after {
            Ok(bundle) => bundle,
            Err(error) => {
                let revision = {
                    let mut inner = self.inner.write().await;
                    if !inner.active
                        || inner.credentials.generation != snapshot.generation
                        || !Arc::ptr_eq(&inner.credentials.client, &snapshot.client)
                    {
                        return Ok(());
                    }
                    inner.active = false;
                    inner.revision = inner
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| "Maple API authentication revision exhausted".to_string())?;
                    inner.revision
                };
                self.event_sink.auth_changed(&self.user_id, revision, false);
                return Err(error);
            }
        };
        if auth_bundle_after == snapshot.auth_bundle_before {
            return Ok(());
        }

        let revision = {
            let mut inner = self.inner.write().await;
            if !inner.active
                || inner.credentials.generation != snapshot.generation
                || !Arc::ptr_eq(&inner.credentials.client, &snapshot.client)
            {
                return Ok(());
            }
            inner.revision = inner
                .revision
                .checked_add(1)
                .ok_or_else(|| "Maple API authentication revision exhausted".to_string())?;
            inner.revision
        };

        self.event_sink.auth_changed(&self.user_id, revision, true);
        Ok(())
    }

    pub(crate) async fn auth_snapshot(&self) -> Result<MapleApiAuthSnapshot, String> {
        let inner = self.inner.read().await;
        if !inner.active {
            return Err("Maple API authentication is no longer active".to_string());
        }
        snapshot_from_inner(&self.user_id, &self.native_instance_id, &inner)
    }

    pub(crate) async fn validate_user(&self) -> Result<(), String> {
        let snapshot = self.client_snapshot().await?;
        let response = snapshot.client.get_user().await;
        self.record_refresh(&snapshot).await?;
        let response = response.map_err(map_sdk_error)?;
        if response.user.id.to_string() != self.user_id {
            return Err("Maple API authentication belongs to a different account".to_string());
        }
        Ok(())
    }

    pub(crate) async fn model_ids(&self) -> Result<Vec<String>, String> {
        let snapshot = self.client_snapshot().await?;
        let response = snapshot.client.get_models().await;
        self.record_refresh(&snapshot).await?;
        let response = response.map_err(map_sdk_error)?;
        Ok(response.data.into_iter().map(|model| model.id).collect())
    }

    pub(crate) async fn send_inference_request(
        self: Arc<Self>,
        request: InferenceRequest,
        cancel_token: CancellationToken,
    ) -> Result<InferenceResponse, opensecret::Error> {
        let snapshot = self
            .client_snapshot()
            .await
            .map_err(opensecret::Error::Authentication)?;
        let operation_cancel = cancel_token.child_token();
        let _cancel_on_drop = CancelOperationOnDrop(operation_cancel.clone());
        let session = Arc::clone(&self);
        let operation = tokio::spawn(async move {
            let response = tokio::select! {
                biased;
                _ = operation_cancel.cancelled() => {
                    Err(opensecret::Error::Other("Inference request was cancelled".to_string()))
                }
                response = snapshot.client.send_inference_request(request) => response,
            };
            if let Err(error) = session.record_refresh(&snapshot).await {
                log::warn!("Failed to reconcile refreshed Maple API credentials: {error}");
            }
            response
        });
        operation.await.map_err(map_operation_join_error)?
    }

    pub(crate) async fn web_search(
        self: Arc<Self>,
        request: WebSearchRequest,
        cancel_token: CancellationToken,
    ) -> Result<WebSearchResponse, opensecret::Error> {
        let snapshot = self
            .client_snapshot()
            .await
            .map_err(opensecret::Error::Authentication)?;
        let operation_cancel = cancel_token.child_token();
        let _cancel_on_drop = CancelOperationOnDrop(operation_cancel.clone());
        let session = Arc::clone(&self);
        let operation = tokio::spawn(async move {
            let response = tokio::select! {
                biased;
                _ = operation_cancel.cancelled() => {
                    Err(opensecret::Error::Other("Web search was cancelled".to_string()))
                }
                response = snapshot.client.web_search(request) => response,
            };
            if let Err(error) = session.record_refresh(&snapshot).await {
                log::warn!("Failed to reconcile refreshed Maple API credentials: {error}");
            }
            response
        });
        operation.await.map_err(map_operation_join_error)?
    }

    pub(crate) async fn web_extract(
        self: Arc<Self>,
        request: WebExtractRequest,
        cancel_token: CancellationToken,
    ) -> Result<WebExtractResponse, opensecret::Error> {
        let snapshot = self
            .client_snapshot()
            .await
            .map_err(opensecret::Error::Authentication)?;
        let operation_cancel = cancel_token.child_token();
        let _cancel_on_drop = CancelOperationOnDrop(operation_cancel.clone());
        let session = Arc::clone(&self);
        let operation = tokio::spawn(async move {
            let response = tokio::select! {
                biased;
                _ = operation_cancel.cancelled() => {
                    Err(opensecret::Error::Other("Web extraction was cancelled".to_string()))
                }
                response = snapshot.client.web_extract(request) => response,
            };
            if let Err(error) = session.record_refresh(&snapshot).await {
                log::warn!("Failed to reconcile refreshed Maple API credentials: {error}");
            }
            response
        });
        operation.await.map_err(map_operation_join_error)?
    }
}

#[cfg(test)]
struct TestMapleApiAuthEventSink;

#[cfg(test)]
impl MapleApiAuthEventSink for TestMapleApiAuthEventSink {
    fn auth_changed(&self, _user_id: &str, _revision: u64, _authenticated: bool) {}
}

#[cfg(test)]
pub(crate) fn test_maple_api_session(user_id: &str) -> Arc<MapleApiSession> {
    let user_id = normalized_user_id(user_id).unwrap();
    let account_scope = account_scope(&user_id).unwrap();
    let api_url = "http://127.0.0.1:1";
    let client = build_client(
        api_url,
        test_transport_v2_auth_bundle(api_url, &user_id, "test"),
    )
    .unwrap();
    Arc::new(
        MapleApiSession::new(
            Arc::new(TestMapleApiAuthEventSink),
            user_id,
            account_scope,
            "test-native-instance".to_string(),
            api_url.to_string(),
            client,
        )
        .unwrap(),
    )
}

#[cfg(test)]
fn test_transport_v2_descriptors(user_id: &str, label: &str) -> (String, String) {
    fn descriptor(audience: &str, kind: &str, subject: &str) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

        let claims = serde_json::json!({
            "iss": "urn:opensecret:transport-v2",
            "aud": audience,
            "tv": 2,
            "tk": kind,
            "pk": "user",
            "sub": subject,
            "exp": 2_000_000_000_u64,
        });
        format!(
            "e30.{}.test-signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    let subject = format!("{user_id}|{label}");
    (
        descriptor(
            "urn:opensecret:internal:transport-v2:user:access-descriptor",
            "access_descriptor",
            &subject,
        ),
        descriptor(
            "urn:opensecret:internal:transport-v2:user:resumption",
            "resumption",
            &subject,
        ),
    )
}

#[cfg(test)]
fn test_transport_v2_auth_bundle(api_url: &str, user_id: &str, label: &str) -> String {
    use base64::{
        engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
        Engine as _,
    };

    #[derive(Serialize)]
    struct TestAuthBundle<'a> {
        version: u8,
        api_origin: &'a str,
        access_token: &'a str,
        refresh_token: &'a str,
        cache_namespace_root_base64: String,
    }

    let (access_token, refresh_token) = test_transport_v2_descriptors(user_id, label);
    URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&TestAuthBundle {
            version: 2,
            api_origin: api_url,
            access_token: &access_token,
            refresh_token: &refresh_token,
            cache_namespace_root_base64: STANDARD.encode([0x42_u8; 32]),
        })
        .unwrap(),
    )
}

fn map_operation_join_error(error: tokio::task::JoinError) -> opensecret::Error {
    log::warn!("Maple API operation task failed: {error}");
    opensecret::Error::Other("Maple API operation failed".to_string())
}

#[async_trait::async_trait]
pub(crate) trait MapleWebTransport: Send + Sync {
    async fn web_search(
        self: Arc<Self>,
        request: WebSearchRequest,
        cancel_token: CancellationToken,
    ) -> opensecret::Result<WebSearchResponse>;

    async fn web_extract(
        self: Arc<Self>,
        request: WebExtractRequest,
        cancel_token: CancellationToken,
    ) -> opensecret::Result<WebExtractResponse>;
}

#[async_trait::async_trait]
impl MapleWebTransport for MapleApiSession {
    async fn web_search(
        self: Arc<Self>,
        request: WebSearchRequest,
        cancel_token: CancellationToken,
    ) -> opensecret::Result<WebSearchResponse> {
        MapleApiSession::web_search(self, request, cancel_token).await
    }

    async fn web_extract(
        self: Arc<Self>,
        request: WebExtractRequest,
        cancel_token: CancellationToken,
    ) -> opensecret::Result<WebExtractResponse> {
        MapleApiSession::web_extract(self, request, cancel_token).await
    }
}

#[async_trait::async_trait]
impl crate::agent::provider::MapleInferenceTransport for MapleApiSession {
    async fn send_inference_request(
        self: Arc<Self>,
        request: InferenceRequest,
        cancel_token: CancellationToken,
    ) -> opensecret::Result<InferenceResponse> {
        MapleApiSession::send_inference_request(self, request, cancel_token).await
    }
}

fn snapshot_from_inner(
    user_id: &str,
    native_instance_id: &str,
    inner: &MapleApiSessionInner,
) -> Result<MapleApiAuthSnapshot, String> {
    let auth_bundle = capture_auth_bundle(&inner.credentials.client)?;
    Ok(MapleApiAuthSnapshot {
        user_id: user_id.to_string(),
        auth_bundle,
        native_instance_id: native_instance_id.to_string(),
        revision: inner.revision,
    })
}

fn capture_auth_bundle(client: &OpenSecretClient) -> Result<String, String> {
    client
        .export_transport_v2_auth_bundle()
        .map_err(map_sdk_error)?
        .filter(|bundle| !bundle.trim().is_empty())
        .ok_or_else(|| "Maple API transport v2 authentication is missing".to_string())
}

fn map_sdk_error(error: opensecret::Error) -> String {
    log::warn!(
        "OpenSecret SDK authentication operation failed ({})",
        crate::agent::provider::opensecret_error_category(&error)
    );
    "Maple API authentication failed".to_string()
}

pub(crate) fn account_scope(user_id: &str) -> Result<String, String> {
    let user_id = normalized_user_id(user_id)?;
    let digest = Sha256::digest(user_id.as_bytes());
    Ok(format!("{digest:x}"))
}

fn normalized_user_id(user_id: &str) -> Result<String, String> {
    let user_id = user_id.trim().to_ascii_lowercase();
    if user_id.is_empty() {
        return Err("Maple API access requires a signed-in account".to_string());
    }
    Ok(user_id)
}

fn build_client(api_url: &str, auth_bundle: String) -> Result<Arc<OpenSecretClient>, String> {
    if auth_bundle.trim().is_empty() {
        return Err("Maple API transport v2 authentication is missing".to_string());
    }
    let client = OpenSecretClient::new_with_pcr0_environment(
        api_url.to_string(),
        configured_pcr0_environment()?,
    )
    .map_err(map_sdk_error)?;
    client
        .import_transport_v2_auth_bundle(&auth_bundle)
        .map_err(map_sdk_error)?;
    Ok(Arc::new(client))
}

pub struct MapleApiAuthState {
    inner: Mutex<Option<Arc<MapleApiSession>>>,
    mutation: Mutex<()>,
    native_instance_id: String,
    credential_validator: Arc<dyn MapleApiCredentialValidator>,
}

#[async_trait::async_trait]
trait MapleApiCredentialValidator: Send + Sync {
    async fn validate(
        &self,
        client: &OpenSecretClient,
        expected_user_id: &str,
    ) -> Result<(), String>;
}

struct BackendCredentialValidator;

#[async_trait::async_trait]
impl MapleApiCredentialValidator for BackendCredentialValidator {
    async fn validate(
        &self,
        client: &OpenSecretClient,
        expected_user_id: &str,
    ) -> Result<(), String> {
        let response = client.get_user().await.map_err(map_sdk_error)?;
        let actual_user_id = normalized_user_id(&response.user.id.to_string())?;
        if actual_user_id != expected_user_id {
            return Err("Maple API authentication belongs to a different account".to_string());
        }
        Ok(())
    }
}

fn new_native_instance_id() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

impl MapleApiAuthState {
    pub fn new() -> Self {
        Self::with_validator(Arc::new(BackendCredentialValidator))
    }

    fn with_validator(credential_validator: Arc<dyn MapleApiCredentialValidator>) -> Self {
        Self {
            inner: Mutex::new(None),
            mutation: Mutex::new(()),
            native_instance_id: new_native_instance_id(),
            credential_validator,
        }
    }

    async fn set_auth(
        &self,
        app_handle: AppHandle,
        request: MapleApiAuthRequest,
    ) -> Result<MapleApiAuthSnapshot, String> {
        self.set_auth_with_sink(Arc::new(TauriAuthEventSink(app_handle)), request)
            .await
    }

    async fn set_auth_with_sink(
        &self,
        event_sink: Arc<dyn MapleApiAuthEventSink>,
        request: MapleApiAuthRequest,
    ) -> Result<MapleApiAuthSnapshot, String> {
        // Keep set/clear ordering intact across the candidate-validation await.
        // Agent calls can continue using the prior client until validation
        // succeeds and the replacement is published atomically.
        let _mutation = self.mutation.lock().await;
        let user_id = normalized_user_id(&request.user_id)?;
        let requested_scope = account_scope(&user_id)?;
        let api_url = normalize_api_url(&request.api_url)?;
        let client = build_client(&api_url, request.auth_bundle)?;
        tokio::time::timeout(
            CREDENTIAL_VALIDATION_TIMEOUT,
            self.credential_validator.validate(&client, &user_id),
        )
        .await
        .map_err(|_| "Maple API authentication validation timed out".to_string())??;

        let mut current = self.inner.lock().await;
        if let Some(session) = current.as_ref() {
            if session.account_scope() != requested_scope {
                return Err(
                    "Maple API authentication belongs to a different signed-in account".to_string(),
                );
            }
            return session.replace_client(api_url, client).await;
        }

        let session = Arc::new(MapleApiSession::new(
            event_sink,
            user_id,
            requested_scope,
            self.native_instance_id.clone(),
            api_url,
            client,
        )?);
        let snapshot = session.auth_snapshot().await?;
        *current = Some(session);
        Ok(snapshot)
    }

    pub(crate) async fn session_for(&self, user_id: &str) -> Result<Arc<MapleApiSession>, String> {
        let requested_scope = account_scope(user_id)?;
        let current = self.inner.lock().await;
        let session = current
            .as_ref()
            .ok_or_else(|| "Maple API authentication is not initialized".to_string())?;
        if session.account_scope() != requested_scope {
            return Err(
                "Maple API authentication belongs to a different signed-in account".to_string(),
            );
        }
        Ok(Arc::clone(session))
    }

    async fn clear_auth(&self, user_id: &str) -> Result<(), String> {
        let _mutation = self.mutation.lock().await;
        let requested_scope = account_scope(user_id)?;
        let session = {
            let mut current = self.inner.lock().await;
            let Some(session) = current.as_ref() else {
                return Ok(());
            };
            if session.account_scope() != requested_scope {
                return Err(
                    "Maple API authentication belongs to a different signed-in account".to_string(),
                );
            }
            current.take().expect("Maple API session disappeared")
        };
        session.invalidate().await;
        Ok(())
    }
}

#[tauri::command]
pub async fn maple_api_set_auth(
    app_handle: AppHandle,
    state: State<'_, MapleApiAuthState>,
    request: MapleApiAuthRequest,
) -> Result<MapleApiAuthSnapshot, String> {
    state.set_auth(app_handle, request).await
}

#[tauri::command]
pub async fn maple_api_get_auth(
    state: State<'_, MapleApiAuthState>,
    user_id: String,
) -> Result<MapleApiAuthSnapshot, String> {
    state.session_for(&user_id).await?.auth_snapshot().await
}

#[tauri::command]
pub async fn maple_api_clear_auth(
    state: State<'_, MapleApiAuthState>,
    user_id: String,
) -> Result<(), String> {
    state.clear_auth(&user_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use std::sync::Mutex as StdMutex;
    use tokio::sync::Notify;

    #[derive(Default)]
    struct RecordingEventSink {
        events: StdMutex<Vec<(String, u64, bool)>>,
    }

    impl MapleApiAuthEventSink for RecordingEventSink {
        fn auth_changed(&self, user_id: &str, revision: u64, authenticated: bool) {
            self.events.lock().expect("event lock").push((
                user_id.to_string(),
                revision,
                authenticated,
            ));
        }
    }

    struct DescriptorCredentialValidator;

    #[async_trait::async_trait]
    impl MapleApiCredentialValidator for DescriptorCredentialValidator {
        async fn validate(
            &self,
            client: &OpenSecretClient,
            expected_user_id: &str,
        ) -> Result<(), String> {
            let tokens = client
                .get_tokens()
                .map_err(map_sdk_error)?
                .ok_or_else(|| "test credential is missing".to_string())?;
            let payload = tokens
                .access_token
                .split('.')
                .nth(1)
                .ok_or_else(|| "test descriptor payload is missing".to_string())?;
            let claims: serde_json::Value = serde_json::from_slice(
                &URL_SAFE_NO_PAD
                    .decode(payload)
                    .map_err(|_| "test descriptor payload is invalid".to_string())?,
            )
            .map_err(|_| "test descriptor claims are invalid".to_string())?;
            let subject = claims
                .get("sub")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "test descriptor subject is missing".to_string())?;
            let (actual_user_id, label) = subject
                .split_once('|')
                .ok_or_else(|| "test credential is missing its account prefix".to_string())?;
            if actual_user_id != expected_user_id {
                return Err("Maple API authentication belongs to a different account".to_string());
            }
            if label == "refresh-during-validation" {
                let (access, refresh) =
                    test_transport_v2_descriptors(expected_user_id, "validated");
                client
                    .set_tokens(access, Some(refresh))
                    .map_err(map_sdk_error)?;
            }
            Ok(())
        }
    }

    struct BlockingCredentialValidator {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl MapleApiCredentialValidator for BlockingCredentialValidator {
        async fn validate(
            &self,
            client: &OpenSecretClient,
            expected_user_id: &str,
        ) -> Result<(), String> {
            self.entered.notify_one();
            self.release.notified().await;
            DescriptorCredentialValidator
                .validate(client, expected_user_id)
                .await
        }
    }

    fn test_state() -> MapleApiAuthState {
        MapleApiAuthState::with_validator(Arc::new(DescriptorCredentialValidator))
    }

    fn auth_request(user_id: &str, label: &str) -> MapleApiAuthRequest {
        let api_url = "https://enclave.trymaple.ai";
        MapleApiAuthRequest {
            user_id: user_id.to_string(),
            api_url: api_url.to_string(),
            auth_bundle: test_transport_v2_auth_bundle(api_url, user_id, label),
        }
    }

    #[test]
    fn api_url_requires_https_or_loopback_http() {
        assert_eq!(
            normalize_api_url("https://enclave.trymaple.ai/").unwrap(),
            "https://enclave.trymaple.ai"
        );
        assert_eq!(
            normalize_api_url("http://127.0.0.1:31745").unwrap(),
            "http://127.0.0.1:31745"
        );
        assert_eq!(
            normalize_api_url("http://localhost:31745/").unwrap(),
            "http://localhost:31745"
        );
        assert!(normalize_api_url("http://enclave.trymaple.ai").is_err());
        assert!(normalize_api_url("https://user:pass@example.com").is_err());
        assert!(normalize_api_url("https://example.com/v1").is_err());
    }

    #[test]
    fn account_scopes_are_normalized_opaque_and_isolated() {
        assert_eq!(
            account_scope(" USER-A ").unwrap(),
            account_scope("user-a").unwrap()
        );
        assert_ne!(
            account_scope("user-a").unwrap(),
            account_scope("user-b").unwrap()
        );
        assert!(!account_scope("user-a").unwrap().contains("user-a"));
        assert!(account_scope("  ").is_err());
    }

    #[tokio::test]
    async fn same_account_updates_are_atomic_and_clear_invalidates_retained_handles() {
        let state = test_state();
        let sink = Arc::new(RecordingEventSink::default());

        let first = state
            .set_auth_with_sink(sink.clone(), auth_request("user-a", "access-one"))
            .await
            .unwrap();
        assert_eq!(first.revision, 1);
        let retained = state.session_for("user-a").await.unwrap();

        let unchanged = state
            .set_auth_with_sink(sink.clone(), auth_request("user-a", "access-one"))
            .await
            .unwrap();
        assert_eq!(unchanged.revision, 1);

        let replaced = state
            .set_auth_with_sink(sink.clone(), auth_request("user-a", "access-two"))
            .await
            .unwrap();
        assert_eq!(replaced.revision, 2);
        assert_eq!(
            replaced.auth_bundle,
            auth_request("user-a", "access-two").auth_bundle
        );
        assert!(state
            .set_auth_with_sink(sink.clone(), auth_request("user-b", "other"))
            .await
            .is_err());

        state.clear_auth("user-a").await.unwrap();
        assert!(retained.auth_snapshot().await.is_err());
        assert!(state.session_for("user-a").await.is_err());

        let next_account = state
            .set_auth_with_sink(sink, auth_request("user-b", "other"))
            .await
            .unwrap();
        assert_eq!(next_account.user_id, "user-b");
        assert_eq!(next_account.revision, 1);
    }

    #[tokio::test]
    async fn late_old_generation_refresh_cannot_publish_or_replace_new_credentials() {
        let state = test_state();
        let sink = Arc::new(RecordingEventSink::default());
        state
            .set_auth_with_sink(sink.clone(), auth_request("user-a", "access-one"))
            .await
            .unwrap();
        let session = state.session_for("user-a").await.unwrap();
        let old_snapshot = session.client_snapshot().await.unwrap();

        state
            .set_auth_with_sink(sink.clone(), auth_request("user-a", "access-two"))
            .await
            .unwrap();
        let (late_access, late_refresh) = test_transport_v2_descriptors("user-a", "late");
        old_snapshot
            .client
            .set_tokens(late_access, Some(late_refresh))
            .unwrap();
        session.record_refresh(&old_snapshot).await.unwrap();

        let current = session.auth_snapshot().await.unwrap();
        assert_eq!(current.revision, 2);
        assert_eq!(
            current.auth_bundle,
            auth_request("user-a", "access-two").auth_bundle
        );
        assert!(sink.events.lock().expect("event lock").is_empty());
    }

    #[tokio::test]
    async fn cancelled_web_calls_stop_inside_account_scoped_transport() {
        let state = test_state();
        let sink = Arc::new(RecordingEventSink::default());
        state
            .set_auth_with_sink(sink, auth_request("user-a", "access-one"))
            .await
            .unwrap();
        let session = state.session_for("user-a").await.unwrap();
        let before = session.auth_snapshot().await.unwrap();

        let search_cancel = CancellationToken::new();
        search_cancel.cancel();
        let search = Arc::clone(&session)
            .web_search(WebSearchRequest::new("maple privacy"), search_cancel)
            .await;
        assert!(
            matches!(search, Err(opensecret::Error::Other(message)) if message.contains("cancelled"))
        );

        let extract_cancel = CancellationToken::new();
        extract_cancel.cancel();
        let extract = Arc::clone(&session)
            .web_extract(
                WebExtractRequest::new(["https://example.com"]),
                extract_cancel,
            )
            .await;
        assert!(
            matches!(extract, Err(opensecret::Error::Other(message)) if message.contains("cancelled"))
        );

        let after = session.auth_snapshot().await.unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.auth_bundle, before.auth_bundle);
    }

    #[tokio::test]
    async fn current_generation_refresh_publishes_one_opaque_bundle_revision() {
        let state = test_state();
        let sink = Arc::new(RecordingEventSink::default());
        state
            .set_auth_with_sink(sink.clone(), auth_request("user-a", "access-one"))
            .await
            .unwrap();
        let session = state.session_for("user-a").await.unwrap();
        let snapshot = session.client_snapshot().await.unwrap();
        let (access, refresh) = test_transport_v2_descriptors("user-a", "resumed");
        snapshot.client.set_tokens(access, Some(refresh)).unwrap();

        session.record_refresh(&snapshot).await.unwrap();

        let current = session.auth_snapshot().await.unwrap();
        assert_eq!(current.revision, 2);
        assert_eq!(
            current.auth_bundle,
            test_transport_v2_auth_bundle("https://enclave.trymaple.ai", "user-a", "resumed")
        );
        assert_eq!(
            sink.events.lock().expect("event lock").as_slice(),
            &[("user-a".to_string(), 2, true)]
        );
    }

    #[tokio::test]
    async fn current_generation_bundle_loss_invalidates_and_notifies_once() {
        let state = test_state();
        let sink = Arc::new(RecordingEventSink::default());
        state
            .set_auth_with_sink(sink.clone(), auth_request("user-a", "access-one"))
            .await
            .unwrap();
        let session = state.session_for("user-a").await.unwrap();
        let snapshot = session.client_snapshot().await.unwrap();

        let error = session
            .record_auth_bundle_change(&snapshot, Err("credentials rejected".to_string()))
            .await
            .expect_err("bundle loss must invalidate the matching native generation");

        assert_eq!(error, "credentials rejected");
        assert!(session.auth_snapshot().await.is_err());
        assert_eq!(
            sink.events.lock().expect("event lock").as_slice(),
            &[("user-a".to_string(), 2, false)]
        );
        session
            .record_auth_bundle_change(&snapshot, Err("late duplicate".to_string()))
            .await
            .unwrap();
        assert_eq!(sink.events.lock().expect("event lock").len(), 1);
    }

    #[tokio::test]
    async fn candidate_identity_is_verified_before_replacing_live_credentials() {
        let state = test_state();
        let sink = Arc::new(RecordingEventSink::default());
        state
            .set_auth_with_sink(sink.clone(), auth_request("user-a", "access-one"))
            .await
            .unwrap();

        let mut wrong_account = auth_request("user-a", "access-two");
        wrong_account.auth_bundle =
            test_transport_v2_auth_bundle("https://enclave.trymaple.ai", "user-b", "access-two");
        let error = match state.set_auth_with_sink(sink, wrong_account).await {
            Ok(_) => panic!("cross-account replacement must be rejected"),
            Err(error) => error,
        };

        assert!(error.contains("different account"));
        let current = state
            .session_for("user-a")
            .await
            .unwrap()
            .auth_snapshot()
            .await
            .unwrap();
        assert_eq!(
            current.auth_bundle,
            auth_request("user-a", "access-one").auth_bundle
        );
        assert_eq!(current.revision, 1);
    }

    #[tokio::test]
    async fn validation_descriptor_rotation_is_returned_to_the_browser_handshake() {
        let state = test_state();
        let snapshot = state
            .set_auth_with_sink(
                Arc::new(RecordingEventSink::default()),
                auth_request("user-a", "refresh-during-validation"),
            )
            .await
            .unwrap();

        assert_eq!(
            snapshot.auth_bundle,
            test_transport_v2_auth_bundle("https://enclave.trymaple.ai", "user-a", "validated")
        );
        assert!(!snapshot.native_instance_id.is_empty());
    }

    #[tokio::test]
    async fn clear_ordering_cannot_be_overtaken_by_in_flight_validation() {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let state = Arc::new(MapleApiAuthState::with_validator(Arc::new(
            BlockingCredentialValidator {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
            },
        )));
        let setter_state = Arc::clone(&state);
        let setter = tokio::spawn(async move {
            setter_state
                .set_auth_with_sink(
                    Arc::new(RecordingEventSink::default()),
                    auth_request("user-a", "access-one"),
                )
                .await
        });

        entered.notified().await;
        let clearer_state = Arc::clone(&state);
        let clearer = tokio::spawn(async move { clearer_state.clear_auth("user-a").await });
        release.notify_one();

        setter.await.unwrap().unwrap();
        clearer.await.unwrap().unwrap();
        assert!(state.session_for("user-a").await.is_err());
    }
}
