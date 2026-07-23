use async_trait::async_trait;
use futures_util::{StreamExt, TryStreamExt};
use goose_providers::base::{MessageStream, Provider};
use goose_providers::conversation::message::Message;
use goose_providers::errors::ProviderError;
use goose_providers::formats::openai::{
    create_request_with_options, response_to_streaming_message, OpenAiFormatOptions,
};
use goose_providers::http_status::is_context_length_exceeded_message;
use goose_providers::images::ImageFormat;
use goose_providers::model::ModelConfig;
use goose_providers::request_log::{start_log, LoggerHandleExt};
use goose_providers::retry::{should_retry, RetryConfig};
use opensecret::{InferenceRequest, InferenceResponse, OpenSecretClient, OpenSecretResponseBody};
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

const CHAT_COMPLETIONS_PATH: &str = "/v1/chat/completions";
pub(super) const MAPLE_PROVIDER_NAME: &str = "maple";
const MAX_ERROR_BODY_BYTES: usize = 16 * 1024;
const MAX_STREAM_LINE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RETRY_AFTER_SECS: f64 = 3_600.0;
#[cfg(not(test))]
const RESPONSE_START_TIMEOUT: Duration = Duration::from_secs(300);
#[cfg(test)]
const RESPONSE_START_TIMEOUT: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
#[cfg(test)]
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_millis(100);

tokio::task_local! {
    static MAPLE_RUN_CANCELLATION: CancellationToken;
}

pub(crate) async fn with_run_cancellation<F>(
    cancellation: CancellationToken,
    future: F,
) -> F::Output
where
    F: Future,
{
    MAPLE_RUN_CANCELLATION.scope(cancellation, future).await
}

fn current_run_cancellation() -> CancellationToken {
    MAPLE_RUN_CANCELLATION
        .try_with(CancellationToken::clone)
        .unwrap_or_default()
}

fn cancellation_error() -> ProviderError {
    ProviderError::ExecutionError("Maple request cancelled".to_string())
}

/// Authenticated, encrypted delivery for a caller-owned OpenSecret inference request.
///
/// The provider intentionally knows nothing about token storage or refresh. The
/// application auth session can implement this trait and select its current SDK
/// client at the start of every call (including Goose retries).
#[async_trait]
pub(crate) trait MapleInferenceTransport: Send + Sync {
    async fn send_inference_request(
        self: Arc<Self>,
        request: InferenceRequest,
        cancel_token: CancellationToken,
    ) -> opensecret::Result<InferenceResponse>;
}

/// A direct SDK client is also a valid transport. Maple's account-scoped auth
/// session can wrap this implementation when it needs to atomically replace the
/// active client after browser credentials change.
#[async_trait]
impl MapleInferenceTransport for OpenSecretClient {
    async fn send_inference_request(
        self: Arc<Self>,
        request: InferenceRequest,
        cancel_token: CancellationToken,
    ) -> opensecret::Result<InferenceResponse> {
        tokio::select! {
            biased;
            _ = cancel_token.cancelled() => {
                Err(opensecret::Error::Other("Inference request was cancelled".to_string()))
            }
            response = OpenSecretClient::send_inference_request(&self, request) => response,
        }
    }
}

pub(crate) struct MapleProvider {
    transport: Arc<dyn MapleInferenceTransport>,
}

impl MapleProvider {
    pub(crate) fn new<T>(transport: Arc<T>) -> Self
    where
        T: MapleInferenceTransport + 'static,
    {
        Self { transport }
    }

