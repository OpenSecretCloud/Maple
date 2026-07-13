use crate::{
    attestation::{AttestationDocument, AttestationVerifier},
    cbor::{self, Value as CborValue},
    crypto::{self},
    error::{Error, Result},
    session::SessionManager,
    types::*,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use http::{header, HeaderMap as HttpHeaderMap, Request as HttpRequest, Response as HttpResponse};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Client,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    pin::Pin,
    sync::{Arc, RwLock},
};
use uuid::Uuid;

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedBody {
    encrypted: String,
}

const MAX_INFERENCE_SSE_LINE_BYTES: usize = 16 * 1024 * 1024;

pub struct OpenSecretClient {
    client: Client,
    base_url: String,
    session_manager: SessionManager,
    use_mock_attestation: bool,
    server_public_key: Arc<RwLock<Option<Vec<u8>>>>, // Store server's public key from attestation
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthHeaderMode {
    None,
    Jwt,
    ApiKeyOrJwt,
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
                && name.as_str() != "x-session-id"
                && *name != header::CONTENT_LENGTH
                && *name != header::CONTENT_TYPE
                && *name != header::CONTENT_ENCODING
                && *name != header::ACCEPT_ENCODING
                && name.as_str() != "content-md5"
                && name.as_str() != "digest"
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

fn is_event_stream(headers: &HttpHeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"))
}

fn transform_sse_line(line: Bytes, session_key: &[u8; 32]) -> Result<Bytes> {
    let (content, line_ending) = if line.ends_with(b"\r\n") {
        (&line[..line.len() - 2], &b"\r\n"[..])
    } else if line.ends_with(b"\n") {
        (&line[..line.len() - 1], &b"\n"[..])
    } else {
        (&line[..], &b""[..])
    };

    let Some(mut payload) = content.strip_prefix(b"data:") else {
        return Ok(line);
    };
    let prefix_len = if payload.starts_with(b" ") {
        payload = &payload[1..];
        6
    } else {
        5
    };

    if payload == b"[DONE]" {
        return Ok(line);
    }

    if payload.is_empty() {
        return Ok(line);
    }

    // OpenSecret encrypts every normal data event. A base64-shaped payload
    // that fails authentication is corrupt transport data, not plaintext.
    let encrypted = match BASE64.decode(payload) {
        Ok(encrypted) => encrypted,
        Err(_) => return Ok(line),
    };
    if encrypted.len() < 28 {
        return Ok(line);
    }
    let decrypted = crypto::decrypt_data(session_key, &encrypted)
        .map_err(|error| Error::Decryption(format!("Failed to decrypt SSE data: {error}")))?;

    let mut transformed = BytesMut::with_capacity(prefix_len + decrypted.len() + line_ending.len());
    transformed.extend_from_slice(&content[..prefix_len]);
    transformed.extend_from_slice(&decrypted);
    transformed.extend_from_slice(line_ending);
    Ok(transformed.freeze())
}

fn decrypt_sse_stream(
    source: OpenSecretResponseBody,
    session_key: [u8; 32],
) -> OpenSecretResponseBody {
    decrypt_sse_stream_with_line_limit(source, session_key, MAX_INFERENCE_SSE_LINE_BYTES)
}

fn decrypt_sse_stream_with_line_limit(
    mut source: OpenSecretResponseBody,
    session_key: [u8; 32],
    max_line_bytes: usize,
) -> OpenSecretResponseBody {
    let stream = async_stream::try_stream! {
        let mut buffered = BytesMut::new();

        while let Some(chunk) = source.next().await {
            buffered.extend_from_slice(&chunk?);
            while let Some(line_end) = buffered.iter().position(|byte| *byte == b'\n') {
                let line_len = line_end + 1;
                if line_len > max_line_bytes {
                    Err(Error::InvalidResponse(format!(
                        "Inference SSE line exceeds {max_line_bytes}-byte limit"
                    )))?;
                }
                let line = buffered.split_to(line_len).freeze();
                yield transform_sse_line(line, &session_key)?;
            }
            if buffered.len() > max_line_bytes {
                Err(Error::InvalidResponse(format!(
                    "Inference SSE line exceeds {max_line_bytes}-byte limit"
                )))?;
            }
        }

        if !buffered.is_empty() {
            yield transform_sse_line(buffered.freeze(), &session_key)?;
        }
    };

    Box::pin(stream)
}

fn decrypt_sse_body(response: reqwest::Response, session_key: [u8; 32]) -> OpenSecretResponseBody {
    let source = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(Error::Http));
    decrypt_sse_stream(Box::pin(source), session_key)
}

async fn collect_response_body(mut body: OpenSecretResponseBody) -> Result<Bytes> {
    let mut collected = BytesMut::new();
    while let Some(chunk) = body.next().await {
        collected.extend_from_slice(&chunk?);
    }
    Ok(collected.freeze())
}

impl OpenSecretClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into();
        let use_mock = base_url.contains("localhost")
            || base_url.contains("127.0.0.1")
            || base_url.contains("0.0.0.0")
            || base_url.contains("10.0.2.2");

