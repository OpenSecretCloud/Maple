//! Classifiers, after Pi's `classify()`: models that answer named questions about a JSON
//! state with probabilities instead of text.
//!
//! A [`ClassifierContext`] holds the state, any images judged with it and the questions;
//! a classifier API ([`ClassifyFn`]) answers every question. As with streamed replies,
//! failures are data: the result's `stop_reason` is `Error` or `Aborted`, with a message.
//! [`crate::system_one`] speaks System One's protocol.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::provider::{PayloadHook, ResponseHook};
use crate::types::{
    ImageContent, InputModality, ModelCost, Timestamp, Usage, default_input, now_ms,
};

/// A model that answers classifications. `api` selects the implementation in a
/// [`ClassifierRegistry`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifierModel {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    /// `image` when the model judges images with the state.
    #[serde(default = "default_input")]
    pub input: Vec<InputModality>,
    #[serde(default)]
    pub cost: ModelCost,
    pub context_window: u64,
    /// Headers sent with every request for this model, under the request's own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

impl ClassifierModel {
    pub fn supports_images(&self) -> bool {
        self.input.contains(&InputModality::Image)
    }

    /// Price `usage` with the model's costs.
    pub fn apply_cost(&self, usage: &mut Usage) {
        self.cost.apply(usage);
    }
}

/// One question about the state. Instructions and criteria are JSON, usually text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClassifierQuestion {
    /// One of the named options, each with its description (`null` for none).
    Choice {
        instructions: Value,
        criteria: IndexMap<String, Value>,
    },
    /// A level on an ordered scale, lowest first.
    Score {
        instructions: Value,
        criteria: Vec<Value>,
    },
    /// A statement, answered with the probability that it is true.
    Bool {
        instructions: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<BoolCriteria>,
    },
}

/// What true and false mean for a [`ClassifierQuestion::Bool`], when the statement alone
/// does not say.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BoolCriteria {
    #[serde(rename = "true", default, skip_serializing_if = "Option::is_none")]
    pub when_true: Option<Value>,
    #[serde(rename = "false", default, skip_serializing_if = "Option::is_none")]
    pub when_false: Option<Value>,
}

impl ClassifierQuestion {
    pub fn choice<K: Into<String>, D: Into<Value>>(
        instructions: impl Into<String>,
        options: impl IntoIterator<Item = (K, D)>,
    ) -> Self {
        Self::Choice {
            instructions: Value::String(instructions.into()),
            criteria: options
                .into_iter()
                .map(|(label, description)| (label.into(), description.into()))
                .collect(),
        }
    }

    pub fn score<L: Into<Value>>(
        instructions: impl Into<String>,
        levels: impl IntoIterator<Item = L>,
    ) -> Self {
        Self::Score {
            instructions: Value::String(instructions.into()),
            criteria: levels.into_iter().map(Into::into).collect(),
        }
    }

    pub fn bool(instructions: impl Into<String>) -> Self {
        Self::Bool {
            instructions: Value::String(instructions.into()),
            criteria: None,
        }
    }
}

/// What to classify.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassifierContext {
    /// Any JSON; the classifier judges it as given.
    pub state: Value,
    /// Images judged with the state. Only a model with image input takes them; others
    /// answer with an error.
    pub images: Vec<ImageContent>,
    /// The questions by name, answered in this order.
    pub questions: IndexMap<String, ClassifierQuestion>,
}

/// How a classifier calibrated an answer, where it says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calibration {
    /// What the answer's logits were divided by before they became probabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// The share of the model's next-token probability that fell on the offered
    /// options. A low share means the question did not fit the state well.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_mass: Option<f64>,
}

