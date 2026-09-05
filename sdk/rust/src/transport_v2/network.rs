use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    pin::Pin,
    sync::Arc,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode};
use p256::elliptic_curve::rand_core::{OsRng, RngCore};
use reqwest::{header, redirect::Policy, Client};
use zeroize::Zeroizing;

use crate::{
    attestation::{AttestationDocument, AttestationVerifier},
    cbor::{self, Value as CborValue},
    error::{Error, Result},
    pcr::Pcr0TrustPolicy,
    session::SessionManager,
    types::AttestationResponse,
};

use super::{
    envelope::{
        CacheNamespaceRoot, Credential, HeaderField, LogicalRequest, ResponseMode,
        MAX_KEY_EXCHANGE_BYTES, MAX_OUTER_RESPONSE_BYTES,
    },
    runtime::{ApiKeyScope, TransportV2Runtime},
    session::{PreparedKeyExchange, PreparedRequest, ResponseContext, V2Session},
    stream::StreamEvent,
    TransportV2Error,
};

const MAX_ATTESTATION_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub(crate) type V2ResponseBody = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send + 'static>>;
pub(crate) type V2HttpResponse = Response<V2ResponseBody>;

pub(crate) struct TransportV2Client {
    client: Client,
    base_url: String,
    use_mock_attestation: bool,
    pcr0_trust_policy: Pcr0TrustPolicy,
    runtime: Arc<TransportV2Runtime>,
}

impl TransportV2Client {
    pub(crate) fn new(
        base_url: String,
        pcr0_trust_policy: Pcr0TrustPolicy,
        cache_namespace_root: [u8; 32],
        session_manager: SessionManager,
    ) -> Result<Self> {
        let (base_url, use_mock_attestation) = canonical_base_url(base_url)?;
        let client = Client::builder().redirect(Policy::none()).build()?;
        Ok(Self {
            client,
            base_url,
            use_mock_attestation,
            pcr0_trust_policy,
            runtime: Arc::new(TransportV2Runtime::new(
                cache_namespace_root,
                session_manager,
            )),
        })
    }

    pub(crate) fn with_cache_namespace_root(mut self, root: [u8; 32]) -> Self {
        self.runtime = Arc::new(TransportV2Runtime::new(
            root,
            self.runtime.session_manager(),
        ));
        self
    }

    pub(crate) const fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    pub(crate) const fn http_client(&self) -> &Client {
        &self.client
    }

    pub(crate) fn cache_namespace_root(&self) -> Result<[u8; 32]> {
        self.runtime.cache_namespace_root().map_err(Into::into)
    }

    pub(crate) fn replace_cache_namespace_root(&self, root: [u8; 32]) -> Result<()> {
        self.runtime
            .replace_cache_namespace_root(root)
            .map_err(Into::into)
    }

    pub(crate) fn active_session_id(&self) -> Result<Option<uuid::Uuid>> {
        self.runtime.active_session_id().map_err(Into::into)
    }

    pub(crate) fn anonymous_session(&self) -> Result<Option<Arc<V2Session>>> {
        self.runtime.anonymous().map_err(Into::into)
    }

    #[cfg(test)]
    pub(crate) fn set_anonymous_session_for_test(&self, session: Arc<V2Session>) -> Result<()> {
        self.runtime.set_anonymous(session).map_err(Into::into)
    }

    pub(crate) fn clear_anonymous_session_if(&self, session: &Arc<V2Session>) -> Result<()> {
        self.runtime.clear_anonymous_if(session).map_err(Into::into)
    }

    pub(crate) fn clear_user_session_if(&self, session: &Arc<V2Session>) -> Result<()> {
        self.runtime.clear_user_if(session).map_err(Into::into)
    }

    pub(crate) fn api_key_session(&self, scope: &ApiKeyScope) -> Result<Option<Arc<V2Session>>> {
        self.runtime.api_key(scope).map_err(Into::into)
    }

    pub(crate) fn set_api_key_session(
        &self,
        scope: &ApiKeyScope,
        session: Arc<V2Session>,
    ) -> Result<()> {
        self.runtime.set_api_key(scope, session).map_err(Into::into)
    }