    fn build_request(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<Value, ProviderError> {
        create_request_with_options(
            model_config,
            system,
            messages,
            tools,
            &ImageFormat::OpenAi,
            true,
            OpenAiFormatOptions {
                preserve_thinking_context: true,
            },
        )
        .map_err(|error| {
            ProviderError::RequestFailed(format!("Failed to create Maple request: {error}"))
        })
    }

    fn inference_request(payload: Vec<u8>) -> Result<InferenceRequest, ProviderError> {
        let mut request = InferenceRequest::new(payload.into());
        *request.method_mut() = tauri::http::Method::POST;
        *request.uri_mut() = tauri::http::Uri::from_static(CHAT_COMPLETIONS_PATH);
        request.headers_mut().insert(
            tauri::http::header::ACCEPT,
            tauri::http::HeaderValue::from_static("text/event-stream"),
        );
        request.headers_mut().insert(
            tauri::http::header::CONTENT_TYPE,
            tauri::http::HeaderValue::from_static("application/json"),
        );
        Ok(request)
    }

    async fn send_attempt(
        &self,
        request: InferenceRequest,
        cancellation: &CancellationToken,
    ) -> Result<InferenceResponse, ProviderError> {
        // The transport owns authentication reconciliation and must get a chance
        // to finish it even when the parent run is cancelled or response headers
        // take too long. Cancelling and then awaiting the transport future keeps
        // a rotated SDK JWT from being stranded only in native memory.
        let transport_cancellation = cancellation.child_token();
        let response = Arc::clone(&self.transport)
            .send_inference_request(request, transport_cancellation.clone());
        tokio::pin!(response);
        let response_start_timeout = tokio::time::sleep(RESPONSE_START_TIMEOUT);
        tokio::pin!(response_start_timeout);

        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                transport_cancellation.cancel();
                let _ = response.await;
                Err(cancellation_error())
            }
            _ = &mut response_start_timeout => {
                transport_cancellation.cancel();
                let _ = response.await;
                Err(ProviderError::NetworkError(
                    "The Maple request timed out".to_string()
                ))
            }
            response = &mut response => {
                response.map_err(map_opensecret_error)
            }
        }
    }

    async fn send_with_retry(
        &self,
        payload_bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<InferenceResponse, ProviderError> {
        let config = Provider::retry_config(self);
        let mut attempts = 0;
        let mut auth_retried = false;

        loop {
            let request = Self::inference_request(payload_bytes.to_vec())?;
            let result = match self.send_attempt(request, cancellation).await {
                Ok(response) => ensure_success(response).await,
                Err(error) => Err(error),
            };
            match result {
                Ok(response) => return Ok(response),
                Err(error) => {
                    if matches!(error, ProviderError::Authentication(_)) && !auth_retried {
                        auth_retried = true;
                        if self.refresh_credentials().await.is_ok() {
                            continue;
                        }
                    }

                    if !should_retry(&error, &config) || attempts >= config.max_retries() {
                        return Err(error);
                    }
                    attempts += 1;
                    let delay = match &error {
                        ProviderError::RateLimitExceeded {
                            retry_delay: Some(provider_delay),
                            ..
                        } => *provider_delay,
                        _ => config.delay_for_attempt(attempts),
                    };
                    let skip_backoff = std::env::var("GOOSE_PROVIDER_SKIP_BACKOFF")
                        .unwrap_or_default()
                        .parse::<bool>()
                        .unwrap_or(false);
                    if !skip_backoff {
                        tokio::select! {
                            biased;
                            _ = cancellation.cancelled() => return Err(cancellation_error()),
                            _ = tokio::time::sleep(delay) => {}
                        }
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Provider for MapleProvider {
    fn get_name(&self) -> &str {
        MAPLE_PROVIDER_NAME
    }

    fn retry_config(&self) -> RetryConfig {
        // Retrying deterministic client failures can repeat side effects and
        // causes the SDK to repeat its own stale-session recovery for a 400.
        RetryConfig::default().transient_only()
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let payload = self.build_request(model_config, system, messages, tools)?;
        let payload_bytes = serde_json::to_vec(&payload).map_err(|error| {
            ProviderError::RequestFailed(format!("Failed to serialize Maple request: {error}"))
        })?;
        let mut request_log = start_log(model_config, &payload)?;
        let cancellation = current_run_cancellation();

        let response = self
            .send_with_retry(&payload_bytes, &cancellation)
            .await
            .inspect_err(|error| {
                let _ = request_log.error(error);
            })?;

        let response_stream = futures_util::stream::unfold(
            (response.into_body(), cancellation, false),
            |(mut body, cancellation, finished)| async move {
                if finished {
                    return None;
                }
                let item = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Maple request cancelled",
                    )),
                    next = tokio::time::timeout(STREAM_IDLE_TIMEOUT, body.next()) => {
                        match next {
                            Ok(Some(Ok(chunk))) => Ok(chunk),
                            Ok(Some(Err(error))) => Err(map_response_stream_error(error)),
                            Ok(None) => return None,
                            Err(_) => Err(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "Maple response stream timed out",
                            )),
                        }
                    }
                };
                let finished = item.is_err();
                Some((item, (body, cancellation, finished)))
            },
        );
        let reader = StreamReader::new(Box::pin(response_stream));
        let lines = FramedRead::new(
            reader,
            LinesCodec::new_with_max_length(MAX_STREAM_LINE_BYTES),
        )
        .map_err(anyhow::Error::from);
        let parsed = response_to_streaming_message(lines);

        let stream = parsed.map(move |result| {
            let (message, usage) = result.map_err(|_| invalid_stream_error())?;
            request_log.write(&message, usage.as_ref().map(|value| &value.usage))?;
            Ok((message, usage))
        });

        Ok(Box::pin(stream))
    }
}

fn invalid_stream_error() -> ProviderError {
    // Goose's parser error may contain the decrypted SSE line. Keep both the
    // application log and the error returned to the UI on a fixed category.
    log::warn!("Failed to parse Maple inference response stream (openai_stream_parser)");
    ProviderError::NetworkError("Maple's response stream was invalid".to_string())
}

async fn ensure_success(response: InferenceResponse) -> Result<InferenceResponse, ProviderError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let retry_after_header = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let (_parts, body) = response.into_parts();
    let (body, truncated) = collect_bounded_body(body).await?;
    let payload = error_payload(&body, truncated);
    let retry_delay = retry_after_delay(payload.as_ref(), retry_after_header.as_deref());
    let error = map_http_error(status, payload.as_ref());

    match error {
        ProviderError::RateLimitExceeded { details, .. } => Err(ProviderError::RateLimitExceeded {
            details,
            retry_delay,
        }),
        error => Err(error),
    }
}

async fn collect_bounded_body(
    mut body: OpenSecretResponseBody,
) -> Result<(Vec<u8>, bool), ProviderError> {
    let mut collected = Vec::new();
    let mut truncated = false;
    let cancellation = current_run_cancellation();

    loop {
        let next = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(cancellation_error()),
            next = tokio::time::timeout(STREAM_IDLE_TIMEOUT, body.next()) => {
                next.map_err(|_| ProviderError::NetworkError(
                    "Maple's error response stream timed out".to_string()
                ))?
            }
        };
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.map_err(|error| {
            log::warn!(
                "Failed to read encrypted Maple error response ({})",
                opensecret_error_category(&error)
            );
            ProviderError::NetworkError("Maple's encrypted response stream failed".to_string())
        })?;
        if chunk.is_empty() {
            continue;
        }

        let remaining = MAX_ERROR_BODY_BYTES.saturating_sub(collected.len());
        if chunk.len() > remaining {
            collected.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        collected.extend_from_slice(&chunk);
    }

    Ok((collected, truncated))
}

fn error_payload(body: &[u8], truncated: bool) -> Option<Value> {
    if body.is_empty() {
        return None;
    }
    if !truncated {
        if let Ok(payload) = serde_json::from_slice(body) {
            return Some(payload);
        }
    }

    let mut message = String::from_utf8_lossy(body).into_owned();
    if truncated {
        message.push_str(" [response truncated]");
    }
    Some(json!({ "message": message }))
}

fn retry_after_delay(payload: Option<&Value>, header: Option<&str>) -> Option<Duration> {
    let body_seconds = payload
        .and_then(|payload| payload.get("error"))
        .and_then(|error| error.get("metadata"))
        .and_then(|metadata| metadata.get("retry_after_seconds"))
        .and_then(Value::as_f64);
    body_seconds
        .and_then(retry_duration_from_seconds)
        .or_else(|| header.and_then(parse_retry_after_header))
}

fn retry_duration_from_seconds(seconds: f64) -> Option<Duration> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    Some(Duration::from_secs_f64(seconds.min(MAX_RETRY_AFTER_SECS)))
}

