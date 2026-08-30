use crate::{
    error::{Error, Result},
    pcr::{Pcr0Environment, Pcr0TrustPolicy},
    session::{CredentialSnapshot, SessionManager, UserAuthEpoch},
    transport_v2::{
        decode_auth_bundle, encode_auth_bundle, validate_v2_user_token_pair, ApiKeyScope,
        CacheNamespaceRoot, Credential, HeaderField, LogicalMethod, LogicalRequest, ResponseMode,
        TransportV2Client, V2HttpResponse, V2Session, ValidatedUserTokenPair,
    },
    types::*,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use http::{header, HeaderMap as HttpHeaderMap, Request as HttpRequest, Response as HttpResponse};
use p256::elliptic_curve::rand_core::{OsRng, RngCore};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::{
    de::{self, DeserializeOwned, Deserializer as _, IgnoredAny, MapAccess, Visitor},
    Serialize,
};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    pin::Pin,
    sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;
use zeroize::Zeroizing;

/// A decrypted response body returned by [`OpenSecretClient::send_inference_request`].
///
/// Ordinary responses contain one chunk. Server-sent event responses remain a
/// stream, with each encrypted `data:` field decrypted without interpreting its
/// payload as JSON.
pub type OpenSecretResponseBody = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send + 'static>>;

/// A caller-owned request to an allowed OpenSecret inference endpoint.
pub type InferenceRequest = HttpRequest<Bytes>;

/// A decrypted HTTP response from an OpenSecret inference endpoint.
pub type InferenceResponse = HttpResponse<OpenSecretResponseBody>;

/// Caller-persistable provider-cache namespace root for transport v2.
///
/// Generate this independently from account identifiers and credentials. Its
/// debug representation is always redacted.
#[derive(Clone, PartialEq, Eq, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct TransportV2CacheNamespaceRoot([u8; 32]);

impl TransportV2CacheNamespaceRoot {
    pub fn generate() -> Result<Self> {
        Ok(Self(random_cache_namespace_root()?))
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_base64(encoded: &str) -> Result<Self> {
        let decoded = Zeroizing::new(BASE64.decode(encoded)?);
        if decoded.len() != 32 || BASE64.encode(decoded.as_slice()) != encoded {
            return Err(Error::Configuration(
                "Transport v2 cache namespace root must be canonical padded base64 for exactly 32 bytes"
                    .to_string(),
            ));
        }
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(decoded.as_slice());
        Ok(Self(bytes))
    }

    pub fn to_base64(&self) -> String {
        BASE64.encode(self.0)
    }

    fn into_bytes(mut self) -> [u8; 32] {
        let bytes = self.0;
        self.0 = [0_u8; 32];
        bytes
    }
}

impl std::fmt::Debug for TransportV2CacheNamespaceRoot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TransportV2CacheNamespaceRoot([REDACTED])")
    }
}

pub struct OpenSecretClient {
    session_manager: SessionManager,
    auth_commit_lock: StdMutex<()>,
    refresh_lock: Mutex<()>,
    transport_v2: TransportV2Client,
}

struct V2SendOptions {
    response_mode: ResponseMode,
    credential: Option<Credential>,
    cache_namespace_root: Option<CacheNamespaceRoot>,
}

const V2_USER_AUTH_RENEWAL_SKEW_SECONDS: u64 = 30;

impl V2SendOptions {
    const fn bound(response_mode: ResponseMode) -> Self {
        Self {
            response_mode,
            credential: None,
            cache_namespace_root: None,
        }
    }

    fn transition(
        response_mode: ResponseMode,
        credential: Option<Credential>,
        cache_namespace_root: CacheNamespaceRoot,
    ) -> Self {
        Self {
            response_mode,
            credential,
            cache_namespace_root: Some(cache_namespace_root),
        }
    }
}

fn append_query_param(query: &mut Vec<String>, key: &str, value: impl ToString) {
    let encoded = utf8_percent_encode(&value.to_string(), NON_ALPHANUMERIC).to_string();
    query.push(format!("{}={}", key, encoded));
}

fn build_agent_items_endpoint(base: &str, params: Option<&AgentItemsListParams>) -> String {
    let mut endpoint = base.to_string();
    let mut query = Vec::new();

    if let Some(params) = params {
        if let Some(limit) = params.limit {
            append_query_param(&mut query, "limit", limit);
        }
        if let Some(after) = params.after {
            append_query_param(&mut query, "after", after);
        }
        if let Some(order) = &params.order {
            append_query_param(&mut query, "order", order);
        }
        if let Some(include) = &params.include {
            for include_value in include {
                append_query_param(&mut query, "include", include_value);
            }
        }
    }

    if !query.is_empty() {
        endpoint.push('?');
        endpoint.push_str(&query.join("&"));
    }

    endpoint
}

fn build_subagents_endpoint(params: Option<&ListSubagentsParams>) -> String {
    let mut endpoint = "/v1/agent/subagents".to_string();
    let mut query = Vec::new();

    if let Some(params) = params {
        if let Some(limit) = params.limit {
            append_query_param(&mut query, "limit", limit);
        }
        if let Some(after) = params.after {
            append_query_param(&mut query, "after", after);
        }
        if let Some(order) = &params.order {
            append_query_param(&mut query, "order", order);
        }
        if let Some(created_by) = &params.created_by {
            append_query_param(&mut query, "created_by", created_by);
        }
    }

    if !query.is_empty() {
        endpoint.push('?');
        endpoint.push_str(&query.join("&"));
    }

    endpoint
}

fn build_conversations_endpoint(params: Option<&ConversationsListParams>) -> String {
    let mut endpoint = "/v1/conversations".to_string();
    let mut query = Vec::new();

    if let Some(params) = params {
        if let Some(limit) = params.limit {
            append_query_param(&mut query, "limit", limit);
        }
        if let Some(after) = params.after {
            append_query_param(&mut query, "after", after);
        }
        if let Some(order) = &params.order {
            append_query_param(&mut query, "order", order);
        }
        if let Some(project_id) = params.project_id {
            append_query_param(&mut query, "project_id", project_id);
        }
        if let Some(unassigned_project) = params.unassigned_project {
            append_query_param(&mut query, "unassigned_project", unassigned_project);
        }
        if let Some(pinned) = params.pinned {
            append_query_param(&mut query, "pinned", pinned);
        }
    }

    if !query.is_empty() {
        endpoint.push('?');
        endpoint.push_str(&query.join("&"));
    }

    endpoint
}

fn build_conversation_projects_endpoint(params: Option<&ConversationProjectListParams>) -> String {
    let mut endpoint = "/v1/conversation-projects".to_string();
    let mut query = Vec::new();

    if let Some(params) = params {
        if let Some(limit) = params.limit {
            append_query_param(&mut query, "limit", limit);
        }
        if let Some(after) = params.after {
            append_query_param(&mut query, "after", after);
        }
        if let Some(order) = &params.order {
            append_query_param(&mut query, "order", order);
        }
    }

    if !query.is_empty() {
        endpoint.push('?');
        endpoint.push_str(&query.join("&"));
    }

    endpoint
}

fn is_allowed_inference_endpoint(method: &http::Method, path: &str) -> bool {
    matches!(
        (method.as_str(), path),
        ("GET", "/v1/models")
            | ("GET", "/v1/models/catalog")
            | ("POST", "/v1/chat/completions")
            | ("POST", "/v1/embeddings")
            | ("POST", "/v1/audio/speech")
            | ("POST", "/v1/audio/transcriptions")
    )
}

fn inference_response_mode(path: &str, body: &[u8]) -> Result<ResponseMode> {
    if path != "/v1/chat/completions" {
        return Ok(ResponseMode::Unary);
    }

    struct StreamSelectionVisitor;
    impl<'de> Visitor<'de> for StreamSelectionVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a chat completion JSON object")
        }

        fn visit_map<A>(self, mut map: A) -> std::result::Result<bool, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut stream = None;
            while let Some(key) = map.next_key::<String>()? {
                if key == "stream" {
                    if stream.is_some() {
                        return Err(de::Error::duplicate_field("stream"));
                    }
                    stream = Some(map.next_value::<bool>()?);
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            Ok(stream.unwrap_or(false))
        }
    }

    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let Ok(stream) = deserializer.deserialize_map(StreamSelectionVisitor) else {
        return Ok(ResponseMode::Unary);
    };
    if deserializer.end().is_err() {
        return Ok(ResponseMode::Unary);
    }
    Ok(if stream {
        ResponseMode::Stream
    } else {
        ResponseMode::Unary
    })
}

