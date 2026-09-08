//! Shared plumbing for the small side models Maple runs alongside a session.
//!
//! Permission classifiers and the image describer all reach for the same
//! shape: a fixed model, isolated from session-level reasoning settings, asked
//! one bounded question with a hard timeout. This module owns that shape so
//! the callers only describe what they are classifying.

use goose::agents::Agent;
use goose::conversation::message::{ActionRequired, ActionRequiredData, Message, MessageContent};
use goose_providers::model::ModelConfig;
use rmcp::model::{JsonObject, Tool};
use rmcp::object;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Permission mode that routes tool confirmations through a classifier.
pub(crate) const READ_ONLY_MODE: &str = "smart_approve";

const CLASSIFIER_MODEL: &str = "llama3-3-70b";
const CLASSIFIER_TEMPERATURE: f32 = 0.0;
const CLASSIFIER_MAX_TOKENS: i32 = 256;
const CLASSIFIER_TIMEOUT: Duration = Duration::from_secs(10);
const REQUIRES_APPROVAL_DECISION: &str = "requires_approval";

/// Upper bound on the free-text reason a classifier may return.
pub(crate) const MAX_REASON_CHARS: usize = 300;

/// Match a plain read-only-mode tool confirmation for `tool_name`, yielding
/// its request id and arguments.
///
/// A confirmation that carries a prompt is a security warning Goose wants a
/// human to read, so it is never eligible for automatic handling.
pub(crate) fn read_only_confirmation<'a>(
    mode: &str,
    action: &'a ActionRequired,
    tool_name: &str,
) -> Option<(&'a str, &'a JsonObject)> {
    if mode != READ_ONLY_MODE {
        return None;
    }
    let ActionRequiredData::ToolConfirmation {
        id,
        tool_name: requested_tool,
        arguments,
        prompt,
    } = &action.data
    else {
        return None;
    };
    if requested_tool != tool_name || prompt.is_some() {
        return None;
    }
    Some((id.as_str(), arguments))
}

/// Request knobs that disable thinking on OpenAI-compatible endpoints that
/// need it spelled out in the request body rather than the model config.
pub(crate) fn thinking_disabled_request_params() -> HashMap<String, serde_json::Value> {
    HashMap::from([
        ("include_reasoning".to_string(), serde_json::json!(false)),
        (
            "chat_template_kwargs".to_string(),
            serde_json::json!({ "enable_thinking": false }),
        ),
    ])
}

/// Materialize a model config for a side model.
///
/// Side models are intentionally isolated from session-level reasoning and
/// request settings. In particular, Goose may otherwise inherit a global
/// thinking effort while materializing this isolated request.
pub(crate) fn side_model_config(
    provider_name: &str,
    model_name: &str,
    request_params: Option<HashMap<String, serde_json::Value>>,
    temperature: f32,
    max_tokens: i32,
) -> anyhow::Result<ModelConfig> {
    let mut model_config =
        goose::model_config::model_config_from_user_config_with_session_settings(
            provider_name,
            model_name,
            None,
            request_params.clone(),
            None,
        )?;
    model_config.request_params = request_params;
    model_config.reasoning = Some(false);
    Ok(model_config
        .with_temperature(Some(temperature))
        .with_max_tokens(Some(max_tokens)))
}

/// What a classifier decided about one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassifierOutcome {
    /// The request may proceed without asking the user.
    Approve,
    /// The user must approve the request.
    RequiresApproval,
    /// The run was cancelled before a decision arrived.
    Cancelled,
}

