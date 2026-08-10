#[cfg(test)]
use super::safeguard::ProposedActionReservation;
use super::safeguard::{
    AgentSafeguard, ProposedActionBudget, SafeguardToolCatalog, SafeguardTrustedUserRequest,
    SafeguardTurnContext,
};
use async_trait::async_trait;
use futures_util::{StreamExt, TryStreamExt};
use goose_providers::base::{collect_stream, MessageStream, Provider};
use goose_providers::conversation::message::{Message, MessageContent};
use goose_providers::conversation::token_usage::ProviderUsage;
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
use std::future::{ready, Future};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

const CHAT_COMPLETIONS_PATH: &str = "/v1/chat/completions";
pub(super) const MAPLE_PROVIDER_NAME: &str = "maple";
const KIMI_K3_MODEL_ID: &str = "kimi-k3";
// Agent Mode forwards the selected catalog ID unchanged for direct model
// selections. Keep Gemma's provider-specific opt-in scoped to that explicit
// selection; aliases and other reasoning models retain their existing behavior.
const GEMMA4_AGENT_MODEL_ID: &str = "gemma4-31b";
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
    static MAPLE_TRUSTED_USER_REQUEST: Option<SafeguardTrustedUserRequest>;
    static MAPLE_ACCOUNT_SCOPE: Option<String>;
    static MAPLE_SAFEGUARD_RUN_STATE: Arc<SafeguardRunState>;
}

#[derive(Default)]
struct SafeguardRunState {
    proposed_action_seen: AtomicBool,
}

impl SafeguardRunState {
    fn follows_untrusted_tool_output(&self) -> bool {
        self.proposed_action_seen.load(AtomicOrdering::Acquire)
    }

    fn mark_proposed_action(&self) {
        self.proposed_action_seen
            .store(true, AtomicOrdering::Release);
    }
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

pub(crate) async fn with_agent_run_context<F>(
    cancellation: CancellationToken,
    account_scope: Option<String>,
    trusted_user_request: Option<SafeguardTrustedUserRequest>,
    future: F,
) -> F::Output
where
    F: Future,
{
    let safeguard_run_state = Arc::new(SafeguardRunState::default());
    MAPLE_RUN_CANCELLATION
        .scope(
            cancellation,
            MAPLE_ACCOUNT_SCOPE.scope(
                account_scope,
                MAPLE_TRUSTED_USER_REQUEST.scope(
                    trusted_user_request,
                    MAPLE_SAFEGUARD_RUN_STATE.scope(safeguard_run_state, future),
                ),
            ),
        )
        .await
}

fn current_run_cancellation() -> CancellationToken {
    MAPLE_RUN_CANCELLATION
        .try_with(CancellationToken::clone)
        .unwrap_or_default()
}

fn current_trusted_user_request() -> Option<SafeguardTrustedUserRequest> {
    MAPLE_TRUSTED_USER_REQUEST
        .try_with(Clone::clone)
        .ok()
        .flatten()
}

fn current_account_scope() -> Option<String> {
    MAPLE_ACCOUNT_SCOPE.try_with(Clone::clone).ok().flatten()
}

fn current_safeguard_run_state() -> Option<Arc<SafeguardRunState>> {
    MAPLE_SAFEGUARD_RUN_STATE.try_with(Arc::clone).ok()
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
    safeguard: Option<Arc<dyn AgentSafeguard>>,
    safeguard_working_directory: Option<String>,
    #[cfg(test)]
    test_retry_config: Option<RetryConfig>,
}

impl MapleProvider {
    pub(crate) fn new<T>(transport: Arc<T>) -> Self
    where
        T: MapleInferenceTransport + 'static,
    {
        Self {
            transport,
            safeguard: None,
            safeguard_working_directory: None,
            #[cfg(test)]
            test_retry_config: None,
        }
    }

    pub(crate) fn with_safeguard(
        mut self,
        safeguard: Arc<dyn AgentSafeguard>,
        working_directory: String,
    ) -> Self {
        self.safeguard = Some(safeguard);
        self.safeguard_working_directory = Some(working_directory);
        self
    }