        Ok(Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            session_manager: SessionManager::new(),
            use_mock_attestation: use_mock,
            server_public_key: Arc::new(RwLock::new(None)),
        })
    }

    pub fn new_with_api_key(base_url: impl Into<String>, api_key: String) -> Result<Self> {
        let base_url = base_url.into();
        let use_mock = base_url.contains("localhost")
            || base_url.contains("127.0.0.1")
            || base_url.contains("0.0.0.0")
            || base_url.contains("10.0.2.2");

        Ok(Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            session_manager: SessionManager::new_with_api_key(api_key),
            use_mock_attestation: use_mock,
            server_public_key: Arc::new(RwLock::new(None)),
        })
    }

    pub fn set_api_key(&self, api_key: String) -> Result<()> {
        self.session_manager.set_api_key(api_key)
    }

    pub fn clear_api_key(&self) -> Result<()> {
        self.session_manager.clear_api_key()
    }

    pub async fn perform_attestation_handshake(&self) -> Result<()> {
        // Generate a nonce
        let nonce = Uuid::new_v4().to_string();

        // Step 1: Get attestation document
        let attestation_doc = self.get_attestation_document(&nonce).await?;

        // Step 2: Parse and verify attestation document
        let doc = if !self.use_mock_attestation {
            let verifier = AttestationVerifier::new();
            verifier.verify_attestation_document(&attestation_doc.attestation_document, &nonce)?
        } else {
            // For mock mode, extract without full verification
            self.parse_mock_attestation(&attestation_doc.attestation_document)?
        };

        // Store server's public key from attestation document
        if let Some(pub_key) = doc.public_key {
            *self.server_public_key.write().map_err(|e| {
                Error::KeyExchange(format!("Failed to write server public key: {}", e))
            })? = Some(pub_key);
        } else {
            return Err(Error::AttestationVerificationFailed(
                "No public key in attestation document".to_string(),
            ));
        }

        // Step 3: Perform key exchange
        self.perform_key_exchange(&nonce).await?;

        Ok(())
    }

    async fn get_attestation_document(&self, nonce: &str) -> Result<AttestationResponse> {
        let url = format!("{}/attestation/{}", self.base_url, nonce);

        let response = self.client.get(&url).send().await?;

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

        response.json().await.map_err(Into::into)
    }

    async fn perform_key_exchange(&self, nonce: &str) -> Result<()> {
        // Generate ephemeral keypair
        let (secret, public_key) = crypto::generate_static_keypair();
        let public_key_bytes = public_key.as_bytes();
        let public_key_b64 = BASE64.encode(public_key_bytes);

        // Send key exchange request
        let url = format!("{}/key_exchange", self.base_url);
        let body = KeyExchangeRequest {
            client_public_key: public_key_b64,
            nonce: nonce.to_string(),
        };

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

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

        let key_exchange_response: KeyExchangeResponse = response.json().await?;

        // Get server's public key from attestation
        let server_public_key_bytes = self
            .server_public_key
            .read()
            .map_err(|e| Error::KeyExchange(format!("Failed to read server public key: {}", e)))?;
        let server_public_key_bytes = server_public_key_bytes
            .as_ref()
            .ok_or_else(|| Error::KeyExchange("Server public key not available".to_string()))?;

        // Convert server's public key bytes to x25519 PublicKey
        let server_public_key = x25519_dalek::PublicKey::from(
            <[u8; 32]>::try_from(server_public_key_bytes.as_slice())
                .map_err(|_| Error::KeyExchange("Invalid server public key length".to_string()))?,
        );

        // Perform ECDH to get shared secret
        let shared_secret = crypto::perform_static_key_exchange(&secret, &server_public_key);

        // Decrypt the session key
        let session_key = crypto::decrypt_session_key(
            &shared_secret,
            &key_exchange_response.encrypted_session_key,
        )?;

        // Parse session_id as UUID
        let session_id = Uuid::parse_str(&key_exchange_response.session_id)
            .map_err(|e| Error::Session(format!("Invalid session ID format: {}", e)))?;

        self.session_manager.set_session(session_id, session_key)?;

        Ok(())
    }

    pub fn get_session_id(&self) -> Result<Option<Uuid>> {
        Ok(self.session_manager.get_session()?.map(|s| s.session_id))
    }

    fn parse_mock_attestation(&self, document_b64: &str) -> Result<AttestationDocument> {
        // For mock/dev mode, just extract the essential fields without full verification
        let document_bytes = BASE64.decode(document_b64)?;
        let cbor_value: CborValue = cbor::from_slice(&document_bytes)?;

        // Parse COSE_Sign1 structure
        let cose_sign1 = match &cbor_value {
            CborValue::Array(arr) if arr.len() == 4 => arr,
            _ => {
                return Err(Error::AttestationVerificationFailed(
                    "Invalid COSE_Sign1 structure".to_string(),
                ))
            }
        };

        // Extract payload
        let payload = match &cose_sign1[2] {
            CborValue::Bytes(b) => b,
            _ => {
                return Err(Error::AttestationVerificationFailed(
                    "Invalid payload".to_string(),
                ))
            }
        };

        // Parse attestation document from payload
        let doc_cbor: CborValue = cbor::from_slice(payload)?;
        let map = match &doc_cbor {
            CborValue::Map(m) => m,
            _ => {
                return Err(Error::AttestationVerificationFailed(
                    "Invalid attestation document format".to_string(),
                ))
            }
        };

        // Extract public key (required for key exchange)
        let mut public_key = None;
        let mut nonce = None;

        for (key, value) in map {
            if let CborValue::Text(key_str) = key {
                match key_str.as_str() {
                    "public_key" => {
                        public_key = match value {
                            CborValue::Bytes(b) => Some(b.clone()),
                            _ => None,
                        };
                    }
                    "nonce" => {
                        nonce = match value {
                            CborValue::Bytes(b) => Some(b.clone()),
                            _ => None,
                        };
                    }
                    _ => {}
                }
            }
        }

        // Return a minimal AttestationDocument with just what we need
        Ok(AttestationDocument {
            module_id: "mock-module".to_string(),
            timestamp: 0,
            digest: "SHA384".to_string(),
            pcrs: std::collections::HashMap::new(),
            certificate: vec![],
            cabundle: vec![],
            public_key,
            user_data: None,
            nonce,
        })
    }

    pub async fn test_connection(&self) -> Result<String> {
        let url = format!("{}/health-check", self.base_url);
        let response = self.client.get(&url).send().await?;

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

    async fn encrypted_api_call<T: Serialize + Clone, U: DeserializeOwned>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
    ) -> Result<U> {
        self.retry_encrypted_json_call_without_refresh(endpoint, method, data, AuthHeaderMode::None)
            .await
    }

    async fn authenticated_api_call<T: Serialize + Clone, U: DeserializeOwned>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
    ) -> Result<U> {
        self.retry_encrypted_json_call(endpoint, method, data, AuthHeaderMode::Jwt, true)
            .await
    }

    async fn retry_encrypted_json_call_without_refresh<
        T: Serialize + Clone,
        U: DeserializeOwned,
    >(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
        auth_mode: AuthHeaderMode,
    ) -> Result<U> {
        let mut retried_attestation = false;

        loop {
            match self
                .encrypted_json_call_inner(endpoint, method, data.clone(), auth_mode)
                .await
            {
                Ok(result) => return Ok(result),
                Err(error) if !retried_attestation && Self::is_attestation_retryable(&error) => {
                    self.perform_attestation_handshake().await?;
                    retried_attestation = true;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn retry_encrypted_json_call<T: Serialize + Clone, U: DeserializeOwned>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
        auth_mode: AuthHeaderMode,
        allow_refresh: bool,
    ) -> Result<U> {
        let mut retried_attestation = false;
        let mut retried_refresh = false;

        loop {
            match self
                .encrypted_json_call_inner(endpoint, method, data.clone(), auth_mode)
                .await
            {
                Ok(result) => return Ok(result),
                Err(error) if !retried_attestation && Self::is_attestation_retryable(&error) => {
                    self.perform_attestation_handshake().await?;
                    retried_attestation = true;
                }
                Err(Error::Api { status: 401, .. })
                    if allow_refresh && !retried_refresh && !self.using_api_key(auth_mode)? =>
                {
                    self.refresh_token().await?;
                    retried_refresh = true;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn encrypted_json_call_inner<T: Serialize, U: DeserializeOwned>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
        auth_mode: AuthHeaderMode,
    ) -> Result<U> {
        let (response, session_key) = self
            .send_encrypted_request(endpoint, method, data, auth_mode, false)
            .await?;
        let encrypted_response: EncryptedResponse<U> = response.json().await?;
        let decrypted =
            crypto::decrypt_data(&session_key, &BASE64.decode(&encrypted_response.encrypted)?)?;
        let result: U = serde_json::from_slice(&decrypted)?;

        Ok(result)
    }

    /// Sends a lossless encrypted request to an OpenSecret inference endpoint.
    ///
    /// The request URI must be relative and target one of the SDK's explicitly
    /// allowed inference routes. Its method, query string, headers, and body
    /// bytes are otherwise caller-owned. The SDK does not parse the request as
    /// JSON and never adds or changes inference parameters such as `stream`.
    ///
    /// OpenSecret authentication, attestation sessions, and the encrypted
    /// envelope remain SDK-owned. Caller-provided `Host`, `Authorization`,
    /// `x-session-id`, `Content-Length`, `Content-Type`, `Content-Encoding`,
    /// `Accept-Encoding`, `Content-MD5`, `Digest`, and hop-by-hop headers
    /// (including fields named by `Connection`) are therefore not forwarded.
    /// Other headers are preserved.
    ///
    /// The returned HTTP response preserves the final OpenSecret status and
    /// safe response headers. Its body is decrypted raw bytes. SSE framing is
    /// preserved while encrypted `data:` fields are decrypted without parsing
    /// their contents as completion JSON. Individual SSE lines are limited to
    /// 16 MiB.
    pub async fn send_inference_request(
        &self,
        request: InferenceRequest,
    ) -> Result<InferenceResponse> {
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

        let path_and_query = parts
            .uri
            .path_and_query()
            .ok_or_else(|| Error::Configuration("Inference request URI has no path".to_string()))?
            .as_str()
            .to_string();
        let headers = sanitize_inference_request_headers(&parts.headers);
        let mut retried_attestation = false;
        let mut retried_refresh = false;

        loop {
            let result = self
                .send_inference_request_once(&parts.method, &path_and_query, &headers, body.clone())
                .await;

            match result {
                // OpenSecret currently uses the same 400 for stale sessions and
                // pre-provider validation. Preserve the legacy one-retry behavior
                // until the backend exposes an explicit re-attestation signal.
                Ok((response, _session_key))
                    if response.status() == reqwest::StatusCode::BAD_REQUEST
                        && !retried_attestation =>
                {
                    self.perform_attestation_handshake().await?;
                    retried_attestation = true;
                }
                Ok((response, session_key))
                    if response.status() == reqwest::StatusCode::UNAUTHORIZED
                        && !retried_refresh
                        && !self.using_api_key(AuthHeaderMode::ApiKeyOrJwt)? =>
                {
                    if self.refresh_token().await.is_ok() {
                        retried_refresh = true;
                    } else {
                        return self.finish_inference_response(response, session_key).await;
                    }
                }
                Ok((response, session_key)) => {
                    return self.finish_inference_response(response, session_key).await
                }
                Err(error) if !retried_attestation && Self::is_attestation_retryable(&error) => {
                    self.perform_attestation_handshake().await?;
                    retried_attestation = true;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn send_inference_request_once(
        &self,
        method: &http::Method,
        path_and_query: &str,
        caller_headers: &HttpHeaderMap,
        body: Bytes,
    ) -> Result<(reqwest::Response, [u8; 32])> {
        let session = self.session_manager.get_session()?.ok_or_else(|| {
            Error::Session(
                "No active session. Call perform_attestation_handshake first".to_string(),
            )
        })?;
        let url = format!("{}{}", self.base_url, path_and_query);
        let mut headers = caller_headers.clone();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-session-id",
            HeaderValue::from_str(&session.session_id.to_string())
                .map_err(|error| Error::Session(format!("Invalid session ID: {error}")))?,
        );
        if let Some(token) = self.resolve_auth_token(AuthHeaderMode::ApiKeyOrJwt)? {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}")).map_err(|error| {
                    Error::Authentication(format!(
                        "Invalid authorization credential format: {error}"
                    ))
                })?,
            );
        }

        let encrypted_body = if body.is_empty() {
            None
        } else {
            let encrypted = crypto::encrypt_data(&session.session_key, &body)?;
            Some(EncryptedRequest {
                encrypted: BASE64.encode(encrypted),
            })
        };
        let request = self.client.request(method.clone(), url).headers(headers);
        let response = match encrypted_body {
            Some(encrypted_body) => request.json(&encrypted_body).send().await?,
            None => request.send().await?,
        };

        Ok((response, session.session_key))
    }

    async fn finish_inference_response(
        &self,
        response: reqwest::Response,
        session_key: [u8; 32],
    ) -> Result<HttpResponse<OpenSecretResponseBody>> {
        let status = response.status();
        let headers = sanitize_inference_response_headers(response.headers());
        let body: OpenSecretResponseBody = if is_event_stream(&headers) {
            decrypt_sse_body(response, session_key)
        } else {
            let raw_body = response.bytes().await?;
            let decrypted_body = match serde_json::from_slice::<EncryptedBody>(&raw_body) {
                Ok(encrypted_body) => {
                    let encrypted = BASE64.decode(encrypted_body.encrypted)?;
                    Bytes::from(crypto::decrypt_data(&session_key, &encrypted)?)
                }
                Err(_) if status.is_success() && !raw_body.is_empty() => {
                    return Err(Error::InvalidResponse(
                        "Successful inference response did not contain an encrypted body"
                            .to_string(),
                    ));
                }
                Err(_) => raw_body,
            };
            Box::pin(futures::stream::once(async move { Ok(decrypted_body) }))
        };

        let mut result = HttpResponse::builder()
            .status(status)
            .body(body)
            .map_err(|error| {
                Error::InvalidResponse(format!("Failed to construct inference response: {error}"))
            })?;
        *result.headers_mut() = headers;
        Ok(result)
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

    async fn retry_encrypted_stream_call<T: Serialize + Clone>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
        auth_mode: AuthHeaderMode,
        allow_refresh: bool,
    ) -> Result<(reqwest::Response, [u8; 32])> {
        let mut retried_attestation = false;
        let mut retried_refresh = false;

        loop {
            match self
                .send_encrypted_request(endpoint, method, data.clone(), auth_mode, true)
                .await
            {
                Ok(response) => return Ok(response),
                Err(error) if !retried_attestation && Self::is_attestation_retryable(&error) => {
                    self.perform_attestation_handshake().await?;
                    retried_attestation = true;
                }
                Err(Error::Api { status: 401, .. })
                    if allow_refresh && !retried_refresh && !self.using_api_key(auth_mode)? =>
                {
                    self.refresh_token().await?;
                    retried_refresh = true;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn send_encrypted_request<T: Serialize>(
        &self,
        endpoint: &str,
        method: &str,
        data: Option<T>,
        auth_mode: AuthHeaderMode,
        accept_sse: bool,
    ) -> Result<(reqwest::Response, [u8; 32])> {
        let session = self.session_manager.get_session()?.ok_or_else(|| {
            Error::Session(
                "No active session. Call perform_attestation_handshake first".to_string(),
            )
        })?;

        let url = format!("{}{}", self.base_url, endpoint);

        let encrypted_body = if let Some(data) = data {
            let json = serde_json::to_string(&data)?;
            let encrypted = crypto::encrypt_data(&session.session_key, json.as_bytes())?;
            Some(EncryptedRequest {
                encrypted: BASE64.encode(&encrypted),
            })
        } else {
            None
        };

        let headers = self.build_encrypted_headers(&session, auth_mode, accept_sse)?;
        let request_builder = match method {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            "PUT" => self.client.put(&url),
            "DELETE" => self.client.delete(&url),
            _ => {
                return Err(Error::Api {
                    status: 0,
                    message: format!("Unsupported HTTP method: {}", method),
                })
            }
        };

        let request_builder = request_builder.headers(headers);
        let response = if let Some(body) = encrypted_body {
            request_builder.json(&body).send().await?
        } else {
            request_builder.send().await?
        };

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_msg = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Error::Api {
                status,
                message: error_msg,
            });
        }

        Ok((response, session.session_key))
    }

    fn build_encrypted_headers(
        &self,
        session: &crate::types::SessionState,
        auth_mode: AuthHeaderMode,
        accept_sse: bool,
    ) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if accept_sse {
            headers.insert("accept", HeaderValue::from_static("text/event-stream"));
        }

        headers.insert(
            "x-session-id",
            HeaderValue::from_str(&session.session_id.to_string())
                .map_err(|e| Error::Session(format!("Invalid session ID: {}", e)))?,
        );

        if let Some(token) = self.resolve_auth_token(auth_mode)? {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token)).map_err(|e| {
                    Error::Authentication(format!("Invalid authorization credential format: {}", e))
                })?,
            );
        }

        Ok(headers)
    }

    fn resolve_auth_token(&self, auth_mode: AuthHeaderMode) -> Result<Option<String>> {
        match auth_mode {
            AuthHeaderMode::None => Ok(None),
            AuthHeaderMode::Jwt => self.session_manager.get_access_token(),
            AuthHeaderMode::ApiKeyOrJwt => {
                if let Some(api_key) = self.session_manager.get_api_key()? {
                    Ok(Some(api_key))
                } else {
                    self.session_manager.get_access_token()
                }
            }
        }
    }

    fn using_api_key(&self, auth_mode: AuthHeaderMode) -> Result<bool> {
        match auth_mode {
            AuthHeaderMode::ApiKeyOrJwt => Ok(self.session_manager.get_api_key()?.is_some()),
            _ => Ok(false),
        }
    }

    fn is_attestation_retryable(error: &Error) -> bool {
        matches!(
            error,
            Error::Session(_)
                | Error::Api { status: 400, .. }
                | Error::Encryption(_)
                | Error::Decryption(_)
        )
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

        let response: LoginResponse = self
            .encrypted_api_call("/login", "POST", Some(credentials))
            .await?;

        // Store the tokens
        self.session_manager.set_tokens(
            response.access_token.clone(),
            Some(response.refresh_token.clone()),
        )?;

        Ok(response)
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

        let response: LoginResponse = self
            .encrypted_api_call("/login", "POST", Some(credentials))
            .await?;

        // Store the tokens
        self.session_manager.set_tokens(
            response.access_token.clone(),
            Some(response.refresh_token.clone()),
        )?;

        Ok(response)
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

        let response: LoginResponse = self
            .encrypted_api_call("/register", "POST", Some(credentials))
            .await?;

        // Store the tokens
        self.session_manager.set_tokens(
            response.access_token.clone(),
            Some(response.refresh_token.clone()),
        )?;

        Ok(response)
    }

    pub async fn register_guest(&self, password: String, client_id: Uuid) -> Result<LoginResponse> {
        let credentials = RegisterCredentials {
            email: None,
            name: None,
            password,
            client_id,
        };

        let response: LoginResponse = self
            .encrypted_api_call("/register", "POST", Some(credentials))
            .await?;

        // Store the tokens
        self.session_manager.set_tokens(
            response.access_token.clone(),
            Some(response.refresh_token.clone()),
        )?;

        Ok(response)
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

        let response: LoginResponse = self
            .encrypted_api_call("/auth/github/callback", "POST", Some(request))
            .await?;

        self.session_manager.set_tokens(
            response.access_token.clone(),
            Some(response.refresh_token.clone()),
        )?;

        Ok(response)
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

        let response: LoginResponse = self
            .encrypted_api_call("/auth/google/callback", "POST", Some(request))
            .await?;

        self.session_manager.set_tokens(
            response.access_token.clone(),
            Some(response.refresh_token.clone()),
        )?;

        Ok(response)
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

        let response: LoginResponse = self
            .encrypted_api_call("/auth/apple/callback", "POST", Some(request))
            .await?;

        self.session_manager.set_tokens(
            response.access_token.clone(),
            Some(response.refresh_token.clone()),
        )?;

        Ok(response)
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

        let response: LoginResponse = self
            .encrypted_api_call("/auth/apple/native", "POST", Some(request))
            .await?;

        self.session_manager.set_tokens(
            response.access_token.clone(),
            Some(response.refresh_token.clone()),
        )?;

        Ok(response)
    }

    pub async fn refresh_token(&self) -> Result<()> {
        let refresh_token = self
            .session_manager
            .get_refresh_token()?
            .ok_or_else(|| Error::Authentication("No refresh token available".to_string()))?;

        let request = RefreshRequest { refresh_token };

        let response: RefreshResponse = self
            .encrypted_api_call("/refresh", "POST", Some(request))
            .await?;

        self.session_manager
            .set_tokens(response.access_token, Some(response.refresh_token))?;

        Ok(())
    }

    async fn logout_inner(&self, push_device_id: Option<Uuid>) -> Result<()> {
        let refresh_token = self
            .session_manager
            .get_refresh_token()?
            .ok_or_else(|| Error::Authentication("No refresh token available".to_string()))?;

        let request = LogoutRequest {
            refresh_token,
            push_device_id,
        };

        let _: serde_json::Value = self
            .encrypted_api_call("/logout", "POST", Some(request))
            .await?;

        // Clear all session data
        self.session_manager.clear_all()?;

        Ok(())
    }

    pub async fn logout(&self) -> Result<()> {
        self.logout_inner(None).await
    }

    pub async fn logout_with_push_device_id(&self, push_device_id: Uuid) -> Result<()> {
        self.logout_inner(Some(push_device_id)).await
    }

    pub fn get_access_token(&self) -> Result<Option<String>> {
        self.session_manager.get_access_token()
    }

    pub fn get_refresh_token(&self) -> Result<Option<String>> {
        self.session_manager.get_refresh_token()
    }

    pub fn set_tokens(&self, access_token: String, refresh_token: Option<String>) -> Result<()> {
        self.session_manager.clear_session()?;
        self.session_manager.set_tokens(access_token, refresh_token)
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
        let response: CredentialUpdateResponse = self
            .authenticated_api_call("/protected/change_password", "POST", Some(request))
            .await?;
        if let Some(access_token) = response.access_token {
            let refresh_token = match response.refresh_token {
                Some(refresh_token) => Some(refresh_token),
                None => self.session_manager.get_refresh_token()?,
            };
            self.session_manager
                .set_tokens(access_token, refresh_token)?;
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
        let _: serde_json::Value = self
            .authenticated_api_call("/protected/delete-account/confirm", "POST", Some(request))
            .await?;
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
        use eventsource_stream::Eventsource;
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

        let stream = response
            .into_body()
            .map(|result| result.map_err(std::io::Error::other));

        let event_stream = stream.eventsource().filter_map(move |event| {
            async move {
                match event {
                    Ok(event) => {
                        // Check if this is the [DONE] event
                        if event.data == "[DONE]" {
                            return None;
                        }

                        match serde_json::from_str::<ChatCompletionChunk>(&event.data) {
                            Ok(chunk) => Some(Ok(chunk)),
                            Err(error) => Some(Err(Error::Api {
                                status: 0,
                                message: format!("Failed to parse chunk: {error}"),
                            })),
                        }
                    }
                    Err(error) => Some(Err(Error::Api {
                        status: 0,
                        message: format!("SSE error: {error}"),
                    })),
                }
            }
        });

        Ok(Box::pin(event_stream))
    }

    async fn agent_chat_stream(
        &self,
        endpoint: String,
        input: &str,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<AgentSseEvent>> + Send>>> {
        use eventsource_stream::Eventsource;
        use futures::StreamExt;

        let request = AgentChatRequest {
            input: input.to_string(),
        };

        let (response, session_key) = self
            .retry_encrypted_stream_call(
                &endpoint,
                "POST",
                Some(request),
                AuthHeaderMode::Jwt,
                true,
            )
            .await?;

        let stream = response
            .bytes_stream()
            .map(|result| result.map_err(std::io::Error::other));

        let event_stream = stream.eventsource().filter_map(move |event| {
            let session_key = session_key;
            async move {
                match event {
                    Ok(event) => {
                        if event.data == "[DONE]" {
                            return None;
                        }

                        // Skip non-base64 events (heartbeats, retries, etc.)
                        let encrypted_bytes = match BASE64.decode(&event.data) {
                            Ok(bytes) => bytes,
                            Err(_) => return None,
                        };
                        match crypto::decrypt_data(&session_key, &encrypted_bytes) {
                            Ok(decrypted) => match String::from_utf8(decrypted) {
                                Ok(json_str) => {
                                    let event_type = event.event.as_str();
                                    match event_type {
                                        "agent.message" => {
                                            match serde_json::from_str::<AgentMessageEvent>(
                                                &json_str,
                                            ) {
                                                Ok(msg) => Some(Ok(AgentSseEvent::Message(msg))),
                                                Err(e) => Some(Err(Error::Api {
                                                    status: 0,
                                                    message: format!(
                                                        "Failed to parse agent message: {}",
                                                        e
                                                    ),
                                                })),
                                            }
                                        }
                                        "agent.reaction" => {
                                            match serde_json::from_str::<AgentReactionEvent>(
                                                &json_str,
                                            ) {
                                                Ok(reaction) => {
                                                    Some(Ok(AgentSseEvent::Reaction(reaction)))
                                                }
                                                Err(e) => Some(Err(Error::Api {
                                                    status: 0,
                                                    message: format!(
                                                        "Failed to parse agent reaction: {}",
                                                        e
                                                    ),
                                                })),
                                            }
                                        }
                                        "agent.typing" => {
                                            match serde_json::from_str::<AgentTypingEvent>(
                                                &json_str,
                                            ) {
                                                Ok(typing) => {
                                                    Some(Ok(AgentSseEvent::Typing(typing)))
                                                }
                                                Err(e) => Some(Err(Error::Api {
                                                    status: 0,
                                                    message: format!(
                                                        "Failed to parse agent typing: {}",
                                                        e
                                                    ),
                                                })),
                                            }
                                        }
                                        "agent.done" => {
                                            match serde_json::from_str::<AgentDoneEvent>(&json_str)
                                            {
                                                Ok(done) => Some(Ok(AgentSseEvent::Done(done))),
                                                Err(e) => Some(Err(Error::Api {
                                                    status: 0,
                                                    message: format!(
                                                        "Failed to parse agent done: {}",
                                                        e
                                                    ),
                                                })),
                                            }
                                        }
                                        "agent.error" => {
                                            match serde_json::from_str::<AgentErrorEvent>(&json_str)
                                            {
                                                Ok(err) => Some(Ok(AgentSseEvent::Error(err))),
                                                Err(e) => Some(Err(Error::Api {
                                                    status: 0,
                                                    message: format!(
                                                        "Failed to parse agent error: {}",
                                                        e
                                                    ),
                                                })),
                                            }
                                        }
                                        _ => None,
                                    }
                                }
                                Err(e) => Some(Err(Error::Api {
                                    status: 0,
                                    message: format!("Invalid UTF-8 in decrypted data: {}", e),
                                })),
                            },
                            Err(e) => Some(Err(Error::Decryption(format!(
                                "Failed to decrypt agent event: {}",
                                e
                            )))),
                        }
                    }
                    Err(e) => Some(Err(Error::Api {
                        status: 0,
                        message: format!("SSE error: {}", e),
                    })),
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PushNotificationKeyPair;
    use futures::StreamExt;
    use serde_json::json;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Match, Mock, MockServer, Request, Respond, ResponseTemplate,
    };

    struct MissingHeaderMatcher(&'static str);

    impl Match for MissingHeaderMatcher {
        fn matches(&self, request: &Request) -> bool {
            !request.headers.contains_key(self.0)
        }
    }

    struct PathPrefixMatcher(&'static str);

    impl Match for PathPrefixMatcher {
        fn matches(&self, request: &Request) -> bool {
            request.url.path().starts_with(self.0)
        }
    }

    #[derive(Debug)]
    struct EncryptedJsonBodyMatcher {
        session_key: [u8; 32],
        expected: serde_json::Value,
    }

    impl Match for EncryptedJsonBodyMatcher {
        fn matches(&self, request: &Request) -> bool {
            let Ok(body) = serde_json::from_slice::<EncryptedRequest>(request.body.as_ref()) else {
                return false;
            };
            let Ok(encrypted) = BASE64.decode(body.encrypted.as_bytes()) else {
                return false;
            };
            let Ok(plaintext) = crypto::decrypt_data(&self.session_key, &encrypted) else {
                return false;
            };
            let Ok(actual) = serde_json::from_slice::<serde_json::Value>(&plaintext) else {
                return false;
            };

            actual == self.expected
        }
    }

    #[derive(Debug)]
    struct EncryptedBytesBodyMatcher {
        session_key: [u8; 32],
        expected: Bytes,
    }

    impl Match for EncryptedBytesBodyMatcher {
        fn matches(&self, request: &Request) -> bool {
            let Ok(body) = serde_json::from_slice::<EncryptedRequest>(request.body.as_ref()) else {
                return false;
            };
            let Ok(encrypted) = BASE64.decode(body.encrypted.as_bytes()) else {
                return false;
            };
            let Ok(actual) = crypto::decrypt_data(&self.session_key, &encrypted) else {
                return false;
            };

            actual == self.expected
        }
    }

    struct AttestationResponder {
        server_public_key: [u8; 32],
    }

    impl Respond for AttestationResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let nonce = request.url.path().rsplit('/').next().unwrap_or_default();
            let attestation_document =
                build_mock_attestation_document(nonce, &self.server_public_key);

            ResponseTemplate::new(200)
                .set_body_json(json!({ "attestation_document": attestation_document }))
        }
    }

    struct KeyExchangeResponder {
        server_secret_key: [u8; 32],
        session_key: [u8; 32],
        session_id: String,
    }

    impl Respond for KeyExchangeResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body: KeyExchangeRequest = serde_json::from_slice(request.body.as_ref()).unwrap();
            let client_public_bytes = BASE64.decode(body.client_public_key.as_bytes()).unwrap();
            let client_public_key = x25519_dalek::PublicKey::from(
                <[u8; 32]>::try_from(client_public_bytes.as_slice()).unwrap(),
            );
            let server_secret = x25519_dalek::StaticSecret::from(self.server_secret_key);
            let shared_secret =
                crypto::perform_static_key_exchange(&server_secret, &client_public_key);
            let encrypted_session_key = BASE64
                .encode(crypto::encrypt_data(shared_secret.as_bytes(), &self.session_key).unwrap());

            ResponseTemplate::new(200).set_body_json(json!({
                "encrypted_session_key": encrypted_session_key,
                "session_id": self.session_id,
            }))
        }
    }

    fn build_mock_attestation_document(nonce: &str, server_public_key: &[u8; 32]) -> String {
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

        let payload = cbor::to_vec(&payload).unwrap();
        let cose_sign1 = CborValue::Array(vec![
            CborValue::Bytes(vec![]),
            CborValue::Map(Vec::new()),
            CborValue::Bytes(payload),
            CborValue::Bytes(vec![]),
        ]);

        BASE64.encode(cbor::to_vec(&cose_sign1).unwrap())
    }

    fn encrypted_response<T: Serialize>(session_key: &[u8; 32], payload: &T) -> serde_json::Value {
        let plaintext = serde_json::to_vec(payload).unwrap();
        let encrypted = crypto::encrypt_data(session_key, &plaintext).unwrap();
        json!({ "encrypted": BASE64.encode(encrypted) })
    }

    fn encrypted_response_bytes(session_key: &[u8; 32], payload: &[u8]) -> serde_json::Value {
        let encrypted = crypto::encrypt_data(session_key, payload).unwrap();
        json!({ "encrypted": BASE64.encode(encrypted) })
    }

    fn encrypted_sse_data<T: Serialize>(session_key: &[u8; 32], payload: &T) -> String {
        let plaintext = serde_json::to_vec(payload).unwrap();
        let encrypted = crypto::encrypt_data(session_key, &plaintext).unwrap();
        format!("data: {}\n\n", BASE64.encode(encrypted))
    }

    fn encrypted_sse_bytes(session_key: &[u8; 32], payload: &[u8]) -> String {
        let encrypted = crypto::encrypt_data(session_key, payload).unwrap();
        BASE64.encode(encrypted)
    }

    fn decrypt_request_body<T: serde::de::DeserializeOwned>(
        request: &Request,
        session_key: &[u8; 32],
    ) -> T {
        let body: EncryptedRequest = serde_json::from_slice(request.body.as_ref()).unwrap();
        let encrypted = BASE64.decode(body.encrypted.as_bytes()).unwrap();
        let plaintext = crypto::decrypt_data(session_key, &encrypted).unwrap();
        serde_json::from_slice(&plaintext).unwrap()
    }

    struct RegisterPushDeviceResponder {
        session_key: [u8; 32],
        expected_request: RegisterPushDeviceRequest,
        response_device: PushDevice,
    }

    impl Respond for RegisterPushDeviceResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body: RegisterPushDeviceRequest = decrypt_request_body(request, &self.session_key);
            assert_eq!(body, self.expected_request);

            ResponseTemplate::new(200)
                .set_body_json(encrypted_response(&self.session_key, &self.response_device))
        }
    }

    struct LogoutWithPushDeviceResponder {
        session_key: [u8; 32],
        expected_push_device_id: Uuid,
    }

    impl Respond for LogoutWithPushDeviceResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body: LogoutRequest = decrypt_request_body(request, &self.session_key);
            assert_eq!(body.push_device_id, Some(self.expected_push_device_id));

            ResponseTemplate::new(200).set_body_json(encrypted_response(
                &self.session_key,
                &json!({ "ok": true }),
            ))
        }
    }

    #[test]
    fn test_build_conversations_endpoint_includes_filters() {
        let endpoint = build_conversations_endpoint(Some(&ConversationsListParams {
            limit: Some(25),
            after: Some(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()),
            order: Some("asc".to_string()),
            project_id: Some(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()),
            unassigned_project: Some(false),
            pinned: Some(false),
        }));

        assert_eq!(
            endpoint,
            "/v1/conversations?limit=25&after=550e8400%2De29b%2D41d4%2Da716%2D446655440000&order=asc&project_id=550e8400%2De29b%2D41d4%2Da716%2D446655440001&unassigned_project=false&pinned=false"
        );
    }

    #[test]
    fn test_build_conversations_endpoint_supports_unassigned_project_filter() {
        let endpoint = build_conversations_endpoint(Some(&ConversationsListParams {
            limit: None,
            after: None,
            order: None,
            project_id: None,
            unassigned_project: Some(true),
            pinned: None,
        }));

        assert_eq!(endpoint, "/v1/conversations?unassigned_project=true");
    }

    #[test]
    fn test_build_conversation_projects_endpoint_includes_pagination() {
        let endpoint = build_conversation_projects_endpoint(Some(&ConversationProjectListParams {
            limit: Some(10),
            after: Some(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()),
            order: Some("desc".to_string()),
        }));

        assert_eq!(
            endpoint,
            "/v1/conversation-projects?limit=10&after=550e8400%2De29b%2D41d4%2Da716%2D446655440000&order=desc"
        );
    }

    #[tokio::test]
    async fn test_update_conversation_rejects_empty_request_locally() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();

        let error = client
            .update_conversation(Uuid::new_v4(), ConversationUpdateRequest::default())
            .await
            .unwrap_err();

        assert!(
            matches!(error, Error::Configuration(message) if message.contains("at least one field"))
        );
    }

    #[tokio::test]
    async fn test_update_conversation_project_rejects_empty_request_locally() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();

        let error = client
            .update_conversation_project(
                Uuid::new_v4(),
                ConversationProjectUpdateRequest::default(),
            )
            .await
            .unwrap_err();

        assert!(
            matches!(error, Error::Configuration(message) if message.contains("at least one field"))
        );
    }

    #[tokio::test]
    async fn test_client_creation() {
        let client = OpenSecretClient::new("http://localhost:3000").unwrap();
        assert_eq!(client.base_url, "http://localhost:3000");
        assert!(client.use_mock_attestation);
    }

    #[tokio::test]
    async fn test_register_push_device_uses_v1_push_endpoint() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [21u8; 32];
        let now = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "access_token".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        let key_pair = PushNotificationKeyPair::generate();
        let request = RegisterPushDeviceRequest::new(
            Uuid::new_v4(),
            PushPlatform::Ios,
            PushEnvironment::Prod,
            "ai.trymaple.ios",
            "opaque-token",
            key_pair.public_key_spki_base64().unwrap(),
        )
        .supports_encrypted_preview(true)
        .supports_background_processing(true);

        let response_device = PushDevice {
            id: Uuid::new_v4(),
            object: "push.device".to_string(),
            installation_id: request.installation_id,
            platform: request.platform,
            provider: request.provider,
            environment: request.environment,
            app_id: request.app_id.clone(),
            key_algorithm: request.key_algorithm,
            supports_encrypted_preview: request.supports_encrypted_preview,
            supports_background_processing: request.supports_background_processing,
            last_seen_at: now,
            created_at: now,
            updated_at: now,
        };

        Mock::given(method("POST"))
            .and(path("/v1/push/devices"))
            .and(header("authorization", "Bearer access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(RegisterPushDeviceResponder {
                session_key,
                expected_request: request.clone(),
                response_device: response_device.clone(),
            })
            .expect(1)
            .mount(&mock_server)
            .await;

        let response = client.register_push_device(request).await.unwrap();

        assert_eq!(response, response_device);
    }

    #[tokio::test]
    async fn test_list_and_revoke_push_devices_use_v1_endpoints() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [22u8; 32];
        let now = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let device_id = Uuid::new_v4();

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "access_token".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        let device = PushDevice {
            id: device_id,
            object: "push.device".to_string(),
            installation_id: Uuid::new_v4(),
            platform: PushPlatform::Android,
            provider: PushProvider::Fcm,
            environment: PushEnvironment::Prod,
            app_id: "ai.trymaple.android".to_string(),
            key_algorithm: PushKeyAlgorithm::P256EcdhV1,
            supports_encrypted_preview: false,
            supports_background_processing: true,
            last_seen_at: now,
            created_at: now,
            updated_at: now,
        };
        let list_response = PushDeviceListResponse {
            object: "list".to_string(),
            data: vec![device.clone()],
        };
        let deleted_response = DeletedPushDeviceResponse {
            id: device_id,
            object: "push.device.deleted".to_string(),
            deleted: true,
        };

        Mock::given(method("GET"))
            .and(path("/v1/push/devices"))
            .and(header("authorization", "Bearer access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(encrypted_response(&session_key, &list_response)),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path(format!("/v1/push/devices/{}", device_id)))
            .and(header("authorization", "Bearer access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(encrypted_response(&session_key, &deleted_response)),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let listed = client.list_push_devices().await.unwrap();
        let deleted = client.revoke_push_device(device_id).await.unwrap();

        assert_eq!(listed, list_response);
        assert_eq!(deleted, deleted_response);
    }

    #[tokio::test]
    async fn test_logout_with_push_device_id_sends_cleanup_hint() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [23u8; 32];
        let push_device_id = Uuid::new_v4();

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "access_token".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/logout"))
            .and(MissingHeaderMatcher("authorization"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(LogoutWithPushDeviceResponder {
                session_key,
                expected_push_device_id: push_device_id,
            })
            .expect(1)
            .mount(&mock_server)
            .await;

        client
            .logout_with_push_device_id(push_device_id)
            .await
            .unwrap();

        assert!(client.get_session_id().unwrap().is_none());
        assert!(client.get_access_token().unwrap().is_none());
        assert!(client.get_refresh_token().unwrap().is_none());
    }

    #[tokio::test]
    async fn test_change_password_preserves_refresh_token_when_response_omits_one() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [24u8; 32];

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "old_access_token".to_string(),
                Some("old_refresh_token".to_string()),
            )
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/protected/change_password"))
            .and(header("authorization", "Bearer old_access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "message": "updated",
                    "access_token": "new_access_token"
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        client
            .change_password("old-credential".to_string(), "new-credential".to_string())
            .await
            .unwrap();

        assert_eq!(
            client.get_access_token().unwrap().as_deref(),
            Some("new_access_token")
        );
        assert_eq!(
            client.get_refresh_token().unwrap().as_deref(),
            Some("old_refresh_token")
        );
    }

    #[tokio::test]
    async fn test_authenticated_calls_refresh_and_retry_seamlessly() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [7u8; 32];
        let expired_access = "expired_access";
        let new_access = "new_access";
        let new_refresh = "new_refresh";
        let expired_header = format!("Bearer {}", expired_access);
        let fresh_header = format!("Bearer {}", new_access);

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                expired_access.to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/protected/user"))
            .and(header("authorization", &expired_header))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(
                ResponseTemplate::new(401).set_body_json(json!({ "message": "jwt expired" })),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/refresh"))
            .and(MissingHeaderMatcher("authorization"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "access_token": new_access,
                    "refresh_token": new_refresh,
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/protected/user"))
            .and(header("authorization", &fresh_header))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "user": {
                        "id": Uuid::new_v4(),
                        "name": null,
                        "email": "sdk@test.dev",
                        "email_verified": true,
                        "login_method": "email",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-01T00:00:00Z"
                    }
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        let response = client.get_user().await.unwrap();

        assert_eq!(response.user.email.as_deref(), Some("sdk@test.dev"));
        assert_eq!(
            client.get_access_token().unwrap().as_deref(),
            Some(new_access)
        );
        assert_eq!(
            client.get_refresh_token().unwrap().as_deref(),
            Some(new_refresh)
        );
    }

    #[tokio::test]
    async fn test_corrupted_access_token_recovers_via_refresh_on_next_call() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [5u8; 32];
        let original_access = "valid_access";
        let original_refresh = "valid_refresh";
        let corrupted_access = "malformed_access";
        let refreshed_access = "refreshed_access";
        let refreshed_refresh = "refreshed_refresh";

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/login"))
            .and(MissingHeaderMatcher("authorization"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "id": Uuid::new_v4(),
                    "email": "sdk@test.dev",
                    "access_token": original_access,
                    "refresh_token": original_refresh,
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/protected/user"))
            .and(header(
                "authorization",
                format!("Bearer {}", original_access),
            ))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "user": {
                        "id": Uuid::new_v4(),
                        "name": null,
                        "email": "sdk@test.dev",
                        "email_verified": true,
                        "login_method": "email",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-01T00:00:00Z"
                    }
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/protected/user"))
            .and(header(
                "authorization",
                format!("Bearer {}", corrupted_access),
            ))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(
                ResponseTemplate::new(401).set_body_json(json!({ "message": "invalid jwt" })),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/refresh"))
            .and(MissingHeaderMatcher("authorization"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "access_token": refreshed_access,
                    "refresh_token": refreshed_refresh,
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/protected/user"))
            .and(header(
                "authorization",
                format!("Bearer {}", refreshed_access),
            ))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "user": {
                        "id": Uuid::new_v4(),
                        "name": null,
                        "email": "sdk@test.dev",
                        "email_verified": true,
                        "login_method": "email",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-01T00:00:00Z"
                    }
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        client
            .login(
                "sdk@test.dev".to_string(),
                "password".to_string(),
                Uuid::new_v4(),
            )
            .await
            .unwrap();

        let initial_user = client.get_user().await.unwrap();
        assert_eq!(initial_user.user.email.as_deref(), Some("sdk@test.dev"));

        client
            .session_manager
            .update_access_token(corrupted_access.to_string())
            .unwrap();

        let recovered_user = client.get_user().await.unwrap();

        assert_eq!(recovered_user.user.email.as_deref(), Some("sdk@test.dev"));
        assert_eq!(
            client.get_access_token().unwrap().as_deref(),
            Some(refreshed_access)
        );
        assert_eq!(
            client.get_refresh_token().unwrap().as_deref(),
            Some(refreshed_refresh)
        );
    }

    #[tokio::test]
    async fn test_streaming_completion_preserves_reasoning_content() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [13u8; 32];

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "access_token".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        let sse_body = format!(
            "{}data: [DONE]\n\n",
            encrypted_sse_data(
                &session_key,
                &json!({
                    "id": "chatcmpl-test",
                    "object": "chat.completion.chunk",
                    "created": 1,
                    "model": "kimi-k2-5",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "reasoning_content": "2 + 2 = 4"
                        },
                        "finish_reason": null
                    }]
                })
            )
        );

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .and(EncryptedJsonBodyMatcher {
                session_key,
                expected: json!({
                    "model": "kimi-k2-5",
                    "messages": [{"role": "user", "content": "What is 2+2?"}],
                    "temperature": 0.0,
                    "max_tokens": 100,
                    "stream": true,
                    "stream_options": {"include_usage": true}
                }),
            })
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let request = ChatCompletionRequest {
            model: "kimi-k2-5".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: serde_json::json!("What is 2+2?"),
                tool_calls: None,
                reasoning_content: None,
            }],
            temperature: Some(0.0),
            max_tokens: Some(100),
            stream: Some(true),
            stream_options: None,
            tools: None,
            tool_choice: None,
        };

        let mut stream = client.create_chat_completion_stream(request).await.unwrap();
        let chunk = stream.next().await.unwrap().unwrap();

        assert_eq!(
            chunk.0["choices"][0]["delta"]["reasoning_content"].as_str(),
            Some("2 + 2 = 4")
        );
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn inference_transport_preserves_raw_request_and_response_bytes() {
        let mock_server = MockServer::start().await;
        let client =
            OpenSecretClient::new_with_api_key(mock_server.uri(), "real_api_key".to_string())
                .unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [27u8; 32];
        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();

        let request_body = Bytes::from_static(
            br#"{ "z": 184467440737095516160000000000000, "stream":false, "a":[1, 2] }
"#,
        );
        let response_body = Bytes::from_static(
            br#"{ "provider": {"huge":184467440737095516160000000000001}, "a": 1 }
"#,
        );

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(query_param("trace", "one two"))
            .and(header("authorization", "Bearer real_api_key"))
            .and(header("x-session-id", session_id.to_string()))
            .and(header("content-type", "application/json"))
            .and(header("x-provider-beta", "raw-v2"))
            .and(MissingHeaderMatcher("x-remove"))
            .and(MissingHeaderMatcher("content-encoding"))
            .and(MissingHeaderMatcher("accept-encoding"))
            .and(MissingHeaderMatcher("content-md5"))
            .and(EncryptedBytesBodyMatcher {
                session_key,
                expected: request_body.clone(),
            })
            .respond_with(
                ResponseTemplate::new(201)
                    .insert_header("content-type", "application/json")
                    .insert_header("x-provider-result", "kept")
                    .insert_header("connection", "x-remove-response")
                    .insert_header("x-remove-response", "gone")
                    .insert_header("content-encoding", "identity")
                    .set_body_json(encrypted_response_bytes(&session_key, &response_body)),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let request = HttpRequest::builder()
            .method(http::Method::POST)
            .uri("/v1/chat/completions?trace=one%20two")
            .header(header::AUTHORIZATION, "Bearer caller_must_not_control_this")
            .header("x-session-id", "caller_must_not_control_this")
            .header(header::CONTENT_TYPE, "application/custom")
            .header(header::CONTENT_ENCODING, "gzip")
            .header(header::ACCEPT_ENCODING, "gzip, br")
            .header("content-md5", "caller-body-digest")
            .header(header::CONNECTION, "x-remove")
            .header("x-remove", "gone")
            .header("x-provider-beta", "raw-v2")
            .body(request_body)
            .unwrap();
        let response = client.send_inference_request(request).await.unwrap();

        assert_eq!(response.status(), http::StatusCode::CREATED);
        assert_eq!(response.headers().get("x-provider-result").unwrap(), "kept");
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
        assert!(!response.headers().contains_key(header::CONNECTION));
        assert!(!response.headers().contains_key("x-remove-response"));
        assert_eq!(
            collect_response_body(response.into_body()).await.unwrap(),
            response_body
        );
    }

    #[tokio::test]
    async fn inference_transport_rejects_absolute_and_non_inference_routes() {
        let client = OpenSecretClient::new("http://localhost:3000").unwrap();
        for (method, uri) in [
            (
                http::Method::POST,
                "https://example.test/v1/chat/completions",
            ),
            (http::Method::POST, "/v1/responses"),
            (http::Method::GET, "/v1/chat/completions"),
            (http::Method::POST, "/protected/user"),
        ] {
            let request = HttpRequest::builder()
                .method(method)
                .uri(uri)
                .body(Bytes::new())
                .unwrap();
            assert!(matches!(
                client.send_inference_request(request).await,
                Err(Error::Configuration(_))
            ));
        }
    }

    #[tokio::test]
    async fn inference_transport_preserves_final_plaintext_error_after_refresh() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [28u8; 32];
        let request_body = Bytes::from_static(br#"{"model":"test","messages":[]}"#);
        let error_body =
            Bytes::from_static(br#"{"error":{"type":"rate_limit","n":999999999999999999999}}"#);
        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "expired_access".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer expired_access"))
            .and(EncryptedBytesBodyMatcher {
                session_key,
                expected: request_body.clone(),
            })
            .respond_with(ResponseTemplate::new(401).set_body_string("jwt expired"))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/refresh"))
            .and(MissingHeaderMatcher("authorization"))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "access_token": "fresh_access",
                    "refresh_token": "fresh_refresh"
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer fresh_access"))
            .and(EncryptedBytesBodyMatcher {
                session_key,
                expected: request_body.clone(),
            })
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("x-request-id", "provider-429")
                    .set_body_bytes(error_body.clone()),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let request = HttpRequest::builder()
            .method(http::Method::POST)
            .uri("/v1/chat/completions")
            .body(request_body)
            .unwrap();
        let response = client.send_inference_request(request).await.unwrap();

        assert_eq!(response.status(), http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers().get("x-request-id").unwrap(),
            "provider-429"
        );
        assert_eq!(
            collect_response_body(response.into_body()).await.unwrap(),
            error_body
        );
    }

    #[tokio::test]
    async fn inference_transport_decrypts_non_success_encrypted_body() {
        let mock_server = MockServer::start().await;
        let client =
            OpenSecretClient::new_with_api_key(mock_server.uri(), "api_key".to_string()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [29u8; 32];
        let error_body = Bytes::from_static(br#"{ "error": "provider rejected", "extra": 7 }"#);
        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_json(encrypted_response_bytes(&session_key, &error_body)),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let request = HttpRequest::builder()
            .method(http::Method::POST)
            .uri("/v1/embeddings")
            .body(Bytes::from_static(br#"{"model":"x","input":"y"}"#))
            .unwrap();
        let response = client.send_inference_request(request).await.unwrap();

        assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            collect_response_body(response.into_body()).await.unwrap(),
            error_body
        );
    }

    #[tokio::test]
    async fn inference_transport_preserves_non_envelope_error_with_encrypted_field() {
        let mock_server = MockServer::start().await;
        let client =
            OpenSecretClient::new_with_api_key(mock_server.uri(), "api_key".to_string()).unwrap();
        let session_key = [36u8; 32];
        let error_body = Bytes::from(
            serde_json::to_vec(&json!({
                "encrypted": BASE64.encode([0u8; 28]),
                "message": "plain backend error"
            }))
            .unwrap(),
        );
        client
            .session_manager
            .set_session(Uuid::new_v4(), session_key)
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(422).set_body_bytes(error_body.clone()))
            .expect(1)
            .mount(&mock_server)
            .await;

        let request = HttpRequest::builder()
            .method(http::Method::POST)
            .uri("/v1/embeddings")
            .body(Bytes::from_static(br#"{"model":"x","input":"y"}"#))
            .unwrap();
        let response = client.send_inference_request(request).await.unwrap();

        assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            collect_response_body(response.into_body()).await.unwrap(),
            error_body
        );
    }

    #[tokio::test]
    async fn inference_transport_establishes_attestation_and_replays_exact_bytes() {
        let mock_server = MockServer::start().await;
        let client =
            OpenSecretClient::new_with_api_key(mock_server.uri(), "api_key".to_string()).unwrap();
        let server_secret_key = [34u8; 32];
        let server_public_key =
            x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(server_secret_key));
        let session_key = [35u8; 32];
        let session_id = Uuid::new_v4();
        let request_body = Bytes::from_static(br#"{ "model":"x", "input":"exact bytes" }"#);
        let response_body = Bytes::from_static(br#"{ "ok": true }"#);

        Mock::given(method("GET"))
            .and(PathPrefixMatcher("/attestation/"))
            .respond_with(AttestationResponder {
                server_public_key: server_public_key.to_bytes(),
            })
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/key_exchange"))
            .and(MissingHeaderMatcher("authorization"))
            .respond_with(KeyExchangeResponder {
                server_secret_key,
                session_key,
                session_id: session_id.to_string(),
            })
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(header("authorization", "Bearer api_key"))
            .and(header("x-session-id", session_id.to_string()))
            .and(EncryptedBytesBodyMatcher {
                session_key,
                expected: request_body.clone(),
            })
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(encrypted_response_bytes(&session_key, &response_body)),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let request = HttpRequest::builder()
            .method(http::Method::POST)
            .uri("/v1/embeddings")
            .body(request_body)
            .unwrap();
        let response = client.send_inference_request(request).await.unwrap();

        assert_eq!(
            collect_response_body(response.into_body()).await.unwrap(),
            response_body
        );
    }

    #[tokio::test]
    async fn inference_sse_transport_preserves_framing_across_arbitrary_chunks() {
        let session_key = [30u8; 32];
        let decrypted_payload =
            br#"{ "delta": {"huge":184467440737095516160000000000000}, "text":"hi" }"#;
        let encrypted_payload = encrypted_sse_bytes(&session_key, decrypted_payload);
        let encrypted_sse = format!(
            ": heartbeat\r\nevent: chunk\r\nid: provider-7\r\nretry: 1500\r\ndata: {encrypted_payload}\r\n\r\ndata: provider-heartbeat\n\ndata: [DONE]\n\n"
        );
        let expected = format!(
            ": heartbeat\r\nevent: chunk\r\nid: provider-7\r\nretry: 1500\r\ndata: {}\r\n\r\ndata: provider-heartbeat\n\ndata: [DONE]\n\n",
            String::from_utf8_lossy(decrypted_payload)
        );
        let chunks = encrypted_sse
            .as_bytes()
            .chunks(3)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<Result<Bytes>>>();
        let source: OpenSecretResponseBody = Box::pin(futures::stream::iter(chunks));

        let actual = collect_response_body(decrypt_sse_stream(source, session_key))
            .await
            .unwrap();

        assert_eq!(actual, expected.as_bytes());
        assert_eq!(
            actual
                .windows(6)
                .filter(|window| *window == b"[DONE]")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn inference_sse_transport_reports_corrupt_ciphertext() {
        let session_key = [31u8; 32];
        let mut encrypted = crypto::encrypt_data(&session_key, br#"{"delta":"x"}"#).unwrap();
        *encrypted.last_mut().unwrap() ^= 0xff;
        let source: OpenSecretResponseBody = Box::pin(futures::stream::iter([Ok(Bytes::from(
            format!("data: {}\n\n", BASE64.encode(encrypted)),
        ))]));

        let error = collect_response_body(decrypt_sse_stream(source, session_key))
            .await
            .unwrap_err();

        assert!(matches!(error, Error::Decryption(message) if message.contains("SSE data")));
    }

    #[tokio::test]
    async fn inference_sse_transport_bounds_each_line_not_each_network_chunk() {
        let session_key = [37u8; 32];
        let within_limit = Bytes::from_static(b":1\n:2\n:3\n");
        let source: OpenSecretResponseBody =
            Box::pin(futures::stream::iter([Ok(within_limit.clone())]));
        let actual =
            collect_response_body(decrypt_sse_stream_with_line_limit(source, session_key, 8))
                .await
                .unwrap();
        assert_eq!(actual, within_limit);

        let exact_limit = Bytes::from_static(b":1234567");
        let source: OpenSecretResponseBody =
            Box::pin(futures::stream::iter([Ok(exact_limit.clone())]));
        let actual =
            collect_response_body(decrypt_sse_stream_with_line_limit(source, session_key, 8))
                .await
                .unwrap();
        assert_eq!(actual, exact_limit);

        for chunks in [
            vec![
                Ok(Bytes::from_static(b":1234")),
                Ok(Bytes::from_static(b"5678")),
            ],
            vec![Ok(Bytes::from_static(b":12345678\n"))],
        ] {
            let source: OpenSecretResponseBody = Box::pin(futures::stream::iter(chunks));
            let error =
                collect_response_body(decrypt_sse_stream_with_line_limit(source, session_key, 8))
                    .await
                    .unwrap_err();
            assert!(matches!(
                error,
                Error::InvalidResponse(message) if message.contains("8-byte limit")
            ));
        }
    }

    #[tokio::test]
    async fn typed_chat_completion_keeps_legacy_stream_false_and_error_mapping() {
        let mock_server = MockServer::start().await;
        let client =
            OpenSecretClient::new_with_api_key(mock_server.uri(), "api_key".to_string()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [32u8; 32];
        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(EncryptedJsonBodyMatcher {
                session_key,
                expected: json!({
                    "model": "typed-model",
                    "messages": [{"role": "user", "content": "hi"}],
                    "stream": false
                }),
            })
            .respond_with(ResponseTemplate::new(409).set_body_string("typed conflict"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let error = client
            .create_chat_completion(ChatCompletionRequest {
                model: "typed-model".to_string(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: json!("hi"),
                    tool_calls: None,
                    reasoning_content: None,
                }],
                temperature: None,
                max_tokens: None,
                stream: Some(true),
                stream_options: None,
                tools: None,
                tool_choice: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            Error::Api { status: 409, message } if message == "typed conflict"
        ));
    }

    #[tokio::test]
    async fn typed_models_and_embeddings_remain_compatible() {
        let mock_server = MockServer::start().await;
        let client =
            OpenSecretClient::new_with_api_key(mock_server.uri(), "api_key".to_string()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [33u8; 32];
        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "object": "list",
                    "data": [{"id": "model-a", "object": "model"}]
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(EncryptedJsonBodyMatcher {
                session_key,
                expected: json!({
                    "input": "hello",
                    "model": "embedding-model"
                }),
            })
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "object": "list",
                    "data": [{"object": "embedding", "index": 0, "embedding": [0.25, 0.5]}],
                    "model": "embedding-model",
                    "usage": {"prompt_tokens": 1, "total_tokens": 1}
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        let models = client.get_models().await.unwrap();
        let embeddings = client
            .create_embeddings(EmbeddingRequest {
                input: "hello".into(),
                model: "embedding-model".to_string(),
                encoding_format: None,
                dimensions: None,
                user: None,
            })
            .await
            .unwrap();

        assert_eq!(models.data[0].id, "model-a");
        assert_eq!(
            embeddings.data[0].embedding.as_floats(),
            Some(&[0.25, 0.5][..])
        );
    }

    #[tokio::test]
    async fn test_refresh_reestablishes_attestation_without_sending_auth_headers() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let server_secret_key = [11u8; 32];
        let server_public_key =
            x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(server_secret_key));
        let session_key = [9u8; 32];
        let session_id = Uuid::new_v4().to_string();
        let refreshed_access = "refreshed_access";
        let refreshed_refresh = "refreshed_refresh";

        client
            .session_manager
            .set_tokens(
                "expired_access".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        Mock::given(method("GET"))
            .and(PathPrefixMatcher("/attestation/"))
            .respond_with(AttestationResponder {
                server_public_key: server_public_key.to_bytes(),
            })
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/key_exchange"))
            .and(MissingHeaderMatcher("authorization"))
            .respond_with(KeyExchangeResponder {
                server_secret_key,
                session_key,
                session_id: session_id.clone(),
            })
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/refresh"))
            .and(MissingHeaderMatcher("authorization"))
            .and(header("x-session-id", session_id.clone()))
            .respond_with(ResponseTemplate::new(200).set_body_json(encrypted_response(
                &session_key,
                &json!({
                    "access_token": refreshed_access,
                    "refresh_token": refreshed_refresh,
                }),
            )))
            .expect(1)
            .mount(&mock_server)
            .await;

        client.refresh_token().await.unwrap();

        assert_eq!(
            client.get_session_id().unwrap(),
            Some(Uuid::parse_str(&session_id).unwrap())
        );
        assert_eq!(
            client.get_access_token().unwrap().as_deref(),
            Some(refreshed_access)
        );
        assert_eq!(
            client.get_refresh_token().unwrap().as_deref(),
            Some(refreshed_refresh)
        );
    }

    #[tokio::test]
    async fn test_init_main_agent_uses_authenticated_encrypted_v1_endpoint() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [31u8; 32];

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "access_token".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        let request = InitMainAgentRequest {
            timezone: Some("America/Chicago".to_string()),
            locale: Some("en-US".to_string()),
        };
        let response = InitMainAgentResponse {
            id: Uuid::new_v4(),
            object: "agent.main".to_string(),
            kind: "main".to_string(),
            conversation_id: Uuid::new_v4(),
            display_name: "Maple".to_string(),
            created_at: 1_710_000_000,
            updated_at: 1_710_000_000,
            messages: vec![ConversationItem::Message {
                id: Uuid::new_v4(),
                status: Some("completed".to_string()),
                role: "assistant".to_string(),
                content: vec![ConversationContent::OutputText {
                    text: "Hey — I'm Maple.".to_string(),
                }],
                reaction: None,
                created_at: Some(1_710_000_000),
            }],
        };
        let expected_request = request.clone();
        let expected_response = response.clone();

        Mock::given(method("POST"))
            .and(path("/v1/agent/init"))
            .and(header("authorization", "Bearer access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(move |req: &Request| {
                let body: InitMainAgentRequest = decrypt_request_body(req, &session_key);
                assert_eq!(body, expected_request);

                ResponseTemplate::new(200)
                    .set_body_json(encrypted_response(&session_key, &expected_response))
            })
            .expect(1)
            .mount(&mock_server)
            .await;

        let initialized = client.init_main_agent(request.clone()).await.unwrap();

        assert_eq!(initialized.id, response.id);
        assert_eq!(initialized.conversation_id, response.conversation_id);
        assert_eq!(initialized.display_name, "Maple");
        assert_eq!(initialized.messages.len(), 1);
    }

    #[tokio::test]
    async fn test_set_main_agent_item_reaction_uses_authenticated_encrypted_v1_endpoint() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [32u8; 32];
        let item_id = Uuid::new_v4();

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "access_token".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        let response = ConversationItem::Message {
            id: item_id,
            status: Some("completed".to_string()),
            role: "assistant".to_string(),
            content: vec![ConversationContent::OutputText {
                text: "Nice!".to_string(),
            }],
            reaction: Some("🎉".to_string()),
            created_at: Some(1_710_000_000),
        };
        let expected_response = response.clone();

        Mock::given(method("POST"))
            .and(path(format!("/v1/agent/items/{}/reaction", item_id)))
            .and(header("authorization", "Bearer access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(move |req: &Request| {
                let body: SetMessageReactionRequest = decrypt_request_body(req, &session_key);
                assert_eq!(
                    body,
                    SetMessageReactionRequest {
                        emoji: "🎉".to_string()
                    }
                );

                ResponseTemplate::new(200)
                    .set_body_json(encrypted_response(&session_key, &expected_response))
            })
            .expect(1)
            .mount(&mock_server)
            .await;

        let item = client
            .set_main_agent_item_reaction(item_id, "🎉")
            .await
            .unwrap();

        match item {
            ConversationItem::Message { id, reaction, .. } => {
                assert_eq!(id, item_id);
                assert_eq!(reaction.as_deref(), Some("🎉"));
            }
            other => panic!("Expected message item, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_clear_subagent_item_reaction_uses_authenticated_v1_endpoint() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [33u8; 32];
        let subagent_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "access_token".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        let response = ConversationItem::Message {
            id: item_id,
            status: Some("completed".to_string()),
            role: "assistant".to_string(),
            content: vec![ConversationContent::OutputText {
                text: "Done".to_string(),
            }],
            reaction: None,
            created_at: Some(1_710_000_001),
        };
        let expected_response = response.clone();

        Mock::given(method("DELETE"))
            .and(path(format!(
                "/v1/agent/subagents/{}/items/{}/reaction",
                subagent_id, item_id
            )))
            .and(header("authorization", "Bearer access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(encrypted_response(&session_key, &expected_response)),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let item = client
            .clear_subagent_item_reaction(subagent_id, item_id)
            .await
            .unwrap();

        match item {
            ConversationItem::Message { id, reaction, .. } => {
                assert_eq!(id, item_id);
                assert_eq!(reaction, None);
            }
            other => panic!("Expected message item, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_agent_chat_stream_parses_reaction_and_message_ids() {
        let mock_server = MockServer::start().await;
        let client = OpenSecretClient::new(mock_server.uri()).unwrap();
        let session_id = Uuid::new_v4();
        let session_key = [34u8; 32];
        let reaction_item_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();

        client
            .session_manager
            .set_session(session_id, session_key)
            .unwrap();
        client
            .session_manager
            .set_tokens(
                "access_token".to_string(),
                Some("refresh_token".to_string()),
            )
            .unwrap();

        let sse_body = format!(
            "{}{}{}{}data: [DONE]\n\n",
            encrypted_sse_data(&session_key, &json!({})).replacen(
                "data:",
                "event: agent.typing\ndata:",
                1
            ),
            encrypted_sse_data(
                &session_key,
                &json!({
                    "item_id": reaction_item_id,
                    "emoji": "🫡"
                })
            )
            .replacen("data:", "event: agent.reaction\ndata:", 1),
            encrypted_sse_data(
                &session_key,
                &json!({
                    "message_id": message_id,
                    "message": "hello there"
                })
            )
            .replacen("data:", "event: agent.message\ndata:", 1),
            encrypted_sse_data(&session_key, &json!({})).replacen(
                "data:",
                "event: agent.done\ndata:",
                1
            ),
        );

        Mock::given(method("POST"))
            .and(path("/v1/agent/chat"))
            .and(header("authorization", "Bearer access_token"))
            .and(header("x-session-id", session_id.to_string()))
            .respond_with(move |req: &Request| {
                let body: AgentChatRequest = decrypt_request_body(req, &session_key);
                assert_eq!(body.input, "hey there");

                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body.clone())
            })
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut stream = client.agent_chat("hey there").await.unwrap();

        match stream.next().await.unwrap().unwrap() {
            AgentSseEvent::Typing(_) => {}
            other => panic!("Expected typing event, got {:?}", other),
        }

        match stream.next().await.unwrap().unwrap() {
            AgentSseEvent::Reaction(event) => {
                assert_eq!(event.item_id, reaction_item_id);
                assert_eq!(event.emoji, "🫡");
            }
            other => panic!("Expected reaction event, got {:?}", other),
        }

        match stream.next().await.unwrap().unwrap() {
            AgentSseEvent::Message(event) => {
                assert_eq!(event.message_id, message_id);
                assert_eq!(event.message, "hello there".to_string());
            }
            other => panic!("Expected message event, got {:?}", other),
        }

        match stream.next().await.unwrap().unwrap() {
            AgentSseEvent::Done(_) => {}
            other => panic!("Expected done event, got {:?}", other),
        }

        assert!(stream.next().await.is_none());
    }
}