/// One classifier: the prompt it asks, the tool it must answer through, and
/// the decision value that approves a request.
pub(crate) struct Classifier {
    /// Subject of the classifier's log lines, such as `Read-only shell`.
    pub(crate) label: &'static str,
    pub(crate) tool_name: &'static str,
    pub(crate) tool_description: &'static str,
    /// Decision value that maps to [`ClassifierOutcome::Approve`].
    pub(crate) approve_decision: &'static str,
    pub(crate) system_prompt: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifierResponse {
    decision: String,
    reason: String,
}

impl Classifier {
    /// The single closed-schema tool the classifier is allowed to call.
    pub(crate) fn tool(&self) -> Tool {
        Tool::new(
            self.tool_name.to_string(),
            self.tool_description.to_string(),
            object!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "decision": {
                        "type": "string",
                        "enum": [self.approve_decision, REQUIRES_APPROVAL_DECISION]
                    },
                    "reason": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MAX_REASON_CHARS,
                        "description": "A short explanation of the decision"
                    }
                },
                "required": ["decision", "reason"]
            }),
        )
    }

    /// Ask the side model about `request`.
    ///
    /// Every failure path resolves to [`ClassifierOutcome::RequiresApproval`]
    /// so a broken classifier can never widen what runs unattended.
    pub(crate) async fn classify<R: Serialize + ?Sized>(
        &self,
        agent: &Agent,
        session_id: &str,
        request: &R,
        cancel_token: &CancellationToken,
    ) -> ClassifierOutcome {
        if cancel_token.is_cancelled() {
            return ClassifierOutcome::Cancelled;
        }

        let label = self.label;
        let provider = match agent.provider().await {
            Ok(provider) => provider,
            Err(error) => {
                log::warn!("{label} classifier could not resolve provider: {error}");
                return ClassifierOutcome::RequiresApproval;
            }
        };
        let model_config = match side_model_config(
            provider.get_name(),
            CLASSIFIER_MODEL,
            None,
            CLASSIFIER_TEMPERATURE,
            CLASSIFIER_MAX_TOKENS,
        ) {
            Ok(model_config) => model_config,
            Err(error) => {
                log::warn!("{label} classifier could not configure {CLASSIFIER_MODEL}: {error}");
                return ClassifierOutcome::RequiresApproval;
            }
        };
        let input = match serde_json::to_string(request) {
            Ok(input) => input,
            Err(error) => {
                log::warn!("{label} classifier could not serialize request: {error}");
                return ClassifierOutcome::RequiresApproval;
            }
        };
        let messages = [Message::user().with_text(input)];
        let tools = [self.tool()];
        let completion = goose::session_context::with_session_id(
            Some(session_id.to_string()),
            provider.complete(&model_config, self.system_prompt, &messages, &tools),
        );

        let result = tokio::select! {
            biased;
            _ = cancel_token.cancelled() => return ClassifierOutcome::Cancelled,
            result = tokio::time::timeout(CLASSIFIER_TIMEOUT, completion) => result,
        };
        let (message, _usage) = match result {
            Ok(Ok(completion)) => completion,
            Ok(Err(error)) => {
                log::warn!("{label} classifier request failed: {error}");
                return ClassifierOutcome::RequiresApproval;
            }
            Err(_) => {
                log::warn!("{label} classifier timed out");
                return ClassifierOutcome::RequiresApproval;
            }
        };

        self.parse_response(&message).unwrap_or_else(|| {
            log::warn!("{label} classifier returned an invalid structured response");
            ClassifierOutcome::RequiresApproval
        })
    }

    /// Read a decision out of exactly one well-formed call to this
    /// classifier's tool. Anything else is `None`.
    pub(crate) fn parse_response(&self, message: &Message) -> Option<ClassifierOutcome> {
        let requests = message
            .content
            .iter()
            .filter_map(|content| match content {
                MessageContent::ToolRequest(request) => Some(request),
                _ => None,
            })
            .collect::<Vec<_>>();
        let [request] = requests.as_slice() else {
            return None;
        };
        let tool_call = request.tool_call.as_ref().ok()?;
        if tool_call.name != self.tool_name {
            return None;
        }
        let arguments = tool_call.arguments.clone()?;
        let response =
            serde_json::from_value::<ClassifierResponse>(serde_json::Value::Object(arguments))
                .ok()?;
        let reason = response.reason.trim();
        if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
            return None;
        }

        if response.decision == self.approve_decision {
            Some(ClassifierOutcome::Approve)
        } else if response.decision == REQUIRES_APPROVAL_DECISION {
            Some(ClassifierOutcome::RequiresApproval)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose::providers::base::Provider;
    use std::sync::{Arc, Mutex as StdMutex};

    const TEST_CLASSIFIER: Classifier = Classifier {
        label: "Test",
        tool_name: "maple__classify_test",
        tool_description: "Return the permission classification for the supplied request.",
        approve_decision: "allow_once",
        system_prompt: "You are a test classifier.",
    };

    #[test]
    fn classifier_uses_llama_without_gemma_thinking_controls() {
        assert_eq!(CLASSIFIER_MODEL, "llama3-3-70b");

        let model_config = side_model_config(
            "openai",
            CLASSIFIER_MODEL,
            None,
            CLASSIFIER_TEMPERATURE,
            CLASSIFIER_MAX_TOKENS,
        )
        .unwrap();
        assert_eq!(model_config.model_name, CLASSIFIER_MODEL);
        assert_eq!(model_config.temperature, Some(CLASSIFIER_TEMPERATURE));
        assert_eq!(model_config.max_tokens, Some(CLASSIFIER_MAX_TOKENS));
        assert_eq!(model_config.reasoning, Some(false));
        assert!(model_config.request_params.is_none());
    }

    #[test]
    fn side_model_config_keeps_explicit_request_params() {
        let model_config = side_model_config(
            "openai",
            CLASSIFIER_MODEL,
            Some(thinking_disabled_request_params()),
            CLASSIFIER_TEMPERATURE,
            CLASSIFIER_MAX_TOKENS,
        )
        .unwrap();
        assert_eq!(
            model_config.request_params,
            Some(thinking_disabled_request_params())
        );
        assert_eq!(model_config.reasoning, Some(false));
    }

    #[tokio::test]
    async fn goose_serializes_classifier_model_without_gemma_thinking_controls() {
        let captured = Arc::new(StdMutex::new(None));
        let handler_capture = Arc::clone(&captured);
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |axum::Json(payload): axum::Json<serde_json::Value>| {
                let handler_capture = Arc::clone(&handler_capture);
                async move {
                    *handler_capture.lock().unwrap() = Some(payload);
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        concat!(
                            "data: {\"id\":\"chatcmpl-classifier\",",
                            "\"object\":\"chat.completion.chunk\",\"created\":1,",
                            "\"model\":\"test\",\"choices\":[{\"index\":0,",
                            "\"delta\":{\"role\":\"assistant\",\"content\":\"ok\"},",
                            "\"finish_reason\":\"stop\"}]}\n\n",
                            "data: [DONE]\n\n"
                        ),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let api_client = goose::providers::api_client::ApiClient::new_with_tls(
            format!("http://{address}"),
            goose::providers::api_client::AuthMethod::NoAuth,
            None,
        )
        .unwrap();
        let provider = goose::providers::openai::OpenAiProvider::new(api_client);
        let model_config = side_model_config(
            provider.get_name(),
            CLASSIFIER_MODEL,
            None,
            CLASSIFIER_TEMPERATURE,
            CLASSIFIER_MAX_TOKENS,
        )
        .unwrap();

        provider
            .complete(
                &model_config,
                TEST_CLASSIFIER.system_prompt,
                &[Message::user().with_text("request")],
                &[TEST_CLASSIFIER.tool()],
            )
            .await
            .unwrap();
        server.abort();

        let payload = captured.lock().unwrap().take().unwrap();
        assert_eq!(payload["model"], CLASSIFIER_MODEL);
        assert_eq!(payload["temperature"], CLASSIFIER_TEMPERATURE);
        assert_eq!(payload["max_tokens"], CLASSIFIER_MAX_TOKENS);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["stream_options"]["include_usage"], true);
        assert!(payload.get("include_reasoning").is_none());
        assert!(payload.get("chat_template_kwargs").is_none());
        assert_eq!(
            payload["tools"][0]["function"]["name"],
            TEST_CLASSIFIER.tool_name
        );
        assert!(payload.get("thinking_effort").is_none());
    }
}