/// The answer to one question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum ClassifierAnswer {
    Choice {
        /// The most probable option.
        choice: String,
        /// Each option's probability, in the order the options were given.
        probabilities: IndexMap<String, f64>,
        /// How peaked the distribution is, from 0 (even) to 1 (certain).
        confidence: f64,
        #[serde(flatten)]
        calibration: Calibration,
    },
    Score {
        /// The expected level, counted from 0 and weighted by probability.
        score: f64,
        confidence: f64,
        /// Each level's probability by level number, where the classifier gives them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        probabilities: Option<IndexMap<String, f64>>,
        /// Each level's text by level number, where the classifier gives them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legend: Option<IndexMap<String, Value>>,
        #[serde(flatten)]
        calibration: Calibration,
    },
    Bool {
        /// The probability that the statement is true.
        probability: f64,
        #[serde(flatten)]
        calibration: Calibration,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClassifierStopReason {
    #[default]
    Stop,
    Error,
    Aborted,
}

/// The outcome of a classification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifierResult {
    pub api: String,
    pub provider: String,
    pub model: String,
    /// The service's id for its reply, where it gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    /// One answer per question, in question order; none when the classification failed.
    #[serde(default)]
    pub answers: IndexMap<String, ClassifierAnswer>,
    /// Token usage priced at the model's costs, when the service reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// How many requests the service made to answer, where it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<u64>,
    pub stop_reason: ClassifierStopReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default)]
    pub timestamp: Timestamp,
}

impl ClassifierResult {
    /// A result from `model` with no answers yet.
    pub fn empty(model: &ClassifierModel) -> Self {
        Self {
            api: model.api.clone(),
            provider: model.provider.clone(),
            model: model.id.clone(),
            response_id: None,
            answers: IndexMap::new(),
            usage: None,
            requests: None,
            stop_reason: ClassifierStopReason::Stop,
            error_message: None,
            timestamp: now_ms(),
        }
    }

    /// A failed result carrying `error`.
    pub fn failed(
        model: &ClassifierModel,
        stop_reason: ClassifierStopReason,
        error: impl Into<String>,
    ) -> Self {
        Self {
            stop_reason,
            error_message: Some(error.into()),
            ..Self::empty(model)
        }
    }

    pub fn is_failure(&self) -> bool {
        self.stop_reason != ClassifierStopReason::Stop
    }
}

/// Options of one classification.
#[derive(Clone, Default)]
pub struct ClassifierOptions {
    pub api_key: Option<String>,
    /// What the answers' logits are divided by before they become probabilities: above
    /// 1 evens the distribution out, below 1 sharpens it. It must be positive. APIs
    /// that cannot apply it leave it out.
    pub temperature: Option<f64>,
    /// Extra request headers, over the API's and the model's. `None` removes one.
    pub headers: BTreeMap<String, Option<String>>,
    /// How long each attempt may take, in milliseconds; unset has no limit.
    pub timeout_ms: Option<u64>,
    /// How often a failed request is sent again: twice when unset.
    pub max_retries: Option<u32>,
    /// The longest wait before a retry a server may ask for, in milliseconds. Asking for
    /// longer fails the request. Unset is a minute; zero has no limit.
    pub max_retry_delay_ms: Option<u64>,
    /// Cancels the classification, which then ends `Aborted`.
    pub cancel: CancellationToken,
    pub on_payload: Option<PayloadHook>,
    pub on_response: Option<ResponseHook>,
}

impl fmt::Debug for ClassifierOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClassifierOptions")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("temperature", &self.temperature)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("timeout_ms", &self.timeout_ms)
            .field("max_retries", &self.max_retries)
            .field("max_retry_delay_ms", &self.max_retry_delay_ms)
            .field("cancelled", &self.cancel.is_cancelled())
            .field("on_payload", &self.on_payload.is_some())
            .field("on_response", &self.on_response.is_some())
            .finish()
    }
}

/// A classifier API: answers a classification. Failures end in the result, with
/// `stop_reason` `Error` or `Aborted`.
#[async_trait]
pub trait ClassifyFn: Send + Sync {
    async fn classify(
        &self,
        model: &ClassifierModel,
        context: ClassifierContext,
        options: ClassifierOptions,
    ) -> ClassifierResult;
}

/// Classifier APIs keyed by [`ClassifierModel::api`]. Classifying through the registry
/// dispatches to the implementation registered for the model's API.
#[derive(Clone, Default)]
pub struct ClassifierRegistry {
    apis: Arc<RwLock<HashMap<String, Arc<dyn ClassifyFn>>>>,
}