fn is_hop_by_hop_header(name: &http::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn sanitize_inference_request_headers(headers: &HttpHeaderMap) -> HttpHeaderMap {
    let connection_headers = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| http::HeaderName::from_bytes(value.as_bytes()).ok())
        .collect::<Vec<_>>();

    headers
        .iter()
        .filter(|(name, _)| {
            !is_hop_by_hop_header(name)
                && !connection_headers.contains(name)
                && *name != header::HOST
                && *name != header::AUTHORIZATION
                && *name != header::COOKIE
                && *name != header::SET_COOKIE
                && name.as_str() != "x-session-id"
                && *name != header::CONTENT_LENGTH
                && *name != header::CONTENT_TYPE
                && *name != header::CONTENT_ENCODING
                && *name != header::ACCEPT_ENCODING
                && name.as_str() != "content-md5"
                && name.as_str() != "digest"
                && name.as_str() != "proxy-connection"
                && name.as_str() != "x-api-key"
                && name.as_str() != "api-key"
                && name.as_str() != "x-openai-api-key"
                && name.as_str() != "x-tinfoil-api-key"
                && name.as_str() != "x-goog-api-key"
                && name.as_str() != "x-anthropic-api-key"
                && name.as_str() != "openai-organization"
                && name.as_str() != "openai-project"
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

fn sanitize_inference_response_headers(headers: &HttpHeaderMap) -> HttpHeaderMap {
    let connection_headers = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| http::HeaderName::from_bytes(value.as_bytes()).ok())
        .collect::<Vec<_>>();

    headers
        .iter()
        .filter(|(name, _)| {
            !is_hop_by_hop_header(name)
                && !connection_headers.contains(name)
                && *name != header::CONTENT_LENGTH
                && *name != header::CONTENT_ENCODING
                && name.as_str() != "content-md5"
                && name.as_str() != "digest"
                && *name != header::ETAG
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

async fn collect_response_body(mut body: OpenSecretResponseBody) -> Result<Bytes> {
    let mut collected = BytesMut::new();
    while let Some(chunk) = body.next().await {
        collected.extend_from_slice(&chunk?);
    }
    Ok(collected.freeze())
}

fn random_cache_namespace_root() -> Result<[u8; 32]> {
    let mut root = [0_u8; 32];
    OsRng
        .try_fill_bytes(&mut root)
        .map_err(|_| Error::Encryption("Secure randomness was unavailable".to_string()))?;
    Ok(root)
}

impl OpenSecretClient {
    /// Construct a client using the official production PCR0 trust roots.
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        Self::new_with_pcr0_environment(base_url, Pcr0Environment::default())
    }

    /// Construct a client using one explicit official PCR0 environment.
    pub fn new_with_pcr0_environment(
        base_url: impl Into<String>,
        pcr0_environment: Pcr0Environment,
    ) -> Result<Self> {
        Self::new_with_pcr0_trust_policy(base_url, Pcr0TrustPolicy::official_for(pcr0_environment))
    }

    /// Construct a client with an explicit PCR0 trust policy.
    pub fn new_with_pcr0_trust_policy(
        base_url: impl Into<String>,
        pcr0_trust_policy: Pcr0TrustPolicy,
    ) -> Result<Self> {
        let base_url = base_url.into();
        let session_manager = SessionManager::new();
        let transport_v2 = TransportV2Client::new(
            base_url,
            pcr0_trust_policy,
            random_cache_namespace_root()?,
            session_manager.clone(),
        )?;

        Ok(Self {
            session_manager,
            auth_commit_lock: StdMutex::new(()),
            refresh_lock: Mutex::new(()),
            transport_v2,
        })
    }

    /// Construct an API-key client using the official production PCR0 trust roots.
    pub fn new_with_api_key(base_url: impl Into<String>, api_key: String) -> Result<Self> {
        Self::new_with_api_key_and_pcr0_environment(base_url, api_key, Pcr0Environment::default())
    }

    /// Construct an API-key client using one explicit official PCR0 environment.
    pub fn new_with_api_key_and_pcr0_environment(
        base_url: impl Into<String>,
        api_key: String,
        pcr0_environment: Pcr0Environment,
    ) -> Result<Self> {
        Self::new_with_api_key_and_pcr0_trust_policy(
            base_url,
            api_key,
            Pcr0TrustPolicy::official_for(pcr0_environment),
        )
    }

    /// Construct an API-key client with an explicit PCR0 trust policy.
    pub fn new_with_api_key_and_pcr0_trust_policy(
        base_url: impl Into<String>,
        api_key: String,
        pcr0_trust_policy: Pcr0TrustPolicy,
    ) -> Result<Self> {
        let base_url = base_url.into();
        let session_manager = SessionManager::new_with_api_key(api_key);
        let transport_v2 = TransportV2Client::new(
            base_url,
            pcr0_trust_policy,
            random_cache_namespace_root()?,
            session_manager.clone(),
        )?;

        Ok(Self {
            session_manager,
            auth_commit_lock: StdMutex::new(()),
            refresh_lock: Mutex::new(()),
            transport_v2,
        })
    }

    /// Replace the random per-client provider-cache namespace root.
    ///
    /// Embedding applications may persist their own independently generated
    /// 32-byte root and supply it during client construction. The SDK never
    /// derives this value from a user identifier or API key.
    #[must_use]
    pub fn with_cache_namespace_root(mut self, root: TransportV2CacheNamespaceRoot) -> Self {
        // Consuming the fresh client lets us replace the runtime without a
        // fallible lock acquisition or leaving any sessions under the old root.
        self.transport_v2 = self
            .transport_v2
            .with_cache_namespace_root(root.into_bytes());
        self
    }

    pub fn set_api_key(&self, api_key: String) -> Result<()> {
        self.transport_v2.clear_api_key_sessions()?;
        self.session_manager.set_api_key(api_key)
    }

    pub fn clear_api_key(&self) -> Result<()> {
        self.transport_v2.clear_api_key_sessions()?;
        self.session_manager.clear_api_key()
    }

    pub async fn perform_attestation_handshake(&self) -> Result<()> {
        self.transport_v2.perform_attestation_handshake().await?;
        Ok(())
    }

    pub fn get_session_id(&self) -> Result<Option<Uuid>> {
        self.transport_v2.active_session_id()
    }

    pub async fn test_connection(&self) -> Result<String> {
        let url = format!("{}/health-check", self.transport_v2.base_url());
        let response = self.transport_v2.http_client().get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Error::Api {
                status,
                message: text,
            });
        }

        response.text().await.map_err(Into::into)
    }

    async fn encrypted_api_call<T: Serialize, U: DeserializeOwned>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
    ) -> Result<U> {
        let binding_transition = matches!(
            endpoint,
            "/login"
                | "/register"
                | "/auth/github/callback"
                | "/auth/google/callback"
                | "/auth/apple/callback"
                | "/auth/apple/native"
        );
        let expected_auth_epoch = binding_transition
            .then(|| {
                self.session_manager
                    .get_credential_snapshot()
                    .map(|credentials| credentials.auth_epoch)
            })
            .transpose()?;
        let body = data.map(|data| serde_json::to_vec(&data)).transpose()?;
        let session = if binding_transition {
            let _guard = self.transport_v2.user_gate().lock().await;
            let expected_auth_epoch = expected_auth_epoch.as_ref().ok_or_else(|| {
                Error::Session("Transport v2 authentication epoch is unavailable".to_string())
            })?;
            self.v2_credentials_for_epoch(expected_auth_epoch)?;
            let oauth_callback = matches!(
                endpoint,
                "/auth/github/callback" | "/auth/google/callback" | "/auth/apple/callback"
            );
            let session = if oauth_callback {
                let session = self.transport_v2.anonymous_session()?.ok_or_else(|| {
                    Error::Authentication(
                        "OAuth attested session is unavailable; restart sign-in".to_string(),
                    )
                })?;
                if session.is_expired()? {
                    self.transport_v2.clear_anonymous_session_if(&session)?;
                    return Err(Error::Authentication(
                        "OAuth attested session expired; restart sign-in".to_string(),
                    ));
                }
                session
            } else {
                self.transport_v2.perform_attestation_handshake().await?
            };
            let response = self
                .v2_send_on_session(
                    &session,
                    endpoint,
                    method,
                    body,
                    Vec::new(),
                    V2SendOptions::transition(
                        ResponseMode::Unary,
                        None,
                        self.v2_cache_namespace_root()?,
                    ),
                )
                .await?;
            let status = response.status();
            if !status.is_success() {
                return Err(Self::v2_api_error_from_response(response).await);
            }
            // An authenticated success means the enclave committed this exact
            // anonymous session's authority transition. Remove it from the
            // anonymous slot before any fallible local response processing.
            self.transport_v2.clear_anonymous_session_if(&session)?;
            let transition_result: Result<U> = async {
                let response_body = collect_response_body(response.into_body()).await?;
                let (login, value, validated) = decode_v2_user_binding_response(&response_body)?;
                let _commit_guard = self.auth_commit_guard()?;
                if self
                    .session_manager
                    .replace_user_tokens_and_session_if_epoch(
                        expected_auth_epoch,
                        login.access_token,
                        Some(login.refresh_token),
                        validated.principal,
                        Arc::clone(&session),
                    )?
                    .is_none()
                {
                    return Err(Error::Session(
                        "Credentials changed while transport v2 authentication was in flight"
                            .to_string(),
                    ));
                }
                Ok(value)
            }
            .await;
            return transition_result;
        } else {
            self.transport_v2.perform_attestation_handshake().await?
        };

        let response = self
            .v2_send_on_session(
                &session,
                endpoint,
                method,
                body,
                Vec::new(),
                V2SendOptions::bound(ResponseMode::Unary),
            )
            .await?;
        Self::v2_decode_json_response(response).await
    }

    async fn authenticated_api_call<T: Serialize, U: DeserializeOwned>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
    ) -> Result<U> {
        self.authenticated_api_call_with_auth(endpoint, method, data)
            .await
            .map(|(value, _, _)| value)
    }

    async fn authenticated_api_call_with_auth<T: Serialize, U: DeserializeOwned>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
    ) -> Result<(U, UserAuthEpoch, Arc<V2Session>)> {
        let expected_auth_epoch = self.session_manager.get_credential_snapshot()?.auth_epoch;
        let body = data.map(|data| serde_json::to_vec(&data)).transpose()?;
        let (session, active_auth_epoch) =
            self.ensure_v2_user_session(&expected_auth_epoch).await?;
        let credentials = self.v2_credentials_for_epoch(&active_auth_epoch)?;
        if !credentials
            .user_session
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &session))
        {
            return Err(Error::Session(
                "Credentials changed before the authenticated request was sent".to_string(),
            ));
        }
        let response = self
            .v2_send_on_session(
                &session,
                endpoint,
                method,
                body,
                Vec::new(),
                V2SendOptions::bound(ResponseMode::Unary),
            )
            .await?;
        let status = response.status();
        if matches!(status.as_u16(), 401 | 403) {
            self.transport_v2.clear_user_session_if(&session)?;
        }
        let value = Self::v2_decode_json_response(response).await?;
        Ok((value, active_auth_epoch, session))
    }

    fn v2_cache_namespace_root(&self) -> Result<CacheNamespaceRoot> {
        Ok(CacheNamespaceRoot::from_bytes(
            self.transport_v2.cache_namespace_root()?,
        ))
    }

    fn auth_commit_guard(&self) -> Result<StdMutexGuard<'_, ()>> {
        self.auth_commit_lock.lock().map_err(|_| {
            Error::Authentication(
                "Transport v2 authentication lifecycle state is unavailable".to_string(),
            )
        })
    }

    fn v2_credentials_for_epoch(
        &self,
        expected_auth_epoch: &UserAuthEpoch,
    ) -> Result<CredentialSnapshot> {
        self.session_manager
            .get_credential_snapshot_if_auth_epoch(expected_auth_epoch)?
            .ok_or_else(|| {
                Error::Session(
                    "Credentials changed while a transport v2 operation was waiting".to_string(),
                )
            })
    }

    fn ensure_credential_generation(&self, expected_generation: u64) -> Result<()> {
        if self
            .session_manager
            .credential_generation_matches(expected_generation)?
        {
            Ok(())
        } else {
            Err(Error::Session(
                "Credentials changed while a transport v2 operation was waiting".to_string(),
            ))
        }
    }

    async fn v2_send_on_session(
        &self,
        session: &Arc<V2Session>,
        endpoint: &str,
        method: &str,
        body: Option<Vec<u8>>,
        mut headers: Vec<HeaderField>,
        options: V2SendOptions,
    ) -> Result<V2HttpResponse> {
        let logical_method = match method {
            "GET" => LogicalMethod::Get,
            "POST" => LogicalMethod::Post,
            "PUT" => LogicalMethod::Put,
            "PATCH" => LogicalMethod::Patch,
            "DELETE" => LogicalMethod::Delete,
            _ => {
                return Err(Error::Configuration(format!(
                    "Unsupported logical HTTP method: {method}"
                )))
            }
        };
        let uri: http::Uri = endpoint.parse().map_err(|error| {
            Error::Configuration(format!("Invalid logical request URI: {error}"))
        })?;
        if uri.scheme().is_some() || uri.authority().is_some() {
            return Err(Error::Configuration(
                "Logical request URI must be origin-relative".to_string(),
            ));
        }
        if body.is_some() {
            headers.push(HeaderField::new(
                header::CONTENT_TYPE.as_str(),
                b"application/json".to_vec(),
            ));
        }
        let request = LogicalRequest::new(
            logical_method,
            uri.path(),
            uri.query().map(str::to_owned),
            headers,
            body,
        );
        self.transport_v2
            .send_request(
                session,
                options.response_mode,
                options.credential,
                options.cache_namespace_root,
                request,
            )
            .await
    }

    async fn v2_decode_json_response<U: DeserializeOwned>(response: V2HttpResponse) -> Result<U> {
        let status = response.status();
        if !status.is_success() {
            return Err(Self::v2_api_error_from_response(response).await);
        }
        let body = collect_response_body(response.into_body()).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    async fn v2_api_error_from_response(response: V2HttpResponse) -> Error {
        let status = response.status().as_u16();
        let message = collect_response_body(response.into_body())
            .await
            .map(|body| String::from_utf8_lossy(&body).into_owned())
            .unwrap_or_else(|_| "Authenticated application error".to_string());
        Error::Api { status, message }
    }

    async fn ensure_v2_user_session(
        &self,
        expected_auth_epoch: &UserAuthEpoch,
    ) -> Result<(Arc<V2Session>, UserAuthEpoch)> {
        let credentials = self.v2_credentials_for_epoch(expected_auth_epoch)?;
        if let Some(session) = credentials.user_session.as_ref() {
            if !session.is_expired()? && self.v2_user_binding_is_fresh(&credentials)? {
                return Ok((Arc::clone(session), credentials.auth_epoch));
            }
            self.transport_v2.clear_user_session_if(session)?;
        }

        let _guard = self.transport_v2.user_gate().lock().await;
        let credentials = self.v2_credentials_for_epoch(expected_auth_epoch)?;
        if let Some(session) = credentials.user_session.as_ref() {
            if !session.is_expired()? && self.v2_user_binding_is_fresh(&credentials)? {
                return Ok((Arc::clone(session), credentials.auth_epoch));
            }
            self.transport_v2.clear_user_session_if(session)?;
        }
        self.resume_v2_user_session(expected_auth_epoch).await
    }

    fn v2_user_binding_is_fresh(&self, credentials: &CredentialSnapshot) -> Result<bool> {
        let Some(tokens) = credentials.tokens.as_ref() else {
            return Ok(false);
        };
        let Some(refresh_token) = tokens.refresh_token.as_deref() else {
            return Ok(false);
        };
        let validated = validate_v2_user_token_pair(&tokens.access_token, refresh_token)?;
        if credentials.auth_epoch.principal.as_deref() != Some(validated.principal.as_str()) {
            return Ok(false);
        }
        let now_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                Error::Authentication(
                    "System clock is before the Unix epoch; transport v2 cannot validate the user binding"
                        .to_string(),
                )
            })?
            .as_secs();
        Ok(v2_user_binding_is_fresh_at(
            validated.access_expires_at_unix_seconds,
            now_unix_seconds,
        ))
    }

    async fn resume_v2_user_session(
        &self,
        expected_auth_epoch: &UserAuthEpoch,
    ) -> Result<(Arc<V2Session>, UserAuthEpoch)> {
        let credentials = self.v2_credentials_for_epoch(expected_auth_epoch)?;
        let expected_principal = credentials.auth_epoch.principal.clone().ok_or_else(|| {
            Error::Authentication(
                "Transport v2 requires a fresh login or resumption credential".to_string(),
            )
        })?;
        let refresh_token = credentials
            .tokens
            .as_ref()
            .and_then(|tokens| tokens.refresh_token.as_deref())
            .ok_or_else(|| {
                Error::Authentication(
                    "Transport v2 requires a fresh login or resumption credential".to_string(),
                )
            })?;
        let credential = Credential::resumption(refresh_token.as_bytes().to_vec());
        let session = self.transport_v2.fresh_session().await?;
        let response = self
            .v2_send_on_session(
                &session,
                "/refresh",
                "POST",
                None,
                Vec::new(),
                V2SendOptions::transition(
                    ResponseMode::Unary,
                    Some(credential),
                    self.v2_cache_namespace_root()?,
                ),
            )
            .await?;
        if !response.status().is_success() {
            if matches!(response.status().as_u16(), 401 | 403) {
                let _commit_guard = self.auth_commit_guard()?;
                if !self
                    .session_manager
                    .invalidate_user_auth_if_epoch(&credentials.auth_epoch)?
                {
                    return Err(Error::Session(
                        "Credentials changed while rejected transport v2 resumption was in flight"
                            .to_string(),
                    ));
                }
                return Err(Error::Authentication(
                    "Stored credentials cannot resume transport v2; sign in again".to_string(),
                ));
            }
            return Err(Self::v2_api_error_from_response(response).await);
        }
        let body = match collect_response_body(response.into_body()).await {
            Ok(body) => body,
            Err(error) => {
                let _commit_guard = self.auth_commit_guard()?;
                self.session_manager
                    .invalidate_user_auth_if_epoch(&credentials.auth_epoch)?;
                return Err(error);
            }
        };
        let response: RefreshResponse = match serde_json::from_slice(&body) {
            Ok(response) => response,
            Err(error) => {
                let _commit_guard = self.auth_commit_guard()?;
                self.session_manager
                    .invalidate_user_auth_if_epoch(&credentials.auth_epoch)?;
                return Err(error.into());
            }
        };
        let validated =
            match validate_v2_user_token_pair(&response.access_token, &response.refresh_token) {
                Ok(validated) => validated,
                Err(error) => {
                    let _commit_guard = self.auth_commit_guard()?;
                    self.session_manager
                        .invalidate_user_auth_if_epoch(&credentials.auth_epoch)?;
                    return Err(error.into());
                }
            };
        if validated.principal != expected_principal {
            let _commit_guard = self.auth_commit_guard()?;
            if !self
                .session_manager
                .invalidate_user_auth_if_epoch(&credentials.auth_epoch)?
            {
                return Err(Error::Session(
                    "Credentials changed while transport v2 resumption was in flight".to_string(),
                ));
            }
            return Err(Error::Authentication(
                "Transport v2 resumption changed the authenticated principal".to_string(),
            ));
        }
        let _commit_guard = self.auth_commit_guard()?;
        let Some(active_auth_epoch) = self
            .session_manager
            .replace_user_tokens_and_session_if_epoch(
                &credentials.auth_epoch,
                response.access_token,
                Some(response.refresh_token),
                validated.principal,
                Arc::clone(&session),
            )?
        else {
            return Err(Error::Session(
                "Credentials changed while transport v2 resumption was in flight".to_string(),
            ));
        };
        Ok((session, active_auth_epoch))
    }

    /// Sends a lossless encrypted request to an OpenSecret inference endpoint.
    ///
    /// The request URI must be relative and target one of the SDK's explicitly
    /// allowed inference routes. Its method, query string, headers, and body
    /// bytes are otherwise caller-owned. For chat completions only, the SDK
    /// reads the top-level boolean `stream` selector so the authenticated
    /// transport can commit to unary or streaming response reconstruction. It
    /// never adds or changes inference parameters.
    ///
    /// OpenSecret authentication, attestation sessions, and the encrypted
    /// envelope remain SDK-owned. Caller-provided `Host`, `Authorization`,
    /// `x-session-id`, `Content-Length`, `Content-Type`, `Content-Encoding`,
    /// `Accept-Encoding`, `Content-MD5`, `Digest`, and hop-by-hop headers
    /// (including fields named by `Connection`) are therefore not forwarded.
    /// Other headers are preserved.
    ///
    /// The returned HTTP response preserves the final OpenSecret status and
    /// safe response headers. Its body is the exact authenticated logical body;
    /// application SSE framing and chunks are preserved without parsing
    /// completion JSON.
    pub async fn send_inference_request(
        &self,
        request: InferenceRequest,
    ) -> Result<InferenceResponse> {
        let operation_credentials = self.session_manager.get_credential_snapshot()?;
        let (parts, body) = request.into_parts();
        if parts.uri.scheme().is_some() || parts.uri.authority().is_some() {
            return Err(Error::Configuration(
                "Inference request URI must be relative".to_string(),
            ));
        }
        if !is_allowed_inference_endpoint(&parts.method, parts.uri.path()) {
            return Err(Error::Configuration(format!(
                "Inference endpoint is not allowed: {} {}",
                parts.method,
                parts.uri.path()
            )));
        }

        let response_mode = inference_response_mode(parts.uri.path(), &body)?;
        let headers = sanitize_inference_request_headers(&parts.headers)
            .iter()
            .map(|(name, value)| HeaderField::new(name.as_str(), value.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        let endpoint = parts
            .uri
            .path_and_query()
            .ok_or_else(|| Error::Configuration("Inference request URI has no path".to_string()))?
            .as_str();
        let body = (!body.is_empty()).then(|| body.to_vec());
        let has_user_credentials = operation_credentials.tokens.is_some();

        let response = if operation_credentials.api_key.is_some() {
            self.v2_api_key_inference_call(
                &operation_credentials,
                endpoint,
                parts.method.as_str(),
                body,
                headers,
                response_mode,
            )
            .await?
        } else {
            let session =
                if should_use_anonymous_models_session(parts.uri.path(), has_user_credentials) {
                    let session = self.transport_v2.perform_attestation_handshake().await?;
                    self.ensure_credential_generation(operation_credentials.generation)?;
                    session
                } else {
                    let (session, active_auth_epoch) = self
                        .ensure_v2_user_session(&operation_credentials.auth_epoch)
                        .await?;
                    let credentials = self.v2_credentials_for_epoch(&active_auth_epoch)?;
                    if credentials.api_key.is_some()
                        || !credentials
                            .user_session
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &session))
                    {
                        return Err(Error::Session(
                            "Credentials changed before the inference request was sent".to_string(),
                        ));
                    }
                    session
                };
            let response = self
                .v2_send_on_session(
                    &session,
                    endpoint,
                    parts.method.as_str(),
                    body,
                    headers,
                    V2SendOptions::bound(response_mode),
                )
                .await?;
            if matches!(response.status().as_u16(), 401 | 403) {
                self.transport_v2.clear_user_session_if(&session)?;
            }
            response
        };

        Self::finish_v2_inference_response(response)
    }

    async fn v2_api_key_inference_call(
        &self,
        operation_credentials: &CredentialSnapshot,
        endpoint: &str,
        method: &str,
        body: Option<Vec<u8>>,
        headers: Vec<HeaderField>,
        response_mode: ResponseMode,
    ) -> Result<V2HttpResponse> {
        let api_key = operation_credentials.api_key.as_deref().ok_or_else(|| {
            Error::Session("API key authority changed before inference was sent".to_string())
        })?;
        let expected_generation = operation_credentials.generation;
        let fingerprint: [u8; 32] = Sha256::digest(api_key.as_bytes()).into();
        let scope = ApiKeyScope::new(fingerprint);

        if let Some(session) = self.transport_v2.api_key_session(&scope)? {
            if !session.is_expired()? {
                self.ensure_credential_generation(expected_generation)?;
                let response = self
                    .v2_send_on_session(
                        &session,
                        endpoint,
                        method,
                        body,
                        headers,
                        V2SendOptions::bound(response_mode),
                    )
                    .await?;
                if matches!(response.status().as_u16(), 401 | 403) {
                    self.transport_v2
                        .clear_api_key_session_if(&scope, &session)?;
                }
                return Ok(response);
            }
            self.transport_v2
                .clear_api_key_session_if(&scope, &session)?;
        }

        let _guard = self.transport_v2.api_key_gate().lock().await;
        if let Some(session) = self.transport_v2.api_key_session(&scope)? {
            if !session.is_expired()? {
                self.ensure_credential_generation(expected_generation)?;
                let response = self
                    .v2_send_on_session(
                        &session,
                        endpoint,
                        method,
                        body,
                        headers,
                        V2SendOptions::bound(response_mode),
                    )
                    .await?;
                if matches!(response.status().as_u16(), 401 | 403) {
                    self.transport_v2
                        .clear_api_key_session_if(&scope, &session)?;
                }
                return Ok(response);
            }
            self.transport_v2
                .clear_api_key_session_if(&scope, &session)?;
        }

        let session = self.transport_v2.fresh_session().await?;
        self.ensure_credential_generation(expected_generation)?;
        let response = self
            .v2_send_on_session(
                &session,
                endpoint,
                method,
                body,
                headers,
                V2SendOptions::transition(
                    response_mode,
                    Some(Credential::api_key(api_key.as_bytes().to_vec())),
                    self.v2_cache_namespace_root()?,
                ),
            )
            .await?;
        if response.status().is_success() {
            self.transport_v2
                .set_api_key_session(&scope, Arc::clone(&session))?;
        }
        Ok(response)
    }

    fn finish_v2_inference_response(response: V2HttpResponse) -> Result<InferenceResponse> {
        let (mut parts, body) = response.into_parts();
        parts.headers = sanitize_inference_response_headers(&parts.headers);
        Ok(HttpResponse::from_parts(parts, body))
    }

    /// Typed compatibility wrapper over the lossless inference transport.
    async fn encrypted_openai_call<T: Serialize, U: DeserializeOwned>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
    ) -> Result<U> {
        let method = http::Method::from_bytes(method.as_bytes()).map_err(|error| {
            Error::Configuration(format!("Invalid inference HTTP method: {error}"))
        })?;
        let body = match data {
            Some(data) => Bytes::from(serde_json::to_vec(&data)?),
            None => Bytes::new(),
        };
        let request = HttpRequest::builder()
            .method(method)
            .uri(endpoint)
            .body(body)
            .map_err(|error| {
                Error::Configuration(format!("Failed to build inference request: {error}"))
            })?;
        let response = self.send_inference_request(request).await?;
        let status = response.status();
        let body = collect_response_body(response.into_body()).await?;

        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                message: String::from_utf8_lossy(&body).into_owned(),
            });
        }

        Ok(serde_json::from_slice(&body)?)
    }

    // Auth Methods
    pub async fn login(
        &self,
        email: String,
        password: String,
        client_id: Uuid,
    ) -> Result<LoginResponse> {
        let credentials = LoginCredentials {
            email: Some(email),
            id: None,
            password,
            client_id,
        };

        self.encrypted_api_call("/login", "POST", Some(credentials))
            .await
    }

    pub async fn login_with_id(
        &self,
        id: Uuid,
        password: String,
        client_id: Uuid,
    ) -> Result<LoginResponse> {
        let credentials = LoginCredentials {
            email: None,
            id: Some(id),
            password,
            client_id,
        };

        self.encrypted_api_call("/login", "POST", Some(credentials))
            .await
    }

    pub async fn register(
        &self,
        email: String,
        password: String,
        client_id: Uuid,
        name: Option<String>,
    ) -> Result<LoginResponse> {
        let credentials = RegisterCredentials {
            email: Some(email),
            name,
            password,
            client_id,
        };

        self.encrypted_api_call("/register", "POST", Some(credentials))
            .await
    }

    pub async fn register_guest(&self, password: String, client_id: Uuid) -> Result<LoginResponse> {
        let credentials = RegisterCredentials {
            email: None,
            name: None,
            password,
            client_id,
        };

        self.encrypted_api_call("/register", "POST", Some(credentials))
            .await
    }

    // OAuth Methods

    pub async fn initiate_github_auth(
        &self,
        client_id: Uuid,
        invite_code: Option<String>,
    ) -> Result<GithubAuthResponse> {
        let request = OAuthInitRequest {
            client_id,
            invite_code,
        };
        self.encrypted_api_call("/auth/github", "POST", Some(request))
            .await
    }

    pub async fn handle_github_callback(
        &self,
        code: String,
        state: String,
        invite_code: String,
    ) -> Result<LoginResponse> {
        let request = OAuthCallbackRequest {
            code,
            state,
            invite_code,
        };

        self.encrypted_api_call("/auth/github/callback", "POST", Some(request))
            .await
    }

    pub async fn initiate_google_auth(
        &self,
        client_id: Uuid,
        invite_code: Option<String>,
    ) -> Result<GoogleAuthResponse> {
        let request = OAuthInitRequest {
            client_id,
            invite_code,
        };
        self.encrypted_api_call("/auth/google", "POST", Some(request))
            .await
    }

    pub async fn handle_google_callback(
        &self,
        code: String,
        state: String,
        invite_code: String,
    ) -> Result<LoginResponse> {
        let request = OAuthCallbackRequest {
            code,
            state,
            invite_code,
        };

        self.encrypted_api_call("/auth/google/callback", "POST", Some(request))
            .await
    }

    pub async fn initiate_apple_auth(
        &self,
        client_id: Uuid,
        invite_code: Option<String>,
    ) -> Result<AppleAuthResponse> {
        let request = OAuthInitRequest {
            client_id,
            invite_code,
        };
        self.encrypted_api_call("/auth/apple", "POST", Some(request))
            .await
    }

    pub async fn handle_apple_callback(
        &self,
        code: String,
        state: String,
        invite_code: String,
    ) -> Result<LoginResponse> {
        let request = OAuthCallbackRequest {
            code,
            state,
            invite_code,
        };

        self.encrypted_api_call("/auth/apple/callback", "POST", Some(request))
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn handle_apple_native_sign_in(
        &self,
        user_identifier: String,
        identity_token: String,
        client_id: Uuid,
        email: Option<String>,
        given_name: Option<String>,
        family_name: Option<String>,
        nonce: Option<String>,
        invite_code: Option<String>,
    ) -> Result<LoginResponse> {
        let request = AppleNativeSignInRequest {
            user_identifier,
            identity_token,
            client_id,
            email,
            given_name,
            family_name,
            nonce,
            invite_code,
        };

        self.encrypted_api_call("/auth/apple/native", "POST", Some(request))
            .await
    }

    async fn refresh_token_inner(&self, expected_auth_epoch: &UserAuthEpoch) -> Result<()> {
        let _guard = self.transport_v2.user_gate().lock().await;
        let credentials = self.v2_credentials_for_epoch(expected_auth_epoch)?;
        if let Some(session) = credentials.user_session.as_ref() {
            self.transport_v2.clear_user_session_if(session)?;
        }
        self.resume_v2_user_session(expected_auth_epoch).await?;
        Ok(())
    }

    pub async fn refresh_token(&self) -> Result<()> {
        let expected_auth_epoch = self.session_manager.get_credential_snapshot()?.auth_epoch;
        let _refresh_guard = self.refresh_lock.lock().await;
        self.refresh_token_inner(&expected_auth_epoch).await
    }

    async fn logout_inner(
        &self,
        push_device_id: Option<Uuid>,
        expected_auth_epoch: &UserAuthEpoch,
    ) -> Result<()> {
        // Serialize logout with refresh so an internal token rotation cannot
        // race the clear. Application-supplied credentials use the short
        // commit lock and win through the generation check below.
        let _refresh_guard = self.refresh_lock.lock().await;
        let (session, active_auth_epoch) = self.ensure_v2_user_session(expected_auth_epoch).await?;
        let credentials = self.v2_credentials_for_epoch(&active_auth_epoch)?;
        if !credentials
            .user_session
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &session))
        {
            return Err(Error::Session(
                "Credentials changed before logout was sent".to_string(),
            ));
        }
        let refresh_token = credentials
            .tokens
            .as_ref()
            .and_then(|tokens| tokens.refresh_token.clone())
            .ok_or_else(|| Error::Authentication("No refresh token available".to_string()))?;

        let request = LogoutRequest {
            refresh_token,
            push_device_id,
        };

        let response = self
            .v2_send_on_session(
                &session,
                "/logout",
                "POST",
                Some(serde_json::to_vec(&request)?),
                Vec::new(),
                V2SendOptions::bound(ResponseMode::Unary),
            )
            .await?;
        let status = response.status();
        if matches!(status.as_u16(), 401 | 403) {
            self.transport_v2.clear_user_session_if(&session)?;
        }
        let _: serde_json::Value = Self::v2_decode_json_response(response).await?;

        // Do not clear credentials installed by the application while the
        // logout request was in flight (for example, a rapid account switch).
        let _commit_guard = self.auth_commit_guard()?;
        let credentials_cleared = self
            .session_manager
            .clear_all_if_generation(credentials.generation)?;
        // The backend closes the exact authenticated session on successful
        // logout. Pointer-scoped cleanup cannot clear a newer account switch.
        self.transport_v2.clear_user_session_if(&session)?;
        if credentials_cleared {
            self.transport_v2.clear_api_key_sessions()?;
        }

        Ok(())
    }

    pub async fn logout(&self) -> Result<()> {
        let expected_auth_epoch = self.session_manager.get_credential_snapshot()?.auth_epoch;
        self.logout_inner(None, &expected_auth_epoch).await
    }

    pub async fn logout_with_push_device_id(&self, push_device_id: Uuid) -> Result<()> {
        let expected_auth_epoch = self.session_manager.get_credential_snapshot()?.auth_epoch;
        self.logout_inner(Some(push_device_id), &expected_auth_epoch)
            .await
    }

    pub fn get_access_token(&self) -> Result<Option<String>> {
        self.session_manager.get_access_token()
    }

    /// Return one coherent snapshot of the current JWT token pair.
    ///
    /// Prefer this over separate access- and refresh-token reads when the pair
    /// will be persisted or copied into another client: an automatic refresh
    /// can replace both values between two independent lock acquisitions.
    pub fn get_tokens(&self) -> Result<Option<TokenPair>> {
        self.session_manager.get_tokens()
    }

    pub fn get_refresh_token(&self) -> Result<Option<String>> {
        self.session_manager.get_refresh_token()
    }

    pub fn set_tokens(&self, access_token: String, refresh_token: Option<String>) -> Result<()> {
        let refresh_token = refresh_token.ok_or_else(|| {
            Error::Authentication(
                "Transport v2 requires paired access and resumption descriptors".to_string(),
            )
        })?;
        let validated = validate_v2_user_token_pair(&access_token, &refresh_token)?;
        let _commit_guard = self.auth_commit_guard()?;
        self.session_manager.clear_session()?;
        self.session_manager
            .replace_user_tokens(access_token, Some(refresh_token), Some(validated.principal))
            .map(|_| ())
    }

    /// Import the opaque transport-v2 authentication bundle produced by the
    /// browser SDK for this exact configured backend.
    pub fn import_transport_v2_auth_bundle(&self, bundle: &str) -> Result<()> {
        let decoded = decode_auth_bundle(bundle, self.transport_v2.base_url())?;
        let validated = validate_v2_user_token_pair(&decoded.access_token, &decoded.refresh_token)?;
        let _commit_guard = self.auth_commit_guard()?;
        let expected = self.session_manager.get_credential_snapshot()?.auth_epoch;
        self.transport_v2
            .replace_cache_namespace_root(decoded.cache_namespace_root)?;
        if self
            .session_manager
            .replace_user_tokens_if_epoch(
                &expected,
                decoded.access_token.clone(),
                Some(decoded.refresh_token.clone()),
                validated.principal,
            )?
            .is_none()
        {
            return Err(Error::Session(
                "Credentials changed while transport v2 authentication was imported".to_string(),
            ));
        }
        Ok(())
    }

    /// Export the current transport-v2 descriptors and cache root as one
    /// opaque, origin-bound bundle.
    pub fn export_transport_v2_auth_bundle(&self) -> Result<Option<String>> {
        let _commit_guard = self.auth_commit_guard()?;
        let Some(tokens) = self.session_manager.get_tokens()? else {
            return Ok(None);
        };
        let Some(refresh_token) = tokens.refresh_token.as_deref() else {
            return Ok(None);
        };
        let root = self.transport_v2.cache_namespace_root()?;
        encode_auth_bundle(
            self.transport_v2.base_url(),
            &tokens.access_token,
            refresh_token,
            &root,
        )
        .map(Some)
        .map_err(Into::into)
    }

    // User Profile API
    pub async fn get_user(&self) -> Result<UserResponse> {
        self.authenticated_api_call("/protected/user", "GET", None::<()>)
            .await
    }

    pub async fn register_push_device(
        &self,
        request: RegisterPushDeviceRequest,
    ) -> Result<PushDevice> {
        self.authenticated_api_call("/v1/push/devices", "POST", Some(request))
            .await
    }

    pub async fn list_push_devices(&self) -> Result<PushDeviceListResponse> {
        self.authenticated_api_call("/v1/push/devices", "GET", None::<()>)
            .await
    }

    pub async fn revoke_push_device(&self, id: Uuid) -> Result<DeletedPushDeviceResponse> {
        self.authenticated_api_call(&format!("/v1/push/devices/{}", id), "DELETE", None::<()>)
            .await
    }

    // API Key Management
    pub async fn create_api_key(&self, name: String) -> Result<ApiKeyCreateResponse> {
        let request = ApiKeyCreateRequest { name };
        self.authenticated_api_call("/protected/api-keys", "POST", Some(request))
            .await
    }

    pub async fn list_api_keys(&self) -> Result<Vec<ApiKey>> {
        let response: ApiKeyListResponse = self
            .authenticated_api_call("/protected/api-keys", "GET", None::<()>)
            .await?;

        // Sort by created_at descending (newest first)
        let mut keys = response.keys;
        keys.sort_by_key(|key| std::cmp::Reverse(key.created_at));

        Ok(keys)
    }

    pub async fn delete_api_key(&self, name: &str) -> Result<()> {
        // URL-encode the name to handle special characters
        let encoded_name = utf8_percent_encode(name, NON_ALPHANUMERIC).to_string();
        let url = format!("/protected/api-keys/{}", encoded_name);
        let _: serde_json::Value = self
            .authenticated_api_call(&url, "DELETE", None::<()>)
            .await?;
        Ok(())
    }

    // Key-Value Storage APIs
    pub async fn kv_get(&self, key: &str) -> Result<String> {
        let encoded_key = utf8_percent_encode(key, NON_ALPHANUMERIC).to_string();
        let url = format!("/protected/kv/{}", encoded_key);
        self.authenticated_api_call(&url, "GET", None::<()>).await
    }

    pub async fn kv_put(&self, key: &str, value: String) -> Result<String> {
        let encoded_key = utf8_percent_encode(key, NON_ALPHANUMERIC).to_string();
        let url = format!("/protected/kv/{}", encoded_key);
        self.authenticated_api_call(&url, "PUT", Some(value)).await
    }

    pub async fn kv_delete(&self, key: &str) -> Result<()> {
        let encoded_key = utf8_percent_encode(key, NON_ALPHANUMERIC).to_string();
        let url = format!("/protected/kv/{}", encoded_key);
        let _: serde_json::Value = self
            .authenticated_api_call(&url, "DELETE", None::<()>)
            .await?;
        Ok(())
    }

    pub async fn kv_delete_all(&self) -> Result<()> {
        let _: serde_json::Value = self
            .authenticated_api_call("/protected/kv", "DELETE", None::<()>)
            .await?;
        Ok(())
    }

    pub async fn kv_list(&self) -> Result<Vec<KVListItem>> {
        self.authenticated_api_call("/protected/kv", "GET", None::<()>)
            .await
    }

    // Private Key APIs
    pub async fn get_private_key(&self, options: Option<KeyOptions>) -> Result<PrivateKeyResponse> {
        let mut url = "/protected/private_key".to_string();
        if let Some(opts) = &options {
            let mut params = Vec::new();
            if let Some(path) = &opts.seed_phrase_derivation_path {
                let encoded = utf8_percent_encode(path, NON_ALPHANUMERIC).to_string();
                params.push(format!("seed_phrase_derivation_path={}", encoded));
            }
            if let Some(path) = &opts.private_key_derivation_path {
                let encoded = utf8_percent_encode(path, NON_ALPHANUMERIC).to_string();
                params.push(format!("private_key_derivation_path={}", encoded));
            }
            if !params.is_empty() {
                url.push('?');
                url.push_str(&params.join("&"));
            }
        }
        self.authenticated_api_call(&url, "GET", None::<()>).await
    }

    pub async fn get_private_key_bytes(
        &self,
        options: Option<KeyOptions>,
    ) -> Result<PrivateKeyBytesResponse> {
        let mut url = "/protected/private_key_bytes".to_string();
        if let Some(opts) = &options {
            let mut params = Vec::new();
            if let Some(path) = &opts.seed_phrase_derivation_path {
                let encoded = utf8_percent_encode(path, NON_ALPHANUMERIC).to_string();
                params.push(format!("seed_phrase_derivation_path={}", encoded));
            }
            if let Some(path) = &opts.private_key_derivation_path {
                let encoded = utf8_percent_encode(path, NON_ALPHANUMERIC).to_string();
                params.push(format!("private_key_derivation_path={}", encoded));
            }
            if !params.is_empty() {
                url.push('?');
                url.push_str(&params.join("&"));
            }
        }
        self.authenticated_api_call(&url, "GET", None::<()>).await
    }

    // Message Signing API
    pub async fn sign_message(
        &self,
        message_bytes: &[u8],
        algorithm: SigningAlgorithm,
        key_options: Option<KeyOptions>,
    ) -> Result<SignMessageResponse> {
        let message_base64 = BASE64.encode(message_bytes);
        let request = SignMessageRequest {
            message_base64,
            algorithm,
            key_options: key_options.map(|opts| SigningKeyOptions {
                private_key_derivation_path: opts.private_key_derivation_path,
                seed_phrase_derivation_path: opts.seed_phrase_derivation_path,
            }),
        };
        self.authenticated_api_call("/protected/sign_message", "POST", Some(request))
            .await
    }

    // Public Key API
    pub async fn get_public_key(
        &self,
        algorithm: SigningAlgorithm,
        key_options: Option<KeyOptions>,
    ) -> Result<PublicKeyResponse> {
        let mut url = format!(
            "/protected/public_key?algorithm={}",
            match algorithm {
                SigningAlgorithm::Schnorr => "schnorr",
                SigningAlgorithm::Ecdsa => "ecdsa",
            }
        );
        if let Some(opts) = key_options {
            if let Some(path) = &opts.private_key_derivation_path {
                let encoded = utf8_percent_encode(path, NON_ALPHANUMERIC).to_string();
                url.push_str(&format!("&private_key_derivation_path={}", encoded));
            }
            if let Some(path) = &opts.seed_phrase_derivation_path {
                let encoded = utf8_percent_encode(path, NON_ALPHANUMERIC).to_string();
                url.push_str(&format!("&seed_phrase_derivation_path={}", encoded));
            }
        }
        self.authenticated_api_call(&url, "GET", None::<()>).await
    }

    // Third Party Token API
    pub async fn generate_third_party_token(
        &self,
        audience: Option<String>,
    ) -> Result<ThirdPartyTokenResponse> {
        let request = ThirdPartyTokenRequest { audience };
        self.authenticated_api_call("/protected/third_party_token", "POST", Some(request))
            .await
    }

    // Encryption/Decryption APIs
    pub async fn encrypt_data(
        &self,
        data: String,
        key_options: Option<KeyOptions>,
    ) -> Result<EncryptDataResponse> {
        let request = EncryptDataRequest {
            data,
            key_options: key_options.map(|opts| EncryptionKeyOptions {
                private_key_derivation_path: opts.private_key_derivation_path,
                seed_phrase_derivation_path: opts.seed_phrase_derivation_path,
            }),
        };
        self.authenticated_api_call("/protected/encrypt", "POST", Some(request))
            .await
    }

    pub async fn decrypt_data(
        &self,
        encrypted_data: String,
        key_options: Option<KeyOptions>,
    ) -> Result<String> {
        let request = DecryptDataRequest {
            encrypted_data,
            key_options: key_options.map(|opts| EncryptionKeyOptions {
                private_key_derivation_path: opts.private_key_derivation_path,
                seed_phrase_derivation_path: opts.seed_phrase_derivation_path,
            }),
        };
        self.authenticated_api_call("/protected/decrypt", "POST", Some(request))
            .await
    }

    // Account Management APIs

    /// Changes the password for the currently authenticated user
    pub async fn change_password(
        &self,
        current_password: String,
        new_password: String,
    ) -> Result<()> {
        let request = ChangePasswordRequest {
            current_password,
            new_password,
        };
        let (response, auth_epoch, session): (
            CredentialUpdateResponse,
            UserAuthEpoch,
            Arc<V2Session>,
        ) = self
            .authenticated_api_call_with_auth("/protected/change_password", "POST", Some(request))
            .await?;
        let _commit_guard = self.auth_commit_guard()?;
        match (response.access_token, response.refresh_token) {
            (Some(access_token), Some(refresh_token)) => {
                let validated = match validate_v2_user_token_pair(&access_token, &refresh_token) {
                    Ok(validated) => validated,
                    Err(error) => {
                        self.session_manager
                            .invalidate_user_auth_if_epoch(&auth_epoch)?;
                        self.transport_v2.clear_user_session_if(&session)?;
                        return Err(error.into());
                    }
                };
                if auth_epoch.principal.as_deref() != Some(validated.principal.as_str()) {
                    if !self
                        .session_manager
                        .invalidate_user_auth_if_epoch(&auth_epoch)?
                    {
                        return Err(Error::Session(
                            "Credentials changed while password change was in flight".to_string(),
                        ));
                    }
                    self.transport_v2.clear_user_session_if(&session)?;
                    return Err(Error::Authentication(
                        "Password change returned descriptors for another principal".to_string(),
                    ));
                }
                if self
                    .session_manager
                    .replace_user_tokens_if_epoch(
                        &auth_epoch,
                        access_token,
                        Some(refresh_token),
                        validated.principal,
                    )?
                    .is_none()
                {
                    return Err(Error::Session(
                        "Credentials changed while password change was in flight".to_string(),
                    ));
                }
            }
            _ => {
                if !self
                    .session_manager
                    .invalidate_user_auth_if_epoch(&auth_epoch)?
                {
                    return Err(Error::Session(
                        "Credentials changed while password change was in flight".to_string(),
                    ));
                }
                return Err(Error::Authentication(
                    "Password changed without replacement transport-v2 descriptors; sign in again"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Requests a password reset for the given email
    /// Note: This does not require authentication but still uses encryption
    pub async fn request_password_reset(
        &self,
        email: String,
        hashed_secret: String,
        client_id: Uuid,
    ) -> Result<()> {
        let request = PasswordResetRequest {
            email,
            hashed_secret,
            client_id,
        };
        let _: serde_json::Value = self
            .encrypted_api_call("/password-reset/request", "POST", Some(request))
            .await?;
        Ok(())
    }

    /// Confirms a password reset with the code from email
    /// Note: This does not require authentication but still uses encryption
    pub async fn confirm_password_reset(
        &self,
        email: String,
        alphanumeric_code: String,
        plaintext_secret: String,
        new_password: String,
        client_id: Uuid,
    ) -> Result<()> {
        let request = PasswordResetConfirmRequest {
            email,
            alphanumeric_code,
            plaintext_secret,
            new_password,
            client_id,
        };
        let _: serde_json::Value = self
            .encrypted_api_call("/password-reset/confirm", "POST", Some(request))
            .await?;
        Ok(())
    }

    /// Verifies an email address with the code from the verification email
    /// Note: This does not require authentication but still uses encryption
    pub async fn verify_email(&self, code: String) -> Result<()> {
        let _: serde_json::Value = self
            .encrypted_api_call(&format!("/verify-email/{}", code), "GET", None::<()>)
            .await?;
        Ok(())
    }

    /// Requests a new email verification code
    pub async fn request_new_verification_code(&self) -> Result<()> {
        let request = RequestVerificationCodeRequest {};
        let _: serde_json::Value = self
            .authenticated_api_call("/protected/request_verification", "POST", Some(request))
            .await?;
        Ok(())
    }

    /// Initiates the account deletion process
    pub async fn request_account_deletion(&self, hashed_secret: String) -> Result<()> {
        let request = InitiateAccountDeletionRequest { hashed_secret };
        let _: serde_json::Value = self
            .authenticated_api_call("/protected/delete-account/request", "POST", Some(request))
            .await?;
        Ok(())
    }

    /// Confirms account deletion with the code from email
    pub async fn confirm_account_deletion(
        &self,
        confirmation_code: String,
        plaintext_secret: String,
    ) -> Result<()> {
        let request = ConfirmAccountDeletionRequest {
            confirmation_code,
            plaintext_secret,
        };
        let (_, auth_epoch, session): (serde_json::Value, UserAuthEpoch, Arc<V2Session>) = self
            .authenticated_api_call_with_auth(
                "/protected/delete-account/confirm",
                "POST",
                Some(request),
            )
            .await?;
        let _commit_guard = self.auth_commit_guard()?;
        let invalidated = self
            .session_manager
            .invalidate_user_auth_if_epoch(&auth_epoch)?;
        self.transport_v2.clear_user_session_if(&session)?;
        if invalidated {
            self.transport_v2
                .replace_cache_namespace_root(random_cache_namespace_root()?)?;
        }
        Ok(())
    }

    // AI/OpenAI API Methods

    /// Creates a new conversation.
    pub async fn create_conversation(
        &self,
        request: ConversationCreateRequest,
    ) -> Result<Conversation> {
        self.authenticated_api_call("/v1/conversations", "POST", Some(request))
            .await
    }

    /// Lists conversations with optional filters and pagination.
    pub async fn list_conversations(
        &self,
        params: Option<ConversationsListParams>,
    ) -> Result<ConversationsListResponse> {
        let endpoint = build_conversations_endpoint(params.as_ref());
        self.authenticated_api_call(&endpoint, "GET", None::<()>)
            .await
    }

    /// Fetches a single conversation by UUID.
    pub async fn get_conversation(&self, conversation_id: Uuid) -> Result<Conversation> {
        self.authenticated_api_call(
            &format!("/v1/conversations/{}", conversation_id),
            "GET",
            None::<()>,
        )
        .await
    }

    /// Partially updates a conversation.
    pub async fn update_conversation(
        &self,
        conversation_id: Uuid,
        request: ConversationUpdateRequest,
    ) -> Result<Conversation> {
        if request.is_empty() {
            return Err(Error::Configuration(
                "Conversation update request must include at least one field".to_string(),
            ));
        }

        self.authenticated_api_call(
            &format!("/v1/conversations/{}", conversation_id),
            "POST",
            Some(request),
        )
        .await
    }

    /// Deletes a single conversation by UUID.
    pub async fn delete_conversation(
        &self,
        conversation_id: Uuid,
    ) -> Result<DeletedObjectResponse> {
        self.authenticated_api_call(
            &format!("/v1/conversations/{}", conversation_id),
            "DELETE",
            None::<()>,
        )
        .await
    }

    /// Lists items in a conversation.
    pub async fn list_conversation_items(
        &self,
        conversation_id: Uuid,
        params: Option<AgentItemsListParams>,
    ) -> Result<ConversationItemsResponse> {
        let endpoint = build_agent_items_endpoint(
            &format!("/v1/conversations/{}/items", conversation_id),
            params.as_ref(),
        );
        self.authenticated_api_call(&endpoint, "GET", None::<()>)
            .await
    }

    /// Fetches a single item from a conversation.
    pub async fn get_conversation_item(
        &self,
        conversation_id: Uuid,
        item_id: Uuid,
    ) -> Result<ConversationItem> {
        self.authenticated_api_call(
            &format!("/v1/conversations/{}/items/{}", conversation_id, item_id),
            "GET",
            None::<()>,
        )
        .await
    }

    /// Deletes all conversations
    pub async fn delete_conversations(&self) -> Result<ConversationsDeleteResponse> {
        self.authenticated_api_call("/v1/conversations", "DELETE", None::<()>)
            .await
    }

    /// Batch deletes multiple conversations by their IDs
    pub async fn batch_delete_conversations(
        &self,
        ids: Vec<String>,
    ) -> Result<BatchDeleteConversationsResponse> {
        let request = BatchDeleteConversationsRequest { ids };
        self.authenticated_api_call("/v1/conversations/batch-delete", "POST", Some(request))
            .await
    }

    /// Batch updates conversation project assignments. Use `None` to clear the project.
    pub async fn batch_update_conversation_project(
        &self,
        ids: Vec<Uuid>,
        project_id: Option<Uuid>,
    ) -> Result<BatchUpdateConversationProjectResponse> {
        let request = BatchUpdateConversationProjectRequest { ids, project_id };
        self.authenticated_api_call(
            "/v1/conversations/batch-update-project",
            "POST",
            Some(request),
        )
        .await
    }

    /// Creates a new conversation project.
    pub async fn create_conversation_project(
        &self,
        request: ConversationProjectCreateRequest,
    ) -> Result<ConversationProject> {
        self.authenticated_api_call("/v1/conversation-projects", "POST", Some(request))
            .await
    }

    /// Lists conversation projects with pagination.
    pub async fn list_conversation_projects(
        &self,
        params: Option<ConversationProjectListParams>,
    ) -> Result<ConversationProjectsListResponse> {
        let endpoint = build_conversation_projects_endpoint(params.as_ref());
        self.authenticated_api_call(&endpoint, "GET", None::<()>)
            .await
    }

    /// Fetches a single conversation project by UUID.
    pub async fn get_conversation_project(&self, project_id: Uuid) -> Result<ConversationProject> {
        self.authenticated_api_call(
            &format!("/v1/conversation-projects/{}", project_id),
            "GET",
            None::<()>,
        )
        .await
    }

    /// Updates a conversation project and/or its project instructions.
    pub async fn update_conversation_project(
        &self,
        project_id: Uuid,
        request: ConversationProjectUpdateRequest,
    ) -> Result<ConversationProject> {
        if request.is_empty() {
            return Err(Error::Configuration(
                "Conversation project update request must include at least one field".to_string(),
            ));
        }

        self.authenticated_api_call(
            &format!("/v1/conversation-projects/{}", project_id),
            "POST",
            Some(request),
        )
        .await
    }

    /// Deletes a conversation project by UUID.
    pub async fn delete_conversation_project(
        &self,
        project_id: Uuid,
    ) -> Result<DeletedObjectResponse> {
        self.authenticated_api_call(
            &format!("/v1/conversation-projects/{}", project_id),
            "DELETE",
            None::<()>,
        )
        .await
    }

    /// Fetches available AI models
    pub async fn get_models(&self) -> Result<ModelsResponse> {
        self.encrypted_openai_call("/v1/models", "GET", None::<()>)
            .await
    }

    /// Fetches the full model catalog with context windows and capabilities.
    pub async fn get_model_catalog(&self) -> Result<ModelCatalogResponse> {
        self.encrypted_openai_call("/v1/models/catalog", "GET", None::<()>)
            .await
    }

    /// Creates embeddings for the given input text(s)
    ///
    /// # Example
    /// ```ignore
    /// let request = EmbeddingRequest {
    ///     input: "Hello, world!".into(),
    ///     model: "nomic-embed-text".to_string(),
    ///     encoding_format: None,
    ///     dimensions: None,
    ///     user: None,
    /// };
    /// let response = client.create_embeddings(request).await?;
    /// ```
    pub async fn create_embeddings(&self, request: EmbeddingRequest) -> Result<EmbeddingResponse> {
        self.encrypted_openai_call("/v1/embeddings", "POST", Some(request))
            .await
    }

    /// Creates a chat completion (non-streaming)
    pub async fn create_chat_completion(
        &self,
        mut request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        request.stream = Some(false);
        self.encrypted_openai_call("/v1/chat/completions", "POST", Some(request))
            .await
    }

    /// Creates a streaming chat completion
    pub async fn create_chat_completion_stream(
        &self,
        mut request: ChatCompletionRequest,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<ChatCompletionChunk>> + Send>>>
    {
        request.stream = Some(true);
        request.stream_options = Some(StreamOptions {
            include_usage: true,
        });
        use eventsource_stream::{EventStreamError, Eventsource};
        use futures::StreamExt;

        let request = HttpRequest::builder()
            .method(http::Method::POST)
            .uri("/v1/chat/completions")
            .header(header::ACCEPT, "text/event-stream")
            .body(Bytes::from(serde_json::to_vec(&request)?))
            .map_err(|error| {
                Error::Configuration(format!("Failed to build inference request: {error}"))
            })?;
        let response = self.send_inference_request(request).await?;
        let status = response.status();
        if !status.is_success() {
            let body = collect_response_body(response.into_body()).await?;
            return Err(Error::Api {
                status: status.as_u16(),
                message: String::from_utf8_lossy(&body).into_owned(),
            });
        }

        let event_stream = response.into_body().eventsource().filter_map(move |event| {
            async move {
                match event {
                    Ok(event) => {
                        // Check if this is the [DONE] event
                        if event.data == "[DONE]" {
                            return None;
                        }

                        match serde_json::from_str::<ChatCompletionChunk>(&event.data) {
                            Ok(chunk) => Some(Ok(chunk)),
                            Err(error) => Some(Err(Error::InvalidResponse(format!(
                                "Failed to parse authenticated completion chunk: {error}"
                            )))),
                        }
                    }
                    Err(EventStreamError::Transport(error)) => Some(Err(error)),
                    Err(error) => Some(Err(Error::InvalidResponse(format!(
                        "Failed to parse authenticated completion SSE: {error}"
                    )))),
                }
            }
        });

        Ok(Box::pin(event_stream))
    }

    // Web API Methods

    /// Searches the public web through OpenSecret's configured search provider.
    pub async fn web_search(&self, request: WebSearchRequest) -> Result<WebSearchResponse> {
        self.authenticated_api_call("/v1/web/search", "POST", Some(request))
            .await
    }

    /// Extracts sanitized Markdown from public URLs through OpenSecret's configured provider.
    pub async fn web_extract(&self, request: WebExtractRequest) -> Result<WebExtractResponse> {
        self.authenticated_api_call("/v1/web/extract", "POST", Some(request))
            .await
    }

    async fn agent_chat_stream(
        &self,
        endpoint: String,
        input: &str,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<AgentSseEvent>> + Send>>> {
        use eventsource_stream::{EventStreamError, Eventsource};
        use futures::StreamExt;

        let expected_auth_epoch = self.session_manager.get_credential_snapshot()?.auth_epoch;
        let request = AgentChatRequest {
            input: input.to_string(),
        };
        let (session, active_auth_epoch) =
            self.ensure_v2_user_session(&expected_auth_epoch).await?;
        let credentials = self.v2_credentials_for_epoch(&active_auth_epoch)?;
        if !credentials
            .user_session
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &session))
        {
            return Err(Error::Session(
                "Credentials changed before the Agent request was sent".to_string(),
            ));
        }
        let response = self
            .v2_send_on_session(
                &session,
                &endpoint,
                "POST",
                Some(serde_json::to_vec(&request)?),
                vec![HeaderField::new(
                    header::ACCEPT.as_str(),
                    b"text/event-stream".to_vec(),
                )],
                V2SendOptions::bound(ResponseMode::Stream),
            )
            .await?;
        let status = response.status();
        if !status.is_success() {
            if matches!(status.as_u16(), 401 | 403) {
                self.transport_v2.clear_user_session_if(&session)?;
            }
            return Err(Self::v2_api_error_from_response(response).await);
        }

        let event_stream = response
            .into_body()
            .eventsource()
            .filter_map(move |event| async move {
                match event {
                    Ok(event) if event.data == "[DONE]" => None,
                    Ok(event) => {
                        let parsed = match event.event.as_str() {
                            "agent.message" => {
                                serde_json::from_str::<AgentMessageEvent>(&event.data)
                                    .map(AgentSseEvent::Message)
                            }
                            "agent.reaction" => {
                                serde_json::from_str::<AgentReactionEvent>(&event.data)
                                    .map(AgentSseEvent::Reaction)
                            }
                            "agent.typing" => serde_json::from_str::<AgentTypingEvent>(&event.data)
                                .map(AgentSseEvent::Typing),
                            "agent.done" => serde_json::from_str::<AgentDoneEvent>(&event.data)
                                .map(AgentSseEvent::Done),
                            "agent.error" => serde_json::from_str::<AgentErrorEvent>(&event.data)
                                .map(AgentSseEvent::Error),
                            _ => return None,
                        };
                        Some(parsed.map_err(|error| {
                            Error::InvalidResponse(format!(
                                "Failed to parse {} agent event: {error}",
                                event.event
                            ))
                        }))
                    }
                    Err(EventStreamError::Transport(error)) => Some(Err(error)),
                    Err(error) => Some(Err(Error::InvalidResponse(format!(
                        "Failed to parse authenticated agent SSE: {error}"
                    )))),
                }
            });

        Ok(Box::pin(event_stream))
    }

    // Agent API Methods

    /// Fetches the current user's main agent.
    pub async fn get_main_agent(&self) -> Result<MainAgentResponse> {
        self.authenticated_api_call("/v1/agent", "GET", None::<()>)
            .await
    }

    /// Explicitly initializes the current user's main agent.
    pub async fn init_main_agent(
        &self,
        request: InitMainAgentRequest,
    ) -> Result<InitMainAgentResponse> {
        self.authenticated_api_call("/v1/agent/init", "POST", Some(request))
            .await
    }

    /// Deletes the current user's main agent and resets shared agent state.
    pub async fn delete_main_agent(&self) -> Result<DeletedObjectResponse> {
        self.authenticated_api_call("/v1/agent", "DELETE", None::<()>)
            .await
    }

    /// Lists items in the main agent conversation.
    pub async fn list_main_agent_items(
        &self,
        params: Option<AgentItemsListParams>,
    ) -> Result<AgentItemsListResponse> {
        let endpoint = build_agent_items_endpoint("/v1/agent/items", params.as_ref());
        self.authenticated_api_call(&endpoint, "GET", None::<()>)
            .await
    }

    /// Fetches a single item from the main agent conversation.
    pub async fn get_main_agent_item(&self, item_id: Uuid) -> Result<ConversationItem> {
        self.authenticated_api_call(&format!("/v1/agent/items/{}", item_id), "GET", None::<()>)
            .await
    }

    /// Sets or replaces the current user's reaction on a main-agent assistant message item.
    pub async fn set_main_agent_item_reaction(
        &self,
        item_id: Uuid,
        emoji: impl Into<String>,
    ) -> Result<ConversationItem> {
        self.authenticated_api_call(
            &format!("/v1/agent/items/{}/reaction", item_id),
            "POST",
            Some(SetMessageReactionRequest {
                emoji: emoji.into(),
            }),
        )
        .await
    }

    /// Clears the current user's reaction on a main-agent assistant message item.
    pub async fn clear_main_agent_item_reaction(&self, item_id: Uuid) -> Result<ConversationItem> {
        self.authenticated_api_call(
            &format!("/v1/agent/items/{}/reaction", item_id),
            "DELETE",
            None::<()>,
        )
        .await
    }

    /// Sends a message to the main agent and returns a stream of SSE events.
    pub async fn agent_chat(
        &self,
        input: &str,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<AgentSseEvent>> + Send>>> {
        self.agent_chat_stream("/v1/agent/chat".to_string(), input)
            .await
    }

    /// Creates a new subagent for the current user.
    pub async fn create_subagent(
        &self,
        request: CreateSubagentRequest,
    ) -> Result<SubagentResponse> {
        self.authenticated_api_call("/v1/agent/subagents", "POST", Some(request))
            .await
    }

    /// Lists subagents for the current user with pagination and filtering.
    pub async fn list_subagents(
        &self,
        params: Option<ListSubagentsParams>,
    ) -> Result<SubagentListResponse> {
        let endpoint = build_subagents_endpoint(params.as_ref());
        self.authenticated_api_call(&endpoint, "GET", None::<()>)
            .await
    }

    /// Fetches a single subagent by UUID.
    pub async fn get_subagent(&self, id: Uuid) -> Result<SubagentResponse> {
        self.authenticated_api_call(&format!("/v1/agent/subagents/{}", id), "GET", None::<()>)
            .await
    }

    /// Sends a message to a specific subagent and returns a stream of SSE events.
    pub async fn subagent_chat(
        &self,
        id: Uuid,
        input: &str,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<AgentSseEvent>> + Send>>> {
        self.agent_chat_stream(format!("/v1/agent/subagents/{}/chat", id), input)
            .await
    }

    /// Lists items in a subagent conversation.
    pub async fn list_subagent_items(
        &self,
        id: Uuid,
        params: Option<AgentItemsListParams>,
    ) -> Result<AgentItemsListResponse> {
        let endpoint = build_agent_items_endpoint(
            &format!("/v1/agent/subagents/{}/items", id),
            params.as_ref(),
        );
        self.authenticated_api_call(&endpoint, "GET", None::<()>)
            .await
    }

    /// Fetches a single item from a subagent conversation.
    pub async fn get_subagent_item(&self, id: Uuid, item_id: Uuid) -> Result<ConversationItem> {
        self.authenticated_api_call(
            &format!("/v1/agent/subagents/{}/items/{}", id, item_id),
            "GET",
            None::<()>,
        )
        .await
    }

    /// Sets or replaces the current user's reaction on a subagent assistant message item.
    pub async fn set_subagent_item_reaction(
        &self,
        id: Uuid,
        item_id: Uuid,
        emoji: impl Into<String>,
    ) -> Result<ConversationItem> {
        self.authenticated_api_call(
            &format!("/v1/agent/subagents/{}/items/{}/reaction", id, item_id),
            "POST",
            Some(SetMessageReactionRequest {
                emoji: emoji.into(),
            }),
        )
        .await
    }

    /// Clears the current user's reaction on a subagent assistant message item.
    pub async fn clear_subagent_item_reaction(
        &self,
        id: Uuid,
        item_id: Uuid,
    ) -> Result<ConversationItem> {
        self.authenticated_api_call(
            &format!("/v1/agent/subagents/{}/items/{}/reaction", id, item_id),
            "DELETE",
            None::<()>,
        )
        .await
    }

    /// Deletes a subagent by UUID.
    pub async fn delete_subagent(&self, id: Uuid) -> Result<DeletedObjectResponse> {
        self.authenticated_api_call(&format!("/v1/agent/subagents/{}", id), "DELETE", None::<()>)
            .await
    }
}

const fn v2_user_binding_is_fresh_at(expires_at_unix_seconds: u64, now_unix_seconds: u64) -> bool {
    expires_at_unix_seconds > now_unix_seconds.saturating_add(V2_USER_AUTH_RENEWAL_SKEW_SECONDS)
}

fn should_use_anonymous_models_session(path: &str, has_user_credentials: bool) -> bool {
    path == "/v1/models" && !has_user_credentials
}

fn decode_v2_user_binding_response<U: DeserializeOwned>(
    response_body: &[u8],
) -> Result<(LoginResponse, U, ValidatedUserTokenPair)> {
    let login: LoginResponse = serde_json::from_slice(response_body)?;
    let validated = validate_v2_user_token_pair(&login.access_token, &login.refresh_token)?;
    if validated.principal != login.id.to_string() {
        return Err(Error::Authentication(
            "Transport v2 authentication response principal did not match its descriptors"
                .to_string(),
        ));
    }
    let value = serde_json::from_slice(response_body)?;
    Ok((login, value, validated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    const USER_ACCESS_AUDIENCE: &str =
        "urn:opensecret:internal:transport-v2:user:access-descriptor";
    const USER_RESUMPTION_AUDIENCE: &str = "urn:opensecret:internal:transport-v2:user:resumption";

    fn descriptor(audience: &str, kind: &str, subject: &str) -> String {
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
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[test]
    fn cache_namespace_root_is_canonical_cloneable_and_redacted() {
        let root = TransportV2CacheNamespaceRoot::from_bytes([0x42; 32]);
        let clone = root.clone();
        let encoded = root.to_base64();
        assert_eq!(
            TransportV2CacheNamespaceRoot::from_base64(&encoded).unwrap(),
            root
        );
        assert_eq!(clone, root);
        assert_eq!(
            format!("{root:?}"),
            "TransportV2CacheNamespaceRoot([REDACTED])"
        );
        assert!(TransportV2CacheNamespaceRoot::from_base64(encoded.trim_end_matches('=')).is_err());
        assert!(TransportV2CacheNamespaceRoot::from_base64(&BASE64.encode([0_u8; 31])).is_err());
    }

    #[test]
    fn inference_response_mode_reads_only_one_boolean_stream_selector() {
        assert_eq!(
            inference_response_mode("/v1/chat/completions", br#"{"stream":true,"model":"x"}"#)
                .unwrap(),
            ResponseMode::Stream
        );
        for body in [
            br#"{"model":"x"}"#.as_slice(),
            br#"{"stream":false}"#.as_slice(),
            br#"{"stream":true,"stream":false}"#.as_slice(),
            br#"{"stream":"true"}"#.as_slice(),
            b"not-json".as_slice(),
        ] {
            assert_eq!(
                inference_response_mode("/v1/chat/completions", body).unwrap(),
                ResponseMode::Unary
            );
        }
        assert_eq!(
            inference_response_mode("/v1/embeddings", br#"{"stream":true}"#).unwrap(),
            ResponseMode::Unary
        );
    }

    #[test]
    fn inference_header_filter_removes_every_credential_and_framing_alias() {
        let mut input = HttpHeaderMap::new();
        for name in [
            "authorization",
            "cookie",
            "x-session-id",
            "content-length",
            "content-type",
            "content-encoding",
            "accept-encoding",
            "content-md5",
            "digest",
            "x-api-key",
            "api-key",
            "x-openai-api-key",
            "x-tinfoil-api-key",
            "x-goog-api-key",
            "x-anthropic-api-key",
            "openai-organization",
            "openai-project",
        ] {
            input.insert(
                http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                http::HeaderValue::from_static("secret"),
            );
        }
        input.insert("x-provider-beta", http::HeaderValue::from_static("safe"));
        input.insert("connection", http::HeaderValue::from_static("x-nominated"));
        input.insert("x-nominated", http::HeaderValue::from_static("remove"));

        let output = sanitize_inference_request_headers(&input);
        assert_eq!(output.len(), 1);
        assert_eq!(output.get("x-provider-beta").unwrap(), "safe");
    }

    #[test]
    fn explicit_allowed_inference_surface_is_unchanged() {
        for (method, path) in [
            (http::Method::GET, "/v1/models"),
            (http::Method::GET, "/v1/models/catalog"),
            (http::Method::POST, "/v1/chat/completions"),
            (http::Method::POST, "/v1/embeddings"),
            (http::Method::POST, "/v1/audio/speech"),
            (http::Method::POST, "/v1/audio/transcriptions"),
        ] {
            assert!(is_allowed_inference_endpoint(&method, path));
        }
        assert!(!is_allowed_inference_endpoint(
            &http::Method::POST,
            "/v1/responses"
        ));
        assert!(!is_allowed_inference_endpoint(
            &http::Method::GET,
            "/v1/chat/completions"
        ));
    }

    #[test]
    fn models_are_anonymous_only_when_no_authority_is_available() {
        assert!(should_use_anonymous_models_session("/v1/models", false));
        assert!(!should_use_anonymous_models_session("/v1/models", true));
        assert!(!should_use_anonymous_models_session(
            "/v1/models/catalog",
            false
        ));
    }

    #[test]
    fn user_binding_renews_before_its_authenticated_deadline() {
        assert!(v2_user_binding_is_fresh_at(1_031, 1_000));
        assert!(!v2_user_binding_is_fresh_at(1_030, 1_000));
        assert!(!v2_user_binding_is_fresh_at(1_029, 1_000));
        assert!(!v2_user_binding_is_fresh_at(u64::MAX, u64::MAX));
    }

    #[test]
    fn successful_binding_response_is_validated_before_session_install() {
        let user_id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let access = descriptor(
            USER_ACCESS_AUDIENCE,
            "access_descriptor",
            &user_id.to_string(),
        );
        let refresh = descriptor(USER_RESUMPTION_AUDIENCE, "resumption", &user_id.to_string());
        let body = serde_json::json!({
            "id": user_id,
            "access_token": access,
            "refresh_token": refresh,
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let (login, value, validated): (LoginResponse, LoginResponse, ValidatedUserTokenPair) =
            decode_v2_user_binding_response(&bytes).unwrap();
        assert_eq!(login.id, user_id);
        assert_eq!(value.id, user_id);
        assert_eq!(validated.principal, user_id.to_string());

        let invalid = serde_json::to_vec(&serde_json::json!({
            "id": user_id,
            "access_token": "legacy-access",
            "refresh_token": "legacy-refresh",
        }))
        .unwrap();
        assert!(decode_v2_user_binding_response::<LoginResponse>(&invalid).is_err());
    }

    #[test]
    fn legacy_credentials_require_a_fresh_v2_login() {
        let client = OpenSecretClient::new("http://localhost:3000").unwrap();
        assert!(client
            .set_tokens(
                "legacy-access".to_string(),
                Some("legacy-refresh".to_string())
            )
            .is_err());
        assert!(client.get_tokens().unwrap().is_none());

        let access = descriptor(USER_ACCESS_AUDIENCE, "access_descriptor", "user-1");
        let refresh = descriptor(USER_RESUMPTION_AUDIENCE, "resumption", "user-1");
        client
            .set_tokens(access.clone(), Some(refresh.clone()))
            .unwrap();
        assert_eq!(client.get_access_token().unwrap(), Some(access));
        assert_eq!(client.get_refresh_token().unwrap(), Some(refresh));
    }

    #[tokio::test]
    async fn operation_start_epoch_cannot_cross_to_a_new_principal() {
        let client = OpenSecretClient::new("http://localhost:3000").unwrap();
        let initial = client
            .session_manager
            .get_credential_snapshot()
            .unwrap()
            .auth_epoch;
        let user_a = "123e4567-e89b-12d3-a456-426614174001";
        let user_b = "123e4567-e89b-12d3-a456-426614174002";
        let user_a_epoch = client
            .session_manager
            .replace_user_tokens_and_session_if_epoch(
                &initial,
                descriptor(USER_ACCESS_AUDIENCE, "access_descriptor", user_a),
                Some(descriptor(USER_RESUMPTION_AUDIENCE, "resumption", user_a)),
                user_a.to_string(),
                Arc::new(
                    V2Session::from_master_for_test(
                        Uuid::from_bytes([0x41; 16]),
                        [0x41; 32],
                        u64::MAX,
                    )
                    .unwrap(),
                ),
            )
            .unwrap()
            .expect("install user A");

        let user_b_epoch = client
            .session_manager
            .replace_user_tokens(
                descriptor(USER_ACCESS_AUDIENCE, "access_descriptor", user_b),
                Some(descriptor(USER_RESUMPTION_AUDIENCE, "resumption", user_b)),
                Some(user_b.to_string()),
            )
            .unwrap();
        client
            .session_manager
            .replace_user_tokens_and_session_if_epoch(
                &user_b_epoch,
                descriptor(USER_ACCESS_AUDIENCE, "access_descriptor", user_b),
                Some(descriptor(USER_RESUMPTION_AUDIENCE, "resumption", user_b)),
                user_b.to_string(),
                Arc::new(
                    V2Session::from_master_for_test(
                        Uuid::from_bytes([0x42; 16]),
                        [0x42; 32],
                        u64::MAX,
                    )
                    .unwrap(),
                ),
            )
            .unwrap()
            .expect("install user B");

        let error = match client.ensure_v2_user_session(&user_a_epoch).await {
            Ok(_) => panic!("stale operation must not acquire user B's session"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Session(_)));
        assert_eq!(
            client
                .session_manager
                .get_credential_snapshot()
                .unwrap()
                .auth_epoch
                .principal
                .as_deref(),
            Some(user_b)
        );
    }

    #[test]
    fn api_key_operation_start_generation_fails_closed_after_authority_change() {
        let client =
            OpenSecretClient::new_with_api_key("http://localhost:3000", "api-key-a".to_string())
                .unwrap();
        let generation = client
            .session_manager
            .get_credential_snapshot()
            .unwrap()
            .generation;
        client.set_api_key("api-key-b".to_string()).unwrap();

        assert!(matches!(
            client.ensure_credential_generation(generation),
            Err(Error::Session(_))
        ));
    }
}