    #[cfg(test)]
    fn with_test_retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.test_retry_config = Some(retry_config);
        self
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
                thinking_preservation_format: None,
            },
        )
        .map_err(|error| {
            ProviderError::RequestFailed(format!("Failed to create Maple request: {error}"))
        })
    }

    fn gemma_agent_model_config(
        model_config: &ModelConfig,
        primary_agent_turn: bool,
    ) -> ModelConfig {
        let mut model_config = model_config.clone();
        if model_config.model_name != GEMMA4_AGENT_MODEL_ID {
            return model_config;
        }

        let request_params = model_config.request_params.get_or_insert_default();
        request_params
            .entry("include_reasoning".to_string())
            .or_insert_with(|| json!(primary_agent_turn));
        let template_kwargs = request_params
            .entry("chat_template_kwargs".to_string())
            .or_insert_with(|| json!({}));
        if let Some(template_kwargs) = template_kwargs.as_object_mut() {
            template_kwargs
                .entry("enable_thinking".to_string())
                .or_insert_with(|| json!(primary_agent_turn));
        }

        model_config
    }

    fn replace_legacy_kimi_k3_tool_ids(
        message: &mut Message,
        replacements: &mut std::collections::HashMap<String, String>,
    ) {
        // Temporary compatibility for Kimi K3 deployments built before vLLM
        // ab98034d4, where tool IDs are scoped to one assistant message and can
        // repeat across turns. Goose treats them as conversation identifiers,
        // so mirror current vLLM with a random ID per distinct call in this
        // inference while preserving same-response duplicate detection.
        // Remove this after Tinfoil's attested K3 image includes vLLM #50420.
        for content in &mut message.content {
            if let MessageContent::ToolRequest(request) = content {
                let legacy_response_local_id = match request.tool_call.as_ref() {
                    Ok(tool_call) => {
                        let tool_name = tool_call.name.as_ref();
                        request.id == tool_name
                            || request
                                .id
                                .strip_prefix(tool_name)
                                .is_some_and(|suffix| suffix.starts_with(':') && suffix.len() > 1)
                    }
                    Err(_) => !request.id.starts_with("chatcmpl-tool-"),
                };
                if legacy_response_local_id {
                    request.id = replacements
                        .entry(request.id.clone())
                        .or_insert_with(|| format!("chatcmpl-tool-{:032x}", rand::random::<u128>()))
                        .clone();
                }
            }
        }
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

    fn message_stream_from_response(
        response: InferenceResponse,
        cancellation: CancellationToken,
    ) -> MessageStream {
        let parser_cancellation = cancellation.clone();
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

        Box::pin(parsed.map(move |result| {
            result.map_err(|_| {
                if parser_cancellation.is_cancelled() {
                    cancellation_error()
                } else {
                    invalid_stream_error()
                }
            })
        }))
    }

    async fn stream_attempt(
        &self,
        payload_bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<MessageStream, ProviderError> {
        let request = Self::inference_request(payload_bytes.to_vec())?;
        let response = self.send_attempt(request, cancellation).await?;
        let response = ensure_success(response).await?;
        Ok(Self::message_stream_from_response(
            response,
            cancellation.clone(),
        ))
    }

    async fn stream_with_retry(
        &self,
        payload_bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<MessageStream, ProviderError> {
        let config = Provider::retry_config(self);
        let mut attempts = 0;
        let mut auth_retried = false;

        loop {
            let error = match self.stream_attempt(payload_bytes, cancellation).await {
                Ok(mut stream) => {
                    // TODO(upstream): Remove this Maple-specific bridge from Agent Mode once
                    // Maple's pinned Goose revision provides equivalent first-item handling:
                    // https://github.com/aaif-goose/goose/issues/10887
                    // If auxiliary complete() calls still need this protection, scope it to
                    // that path instead. Recovery after any successful item remains out of
                    // scope here:
                    // https://github.com/aaif-goose/goose/issues/10897
                    let first = tokio::select! {
                        biased;
                        _ = cancellation.cancelled() => return Err(cancellation_error()),
                        first = stream.next() => first,
                    };
                    match first {
                        Some(Ok(first)) => {
                            return Ok(Box::pin(
                                futures_util::stream::once(ready(Ok(first))).chain(stream),
                            ));
                        }
                        Some(Err(error)) => error,
                        None => return Ok(stream),
                    }
                }
                Err(error) => error,
            };

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

    async fn stream_request(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
        enable_primary_agent_thinking: bool,
    ) -> Result<MessageStream, ProviderError> {
        let effective_model_config =
            Self::gemma_agent_model_config(model_config, enable_primary_agent_thinking);
        let payload = self.build_request(&effective_model_config, system, messages, tools)?;
        let payload_bytes = serde_json::to_vec(&payload).map_err(|error| {
            ProviderError::RequestFailed(format!("Failed to serialize Maple request: {error}"))
        })?;
        let mut request_log = start_log(&effective_model_config, &payload)?;
        let cancellation = current_run_cancellation();

        let stream = self
            .stream_with_retry(&payload_bytes, &cancellation)
            .await
            .inspect_err(|error| {
                let _ = request_log.error(error);
            })?;
        let replace_legacy_kimi_k3_tool_ids = effective_model_config.model_name == KIMI_K3_MODEL_ID;
        let mut kimi_k3_tool_id_replacements = std::collections::HashMap::new();

        let stream = stream.map(move |result| {
            let (mut message, usage) = result?;
            if replace_legacy_kimi_k3_tool_ids {
                if let Some(message) = message.as_mut() {
                    Self::replace_legacy_kimi_k3_tool_ids(
                        message,
                        &mut kimi_k3_tool_id_replacements,
                    );
                }
            }
            request_log.write(&message, usage.as_ref().map(|value| &value.usage))?;
            Ok((message, usage))
        });

        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl Provider for MapleProvider {
    fn get_name(&self) -> &str {
        MAPLE_PROVIDER_NAME
    }

    fn retry_config(&self) -> RetryConfig {
        #[cfg(test)]
        if let Some(config) = &self.test_retry_config {
            return config.clone();
        }

        // Retrying deterministic client failures can repeat side effects and
        // causes the SDK to repeat its own stale-session recovery for a 400. One
        // shared transient budget covers both setup and pre-first-item failures.
        RetryConfig::default().transient_only()
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        // Goose uses stream for the interactive Agent loop. Selecting Gemma
        // directly in Agent Mode is the product-level opt-in to thinking.
        let cancellation = current_run_cancellation();
        let account_scope = current_account_scope();
        let trusted_user_request = current_trusted_user_request();
        let safeguard_run_state = current_safeguard_run_state();
        let session_id = goose::session_context::current_session_id();
        let safeguard_context = self
            .safeguard
            .as_ref()
            .zip(self.safeguard_working_directory.as_deref())
            .map(|(_, working_directory)| {
                SafeguardTurnContext::from_messages(
                    account_scope,
                    session_id,
                    working_directory,
                    trusted_user_request,
                    safeguard_run_state
                        .as_ref()
                        .is_some_and(|state| state.follows_untrusted_tool_output()),
                    messages,
                    &cancellation,
                )
            });
        if let (Some(safeguard), Some(context)) =
            (self.safeguard.as_ref(), safeguard_context.as_ref())
        {
            safeguard
                .inspect_untrusted_inputs(context, messages, &cancellation)
                .await;
        }
        if cancellation.is_cancelled() {
            return Err(cancellation_error());
        }

        let stream = self
            .stream_request(model_config, system, messages, tools, true)
            .await?;
        let (Some(safeguard), Some(context)) = (self.safeguard.as_ref(), safeguard_context) else {
            return Ok(stream);
        };
        let safeguard = Arc::clone(safeguard);
        let context = Arc::new(context);
        let safeguard_tools = Arc::new(SafeguardToolCatalog::from_tools(tools, &cancellation));
        safeguard.record_provider_preparation(
            &context,
            &safeguard_tools,
            cancellation.is_cancelled(),
        );
        if cancellation.is_cancelled() {
            return Err(cancellation_error());
        }
        let mut proposed_action_budget = ProposedActionBudget::default();
        let guarded_stream = stream.then(move |result| {
            let safeguard = Arc::clone(&safeguard);
            let safeguard_tools = Arc::clone(&safeguard_tools);
            let context = Arc::clone(&context);
            let cancellation = cancellation.clone();
            let safeguard_run_state = safeguard_run_state.clone();
            let reservation = result
                .as_ref()
                .ok()
                .and_then(|(message, _)| message.as_ref())
                .and_then(|message| {
                    proposed_action_budget.reserve_message(
                        message,
                        &cancellation,
                        context.preprocessing_exhausted()
                            || safeguard_tools.preprocessing_exhausted(),
                    )
                });
            async move {
                if let Some(reservation) = reservation {
                    let has_valid_action = reservation.has_valid_action();
                    if let Ok((Some(message), _)) = &result {
                        if reservation.should_inspect() {
                            safeguard
                                .inspect_proposed_actions(
                                    &context,
                                    message,
                                    &safeguard_tools,
                                    reservation,
                                    &cancellation,
                                )
                                .await;
                        }
                        if cancellation.is_cancelled() {
                            return Err(cancellation_error());
                        }
                        if has_valid_action {
                            if let Some(state) = safeguard_run_state {
                                state.mark_proposed_action();
                            }
                        }
                    }
                }
                if cancellation.is_cancelled() {
                    return Err(cancellation_error());
                }
                result
            }
        });
        Ok(Box::pin(guarded_stream))
    }

    async fn complete(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        // Goose and Maple use complete for auxiliary work such as compaction,
        // classifiers, and image descriptions; keep those requests non-thinking.
        let stream = self
            .stream_request(model_config, system, messages, tools, false)
            .await?;
        collect_stream(stream).await
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
    use goose_providers::conversation::message::MessageContent;
    use goose_providers::retry::should_retry;
    use rmcp::object;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::sync::Notify;

    #[derive(Debug)]
    struct CapturedRequest {
        method: String,
        uri: String,
        accept: Option<String>,
        raw_body: Vec<u8>,
        body: Value,
    }

    struct FakeTransport {
        requests: Mutex<Vec<CapturedRequest>>,
        responses: Mutex<VecDeque<opensecret::Result<InferenceResponse>>>,
        request_notify: Notify,
    }

    struct PendingTransport;

    struct BlockingSafeguard {
        action_entered: Notify,
        action_release: tokio::sync::Semaphore,
    }

    impl Default for BlockingSafeguard {
        fn default() -> Self {
            Self {
                action_entered: Notify::new(),
                action_release: tokio::sync::Semaphore::new(0),
            }
        }
    }

    #[derive(Default)]
    struct RecordingSafeguard {
        provider_preparations: AtomicUsize,
        untrusted_input_checks: AtomicUsize,
        proposed_action_checks: AtomicUsize,
        proposed_action_tools: AtomicUsize,
        proposed_action_follows: Mutex<Vec<bool>>,
    }

    #[async_trait]
    impl AgentSafeguard for RecordingSafeguard {
        fn record_provider_preparation(
            &self,
            _context: &SafeguardTurnContext,
            _tools: &SafeguardToolCatalog,
            _cancelled: bool,
        ) {
            self.provider_preparations.fetch_add(1, Ordering::SeqCst);
        }

        async fn inspect_untrusted_inputs(
            &self,
            _context: &SafeguardTurnContext,
            messages: &[Message],
            _cancel_token: &CancellationToken,
        ) {
            if messages.iter().any(|message| {
                message
                    .content
                    .iter()
                    .any(|content| matches!(content, MessageContent::ToolResponse(_)))
            }) {
                self.untrusted_input_checks.fetch_add(1, Ordering::SeqCst);
            }
        }

        async fn inspect_proposed_actions(
            &self,
            context: &SafeguardTurnContext,
            message: &Message,
            tools: &SafeguardToolCatalog,
            _reservation: ProposedActionReservation,
            _cancel_token: &CancellationToken,
        ) {
            if message
                .content
                .iter()
                .any(|content| matches!(content, MessageContent::ToolRequest(_)))
            {
                self.proposed_action_checks.fetch_add(1, Ordering::SeqCst);
                self.proposed_action_tools
                    .store(tools.len(), Ordering::SeqCst);
                self.proposed_action_follows
                    .lock()
                    .unwrap()
                    .push(context.follows_untrusted_tool_output());
            }
        }
    }

    #[async_trait]
    impl AgentSafeguard for BlockingSafeguard {
        async fn inspect_untrusted_inputs(
            &self,
            _context: &SafeguardTurnContext,
            _messages: &[Message],
            _cancel_token: &CancellationToken,
        ) {
        }

        async fn inspect_proposed_actions(
            &self,
            _context: &SafeguardTurnContext,
            _message: &Message,
            _tools: &SafeguardToolCatalog,
            _reservation: ProposedActionReservation,
            _cancel_token: &CancellationToken,
        ) {
            self.action_entered.notify_one();
            self.action_release
                .acquire()
                .await
                .expect("test semaphore remains open")
                .forget();
        }
    }

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
            Self::queued(vec![response])
        }

        fn with_responses(responses: Vec<InferenceResponse>) -> Self {
            Self::queued(responses)
        }

        fn queued(responses: Vec<InferenceResponse>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into_iter().map(Ok).collect()),
                request_notify: Notify::new(),
            }
        }

        fn request_count(&self) -> usize {
            self.requests.lock().expect("request lock").len()
        }

        fn remaining_response_count(&self) -> usize {
            self.responses.lock().expect("response lock").len()
        }

        async fn wait_for_request_count(&self, expected: usize) {
            loop {
                let notified = self.request_notify.notified();
                if self.request_count() >= expected {
                    return;
                }
                notified.await;
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
                raw_body: body.to_vec(),
                body: serde_json::from_slice(&body).expect("request body should be JSON"),
            };
            self.requests.lock().expect("request lock").push(captured);
            self.request_notify.notify_one();
            self.responses
                .lock()
                .expect("response lock")
                .pop_front()
                .expect("a fake response should be queued")
        }
    }

    fn response_with_items(
        status: u16,
        items: Vec<opensecret::Result<Vec<u8>>>,
        retry_after: Option<&str>,
    ) -> InferenceResponse {
        let body: OpenSecretResponseBody = Box::pin(futures_util::stream::iter(
            items.into_iter().map(|item| item.map(Into::into)),
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

    fn response(status: u16, chunks: Vec<Vec<u8>>, retry_after: Option<&str>) -> InferenceResponse {
        response_with_items(status, chunks.into_iter().map(Ok).collect(), retry_after)
    }

    fn malformed_response(detail: &str) -> InferenceResponse {
        response(
            200,
            vec![format!("data: {detail}\n\ndata: [DONE]\n\n").into_bytes()],
            None,
        )
    }

    fn incomplete_tool_call_sse() -> Vec<u8> {
        concat!(
            "data: {\"id\":\"tools-incomplete\",\"object\":\"chat.completion.chunk\",",
            "\"created\":1,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,",
            "\"id\":\"call-1\",\"type\":\"function\",\"function\":{",
            "\"name\":\"web_search\",\"arguments\":\"{\\\"query\\\":\\\"map\"}}]},",
            "\"finish_reason\":null}]}\n\n"
        )
        .as_bytes()
        .to_vec()
    }

    fn complete_tool_call_event() -> Vec<u8> {
        concat!(
            "data: {\"id\":\"tools-complete\",\"object\":\"chat.completion.chunk\",",
            "\"created\":2,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,",
            "\"id\":\"call-1\",\"type\":\"function\",\"function\":{",
            "\"name\":\"web_search\",\"arguments\":\"{\\\"query\\\":\\\"maple\\\"}\"}}]},",
            "\"finish_reason\":\"tool_calls\"}],",
            "\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n"
        )
        .as_bytes()
        .to_vec()
    }

    fn complete_tool_call_sse() -> Vec<u8> {
        let mut event = complete_tool_call_event();
        event.extend_from_slice(b"data: [DONE]\n\n");
        event
    }

    fn fast_retry_config(max_retries: usize) -> RetryConfig {
        RetryConfig::new(max_retries, 0, 1.0, 0).transient_only()
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

    fn tool_call_response(completion_id: &str, tool_id: &str) -> InferenceResponse {
        let tool_chunk = json!({
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": 1,
            "model": KIMI_K3_MODEL_ID,
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": tool_id,
                        "type": "function",
                        "function": { "name": "shell", "arguments": "{}" }
                    }]
                },
                "finish_reason": null
            }]
        });
        let finish_chunk = json!({
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": 2,
            "model": KIMI_K3_MODEL_ID,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        });
        let sse = format!("data: {tool_chunk}\n\ndata: {finish_chunk}\n\ndata: [DONE]\n\n");
        response(200, vec![sse.into_bytes()], None)
    }

    async fn streamed_tool_request_id(
        model_name: &str,
        completion_id: &str,
        tool_id: &str,
    ) -> String {
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(tool_call_response(
            completion_id,
            tool_id,
        ))));
        let stream = provider
            .stream(
                &ModelConfig::new(model_name),
                "system",
                &[Message::user().with_text("use a tool")],
                &[],
            )
            .await
            .expect("stream should start");
        let (message, _) = collect_stream(stream)
            .await
            .expect("tool call should parse");
        message
            .content
            .iter()
            .find_map(|content| match content {
                MessageContent::ToolRequest(request) => Some(request.id.clone()),
                _ => None,
            })
            .expect("tool request should be present")
    }

    fn conversation_with_trailing_tool_output() -> Vec<Message> {
        vec![
            Message::user().with_text("inspect the project"),
            Message::assistant().with_tool_request(
                "read-1",
                Ok(rmcp::model::CallToolRequestParams::new("read")
                    .with_arguments(object!({"path": "README.md"}))),
            ),
            Message::user().with_tool_response(
                "read-1",
                Ok(rmcp::model::CallToolResult::success(vec![
                    rmcp::model::ContentBlock::text("tool output"),
                ])),
            ),
        ]
    }

    #[tokio::test]
    async fn stream_runs_shadow_checks_without_changing_the_original_tool_call() {
        let guard = Arc::new(RecordingSafeguard::default());
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(tool_call_response(
            "completion-guarded",
            "shell:0",
        ))))
        .with_safeguard(guard.clone(), "/project".to_string());
        let messages = conversation_with_trailing_tool_output();
        let tools = [Tool::new(
            "shell",
            "Run a shell command",
            object!({"type": "object"}),
        )];

        let stream = provider
            .stream(&ModelConfig::new("test-model"), "system", &messages, &tools)
            .await
            .expect("stream should start");
        assert_eq!(guard.provider_preparations.load(Ordering::SeqCst), 1);
        assert_eq!(guard.untrusted_input_checks.load(Ordering::SeqCst), 1);
        assert_eq!(guard.proposed_action_checks.load(Ordering::SeqCst), 0);

        let (message, _) = collect_stream(stream)
            .await
            .expect("guarded tool call should parse");
        assert_eq!(guard.proposed_action_checks.load(Ordering::SeqCst), 1);
        assert_eq!(guard.proposed_action_tools.load(Ordering::SeqCst), 1);
        let request = message
            .content
            .iter()
            .find_map(|content| match content {
                MessageContent::ToolRequest(request) => Some(request),
                _ => None,
            })
            .expect("tool request should be preserved");
        assert_eq!(request.id, "shell:0");
        assert_eq!(
            request.tool_call.as_ref().expect("valid tool call").name,
            "shell"
        );
    }

    #[tokio::test]
    async fn text_only_stream_records_bounded_provider_preparation_without_an_action() {
        let guard = Arc::new(RecordingSafeguard::default());
        let provider =
            MapleProvider::new(Arc::new(FakeTransport::new(fragmented_success_response())))
                .with_safeguard(guard.clone(), "/project".to_string());
        let tools = [Tool::new(
            "read",
            "Read a file",
            object!({"type": "object"}),
        )];

        let stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("say hello")],
                &tools,
            )
            .await
            .expect("text stream should start");
        assert_eq!(guard.provider_preparations.load(Ordering::SeqCst), 1);

        let (message, _) = collect_stream(stream)
            .await
            .expect("text response should parse");
        assert!(message
            .content
            .iter()
            .all(|content| !matches!(content, MessageContent::ToolRequest(_))));
        assert_eq!(guard.proposed_action_checks.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancellation_during_action_shadow_never_yields_the_buffered_tool_call() {
        let guard = Arc::new(BlockingSafeguard::default());
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(tool_call_response(
            "completion-cancelled-guard",
            "shell:0",
        ))))
        .with_safeguard(guard.clone(), "/project".to_string());
        let cancellation = CancellationToken::new();
        let run_state = Arc::new(SafeguardRunState::default());
        let stream = MAPLE_RUN_CANCELLATION
            .scope(
                cancellation.clone(),
                MAPLE_SAFEGUARD_RUN_STATE.scope(
                    Arc::clone(&run_state),
                    provider.stream(
                        &ModelConfig::new("test-model"),
                        "system",
                        &[Message::user().with_text("use a tool")],
                        &[],
                    ),
                ),
            )
            .await
            .expect("stream should start");
        let collected = tokio::spawn(async move { collect_stream(stream).await });

        guard.action_entered.notified().await;
        cancellation.cancel();
        guard.action_release.add_permits(1);

        assert!(collected
            .await
            .expect("collector task should finish")
            .is_err());
        assert!(!run_state.follows_untrusted_tool_output());
    }

    #[tokio::test]
    async fn cancellation_before_poll_never_yields_a_buffered_tool_call() {
        let guard = Arc::new(RecordingSafeguard::default());
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(tool_call_response(
            "completion-cancelled-before-poll",
            "shell:0",
        ))))
        .with_safeguard(guard, "/project".to_string());
        let cancellation = CancellationToken::new();
        let stream = MAPLE_RUN_CANCELLATION
            .scope(
                cancellation.clone(),
                provider.stream(
                    &ModelConfig::new("test-model"),
                    "system",
                    &[Message::user().with_text("use a tool")],
                    &[],
                ),
            )
            .await
            .expect("stream should start");

        cancellation.cancel();

        assert!(collect_stream(stream).await.is_err());
    }

    #[tokio::test]
    async fn provider_run_state_preserves_the_post_tool_signal_when_kickoff_id_is_missing() {
        let guard = Arc::new(RecordingSafeguard::default());
        let provider = MapleProvider::new(Arc::new(FakeTransport::with_responses(vec![
            tool_call_response("completion-first", "shell:0"),
            tool_call_response("completion-second", "shell:1"),
        ])))
        .with_safeguard(guard.clone(), "/project".to_string());
        let model = ModelConfig::new("test-model");
        let messages = [Message::user().with_text("compacted kickoff without its id")];

        with_agent_run_context(
            CancellationToken::new(),
            Some("test-account".to_string()),
            Some(SafeguardTrustedUserRequest::new(
                "missing-kickoff-id".to_string(),
                "trusted request".to_string(),
            )),
            async {
                for _ in 0..2 {
                    let stream = provider
                        .stream(&model, "system", &messages, &[])
                        .await
                        .expect("stream should start");
                    collect_stream(stream)
                        .await
                        .expect("tool call should parse");
                }
            },
        )
        .await;

        assert_eq!(
            *guard.proposed_action_follows.lock().unwrap(),
            [false, true]
        );
    }

    #[tokio::test]
    async fn auxiliary_complete_requests_bypass_the_shadow_guard() {
        let guard = Arc::new(RecordingSafeguard::default());
        let provider =
            MapleProvider::new(Arc::new(FakeTransport::new(fragmented_success_response())))
                .with_safeguard(guard.clone(), "/project".to_string());

        provider
            .complete(
                &ModelConfig::new("test-model"),
                "system",
                &conversation_with_trailing_tool_output(),
                &[],
            )
            .await
            .expect("auxiliary request should complete");

        assert_eq!(guard.untrusted_input_checks.load(Ordering::SeqCst), 0);
        assert_eq!(guard.proposed_action_checks.load(Ordering::SeqCst), 0);
    }

    fn pending_success_response() -> InferenceResponse {
        let body: OpenSecretResponseBody = Box::pin(futures_util::stream::pending());
        let mut response = InferenceResponse::new(body);
        *response.status_mut() = tauri::http::StatusCode::OK;
        response
    }

    fn notifying_body_error_response(error_read: Arc<Notify>) -> InferenceResponse {
        let body: OpenSecretResponseBody = Box::pin(futures_util::stream::once(async move {
            error_read.notify_one();
            Err(opensecret::Error::InvalidResponse(
                "private first-item stream failure".to_string(),
            ))
        }));
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
    async fn primary_agent_stream_enables_thinking_only_for_direct_gemma_selection() {
        let gemma_transport = Arc::new(FakeTransport::new(fragmented_success_response()));
        let gemma_provider = MapleProvider::new(gemma_transport.clone());
        let gemma_config = super::super::maple_model_config(GEMMA4_AGENT_MODEL_ID, None)
            .expect("Gemma model config");

        let gemma_stream = gemma_provider
            .stream(
                &gemma_config,
                "system",
                &[Message::user().with_text("reason carefully")],
                &[],
            )
            .await
            .expect("Gemma stream should start");
        let _ = collect_stream(gemma_stream)
            .await
            .expect("Gemma stream should parse");

        let gemma_requests = gemma_transport.requests.lock().expect("request lock");
        assert_eq!(gemma_requests[0].body["include_reasoning"], true);
        assert_eq!(
            gemma_requests[0].body["chat_template_kwargs"]["enable_thinking"],
            true
        );
        drop(gemma_requests);

        let llama_transport = Arc::new(FakeTransport::new(fragmented_success_response()));
        let llama_provider = MapleProvider::new(llama_transport.clone());
        let llama_config =
            super::super::maple_model_config("llama3-3-70b", None).expect("Llama model config");

        let llama_stream = llama_provider
            .stream(
                &llama_config,
                "system",
                &[Message::user().with_text("answer directly")],
                &[],
            )
            .await
            .expect("Llama stream should start");
        let _ = collect_stream(llama_stream)
            .await
            .expect("Llama stream should parse");

        let llama_requests = llama_transport.requests.lock().expect("request lock");
        assert!(llama_requests[0].body.get("include_reasoning").is_none());
        assert!(llama_requests[0].body.get("chat_template_kwargs").is_none());
    }

    #[tokio::test]
    async fn gemma_auxiliary_completion_keeps_thinking_disabled() {
        let transport = Arc::new(FakeTransport::new(fragmented_success_response()));
        let provider = MapleProvider::new(transport.clone());
        let model_config = super::super::maple_model_config(GEMMA4_AGENT_MODEL_ID, None)
            .expect("Gemma model config");

        provider
            .complete(
                &model_config,
                "system",
                &[Message::user().with_text("summarize")],
                &[],
            )
            .await
            .expect("Gemma completion should parse");

        let requests = transport.requests.lock().expect("request lock");
        assert_eq!(requests[0].body["include_reasoning"], false);
        assert_eq!(
            requests[0].body["chat_template_kwargs"]["enable_thinking"],
            false
        );
    }

    #[test]
    fn gemma_request_defaults_preserve_explicit_controls_and_skip_aliases() {
        let explicit_off =
            ModelConfig::new(GEMMA4_AGENT_MODEL_ID).with_merged_request_params(HashMap::from([
                ("include_reasoning".to_string(), json!(false)),
                (
                    "chat_template_kwargs".to_string(),
                    json!({ "enable_thinking": false, "custom": "kept" }),
                ),
            ]));
        let primary = MapleProvider::gemma_agent_model_config(&explicit_off, true);
        let primary_params = primary.request_params.expect("primary request params");
        assert_eq!(primary_params["include_reasoning"], false);
        assert_eq!(
            primary_params["chat_template_kwargs"]["enable_thinking"],
            false
        );
        assert_eq!(primary_params["chat_template_kwargs"]["custom"], "kept");

        let explicit_on =
            ModelConfig::new(GEMMA4_AGENT_MODEL_ID).with_merged_request_params(HashMap::from([
                ("include_reasoning".to_string(), json!(true)),
                (
                    "chat_template_kwargs".to_string(),
                    json!({ "enable_thinking": true }),
                ),
            ]));
        let auxiliary = MapleProvider::gemma_agent_model_config(&explicit_on, false);
        let auxiliary_params = auxiliary.request_params.expect("auxiliary request params");
        assert_eq!(auxiliary_params["include_reasoning"], true);
        assert_eq!(
            auxiliary_params["chat_template_kwargs"]["enable_thinking"],
            true
        );

        let alias =
            MapleProvider::gemma_agent_model_config(&ModelConfig::new("auto:powerful"), true);
        assert!(alias.request_params.is_none());
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
    async fn replaces_only_legacy_kimi_k3_tool_ids_with_conversation_unique_ids() {
        let first = streamed_tool_request_id(KIMI_K3_MODEL_ID, "completion-1", "shell:0").await;
        let second = streamed_tool_request_id(KIMI_K3_MODEL_ID, "completion-2", "shell:0").await;

        assert!(first.starts_with("chatcmpl-tool-"));
        assert!(second.starts_with("chatcmpl-tool-"));
        assert_ne!(first, second);

        let missing_index =
            streamed_tool_request_id(KIMI_K3_MODEL_ID, "completion-3", "shell").await;
        assert!(missing_index.starts_with("chatcmpl-tool-"));

        let malformed_index =
            streamed_tool_request_id(KIMI_K3_MODEL_ID, "completion-4", "shell:not-a-number").await;
        assert!(malformed_index.starts_with("chatcmpl-tool-"));

        let already_unique = streamed_tool_request_id(
            KIMI_K3_MODEL_ID,
            "completion-5",
            "chatcmpl-tool-upstream-unique",
        )
        .await;
        assert_eq!(already_unique, "chatcmpl-tool-upstream-unique");

        let other_model = streamed_tool_request_id("glm-5-2", "completion-6", "shell:0").await;
        assert_eq!(other_model, "shell:0");

        let kimi_k2_6 = streamed_tool_request_id("kimi-k2-6", "completion-7", "shell:0").await;
        assert_eq!(kimi_k2_6, "shell:0");
    }

    #[test]
    fn normalizes_kimi_k3_ids_without_defeating_duplicate_detection() {
        let mut message = Message::assistant()
            .with_tool_request(
                "shell:0",
                Ok(rmcp::model::CallToolRequestParams::new("shell")),
            )
            .with_tool_request(
                "shell:0",
                Ok(rmcp::model::CallToolRequestParams::new("shell")),
            );
        let mut replacements = HashMap::new();

        MapleProvider::replace_legacy_kimi_k3_tool_ids(&mut message, &mut replacements);

        let ids = message
            .content
            .iter()
            .filter_map(|content| match content {
                MessageContent::ToolRequest(request) => Some(request.id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1]);
        assert!(ids[0].starts_with("chatcmpl-tool-"));

        let mut errored_message = Message::assistant()
            .with_tool_request(
                "shell:0",
                Err(rmcp::model::ErrorData::invalid_params(
                    "bad arguments",
                    None,
                )),
            )
            .with_tool_request(
                "chatcmpl-tool-upstream-unique",
                Err(rmcp::model::ErrorData::invalid_params(
                    "bad arguments",
                    None,
                )),
            );
        let mut errored_replacements = HashMap::new();
        MapleProvider::replace_legacy_kimi_k3_tool_ids(
            &mut errored_message,
            &mut errored_replacements,
        );
        let errored_ids = errored_message
            .content
            .iter()
            .filter_map(|content| match content {
                MessageContent::ToolRequest(request) => Some(request.id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(errored_ids[0].starts_with("chatcmpl-tool-"));
        assert_eq!(errored_ids[1], "chatcmpl-tool-upstream-unique");
    }

    #[tokio::test]
    async fn normalized_kimi_k3_id_round_trips_with_its_tool_response() {
        let transport = Arc::new(FakeTransport::with_responses(vec![
            tool_call_response("completion-1", "shell:0"),
            tool_call_response("completion-2", "shell:0"),
        ]));
        let provider = MapleProvider::new(Arc::clone(&transport));
        let model_config = ModelConfig::new(KIMI_K3_MODEL_ID);
        let initial_message = Message::user().with_text("use a tool twice");

        let first_stream = provider
            .stream(
                &model_config,
                "system",
                std::slice::from_ref(&initial_message),
                &[],
            )
            .await
            .expect("first stream should start");
        let (first_tool_message, _) = collect_stream(first_stream)
            .await
            .expect("first tool call should parse");
        let first_id = first_tool_message
            .content
            .iter()
            .find_map(|content| match content {
                MessageContent::ToolRequest(request) => Some(request.id.clone()),
                _ => None,
            })
            .expect("first tool request should be present");
        let first_tool_response = Message::user().with_tool_response(
            first_id.clone(),
            Ok(rmcp::model::CallToolResult::success(vec![
                rmcp::model::ContentBlock::text("done"),
            ])),
        );

        let second_stream = provider
            .stream(
                &model_config,
                "system",
                &[initial_message, first_tool_message, first_tool_response],
                &[],
            )
            .await
            .expect("second stream should start");
        let (second_tool_message, _) = collect_stream(second_stream)
            .await
            .expect("second tool call should parse");
        let second_id = second_tool_message
            .content
            .iter()
            .find_map(|content| match content {
                MessageContent::ToolRequest(request) => Some(request.id.clone()),
                _ => None,
            })
            .expect("second tool request should be present");

        assert_ne!(first_id, second_id);
        let requests = transport.requests.lock().expect("request lock");
        let second_request_messages = requests[1].body["messages"]
            .as_array()
            .expect("request messages should be an array");
        let serialized_tool_request = second_request_messages
            .iter()
            .find(|message| message["tool_calls"].is_array())
            .expect("assistant tool request should be serialized");
        let serialized_tool_response = second_request_messages
            .iter()
            .find(|message| message["role"] == "tool")
            .expect("tool response should be serialized");
        assert_eq!(serialized_tool_request["tool_calls"][0]["id"], first_id);
        assert_eq!(serialized_tool_response["tool_call_id"], first_id);
    }

    #[tokio::test]
    async fn retries_invalid_stream_before_first_item_with_the_same_request() {
        let transport = Arc::new(FakeTransport::queued(vec![
            malformed_response("transient-invalid-stream"),
            fragmented_success_response(),
        ]));
        let provider =
            MapleProvider::new(Arc::clone(&transport)).with_test_retry_config(fast_retry_config(3));

        let stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await
            .expect("replacement stream should start");
        let (message, usage) = collect_stream(stream)
            .await
            .expect("replacement stream should parse");
        let text = message
            .content
            .iter()
            .filter_map(|content| match content {
                MessageContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<String>();

        assert_eq!(text, "Hello world");
        assert_eq!(usage.usage.total_tokens, Some(5));
        let requests = transport.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].raw_body, requests[1].raw_body);
        assert_eq!(requests[0].body, requests[1].body);
    }

    #[tokio::test]
    async fn does_not_retry_after_the_first_successful_item() {
        let first_chunk = concat!(
            "data: {\"id\":\"chunk-1\",\"object\":\"chat.completion.chunk\",",
            "\"created\":1,\"model\":\"test-model\",\"choices\":[{\"index\":0,",
            "\"delta\":{\"role\":\"assistant\",\"content\":\"partial\"},",
            "\"finish_reason\":null}]}\n\n"
        )
        .as_bytes()
        .to_vec();
        let interrupted = response_with_items(
            200,
            vec![
                Ok(first_chunk),
                Err(opensecret::Error::InvalidResponse(
                    "private post-item stream failure".to_string(),
                )),
            ],
            None,
        );
        let transport = Arc::new(FakeTransport::queued(vec![
            interrupted,
            fragmented_success_response(),
        ]));
        let provider =
            MapleProvider::new(Arc::clone(&transport)).with_test_retry_config(fast_retry_config(3));

        let mut stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await
            .expect("the first item should start the stream");
        let (message, usage) = stream
            .next()
            .await
            .expect("first item")
            .expect("first item should parse");
        let message = message.expect("first item should contain text");
        assert!(usage.is_none());
        assert!(matches!(
            message.content.as_slice(),
            [MessageContent::Text(text)] if text.text == "partial"
        ));

        let error = stream
            .next()
            .await
            .expect("the interruption should be surfaced")
            .expect_err("the interruption should remain an error");
        assert_eq!(
            error,
            ProviderError::NetworkError("Maple's response stream was invalid".to_string())
        );
        assert_eq!(transport.request_count(), 1);
        assert_eq!(transport.remaining_response_count(), 1);
    }

    #[tokio::test]
    async fn does_not_retry_after_a_usage_only_item() {
        let usage_chunk = concat!(
            "data: {\"id\":\"usage-1\",\"object\":\"chat.completion.chunk\",",
            "\"created\":1,\"model\":\"test-model\",\"choices\":[],",
            "\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n"
        )
        .as_bytes()
        .to_vec();
        let interrupted = response_with_items(
            200,
            vec![
                Ok(usage_chunk),
                Err(opensecret::Error::InvalidResponse(
                    "private post-usage stream failure".to_string(),
                )),
            ],
            None,
        );
        let transport = Arc::new(FakeTransport::queued(vec![
            interrupted,
            fragmented_success_response(),
        ]));
        let provider =
            MapleProvider::new(Arc::clone(&transport)).with_test_retry_config(fast_retry_config(3));

        let mut stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await
            .expect("usage should count as the first item");
        let (message, usage) = stream
            .next()
            .await
            .expect("usage item")
            .expect("usage item should parse");
        assert!(message.is_none());
        assert_eq!(usage.expect("usage").usage.total_tokens, Some(5));
        assert!(matches!(
            stream.next().await,
            Some(Err(ProviderError::NetworkError(_)))
        ));
        assert_eq!(transport.request_count(), 1);
        assert_eq!(transport.remaining_response_count(), 1);
    }

    #[tokio::test]
    async fn shares_one_retry_budget_across_status_and_first_item_failures() {
        let transport = Arc::new(FakeTransport::queued(vec![
            response(
                503,
                vec![br#"{"error":{"message":"temporarily unavailable"}}"#.to_vec()],
                None,
            ),
            malformed_response("invalid-stream-1"),
            malformed_response("invalid-stream-2"),
            malformed_response("invalid-stream-3"),
            fragmented_success_response(),
        ]));
        let provider = MapleProvider::new(Arc::clone(&transport));
        let default_max_retries = Provider::retry_config(&provider).max_retries();
        assert_eq!(default_max_retries, 3);
        let provider = provider.with_test_retry_config(fast_retry_config(default_max_retries));

        let result = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await;
        let error = match result {
            Ok(_) => panic!("the shared retry budget should be exhausted"),
            Err(error) => error,
        };

        assert_eq!(
            error,
            ProviderError::NetworkError("Maple's response stream was invalid".to_string())
        );
        assert_eq!(transport.request_count(), 4);
        assert_eq!(transport.remaining_response_count(), 1);
    }

    #[tokio::test]
    async fn retries_an_incomplete_tool_call_before_it_is_yielded() {
        let interrupted = response_with_items(
            200,
            vec![
                Ok(incomplete_tool_call_sse()),
                Err(opensecret::Error::InvalidResponse(
                    "private incomplete tool stream failure".to_string(),
                )),
            ],
            None,
        );
        let replacement = response(200, vec![complete_tool_call_sse()], None);
        let transport = Arc::new(FakeTransport::queued(vec![interrupted, replacement]));
        let provider =
            MapleProvider::new(Arc::clone(&transport)).with_test_retry_config(fast_retry_config(3));

        let stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("search")],
                &[],
            )
            .await
            .expect("replacement tool stream should start");
        let (message, usage) = collect_stream(stream)
            .await
            .expect("replacement tool stream should parse");
        let calls = message
            .content
            .iter()
            .filter_map(|content| match content {
                MessageContent::ToolRequest(request) => request.tool_call.as_ref().ok(),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(
            calls[0]
                .arguments
                .as_ref()
                .and_then(|arguments| arguments.get("query")),
            Some(&json!("maple"))
        );
        assert_eq!(usage.usage.total_tokens, Some(5));
        assert_eq!(transport.request_count(), 2);
    }

    #[tokio::test]
    async fn leaves_an_incomplete_tool_call_ending_in_done_as_an_empty_stream() {
        let mut incomplete_then_done = incomplete_tool_call_sse();
        incomplete_then_done.extend_from_slice(b"data: [DONE]\n\n");
        let transport = Arc::new(FakeTransport::queued(vec![
            response(200, vec![incomplete_then_done], None),
            fragmented_success_response(),
        ]));
        let provider =
            MapleProvider::new(Arc::clone(&transport)).with_test_retry_config(fast_retry_config(3));

        let mut stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("search")],
                &[],
            )
            .await
            .expect("DONE should preserve Goose's empty-stream recovery path");

        assert!(stream.next().await.is_none());
        assert_eq!(transport.request_count(), 1);
        assert_eq!(transport.remaining_response_count(), 1);
    }

    #[tokio::test]
    async fn does_not_retry_after_a_complete_tool_call_is_yielded() {
        let interrupted = response_with_items(
            200,
            vec![
                Ok(complete_tool_call_event()),
                Err(opensecret::Error::InvalidResponse(
                    "private completed tool stream failure".to_string(),
                )),
            ],
            None,
        );
        let transport = Arc::new(FakeTransport::queued(vec![
            interrupted,
            fragmented_success_response(),
        ]));
        let provider =
            MapleProvider::new(Arc::clone(&transport)).with_test_retry_config(fast_retry_config(3));

        let mut stream = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("search")],
                &[],
            )
            .await
            .expect("complete tool item should start the stream");
        let (message, _) = stream
            .next()
            .await
            .expect("tool item")
            .expect("tool item should parse");
        let message = message.expect("tool item should contain a message");
        assert!(message
            .content
            .iter()
            .any(|content| matches!(content, MessageContent::ToolRequest(_))));

        let error = stream
            .next()
            .await
            .expect("the interruption should be surfaced")
            .expect_err("the interruption should remain an error");
        assert!(matches!(error, ProviderError::NetworkError(_)));
        assert_eq!(transport.request_count(), 1);
        assert_eq!(transport.remaining_response_count(), 1);
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
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(malformed_response(
            PRIVATE_MALFORMED_LINE,
        ))))
        .with_test_retry_config(fast_retry_config(0));
        let result = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await;
        let error = match result {
            Ok(_) => panic!("malformed completion data should fail"),
            Err(error) => error,
        };
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
        let transport = Arc::new(FakeTransport::new(pending_success_response()));
        let provider = MapleProvider::new(Arc::clone(&transport));
        let cancellation = CancellationToken::new();
        let model_config = ModelConfig::new("test-model");
        let messages = [Message::user().with_text("hello")];
        let stream = with_run_cancellation(
            cancellation.clone(),
            provider.stream(&model_config, "system", &messages, &[]),
        );
        tokio::pin!(stream);

        tokio::select! {
            _ = transport.wait_for_request_count(1) => {}
            result = &mut stream => panic!("stream unexpectedly finished before cancellation: {}", result.is_ok()),
        }

        cancellation.cancel();
        let result = stream.await;
        assert!(
            matches!(result, Err(ProviderError::ExecutionError(message)) if message.contains("cancelled"))
        );
        assert_eq!(transport.request_count(), 1);
    }

    #[tokio::test]
    async fn cancellation_interrupts_first_item_retry_backoff() {
        let error_read = Arc::new(Notify::new());
        let transport = Arc::new(FakeTransport::queued(vec![
            notifying_body_error_response(Arc::clone(&error_read)),
            fragmented_success_response(),
        ]));
        let retry_config = RetryConfig::new(3, 60_000, 1.0, 60_000).transient_only();
        let provider =
            MapleProvider::new(Arc::clone(&transport)).with_test_retry_config(retry_config);
        let cancellation = CancellationToken::new();
        let model_config = ModelConfig::new("test-model");
        let messages = [Message::user().with_text("hello")];
        let stream = with_run_cancellation(
            cancellation.clone(),
            provider.stream(&model_config, "system", &messages, &[]),
        );
        tokio::pin!(stream);

        tokio::select! {
            _ = error_read.notified() => {}
            result = &mut stream => panic!("stream unexpectedly finished before cancellation: {}", result.is_ok()),
        }

        cancellation.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), stream)
            .await
            .expect("cancellation should interrupt backoff");
        assert!(
            matches!(result, Err(ProviderError::ExecutionError(message)) if message.contains("cancelled"))
        );
        assert_eq!(transport.request_count(), 1);
        assert_eq!(transport.remaining_response_count(), 1);
    }

    #[tokio::test]
    async fn stalled_response_stream_has_a_bounded_idle_timeout() {
        let provider = MapleProvider::new(Arc::new(FakeTransport::new(pending_success_response())))
            .with_test_retry_config(fast_retry_config(0));
        let result = provider
            .stream(
                &ModelConfig::new("test-model"),
                "system",
                &[Message::user().with_text("hello")],
                &[],
            )
            .await;
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