fn parse_retry_after_header(value: &str) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return retry_duration_from_seconds(seconds as f64);
    }

    let retry_at = httpdate::parse_http_date(value).ok()?;
    let delay = retry_at
        .duration_since(SystemTime::now())
        .unwrap_or(Duration::ZERO);
    retry_duration_from_seconds(delay.as_secs_f64())
}

fn map_http_error(status: tauri::http::StatusCode, payload: Option<&Value>) -> ProviderError {
    log::warn!(
        "Maple inference request failed (http_status_{})",
        status.as_u16()
    );

    match status {
        tauri::http::StatusCode::UNAUTHORIZED | tauri::http::StatusCode::FORBIDDEN => {
            ProviderError::Authentication("Maple authentication failed".to_string())
        }
        tauri::http::StatusCode::NOT_FOUND => ProviderError::EndpointNotFound(
            "The Maple inference endpoint was not found".to_string(),
        ),
        tauri::http::StatusCode::PAYMENT_REQUIRED => ProviderError::CreditsExhausted {
            details: "Maple credits are exhausted".to_string(),
            top_up_url: None,
        },
        tauri::http::StatusCode::PAYLOAD_TOO_LARGE => ProviderError::ContextLengthExceeded(
            "The Maple request exceeds the model's context window".to_string(),
        ),
        tauri::http::StatusCode::BAD_REQUEST
            if error_message(payload).is_some_and(is_context_length_exceeded_message) =>
        {
            ProviderError::ContextLengthExceeded(
                "The Maple request exceeds the model's context window".to_string(),
            )
        }
        tauri::http::StatusCode::BAD_REQUEST => {
            ProviderError::RequestFailed("Maple rejected the inference request (400)".to_string())
        }
        tauri::http::StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimitExceeded {
            details: "Maple rate limit exceeded".to_string(),
            retry_delay: None,
        },
        _ if status.is_server_error() => ProviderError::ServerError(format!(
            "Maple's server returned status {}",
            status.as_u16()
        )),
        _ => ProviderError::RequestFailed(format!(
            "Maple request failed with status {}",
            status.as_u16()
        )),
    }
}