    pub(crate) fn clear_api_key_sessions(&self) -> Result<()> {
        self.runtime.clear_api_key().map_err(Into::into)
    }

    pub(crate) fn clear_api_key_session_if(
        &self,
        scope: &ApiKeyScope,
        session: &Arc<V2Session>,
    ) -> Result<()> {
        self.runtime
            .clear_api_key_if(scope, session)
            .map_err(Into::into)
    }

    pub(crate) fn user_gate(&self) -> &tokio::sync::Mutex<()> {
        self.runtime.user_gate()
    }

    pub(crate) fn api_key_gate(&self) -> &tokio::sync::Mutex<()> {
        self.runtime.api_key_gate()
    }

    pub(crate) async fn perform_attestation_handshake(&self) -> Result<Arc<V2Session>> {
        let _guard = self.runtime.anonymous_gate().lock().await;
        if let Some(session) = self.runtime.anonymous().map_err(Error::from)? {
            if !session.is_expired()? {
                return Ok(session);
            }
            self.runtime
                .clear_anonymous_if(&session)
                .map_err(Error::from)?;
        }
        let session = self.establish_fresh_session().await?;
        self.runtime
            .set_anonymous(Arc::clone(&session))
            .map_err(Error::from)?;
        Ok(session)
    }

    pub(crate) async fn fresh_session(&self) -> Result<Arc<V2Session>> {
        self.establish_fresh_session().await
    }

    async fn send_prepared(
        &self,
        prepared: PreparedRequest,
        session: Arc<V2Session>,
    ) -> Result<V2HttpResponse> {
        let session_id = prepared.session_id();
        let response_mode = prepared.response_mode();
        let (body, response_context) = prepared.into_parts();
        let response = self
            .client
            .post(format!("{}/v2/request", self.base_url))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header("x-session-id", session_id.hyphenated().to_string())
            .body(body)
            .send()
            .await?;

        let outer_status = response.status();
        let content_type = exact_content_type(response.headers());

        match response_mode {
            super::envelope::ResponseMode::Unary => {
                if outer_status != reqwest::StatusCode::OK
                    || content_type != Some("application/octet-stream")
                {
                    return Err(untrusted_outer_error(outer_status));
                }
                let body = read_bounded_response(response, MAX_OUTER_RESPONSE_BYTES).await?;
                let logical = response_context.decrypt_unary_outer(&body)?;
                retire_authenticated_session_if_exhausted(
                    &self.runtime,
                    &session,
                    logical.status,
                    logical.body.as_deref(),
                )?;
                logical_response(logical.status, logical.headers, logical.body)
            }
            super::envelope::ResponseMode::Stream => match content_type {
                Some("application/octet-stream") if outer_status == reqwest::StatusCode::OK => {
                    let body = read_bounded_response(response, MAX_OUTER_RESPONSE_BYTES).await?;
                    let logical = response_context.decrypt_stream_pre_start_error_outer(&body)?;
                    retire_authenticated_session_if_exhausted(
                        &self.runtime,
                        &session,
                        logical.status,
                        logical.body.as_deref(),
                    )?;
                    logical_response(logical.status, logical.headers, logical.body)
                }
                Some("text/event-stream") if outer_status == reqwest::StatusCode::OK => {
                    stream_response(
                        response,
                        response_context,
                        Arc::clone(&self.runtime),
                        session,
                    )
                    .await
                }
                _ => Err(untrusted_outer_error(outer_status)),
            },
            super::envelope::ResponseMode::Auto => Err(Error::InvalidResponse(
                "Transport v2 returned a reserved response mode".to_string(),
            )),
        }
    }

