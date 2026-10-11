use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, RwLock};

use futures_util::future::BoxFuture;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::stream::{AssistantMessageBuilder, AssistantMessageStream};
use crate::types::{Context, Model, SamplingParams, StopReason, ThinkingLevel, ToolChoice};

/// Inspects or replaces a provider request body before it is sent.
pub type PayloadHook = Arc<dyn Fn(Value) -> BoxFuture<'static, Value> + Send + Sync>;

/// Rewrites a request's own headers before a provider adds them to its defaults, as
/// Pi's `transformHeaders` does; a `None` value removes a header.
pub type HeadersHook = Arc<
    dyn Fn(BTreeMap<String, Option<String>>) -> BoxFuture<'static, BTreeMap<String, Option<String>>>
        + Send
        + Sync,
>;

/// Sees a response once it starts, before its body is read.
pub type ResponseHook = Arc<dyn Fn(ProviderResponse) -> BoxFuture<'static, ()> + Send + Sync>;

/// Sees each event a provider streams, as the provider parsed it, before it is read.
pub type ProviderEventHook = Arc<dyn Fn(Value) -> BoxFuture<'static, ()> + Send + Sync>;

/// The start of a response, as a [`ResponseHook`] sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderResponse {
    pub status: u16,
    /// Header names are in lowercase.
    pub headers: BTreeMap<String, String>,
}

/// Options shared by every provider request.
#[derive(Clone, Default)]
pub struct StreamOptions {
    pub api_key: Option<String>,
    /// Requested reasoning effort; `None` or `Off` leaves reasoning off.
    pub reasoning: Option<ThinkingLevel>,
    /// Output token cap; providers default to the model's `max_tokens`, and lower it so
    /// the estimated input and the output fit in the context window.
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    /// Identifies the conversation to providers that route or cache by session.
    pub session_id: Option<String>,
    /// Extra request headers, over the provider's and the model's. `None` removes one.
    pub headers: BTreeMap<String, Option<String>>,
    /// `auto` or `none`; unset leaves it to the provider.
    pub tool_choice: Option<ToolChoice>,
    /// Parameters merged into the request after the named fields, over the model's.
    pub sampling_params: SamplingParams,
    /// How long to wait for a response to start, in milliseconds. Providers default to
    /// ten minutes, as the OpenAI SDK does.
    pub timeout_ms: Option<u64>,
    /// How often a failed request is sent again before the response fails; unset is
    /// never.
    pub max_retries: Option<u32>,
    /// The longest wait before a retry a server may ask for, in milliseconds. Asking
    /// for longer fails the request at once. Unset is a minute; zero has no limit.
    pub max_retry_delay_ms: Option<u64>,
    /// Cancels the request. Providers answer with an `Aborted` response.
    pub cancel: CancellationToken,
    pub transform_headers: Option<HeadersHook>,
    pub on_payload: Option<PayloadHook>,
    pub on_response: Option<ResponseHook>,
    pub on_provider_stream_event: Option<ProviderEventHook>,
}

impl fmt::Debug for StreamOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamOptions")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("reasoning", &self.reasoning)
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("session_id", &self.session_id)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("tool_choice", &self.tool_choice)
            .field("sampling_params", &self.sampling_params)
            .field("timeout_ms", &self.timeout_ms)
            .field("max_retries", &self.max_retries)
            .field("max_retry_delay_ms", &self.max_retry_delay_ms)
            .field("cancelled", &self.cancel.is_cancelled())
            .field("transform_headers", &self.transform_headers.is_some())
            .field("on_payload", &self.on_payload.is_some())
            .field("on_response", &self.on_response.is_some())
            .field(
                "on_provider_stream_event",
                &self.on_provider_stream_event.is_some(),
            )
            .finish()
    }
}

/// A model API: turns a transcript into a streamed response.
///
/// The returned stream must end with `Done` or `Error`. Failures after the call (an
/// unreachable server, a rejected request, cancellation) are encoded as an `Error`
/// event carrying an assistant message with `stop_reason` `Error` or `Aborted`.
pub trait StreamFn: Send + Sync {
    fn stream(
        &self,
        model: &Model,
        context: Context,
        options: StreamOptions,
    ) -> AssistantMessageStream;
}

impl<F> StreamFn for F
where
    F: Fn(&Model, Context, StreamOptions) -> AssistantMessageStream + Send + Sync,
{
    fn stream(
        &self,
        model: &Model,
        context: Context,
        options: StreamOptions,
    ) -> AssistantMessageStream {
        self(model, context, options)
    }
}

/// A stream that fails immediately with `error`.
pub fn error_stream(model: &Model, error: impl Into<String>) -> AssistantMessageStream {
    let (sender, stream) = AssistantMessageStream::channel();
    AssistantMessageBuilder::new(sender, model).fail(StopReason::Error, error);
    stream
}

/// Model APIs keyed by [`Model::api`]. Streaming through the registry dispatches to the
/// implementation registered for the model's API.
#[derive(Clone, Default)]
pub struct ApiRegistry {
    apis: Arc<RwLock<HashMap<String, Arc<dyn StreamFn>>>>,
}

impl ApiRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `implementation` for `api`, replacing an earlier registration.
    pub fn register(&self, api: impl Into<String>, implementation: Arc<dyn StreamFn>) {
        self.apis
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(api.into(), implementation);
    }

    pub fn unregister(&self, api: &str) {
        self.apis
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(api);
    }

    pub fn get(&self, api: &str) -> Option<Arc<dyn StreamFn>> {
        self.apis
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(api)
            .cloned()
    }
}

impl StreamFn for ApiRegistry {
    fn stream(
        &self,
        model: &Model,
        context: Context,
        options: StreamOptions,
    ) -> AssistantMessageStream {
        match self.get(&model.api) {
            Some(implementation) => implementation.stream(model, context, options),
            None => error_stream(
                model,
                format!("No provider registered for API \"{}\"", model.api),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faux::FauxProvider;

    #[tokio::test]
    async fn the_registry_dispatches_on_the_model_api() {
        let faux = FauxProvider::new();
        faux.push_text("from faux");
        let registry = ApiRegistry::new();
        registry.register("faux", Arc::new(faux.clone()));

        let model = faux.model();
        let message = registry
            .stream(&model, Context::default(), StreamOptions::default())
            .result()
            .await;
        assert_eq!(message.text(), "from faux");

        let mut other = model.clone();
        other.api = "missing".into();
        let message = registry
            .stream(&other, Context::default(), StreamOptions::default())
            .result()
            .await;
        assert_eq!(message.stop_reason, StopReason::Error);
        assert_eq!(
            message.error_message.as_deref(),
            Some("No provider registered for API \"missing\"")
        );
    }
}