fn error_message(payload: Option<&Value>) -> Option<&str> {
    payload.and_then(|payload| {
        payload
            .get("error")
            .and_then(|error| error.get("message"))
            .or_else(|| payload.get("message"))
            .and_then(Value::as_str)
    })
}

fn map_opensecret_error(error: opensecret::Error) -> ProviderError {
    log::warn!(
        "OpenSecret inference transport failed ({})",
        opensecret_error_category(&error)
    );
    match error {
        opensecret::Error::Authentication(_)
        | opensecret::Error::Api {
            status: 401 | 403, ..
        } => ProviderError::Authentication("Maple authentication failed".to_string()),
        opensecret::Error::Api {
            status: 402,
            message: _,
        } => ProviderError::CreditsExhausted {
            details: "Maple credits are exhausted".to_string(),
            top_up_url: None,
        },
        opensecret::Error::Api {
            status: 413,
            message: _,
        } => ProviderError::ContextLengthExceeded(
            "The Maple request exceeds the model's context window".to_string(),
        ),
        opensecret::Error::Api {
            status: 400,
            message,
        } if is_context_length_exceeded_message(&message) => ProviderError::ContextLengthExceeded(
            "The Maple request exceeds the model's context window".to_string(),
        ),
        opensecret::Error::Api {
            status: 429,
            message: _,
        } => ProviderError::RateLimitExceeded {
            details: "Maple rate limit exceeded".to_string(),
            retry_delay: None,
        },
        opensecret::Error::Api { status, message: _ } if (500..=599).contains(&status) => {
            ProviderError::ServerError(format!("Maple's server returned status {status}"))
        }
        opensecret::Error::Api {
            status: 404,
            message: _,
        } => ProviderError::EndpointNotFound(
            "The Maple inference endpoint was not found".to_string(),
        ),
        opensecret::Error::Api { status, message: _ } => {
            ProviderError::RequestFailed(format!("Maple request failed with status {status}"))
        }
        opensecret::Error::Http(error) => {
            if error.is_timeout() {
                ProviderError::NetworkError("The Maple request timed out".to_string())
            } else if error.is_connect() {
                ProviderError::NetworkError("Could not connect to Maple".to_string())
            } else {
                ProviderError::NetworkError("The Maple network request failed".to_string())
            }
        }
        opensecret::Error::AttestationVerificationFailed(_) => ProviderError::ExecutionError(
            "Maple could not verify the secure server connection".to_string(),
        ),
        opensecret::Error::Session(_)
        | opensecret::Error::KeyExchange(_)
        | opensecret::Error::Encryption(_)
        | opensecret::Error::Decryption(_)
        | opensecret::Error::InvalidResponse(_)
        | opensecret::Error::Crypto(_)
        | opensecret::Error::Cbor(_)
        | opensecret::Error::Io(_)
        | opensecret::Error::Utf8(_)
        | opensecret::Error::Base64Decode(_) => {
            ProviderError::NetworkError("Maple's encrypted connection failed".to_string())
        }
        opensecret::Error::Serialization(_)
        | opensecret::Error::Configuration(_)
        | opensecret::Error::Other(_) => ProviderError::ExecutionError(
            "Maple could not prepare the encrypted request".to_string(),
        ),
    }
}

fn map_response_stream_error(error: opensecret::Error) -> std::io::Error {
    log::warn!(
        "Failed to read encrypted Maple response stream ({})",
        opensecret_error_category(&error)
    );
    std::io::Error::other("Maple's encrypted response stream failed")
}