    pub(crate) async fn send_request(
        &self,
        session: &Arc<V2Session>,
        response_mode: ResponseMode,
        credential: Option<Credential>,
        cache_namespace_root: Option<CacheNamespaceRoot>,
        request: LogicalRequest,
    ) -> Result<V2HttpResponse> {
        let prepared =
            match session.prepare_request(response_mode, credential, cache_namespace_root, request)
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    if matches!(
                        error,
                        TransportV2Error::SessionExpired
                            | TransportV2Error::RequestRecordBudgetExhausted
                            | TransportV2Error::ResponseRecordBudgetExhausted
                    ) {
                        self.runtime
                            .clear_session_if(session)
                            .map_err(Error::from)?;
                    }
                    return Err(error.into());
                }
            };
        match self.send_prepared(prepared, Arc::clone(session)).await {
            Ok(response) => Ok(response),
            Err(error) => {
                self.runtime
                    .clear_session_if(session)
                    .map_err(Error::from)?;
                Err(error)
            }
        }
    }

    async fn establish_fresh_session(&self) -> Result<Arc<V2Session>> {
        let nonce = fresh_attestation_nonce()?;
        let response = self
            .client
            .get(format!("{}/v2/attestation/{nonce}", self.base_url))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(untrusted_outer_error(response.status()));
        }
        let body = read_bounded_response(response, MAX_ATTESTATION_RESPONSE_BYTES).await?;
        let attestation: AttestationResponse = serde_json::from_slice(&body)?;

        let document = if self.use_mock_attestation {
            parse_mock_attestation(&attestation.attestation_document)?
        } else {
            let verifier = AttestationVerifier::new();
            let document =
                verifier.verify_attestation_document(&attestation.attestation_document, &nonce)?;
            let pcr0 = document.pcrs.get(&0).ok_or_else(|| {
                Error::AttestationVerificationFailed(
                    "Missing PCR0 in attestation document".to_string(),
                )
            })?;
            self.pcr0_trust_policy.verify_pcr0(pcr0).await?;
            document
        };
        if document.nonce.as_deref() != Some(nonce.as_bytes()) {
            return Err(Error::AttestationVerificationFailed(
                "Attestation nonce did not match the fresh client challenge".to_string(),
            ));
        }
        let enclave_public_key: [u8; 32] = document
            .public_key
            .ok_or_else(|| {
                Error::AttestationVerificationFailed(
                    "Attestation document did not contain an enclave public key".to_string(),
                )
            })?
            .try_into()
            .map_err(|_| {
                Error::AttestationVerificationFailed(
                    "Attested enclave public key had the wrong length".to_string(),
                )
            })?;

        // Key exchange is one-shot. Once this POST is attempted, the SDK never
        // transparently reuses the prepared client secret or nonce.
        let prepared = PreparedKeyExchange::new(nonce, enclave_public_key)?;
        let (body, completion) = prepared.into_parts();
        let response = self
            .client
            .post(format!("{}/v2/key_exchange", self.base_url))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(untrusted_outer_error(response.status()));
        }
        let body = read_bounded_response(response, MAX_KEY_EXCHANGE_BYTES).await?;
        Ok(Arc::new(completion.complete(&body)?))
    }
}

fn exact_content_type(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(header::CONTENT_TYPE).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    Some(value)
}

async fn stream_response(
    response: reqwest::Response,
    response_context: ResponseContext,
    runtime: Arc<TransportV2Runtime>,
    session: Arc<V2Session>,
) -> Result<V2HttpResponse> {
    let mut source = response.bytes_stream();
    let mut decoder = response_context.into_stream_decoder()?;
    let mut pending = VecDeque::new();
    let (status, headers) = 'start: loop {
        let Some(chunk) = source.next().await else {
            decoder.finish()?;
            return Err(Error::InvalidResponse(
                "Transport v2 stream ended before its authenticated start".to_string(),
            ));
        };
        let mut events = decoder.push(&chunk?)?.into_iter();
        let Some(event) = events.next() else {
            continue;
        };
        match event {
            StreamEvent::Start { status, headers } => {
                pending.extend(events);
                break 'start (status, headers);
            }
            _ => {
                return Err(Error::InvalidResponse(
                    "Transport v2 stream emitted data before its authenticated start".to_string(),
                ))
            }
        }
    };

    let body = async_stream::try_stream! {
        let mut terminal = false;
        loop {
            while let Some(event) = pending.pop_front() {
                match event {
                    StreamEvent::Start { .. } => Err(Error::InvalidResponse(
                        "Transport v2 stream emitted more than one start record".to_string(),
                    ))?,
                    StreamEvent::Chunk(bytes) => yield Bytes::from(bytes),
                    StreamEvent::End => terminal = true,
                    StreamEvent::Error { status, body } => Err(Error::Api {
                        status,
                        message: String::from_utf8_lossy(&body).into_owned(),
                    })?,
                }
            }

            let Some(chunk) = source.next().await else {
                decoder.finish()?;
                if !terminal {
                    Err(Error::InvalidResponse(
                        "Transport v2 stream lacked an authenticated terminal record".to_string(),
                    ))?;
                }
                break;
            };
            pending.extend(decoder.push(&chunk?)?);
        }
    }
    .map(move |item| retire_session_on_failed_stream_item(&runtime, &session, item));

    build_response(status, headers, Box::pin(body))
}