impl ClassifierRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `implementation` for `api`, replacing an earlier registration.
    pub fn register(&self, api: impl Into<String>, implementation: Arc<dyn ClassifyFn>) {
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

    pub fn get(&self, api: &str) -> Option<Arc<dyn ClassifyFn>> {
        self.apis
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(api)
            .cloned()
    }
}

#[async_trait]
impl ClassifyFn for ClassifierRegistry {
    /// As Pi's `classify`: images go only to a model that takes them.
    async fn classify(
        &self,
        model: &ClassifierModel,
        context: ClassifierContext,
        options: ClassifierOptions,
    ) -> ClassifierResult {
        let failed =
            |error: String| ClassifierResult::failed(model, ClassifierStopReason::Error, error);
        if !context.images.is_empty() && !model.supports_images() {
            return failed(format!(
                "Model {}/{} does not accept image input",
                model.provider, model.id
            ));
        }
        match self.get(&model.api) {
            Some(implementation) => implementation.classify(model, context, options).await,
            None => failed(format!(
                "No classifier registered for API \"{}\"",
                model.api
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model(input: &[&str]) -> ClassifierModel {
        serde_json::from_value(json!({
            "id": "judge", "name": "Judge", "api": "test", "provider": "local",
            "input": input, "contextWindow": 8192,
        }))
        .unwrap()
    }

    struct Fixed;

    #[async_trait]
    impl ClassifyFn for Fixed {
        async fn classify(
            &self,
            model: &ClassifierModel,
            _context: ClassifierContext,
            _options: ClassifierOptions,
        ) -> ClassifierResult {
            ClassifierResult::empty(model)
        }
    }

    #[tokio::test]
    async fn the_registry_dispatches_by_api_and_keeps_images_from_blind_models() {
        let registry = ClassifierRegistry::new();
        let images = ClassifierContext {
            images: vec![ImageContent {
                data: "aGk=".into(),
                mime_type: "image/png".into(),
            }],
            ..ClassifierContext::default()
        };
        let missing = registry
            .classify(
                &model(&["text"]),
                ClassifierContext::default(),
                ClassifierOptions::default(),
            )
            .await;
        assert_eq!(
            missing.error_message.as_deref(),
            Some("No classifier registered for API \"test\"")
        );

        registry.register("test", Arc::new(Fixed));
        let blind = registry
            .classify(
                &model(&["text"]),
                images.clone(),
                ClassifierOptions::default(),
            )
            .await;
        assert_eq!(blind.stop_reason, ClassifierStopReason::Error);
        assert_eq!(
            blind.error_message.as_deref(),
            Some("Model local/judge does not accept image input")
        );
        let seeing = registry
            .classify(
                &model(&["text", "image"]),
                images,
                ClassifierOptions::default(),
            )
            .await;
        assert!(!seeing.is_failure());
    }

    #[test]
    fn questions_and_answers_serialize_as_their_kinds() {
        let question =
            ClassifierQuestion::choice("Which?", [("a", json!("first")), ("b", Value::Null)]);
        assert_eq!(
            serde_json::to_value(&question).unwrap(),
            json!({ "type": "choice", "instructions": "Which?", "criteria": { "a": "first", "b": null } })
        );
        assert_eq!(
            serde_json::to_value(ClassifierQuestion::bool("Is it?")).unwrap(),
            json!({ "type": "bool", "instructions": "Is it?" })
        );
        let answer = ClassifierAnswer::Bool {
            probability: 0.9,
            calibration: Calibration {
                temperature: Some(2.0),
                option_mass: Some(0.99),
            },
        };
        let value = serde_json::to_value(&answer).unwrap();
        assert_eq!(
            value,
            json!({ "type": "bool", "probability": 0.9, "temperature": 2.0, "optionMass": 0.99 })
        );
        assert_eq!(
            serde_json::from_value::<ClassifierAnswer>(value).unwrap(),
            answer
        );
    }
}