pub(crate) fn opensecret_error_category(error: &opensecret::Error) -> &'static str {
    match error {
        opensecret::Error::Http(_) => "http",
        opensecret::Error::Serialization(_) => "serialization",
        opensecret::Error::Cbor(_) => "cbor",
        opensecret::Error::Crypto(_) => "crypto",
        opensecret::Error::AttestationVerificationFailed(_) => "attestation",
        opensecret::Error::Session(_) => "session",
        opensecret::Error::KeyExchange(_) => "key_exchange",
        opensecret::Error::Encryption(_) => "encryption",
        opensecret::Error::Decryption(_) => "decryption",
        opensecret::Error::Authentication(_) => "authentication",
        opensecret::Error::InvalidResponse(_) => "invalid_response",
        opensecret::Error::Api { .. } => "api",
        opensecret::Error::Configuration(_) => "configuration",
        opensecret::Error::Io(_) => "io",
        opensecret::Error::Utf8(_) => "utf8",
        opensecret::Error::Base64Decode(_) => "base64",
        opensecret::Error::Other(_) => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_providers::base::collect_stream;
    use goose_providers::conversation::message::MessageContent;
    use goose_providers::retry::should_retry;
    use rmcp::object;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    #[derive(Debug)]
    struct CapturedRequest {
        method: String,
        uri: String,
        accept: Option<String>,
        body: Value,
    }

    struct FakeTransport {
        requests: Mutex<Vec<CapturedRequest>>,
        responses: Mutex<VecDeque<opensecret::Result<InferenceResponse>>>,
    }

    struct PendingTransport;

    #[async_trait]
    impl MapleInferenceTransport for PendingTransport {
        async fn send_inference_request(
            self: Arc<Self>,
            _request: InferenceRequest,
            cancel_token: CancellationToken,
        ) -> opensecret::Result<InferenceResponse> {
            cancel_token.cancelled().await;
            Err(opensecret::Error::Other(
                "Pending transport was cancelled".to_string(),
            ))
        }
    }

    impl FakeTransport {
        fn new(response: InferenceResponse) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(VecDeque::from([Ok(response)])),
            }
        }
    }

    #[async_trait]
    impl MapleInferenceTransport for FakeTransport {
        async fn send_inference_request(
            self: Arc<Self>,
            request: InferenceRequest,
            _cancel_token: CancellationToken,
        ) -> opensecret::Result<InferenceResponse> {
            let (parts, body) = request.into_parts();
            let captured = CapturedRequest {
                method: parts.method.to_string(),
                uri: parts.uri.to_string(),
                accept: parts
                    .headers
                    .get("accept")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned),
                body: serde_json::from_slice(&body).expect("request body should be JSON"),
            };
            self.requests.lock().expect("request lock").push(captured);
            self.responses
                .lock()
                .expect("response lock")
                .pop_front()
                .expect("a fake response should be queued")
        }
    }

    fn response(status: u16, chunks: Vec<Vec<u8>>, retry_after: Option<&str>) -> InferenceResponse {
        let body: OpenSecretResponseBody = Box::pin(futures_util::stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok::<_, opensecret::Error>(chunk.into())),
        ));
        let mut response = InferenceResponse::new(body);
        *response.status_mut() =
            tauri::http::StatusCode::from_u16(status).expect("valid fake status");
        if let Some(retry_after) = retry_after {
            response.headers_mut().insert(
                tauri::http::header::RETRY_AFTER,
                tauri::http::HeaderValue::from_str(retry_after).expect("valid retry header"),
            );
        }
        response
    }

    fn fragmented_success_response() -> InferenceResponse {
        let response_bytes = concat!(
            "data: {\"id\":\"chunk-1\",\"object\":\"chat.completion.chunk\",",
            "\"created\":1,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},",
            "\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chunk-2\",\"object\":\"chat.completion.chunk\",",
            "\"created\":2,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chunk-3\",\"object\":\"chat.completion.chunk\",",
            "\"created\":3,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{},\"finish_reason\":\"stop\"}],",
            "\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        )
        .as_bytes();
        response(
            200,
            vec![
                response_bytes[..23].to_vec(),
                response_bytes[23..91].to_vec(),
                response_bytes[91..response_bytes.len() - 7].to_vec(),
                response_bytes[response_bytes.len() - 7..].to_vec(),
            ],
            None,
        )
    }

    fn pending_success_response() -> InferenceResponse {
        let body: OpenSecretResponseBody = Box::pin(futures_util::stream::pending());
        let mut response = InferenceResponse::new(body);
        *response.status_mut() = tauri::http::StatusCode::OK;
        response
    }

    #[tokio::test]
    async fn formats_openai_request_and_preserves_images_and_thinking() {
        let transport = Arc::new(FakeTransport::new(fragmented_success_response()));
        let provider = MapleProvider::new(transport.clone());
        let messages = vec![
            Message::user()
                .with_text("What is in this image?")
                .with_image("aGVsbG8=", "image/png"),
            Message::assistant()
                .with_content(MessageContent::thinking("private chain", ""))
                .with_text("Prior answer"),
        ];
        let model_config =
            ModelConfig::new("test-model").with_merged_request_params(HashMap::from([
                ("include_reasoning".to_string(), json!(false)),
                (
                    "chat_template_kwargs".to_string(),
                    json!({ "enable_thinking": false }),
                ),
            ]));

        let stream = provider
            .stream(&model_config, "Maple system prompt", &messages, &[])
            .await
            .expect("stream should start");
        let _ = collect_stream(stream).await.expect("stream should parse");

        assert_eq!(provider.get_name(), MAPLE_PROVIDER_NAME);
        let requests = transport.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, "POST");
        assert_eq!(request.uri, CHAT_COMPLETIONS_PATH);
        assert_eq!(request.accept.as_deref(), Some("text/event-stream"));
        assert_eq!(request.body["model"], "test-model");
        assert_eq!(request.body["stream"], true);
        assert_eq!(request.body["stream_options"]["include_usage"], true);
        assert_eq!(request.body["include_reasoning"], false);
        assert_eq!(
            request.body["chat_template_kwargs"]["enable_thinking"],
            false
        );
        assert_eq!(request.body["messages"][0]["role"], "system");
        assert_eq!(
            request.body["messages"][0]["content"],
            "Maple system prompt"
        );
        assert_eq!(
            request.body["messages"][2]["reasoning_content"],
            "private chain"
        );
        assert_eq!(
            request.body["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,aGVsbG8="
        );
    }

    #[tokio::test]
    async fn reassembles_fragmented_sse_chunks_before_openai_parsing() {
        let provider =
            MapleProvider::new(Arc::new(FakeTransport::new(fragmented_success_response())));

        let stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await
            .expect("stream should start");
        let (message, usage) = collect_stream(stream).await.expect("stream should parse");
        let text = message
            .content
            .iter()
            .filter_map(|content| match content {
                MessageContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<String>();

        assert_eq!(text, "Hello world");
        assert_eq!(usage.model, "test-model");
        assert_eq!(usage.usage.input_tokens, Some(2));
        assert_eq!(usage.usage.output_tokens, Some(3));
        assert_eq!(usage.usage.total_tokens, Some(5));
    }

    #[tokio::test]
    async fn maps_rate_limit_without_exposing_body_and_preserves_retry_hint() {
        let response = response(
            429,
            vec![br#"{"error":{"message":"private upstream detail"}}"#.to_vec()],
            Some("7"),
        );

        let error = match ensure_success(response).await {
            Ok(_) => panic!("429 should fail"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ProviderError::RateLimitExceeded {
                details: "Maple rate limit exceeded".to_string(),
                retry_delay: Some(Duration::from_secs(7)),
            }
        );
    }

    #[test]
    fn invalid_body_retry_hint_falls_back_to_retry_after_header() {
        let valid = json!({
            "error": { "metadata": { "retry_after_seconds": 2.5 } }
        });
        assert_eq!(
            retry_after_delay(Some(&valid), Some("7")),
            Some(Duration::from_secs_f64(2.5))
        );

        let invalid = json!({
            "error": { "metadata": { "retry_after_seconds": "not-a-number" } }
        });
        assert_eq!(
            retry_after_delay(Some(&invalid), Some("7")),
            Some(Duration::from_secs(7))
        );

        let negative = json!({
            "error": { "metadata": { "retry_after_seconds": -1 } }
        });
        assert_eq!(
            retry_after_delay(Some(&negative), Some("9")),
            Some(Duration::from_secs(9))
        );
    }

    #[test]
    fn parses_http_date_retry_after_header() {
        let retry_at = SystemTime::now() + Duration::from_secs(120);
        let header = httpdate::fmt_http_date(retry_at);
        let delay = retry_after_delay(None, Some(&header)).expect("HTTP date should parse");

        assert!(delay >= Duration::from_secs(118));
        assert!(delay <= Duration::from_secs(120));
    }

    #[tokio::test]
    async fn maps_common_http_failures_to_typed_provider_errors() {
        let unauthorized = ensure_success(response(
            401,
            vec![br#"{"error":{"message":"expired"}}"#.to_vec()],
            None,
        ))
        .await;
        assert!(matches!(
            unauthorized,
            Err(ProviderError::Authentication(_))
        ));

        let context = ensure_success(response(
            400,
            vec![br#"{"error":{"message":"maximum context length exceeded"}}"#.to_vec()],
            None,
        ))
        .await;
        assert!(matches!(
            context,
            Err(ProviderError::ContextLengthExceeded(ref message))
                if message == "The Maple request exceeds the model's context window"
        ));

        let credits = ensure_success(response(
            402,
            vec![br#"{"error":{"message":"insufficient credits"}}"#.to_vec()],
            None,
        ))
        .await;
        assert!(matches!(
            credits,
            Err(ProviderError::CreditsExhausted { .. })
        ));

        let server = ensure_success(response(
            503,
            vec![br#"{"error":{"message":"temporarily unavailable"}}"#.to_vec()],
            None,
        ))
        .await;
        assert!(matches!(server, Err(ProviderError::ServerError(_))));
    }

    #[tokio::test]
    async fn http_and_sdk_error_details_are_redacted_from_returned_errors() {
        const PRIVATE_DETAIL: &str = "tenant-secret-provider-debug-message";

        for status in [400, 401, 402, 404, 413, 429, 500] {
            let body = serde_json::to_vec(&json!({
                "error": { "message": PRIVATE_DETAIL }
            }))
            .expect("error payload should serialize");
            let error = match ensure_success(response(status, vec![body], None)).await {
                Ok(_) => panic!("non-success response should fail"),
                Err(error) => error,
            };
            assert!(!error.to_string().contains(PRIVATE_DETAIL));
            assert!(!format!("{error:?}").contains(PRIVATE_DETAIL));

            let sdk_error = map_opensecret_error(opensecret::Error::Api {
                status,
                message: PRIVATE_DETAIL.to_string(),
            });
            assert!(!sdk_error.to_string().contains(PRIVATE_DETAIL));
            assert!(!format!("{sdk_error:?}").contains(PRIVATE_DETAIL));
        }
    }

    #[test]
    fn sdk_context_error_is_classified_without_exposing_provider_message() {
        let error = map_opensecret_error(opensecret::Error::Api {
            status: 400,
            message: "maximum context length exceeded; private token counts".to_string(),
        });

        assert_eq!(
            error,
            ProviderError::ContextLengthExceeded(
                "The Maple request exceeds the model's context window".to_string()
            )
        );
        assert!(!error.to_string().contains("private token counts"));
    }

    #[tokio::test]
    async fn stalled_error_response_stream_has_a_bounded_idle_timeout() {
        let mut response = pending_success_response();
        *response.status_mut() = tauri::http::StatusCode::SERVICE_UNAVAILABLE;

        let error = match ensure_success(response).await {
            Ok(_) => panic!("stalled error body should time out"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ProviderError::NetworkError("Maple's error response stream timed out".to_string())
        );
    }

    #[tokio::test]
    async fn malformed_sse_is_a_typed_stream_error() {
        const PRIVATE_MALFORMED_LINE: &str = "private-decrypted-malformed-completion";
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(response(
            200,
            vec![format!("data: {PRIVATE_MALFORMED_LINE}\n\ndata: [DONE]\n\n").into_bytes()],
            None,
        ))));
        let stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await
            .expect("stream setup should succeed");

        let error = collect_stream(stream)
            .await
            .expect_err("malformed completion data should fail");
        assert_eq!(
            error,
            ProviderError::NetworkError("Maple's response stream was invalid".to_string())
        );
        assert!(!error.to_string().contains(PRIVATE_MALFORMED_LINE));
        assert!(!format!("{error:?}").contains(PRIVATE_MALFORMED_LINE));
    }

    #[tokio::test]
    async fn deterministic_client_errors_are_not_retried() {
        let transport = Arc::new(FakeTransport::new(response(
            400,
            vec![br#"{"error":{"message":"invalid model argument"}}"#.to_vec()],
            None,
        )));
        let provider = MapleProvider::new(Arc::clone(&transport));

        let result = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await;

        assert!(matches!(result, Err(ProviderError::RequestFailed(_))));
        assert_eq!(transport.requests.lock().expect("request lock").len(), 1);
        let retry_config = Provider::retry_config(&provider);
        assert!(!should_retry(
            &ProviderError::RequestFailed("invalid".to_string()),
            &retry_config
        ));
        assert!(should_retry(
            &ProviderError::ServerError("temporary".to_string()),
            &retry_config
        ));
        assert!(should_retry(
            &ProviderError::RateLimitExceeded {
                details: "slow down".to_string(),
                retry_delay: None,
            },
            &retry_config
        ));
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_request_before_response_start() {
        let provider = MapleProvider::new(Arc::new(PendingTransport));
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let result = with_run_cancellation(
            cancellation,
            provider.stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            ),
        )
        .await;

        assert!(
            matches!(result, Err(ProviderError::ExecutionError(message)) if message.contains("cancelled"))
        );
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_response_stream() {
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(pending_success_response())));
        let cancellation = CancellationToken::new();
        let mut stream = with_run_cancellation(
            cancellation.clone(),
            provider.stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            ),
        )
        .await
        .expect("stream should start");

        cancellation.cancel();
        let result = stream
            .next()
            .await
            .expect("cancelled stream should finish with an error");
        assert!(matches!(result, Err(ProviderError::NetworkError(_))));
    }

    #[tokio::test]
    async fn stalled_response_stream_has_a_bounded_idle_timeout() {
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(pending_success_response())));
        let mut stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await
            .expect("stream should start");

        let result = stream
            .next()
            .await
            .expect("idle timeout should emit an error");
        assert!(matches!(result, Err(ProviderError::NetworkError(_))));
    }

    #[tokio::test]
    async fn reconstructs_fragmented_parallel_tool_calls_with_empty_finish_reasons_and_formats_schema(
    ) {
        let sse = concat!(
            "data: {\"id\":\"tools-1\",\"object\":\"chat.completion.chunk\",",
            "\"created\":1,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{\"role\":\"assistant\",\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",",
            "\"function\":{\"name\":\"web_search\",\"arguments\":\"{\\\"query\\\":\\\"map\"}},",
            "{\"index\":1,\"id\":\"call-2\",\"type\":\"function\",",
            "\"function\":{\"name\":\"web_search\",\"arguments\":\"{\\\"query\\\":\\\"kag\"}}]},",
            "\"finish_reason\":\"\"}]}\n\n",
            "data: {\"id\":\"tools-2\",\"object\":\"chat.completion.chunk\",",
            "\"created\":2,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"function\":{\"arguments\":\"le\\\"}\"}},",
            "{\"index\":1,\"function\":{\"arguments\":\"i\\\"}\"}}]},",
            "\"finish_reason\":\"\"}]}\n\n",
            "data: {\"id\":\"tools-3\",\"object\":\"chat.completion.chunk\",",
            "\"created\":3,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{},\"finish_reason\":\"tool_calls\"}],",
            "\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":5,\"total_tokens\":9}}\n\n",
            "data: [DONE]\n\n"
        );
        let split = sse.len() / 3;
        let transport = Arc::new(FakeTransport::new(response(
            200,
            vec![
                sse.as_bytes()[..split].to_vec(),
                sse.as_bytes()[split..split * 2].to_vec(),
                sse.as_bytes()[split * 2..].to_vec(),
            ],
            None,
        )));
        let provider = MapleProvider::new(Arc::clone(&transport));
        let tool = Tool::new(
            "web_search",
            "Search the web",
            object!({
                "type": "object",
                "$defs": {
                    "query_value": {
                        "oneOf": [
                            { "type": "string" },
                            { "type": "number" }
                        ]
                    }
                },
                "properties": {
                    "query": { "$ref": "#/$defs/query_value" }
                },
                "required": ["query"]
            }),
        );

        let stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("search twice")],
                &[tool],
            )
            .await
            .expect("stream should start");
        let (message, usage) = collect_stream(stream).await.expect("tools should parse");
        let calls = message
            .content
            .iter()
            .filter_map(|content| match content {
                MessageContent::ToolRequest(request) => request.tool_call.as_ref().ok(),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(
            calls[0]
                .arguments
                .as_ref()
                .and_then(|arguments| arguments.get("query")),
            Some(&json!("maple"))
        );
        assert_eq!(calls[1].name, "web_search");
        assert_eq!(
            calls[1]
                .arguments
                .as_ref()
                .and_then(|arguments| arguments.get("query")),
            Some(&json!("kagi"))
        );
        assert_eq!(usage.usage.total_tokens, Some(9));

        let requests = transport.requests.lock().expect("request lock");
        assert_eq!(requests[0].body["tools"][0]["type"], "function");
        assert_eq!(
            requests[0].body["tools"][0]["function"]["name"],
            "web_search"
        );
        assert_eq!(
            requests[0].body["tools"][0]["function"]["parameters"]["required"][0],
            "query"
        );
        let parameters = &requests[0].body["tools"][0]["function"]["parameters"];
        assert!(parameters["$defs"]["query_value"].get("oneOf").is_none());
        assert!(parameters["$defs"]["query_value"]["anyOf"].is_array());
    }

    #[tokio::test]
    async fn bounds_and_redacts_non_json_error_bodies() {
        let response = response(500, vec![vec![b'x'; MAX_ERROR_BODY_BYTES + 50]], None);
        let error = match ensure_success(response).await {
            Ok(_) => panic!("500 should fail"),
            Err(error) => error,
        };
        let ProviderError::ServerError(details) = error else {
            panic!("expected server error");
        };

        assert_eq!(details, "Maple's server returned status 500");
        assert!(!details.contains('x'));
    }
}