fn retire_session_on_failed_stream_item(
    runtime: &TransportV2Runtime,
    session: &Arc<V2Session>,
    item: Result<Bytes>,
) -> Result<Bytes> {
    let retires_session = item.as_ref().is_err_and(|error| match error {
        Error::Api { status, message } => {
            authenticated_session_exhausted(*status, message.as_bytes())
        }
        _ => true,
    });
    if retires_session {
        runtime.clear_session_if(session).map_err(Error::from)?;
    }
    item
}

fn retire_authenticated_session_if_exhausted(
    runtime: &TransportV2Runtime,
    session: &Arc<V2Session>,
    status: u16,
    body: Option<&[u8]>,
) -> Result<()> {
    if body.is_some_and(|body| authenticated_session_exhausted(status, body)) {
        runtime.clear_session_if(session).map_err(Error::from)?;
    }
    Ok(())
}

fn authenticated_session_exhausted(status: u16, body: &[u8]) -> bool {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ProtocolError {
        error: ProtocolErrorDetails,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ProtocolErrorDetails {
        code: String,
        message: String,
    }

    if status != StatusCode::SERVICE_UNAVAILABLE.as_u16() {
        return false;
    }
    serde_json::from_slice::<ProtocolError>(body).is_ok_and(|error| {
        error.error.code == "session_exhausted" && !error.error.message.is_empty()
    })
}

fn logical_response(
    status: u16,
    headers: Vec<HeaderField>,
    body: Option<Vec<u8>>,
) -> Result<V2HttpResponse> {
    let body = Bytes::from(body.unwrap_or_default());
    build_response(
        status,
        headers,
        Box::pin(futures::stream::once(async move { Ok(body) })),
    )
}

fn build_response(
    status: u16,
    headers: Vec<HeaderField>,
    body: V2ResponseBody,
) -> Result<V2HttpResponse> {
    let status = StatusCode::from_u16(status).map_err(|_| {
        Error::InvalidResponse("Transport v2 returned an invalid logical status".to_string())
    })?;
    let mut response = Response::builder()
        .status(status)
        .body(body)
        .map_err(|error| {
            Error::InvalidResponse(format!("Failed to construct logical response: {error}"))
        })?;
    *response.headers_mut() = logical_headers(headers)?;
    Ok(response)
}

fn logical_headers(headers: Vec<HeaderField>) -> Result<HeaderMap> {
    let mut output = HeaderMap::new();
    for header in headers {
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| {
            Error::InvalidResponse("Transport v2 returned an invalid header name".to_string())
        })?;
        let value = HeaderValue::from_bytes(header.value()).map_err(|_| {
            Error::InvalidResponse("Transport v2 returned an invalid header value".to_string())
        })?;
        output.append(name, value);
    }
    Ok(output)
}

