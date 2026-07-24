use opensecret::{
    InferenceRequest, InferenceResponse, OpenSecretClient, WebExtractRequest, WebExtractResponse,
    WebSearchRequest, WebSearchResponse,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

const AUTH_CHANGED_EVENT: &str = "maple-api-auth-changed";
const CREDENTIAL_VALIDATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapleApiAuthRequest {
    pub user_id: String,
    pub api_url: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapleApiAuthSnapshot {
    pub user_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub native_instance_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MapleApiAuthChanged {
    user_id: String,
    revision: u64,
}

trait MapleApiAuthEventSink: Send + Sync {
    fn auth_changed(&self, user_id: &str, revision: u64);
}

struct TauriAuthEventSink(AppHandle);

impl MapleApiAuthEventSink for TauriAuthEventSink {
    fn auth_changed(&self, user_id: &str, revision: u64) {
        if let Err(error) = self.0.emit(
            AUTH_CHANGED_EVENT,
            MapleApiAuthChanged {
                user_id: user_id.to_string(),
                revision,
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

struct ClientSnapshot {
    generation: u64,
    client: Arc<OpenSecretClient>,
    tokens_before: TokenPair,
}

struct CancelOperationOnDrop(CancellationToken);

impl Drop for CancelOperationOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[derive(Clone, PartialEq, Eq)]
struct TokenPair {
    access_token: String,
    refresh_token: Option<String>,
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
        capture_tokens(&client)?;
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
        let replacement_tokens = capture_tokens(&client)?;
        let mut inner = self.inner.write().await;
        if !inner.active {
            return Err("Maple API authentication is no longer active".to_string());
        }

        let current_tokens = capture_tokens(&inner.credentials.client)?;
        if inner.credentials.api_url == api_url && current_tokens == replacement_tokens {
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

    async fn client_snapshot(&self) -> Result<ClientSnapshot, String> {
        let inner = self.inner.read().await;
        if !inner.active {
            return Err("Maple API authentication is no longer active".to_string());
        }
        let client = Arc::clone(&inner.credentials.client);
        Ok(ClientSnapshot {
            generation: inner.credentials.generation,
            tokens_before: capture_tokens(&client)?,
            client,
        })
    }

    async fn record_refresh(&self, snapshot: &ClientSnapshot) -> Result<(), String> {
        let tokens_after = capture_tokens(&snapshot.client)?;
        if tokens_after == snapshot.tokens_before {
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

        self.event_sink.auth_changed(&self.user_id, revision);
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
    let tokens = capture_tokens(&inner.credentials.client)?;
    Ok(MapleApiAuthSnapshot {
        user_id: user_id.to_string(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        native_instance_id: native_instance_id.to_string(),
        revision: inner.revision,
    })
}

fn capture_tokens(client: &OpenSecretClient) -> Result<TokenPair, String> {
    let tokens = client
        .get_tokens()
        .map_err(map_sdk_error)?
        .ok_or_else(|| "Maple API access token is missing".to_string())?;
    let access_token = (!tokens.access_token.trim().is_empty())
        .then_some(tokens.access_token)
        .ok_or_else(|| "Maple API access token is missing".to_string())?;
    let refresh_token = tokens
        .refresh_token
        .filter(|token| !token.trim().is_empty());
    Ok(TokenPair {
        access_token,
        refresh_token,
    })
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

fn normalize_api_url(api_url: &str) -> Result<String, String> {
    let mut url =
        reqwest::Url::parse(api_url.trim()).map_err(|_| "Maple API URL is invalid".to_string())?;
    let host = url
        .host_str()
        .ok_or_else(|| "Maple API URL must include a host".to_string())?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("Maple API URL must use HTTPS or a loopback development address".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Maple API URL must not contain credentials".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("Maple API URL must not contain a query or fragment".to_string());
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err("Maple API URL must not contain a path".to_string());
    }
    url.set_path("");
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn build_client(
    api_url: &str,
    access_token: String,
    refresh_token: Option<String>,
) -> Result<Arc<OpenSecretClient>, String> {
    if access_token.trim().is_empty() {
        return Err("Maple API access token is missing".to_string());
    }
    let refresh_token = refresh_token.filter(|token| !token.trim().is_empty());
    let client = OpenSecretClient::new(api_url.to_string()).map_err(map_sdk_error)?;
    client
        .set_tokens(access_token, refresh_token)
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
        let client = build_client(&api_url, request.access_token, request.refresh_token)?;
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
    use axum::{
        extract::{Path, State},
        http::{header::AUTHORIZATION, HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{get, post},
        Json, Router,
    };
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use ciborium::value::Value as CborValue;
    use goose_providers::{base::Provider, conversation::message::Message, model::ModelConfig};
    use opensecret::types::KeyExchangeRequest;
    use std::sync::Mutex as StdMutex;
    use tokio::sync::Notify;

    #[derive(Default)]
    struct RecordingEventSink {
        events: StdMutex<Vec<(String, u64)>>,
    }

    impl MapleApiAuthEventSink for RecordingEventSink {
        fn auth_changed(&self, user_id: &str, revision: u64) {
            self.events
                .lock()
                .expect("event lock")
                .push((user_id.to_string(), revision));
        }
    }

    struct TokenPrefixCredentialValidator;

    #[async_trait::async_trait]
    impl MapleApiCredentialValidator for TokenPrefixCredentialValidator {
        async fn validate(
            &self,
            client: &OpenSecretClient,
            expected_user_id: &str,
        ) -> Result<(), String> {
            let tokens = capture_tokens(client)?;
            let actual_user_id = tokens
                .access_token
                .split_once('|')
                .map(|(user_id, _)| user_id)
                .ok_or_else(|| "test credential is missing its account prefix".to_string())?;
            if actual_user_id != expected_user_id {
                return Err("Maple API authentication belongs to a different account".to_string());
            }
            if tokens.access_token.ends_with("refresh-during-validation") {
                client
                    .set_tokens(
                        format!("{expected_user_id}|validated-access"),
                        Some(format!("{expected_user_id}|validated-refresh")),
                    )
                    .map_err(map_sdk_error)?;
            }
            Ok(())
        }
    }

    struct BlockingCredentialValidator {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Clone)]
    struct RefreshThenStallState {
        key_pair: Arc<opensecret::crypto::KeyPair>,
        session_key: [u8; 32],
        session_id: String,
        retry_started: Arc<Notify>,
    }

    struct RefreshThenStallFixture {
        session: Arc<MapleApiSession>,
        sink: Arc<RecordingEventSink>,
        retry_started: Arc<Notify>,
        server: tokio::task::JoinHandle<()>,
    }

    fn mock_attestation_document(nonce: &str, server_public_key: &[u8; 32]) -> String {
        let payload = CborValue::Map(vec![
            (
                CborValue::Text("public_key".to_string()),
                CborValue::Bytes(server_public_key.to_vec()),
            ),
            (
                CborValue::Text("nonce".to_string()),
                CborValue::Bytes(nonce.as_bytes().to_vec()),
            ),
        ]);
        let mut payload_bytes = Vec::new();
        ciborium::ser::into_writer(&payload, &mut payload_bytes).unwrap();
        let cose_sign1 = CborValue::Array(vec![
            CborValue::Bytes(Vec::new()),
            CborValue::Map(Vec::new()),
            CborValue::Bytes(payload_bytes),
            CborValue::Bytes(Vec::new()),
        ]);
        let mut cose_bytes = Vec::new();
        ciborium::ser::into_writer(&cose_sign1, &mut cose_bytes).unwrap();
        BASE64.encode(cose_bytes)
    }

    async fn attestation_handler(
        State(state): State<RefreshThenStallState>,
        Path(nonce): Path<String>,
    ) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "attestation_document": mock_attestation_document(
                &nonce,
                state.key_pair.public.as_bytes(),
            )
        }))
    }

    async fn key_exchange_handler(
        State(state): State<RefreshThenStallState>,
        Json(request): Json<KeyExchangeRequest>,
    ) -> Json<serde_json::Value> {
        let client_public_bytes = BASE64.decode(request.client_public_key).unwrap();
        let client_public_key = opensecret::crypto::PublicKey::from(
            <[u8; 32]>::try_from(client_public_bytes.as_slice()).unwrap(),
        );
        let shared_secret =
            opensecret::crypto::derive_shared_secret(&state.key_pair.secret, &client_public_key);
        let encrypted_session_key = BASE64.encode(
            opensecret::crypto::encrypt_data(shared_secret.as_bytes(), &state.session_key).unwrap(),
        );
        Json(serde_json::json!({
            "encrypted_session_key": encrypted_session_key,
            "session_id": state.session_id,
        }))
    }

    async fn refresh_handler(
        State(state): State<RefreshThenStallState>,
    ) -> Json<serde_json::Value> {
        let plaintext = serde_json::to_vec(&serde_json::json!({
            "access_token": "fresh_access",
            "refresh_token": "fresh_refresh",
        }))
        .unwrap();
        let encrypted = opensecret::crypto::encrypt_data(&state.session_key, &plaintext).unwrap();
        Json(serde_json::json!({ "encrypted": BASE64.encode(encrypted) }))
    }

    async fn refresh_then_stall_handler(
        State(state): State<RefreshThenStallState>,
        headers: HeaderMap,
    ) -> Response {
        match headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
        {
            Some("Bearer expired_access") => {
                (StatusCode::UNAUTHORIZED, "expired access token").into_response()
            }
            Some("Bearer fresh_access") => {
                state.retry_started.notify_one();
                futures_util::future::pending::<Response>().await
            }
            _ => (StatusCode::FORBIDDEN, "unexpected credential").into_response(),
        }
    }

    async fn refresh_then_stall_fixture() -> RefreshThenStallFixture {
        let key_pair = Arc::new(opensecret::crypto::generate_key_pair());
        let retry_started = Arc::new(Notify::new());
        let state = RefreshThenStallState {
            key_pair,
            session_key: [41; 32],
            session_id: "00000000-0000-0000-0000-000000000041".to_string(),
            retry_started: Arc::clone(&retry_started),
        };
        let app = Router::new()
            .route("/attestation/{nonce}", get(attestation_handler))
            .route("/key_exchange", post(key_exchange_handler))
            .route("/refresh", post(refresh_handler))
            .route("/v1/chat/completions", post(refresh_then_stall_handler))
            .route("/v1/web/search", post(refresh_then_stall_handler))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = Arc::new(OpenSecretClient::new(api_url.clone()).unwrap());
        client
            .set_tokens(
                "expired_access".to_string(),
                Some("old_refresh".to_string()),
            )
            .unwrap();
        client.perform_attestation_handshake().await.unwrap();
        let sink = Arc::new(RecordingEventSink::default());
        let session = Arc::new(
            MapleApiSession::new(
                sink.clone(),
                "user-a".to_string(),
                account_scope("user-a").unwrap(),
                "native-test-instance".to_string(),
                api_url,
                client,
            )
            .unwrap(),
        );
        RefreshThenStallFixture {
            session,
            sink,
            retry_started,
            server,
        }
    }

    async fn assert_refresh_reconciled(fixture: &RefreshThenStallFixture) {
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let snapshot = fixture.session.auth_snapshot().await.unwrap();
                if snapshot.revision == 2 {
                    break snapshot;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("refreshed credentials should be reconciled");
        assert_eq!(snapshot.access_token, "fresh_access");
        assert_eq!(snapshot.refresh_token.as_deref(), Some("fresh_refresh"));
        assert_eq!(
            fixture.sink.events.lock().expect("event lock").as_slice(),
            &[("user-a".to_string(), 2)]
        );
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
            TokenPrefixCredentialValidator
                .validate(client, expected_user_id)
                .await
        }
    }

    fn test_state() -> MapleApiAuthState {
        MapleApiAuthState::with_validator(Arc::new(TokenPrefixCredentialValidator))
    }

    fn auth_request(user_id: &str, access_token: &str) -> MapleApiAuthRequest {
        MapleApiAuthRequest {
            user_id: user_id.to_string(),
            api_url: "https://enclave.trymaple.ai".to_string(),
            access_token: format!("{user_id}|{access_token}"),
            refresh_token: Some(format!("{user_id}|refresh-{access_token}")),
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
        assert_eq!(replaced.access_token, "user-a|access-two");
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
        old_snapshot
            .client
            .set_tokens("late-access".to_string(), Some("late-refresh".to_string()))
            .unwrap();
        session.record_refresh(&old_snapshot).await.unwrap();

        let current = session.auth_snapshot().await.unwrap();
        assert_eq!(current.revision, 2);
        assert_eq!(current.access_token, "user-a|access-two");
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
        assert_eq!(after.access_token, before.access_token);
        assert_eq!(after.refresh_token, before.refresh_token);
    }

    #[tokio::test]
    async fn provider_cancellation_after_sdk_refresh_reconciles_rotated_credentials() {
        let fixture = refresh_then_stall_fixture().await;
        let provider = crate::agent::provider::MapleProvider::new(Arc::clone(&fixture.session));
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let request = tokio::spawn(async move {
            crate::agent::provider::with_run_cancellation(
                task_cancellation,
                provider.stream(
                    &ModelConfig::new("test-model"),
                    "system",
                    &[Message::user().with_text("classify this URL")],
                    &[],
                ),
            )
            .await
        });

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            fixture.retry_started.notified(),
        )
        .await
        .expect("refreshed inference retry should start");
        cancellation.cancel();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), request)
            .await
            .expect("provider cancellation should finish")
            .unwrap();
        assert!(matches!(
            result,
            Err(goose_providers::errors::ProviderError::ExecutionError(message))
                if message.contains("cancelled")
        ));
        assert_refresh_reconciled(&fixture).await;
        fixture.server.abort();
    }

    #[tokio::test]
    async fn dropped_web_call_after_sdk_refresh_still_reconciles_rotated_credentials() {
        let fixture = refresh_then_stall_fixture().await;
        let session = Arc::clone(&fixture.session);
        let request = tokio::spawn(async move {
            session
                .web_search(
                    WebSearchRequest::new("maple privacy"),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            fixture.retry_started.notified(),
        )
        .await
        .expect("refreshed web retry should start");
        request.abort();
        let _ = request.await;
        assert_refresh_reconciled(&fixture).await;
        fixture.server.abort();
    }

    #[tokio::test]
    async fn dropped_classifier_provider_future_after_refresh_still_reconciles_credentials() {
        let fixture = refresh_then_stall_fixture().await;
        let provider = crate::agent::provider::MapleProvider::new(Arc::clone(&fixture.session));
        let request = tokio::spawn(async move {
            provider
                .complete(
                    &ModelConfig::new("gemma4-31b"),
                    "classify web permission",
                    &[Message::user().with_text("untrusted classifier input")],
                    &[],
                )
                .await
        });

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            fixture.retry_started.notified(),
        )
        .await
        .expect("refreshed classifier retry should start");
        request.abort();
        let _ = request.await;
        assert_refresh_reconciled(&fixture).await;
        fixture.server.abort();
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
        wrong_account.access_token = "user-b|access-two".to_string();
        wrong_account.refresh_token = Some("user-b|refresh-access-two".to_string());
        let error = state
            .set_auth_with_sink(sink, wrong_account)
            .await
            .expect_err("cross-account replacement must be rejected");

        assert!(error.contains("different account"));
        let current = state
            .session_for("user-a")
            .await
            .unwrap()
            .auth_snapshot()
            .await
            .unwrap();
        assert_eq!(current.access_token, "user-a|access-one");
        assert_eq!(current.revision, 1);
    }

    #[tokio::test]
    async fn validation_token_rotation_is_returned_to_the_browser_handshake() {
        let state = test_state();
        let snapshot = state
            .set_auth_with_sink(
                Arc::new(RecordingEventSink::default()),
                auth_request("user-a", "refresh-during-validation"),
            )
            .await
            .unwrap();

        assert_eq!(snapshot.access_token, "user-a|validated-access");
        assert_eq!(
            snapshot.refresh_token.as_deref(),
            Some("user-a|validated-refresh")
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