async fn read_bounded_response(response: reqwest::Response, limit: usize) -> Result<Bytes> {
    let mut source = response.bytes_stream();
    let mut body = BytesMut::new();
    while let Some(chunk) = source.next().await {
        let chunk = chunk?;
        let next = body.len().checked_add(chunk.len()).ok_or_else(|| {
            Error::InvalidResponse("Transport response size overflowed".to_string())
        })?;
        if next > limit {
            return Err(Error::InvalidResponse(format!(
                "Transport response exceeded the {limit}-byte limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

fn fresh_attestation_nonce() -> Result<String> {
    let mut nonce = Zeroizing::new([0_u8; 32]);
    OsRng
        .try_fill_bytes(&mut *nonce)
        .map_err(|_| Error::Crypto("Secure randomness was unavailable".to_string()))?;
    Ok(hex::encode(*nonce))
}

fn untrusted_outer_error(status: reqwest::StatusCode) -> Error {
    let status = status.as_u16();
    Error::Api {
        status,
        message: format!(
            "Transport v2 request failed before an authenticated logical response (outer status {status})"
        ),
    }
}

fn canonical_base_url(base_url: String) -> Result<(String, bool)> {
    let mut parsed = reqwest::Url::parse(&base_url)
        .map_err(|error| Error::Configuration(format!("Invalid base URL: {error}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::Configuration(
            "Base URL must use HTTP or HTTPS".to_string(),
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(Error::Configuration(
            "Base URL must not contain credentials".to_string(),
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(Error::Configuration(
            "Base URL must not contain a query or fragment".to_string(),
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::Configuration("Base URL must include a host".to_string()))?;
    let host = host.trim_end_matches('.');
    let is_mock_host = if host.eq_ignore_ascii_case("localhost") {
        true
    } else {
        let address_host = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host);
        address_host.parse::<IpAddr>().is_ok_and(|address| {
            address.is_loopback()
                || address.is_unspecified()
                || (cfg!(target_os = "android") && address == IpAddr::from([10, 0, 2, 2]))
        })
    };
    if parsed.scheme() != "https" && !is_mock_host {
        return Err(Error::Configuration(
            "Non-local base URLs must use HTTPS".to_string(),
        ));
    }

    let path = parsed.path().trim_end_matches('/').to_string();
    parsed.set_path(if path.is_empty() { "/" } else { &path });
    Ok((
        parsed.as_str().trim_end_matches('/').to_string(),
        is_mock_host,
    ))
}

fn parse_mock_attestation(document_b64: &str) -> Result<AttestationDocument> {
    let document_bytes = STANDARD.decode(document_b64)?;
    let cbor_value: CborValue = cbor::from_slice(&document_bytes)?;
    let cose_sign1 = match &cbor_value {
        CborValue::Array(values) if values.len() == 4 => values,
        _ => {
            return Err(Error::AttestationVerificationFailed(
                "Invalid mock COSE_Sign1 structure".to_string(),
            ))
        }
    };
    let payload = match &cose_sign1[2] {
        CborValue::Bytes(bytes) => bytes,
        _ => {
            return Err(Error::AttestationVerificationFailed(
                "Invalid mock attestation payload".to_string(),
            ))
        }
    };
    let document: CborValue = cbor::from_slice(payload)?;
    let map = match document {
        CborValue::Map(map) => map,
        _ => {
            return Err(Error::AttestationVerificationFailed(
                "Invalid mock attestation document".to_string(),
            ))
        }
    };

    let mut public_key = None;
    let mut nonce = None;
    for (key, value) in map {
        let CborValue::Text(key) = key else {
            continue;
        };
        match (key.as_str(), value) {
            ("public_key", CborValue::Bytes(bytes)) => public_key = Some(bytes),
            ("nonce", CborValue::Bytes(bytes)) => nonce = Some(bytes),
            _ => {}
        }
    }
    Ok(AttestationDocument {
        module_id: "mock-module".to_string(),
        timestamp: 1,
        digest: "SHA384".to_string(),
        pcrs: HashMap::new(),
        certificate: Vec::new(),
        cabundle: Vec::new(),
        public_key,
        user_data: None,
        nonce,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(marker: u8) -> Arc<V2Session> {
        Arc::new(
            V2Session::from_master_for_test(
                uuid::Uuid::from_bytes([marker; 16]),
                [marker; 32],
                u64::MAX,
            )
            .expect("test session"),
        )
    }

    fn runtime_with_user(session: Arc<V2Session>) -> TransportV2Runtime {
        let runtime = TransportV2Runtime::new([0x11; 32], SessionManager::new());
        runtime.set_user_for_test(session).unwrap();
        runtime
    }

    #[tokio::test]
    async fn local_session_capacity_exhaustion_retires_without_sending() {
        let manager = SessionManager::new();
        let session = Arc::new(
            V2Session::from_master_with_budgets_for_test(
                uuid::Uuid::from_bytes([0x21; 16]),
                [0x22; 32],
                u64::MAX,
                0,
                1,
            )
            .expect("capacity-limited session"),
        );
        let runtime = Arc::new(TransportV2Runtime::new([0x11; 32], manager));
        runtime.set_user_for_test(Arc::clone(&session)).unwrap();
        let client = TransportV2Client {
            client: Client::new(),
            base_url: "http://127.0.0.1:9".to_string(),
            use_mock_attestation: true,
            pcr0_trust_policy: Pcr0TrustPolicy::official_for(Default::default()),
            runtime: Arc::clone(&runtime),
        };

        let result = client
            .send_request(
                &session,
                ResponseMode::Unary,
                None,
                None,
                LogicalRequest::new(
                    super::super::envelope::LogicalMethod::Get,
                    "/v1/models",
                    None,
                    Vec::new(),
                    None,
                ),
            )
            .await;
        let error = match result {
            Ok(_) => panic!("local capacity must fail before network I/O"),
            Err(error) => error,
        };

        assert!(matches!(error, Error::Session(_)));
        assert!(runtime.user().unwrap().is_none());
    }

    #[test]
    fn authenticated_session_exhaustion_retires_exact_session() {
        let exhausted = session(0x22);
        let runtime = runtime_with_user(Arc::clone(&exhausted));
        let body = br#"{"error":{"code":"session_exhausted","message":"Session request capacity is exhausted"}}"#;

        retire_authenticated_session_if_exhausted(
            &runtime,
            &exhausted,
            StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            Some(body),
        )
        .unwrap();
        assert!(runtime.user().unwrap().is_none());

        let replacement = session(0x33);
        runtime.set_user_for_test(Arc::clone(&replacement)).unwrap();
        retire_authenticated_session_if_exhausted(
            &runtime,
            &exhausted,
            StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            Some(body),
        )
        .unwrap();
        assert!(Arc::ptr_eq(
            &runtime.user().unwrap().expect("replacement remains"),
            &replacement,
        ));
    }

    #[test]
    fn late_stream_transport_failure_retires_only_the_failed_session() {
        let runtime = TransportV2Runtime::new([0x11; 32], SessionManager::new());
        let failed = session(0x22);
        runtime.set_user_for_test(Arc::clone(&failed)).unwrap();

        let result = retire_session_on_failed_stream_item(
            &runtime,
            &failed,
            Err(Error::InvalidResponse("truncated stream".to_string())),
        );
        assert!(result.is_err());
        assert!(runtime.user().unwrap().is_none());

        let replacement = session(0x33);
        runtime.set_user_for_test(Arc::clone(&replacement)).unwrap();
        let result = retire_session_on_failed_stream_item(
            &runtime,
            &failed,
            Err(Error::InvalidResponse("late failed stream".to_string())),
        );
        assert!(result.is_err());
        assert!(Arc::ptr_eq(
            &runtime.user().unwrap().expect("replacement remains"),
            &replacement,
        ));
    }

    #[test]
    fn authenticated_stream_error_preserves_the_bound_session() {
        let runtime = TransportV2Runtime::new([0x11; 32], SessionManager::new());
        let session = session(0x22);
        runtime.set_user_for_test(Arc::clone(&session)).unwrap();

        let result = retire_session_on_failed_stream_item(
            &runtime,
            &session,
            Err(Error::Api {
                status: 503,
                message: "authenticated application error".to_string(),
            }),
        );
        assert!(result.is_err());
        assert!(Arc::ptr_eq(
            &runtime.user().unwrap().expect("session remains"),
            &session,
        ));
    }

    #[test]
    fn authenticated_stream_session_exhaustion_retires_the_bound_session() {
        let session = session(0x22);
        let runtime = runtime_with_user(Arc::clone(&session));

        let result = retire_session_on_failed_stream_item(
            &runtime,
            &session,
            Err(Error::Api {
                status: 503,
                message: r#"{"error":{"code":"session_exhausted","message":"Session response capacity is exhausted"}}"#.to_string(),
            }),
        );
        assert!(result.is_err());
        assert!(runtime.user().unwrap().is_none());
    }
}
