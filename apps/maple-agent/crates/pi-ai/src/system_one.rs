//! System One's protocol (`api: "system-one"`), after Pi's System One APIs.
//!
//! A request goes to `{base_url}/systemone` with the model, the state and the questions,
//! and with the images (as data URLs) and the temperature when there are any. A yes/no
//! question is a `noul` question on the wire. Each answer comes back with its
//! probabilities. Pi's `typesafe-system-one` speaks the same protocol without images or
//! a temperature; this is the protocol as Maple's server serves it, with the extra
//! fields its answers carry. The request is sent as Pi's `postClassifierRequest` sends
//! it, through the host's [`HttpTransport`].

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use indexmap::IndexMap;
use serde_json::{Map, Value, json};

use crate::classifier::{
    Calibration, ClassifierAnswer, ClassifierContext, ClassifierModel, ClassifierOptions,
    ClassifierQuestion, ClassifierResult, ClassifierStopReason, ClassifyFn,
};
use crate::openai::{HttpRequest, HttpTransport, truncate_error_text};
use crate::provider::ProviderResponse;
use crate::provider_retry::RequestFailure;
use crate::types::Usage;

pub const API: &str = "system-one";

/// The service's name in error messages, as Pi names it.
const LABEL: &str = "System One API";

/// Pi's default number of retries for a classification.
const DEFAULT_MAX_RETRIES: u32 = 2;

const ABORTED: &str = "Request was aborted";

/// The longest reply read, in bytes.
const BODY_LIMIT: usize = 16 * 1024 * 1024;

/// The System One API.
#[derive(Clone)]
pub struct SystemOne {
    transport: Arc<dyn HttpTransport>,
}

impl SystemOne {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }
}

#[async_trait]
impl ClassifyFn for SystemOne {
    async fn classify(
        &self,
        model: &ClassifierModel,
        context: ClassifierContext,
        options: ClassifierOptions,
    ) -> ClassifierResult {
        let mut result = ClassifierResult::empty(model);
        let outcome = match post(&*self.transport, model, &context, &options).await {
            Ok(body) => {
                // Before the answers: a reply with malformed answers was still billed.
                if let Some(usage) = body.get("usage") {
                    result.usage = parse_usage(model, usage);
                    result.requests = usage.get("requests").and_then(Value::as_u64);
                }
                result.response_id = body.get("id").and_then(Value::as_str).map(str::to_string);
                parse_answers(body.get("answers"), &context).map(|answers| result.answers = answers)
            }
            Err(error) => Err(error),
        };
        if let Err(error) = outcome {
            result.stop_reason = if options.cancel.is_cancelled() {
                ClassifierStopReason::Aborted
            } else {
                ClassifierStopReason::Error
            };
            result.error_message = Some(error);
            result.answers.clear();
        }
        result
    }
}

/// The request body: the model, the state and the questions, `bool` ones as `noul`,
/// then the images and the temperature when there are any.
fn wire_request(
    model: &ClassifierModel,
    context: &ClassifierContext,
    options: &ClassifierOptions,
) -> Value {
    let questions: Map<String, Value> = context
        .questions
        .iter()
        .map(|(id, question)| {
            let mut value = serde_json::to_value(question).expect("questions serialize");
            if matches!(question, ClassifierQuestion::Bool { .. }) {
                value["type"] = json!("noul");
            }
            (id.clone(), value)
        })
        .collect();
    let mut body = json!({
        "model": model.id,
        "state": context.state,
        "questions": questions,
    });
    if !context.images.is_empty() {
        let images: Vec<String> = context
            .images
            .iter()
            .map(|image| format!("data:{};base64,{}", image.mime_type, image.data))
            .collect();
        body["images"] = json!(images);
    }
    if let Some(temperature) = options.temperature {
        body["temperature"] = json!(temperature);
    }
    body
}

fn request_headers(model: &ClassifierModel, options: &ClassifierOptions) -> Vec<(String, String)> {
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    if let Some(key) = &options.api_key {
        headers.push(("authorization".to_string(), format!("Bearer {key}")));
    }
    let set = |headers: &mut Vec<(String, String)>, name: &str, value: Option<&str>| {
        headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        if let Some(value) = value {
            headers.push((name.to_string(), value.to_string()));
        }
    };
    for (name, value) in &model.headers {
        set(&mut headers, name, Some(value));
    }
    for (name, value) in &options.headers {
        set(&mut headers, name, value.as_deref());
    }
    headers
}

/// How one attempt ended.
enum Attempt {
    Replied(Map<String, Value>, ProviderResponse),
    /// May be retried, as the failure's kind allows.
    Failed(RequestFailure),
    /// Not retried: the transport failed, or the reply was no JSON object.
    Fatal(String),
}

/// Post the classification as Pi's `postClassifierRequest` does: the payload hook, then
/// attempts with a fresh timeout each, retried as Pi retries provider requests, then
/// the response hook. The reply's body, or the error the result fails with.
async fn post(
    transport: &dyn HttpTransport,
    model: &ClassifierModel,
    context: &ClassifierContext,
    options: &ClassifierOptions,
) -> Result<Map<String, Value>, String> {
    let mut payload = wire_request(model, context, options);
    if let Some(hook) = &options.on_payload {
        payload = hook(payload).await;
    }
    let request = HttpRequest {
        url: format!("{}/systemone", model.base_url.trim_end_matches('/')),
        headers: request_headers(model, options),
        body: serde_json::to_vec(&payload).unwrap_or_default(),
    };
    let max_retries = options.max_retries.unwrap_or(DEFAULT_MAX_RETRIES);
    let mut retry_index = 0;
    loop {
        let failure = match attempt(transport, request.clone(), options).await {
            Attempt::Replied(body, response) => {
                if let Some(hook) = &options.on_response {
                    hook(response).await;
                }
                return Ok(body);
            }
            Attempt::Fatal(error) => return Err(error),
            Attempt::Failed(failure) => failure,
        };
        if retry_index >= max_retries || !failure.is_retryable() {
            return Err(failure.message);
        }
        let delay = failure.retry_delay(retry_index, options.max_retry_delay_ms)?;
        retry_index += 1;
        tokio::select! {
            _ = options.cancel.cancelled() => return Err(ABORTED.to_string()),
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

async fn attempt(
    transport: &dyn HttpTransport,
    request: HttpRequest,
    options: &ClassifierOptions,
) -> Attempt {
    // A timeout reaches the transport as the cancellation of this attempt alone.
    let attempt_cancel = options.cancel.child_token();
    let exchange = async {
        let response = match transport.post(request, attempt_cancel.clone()).await {
            Ok(response) => response,
            Err(error) => return Attempt::Fatal(error),
        };
        let (status, headers) = (response.status, response.headers);
        let text = match read_body(response.body).await {
            Ok(text) => text,
            Err(error) => return Attempt::Fatal(error),
        };
        if !(200..300).contains(&status) {
            let sdk_message = format!("{LABEL} returned {status}");
            return Attempt::Failed(RequestFailure {
                status: Some(status),
                message: error_message(status, &text, &sdk_message),
                headers,
                sdk_message,
            });
        }
        let Ok(Value::Object(body)) = serde_json::from_str::<Value>(&text) else {
            return Attempt::Fatal(format!("{LABEL} returned an unexpected response"));
        };
        let headers = headers
            .into_iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .collect();
        Attempt::Replied(body, ProviderResponse { status, headers })
    };
    let timeout = async {
        match options.timeout_ms {
            Some(timeout) => tokio::time::sleep(Duration::from_millis(timeout)).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = options.cancel.cancelled() => Attempt::Fatal(ABORTED.to_string()),
        _ = timeout => {
            attempt_cancel.cancel();
            let timeout = options.timeout_ms.unwrap_or_default();
            Attempt::Failed(RequestFailure::without_response(format!(
                "Request timed out after {timeout}ms"
            )))
        }
        attempt = exchange => attempt,
    }
}

async fn read_body(mut body: BoxStream<'static, Result<Bytes, String>>) -> Result<String, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk?);
        if bytes.len() > BODY_LIMIT {
            return Err(format!("{LABEL} returned a reply over {BODY_LIMIT} bytes"));
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// An error reply's message, as Pi composes it: the status and the body, or the status
/// and the short message when the body is empty.
fn error_message(status: u16, text: &str, sdk_message: &str) -> String {
    let body = text.trim();
    if body.is_empty() {
        format!("{LABEL} error ({status}): {sdk_message}")
    } else {
        format!("{LABEL} error ({status}): {}", truncate_error_text(body))
    }
}

fn required_number(value: Option<&Value>, field: &str) -> Result<f64, String> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
        .ok_or_else(|| format!("{LABEL} returned an invalid {field}"))
}

fn optional_number(answer: &Map<String, Value>, key: &str) -> Option<f64> {
    answer
        .get(key)
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
}

fn probabilities(value: Option<&Value>, id: &str) -> Result<IndexMap<String, f64>, String> {
    let Some(Value::Object(entries)) = value else {
        return Err(format!("{LABEL} returned invalid probabilities for {id}"));
    };
    entries
        .iter()
        .map(|(key, probability)| {
            required_number(Some(probability), &format!("probability for {id}.{key}"))
                .map(|probability| (key.clone(), probability))
        })
        .collect()
}

/// The answer to each question, in question order, checked as Pi checks them. The fields
/// Maple's server adds are read where they are well formed.
fn parse_answers(
    value: Option<&Value>,
    context: &ClassifierContext,
) -> Result<IndexMap<String, ClassifierAnswer>, String> {
    let Some(Value::Object(answers)) = value else {
        return Err(format!("{LABEL} returned an unexpected response"));
    };
    let mut parsed = IndexMap::new();
    for (id, question) in &context.questions {
        let Some(Value::Object(answer)) = answers.get(id) else {
            return Err(format!("{LABEL} did not return an answer for {id}"));
        };
        let kind = answer.get("type").and_then(Value::as_str);
        let calibration = Calibration {
            temperature: optional_number(answer, "temperature"),
            option_mass: optional_number(answer, "option_mass"),
        };
        let parsed_answer = match question {
            ClassifierQuestion::Choice { .. } => {
                let (Some("choice"), Some(Value::String(choice))) = (kind, answer.get("choice"))
                else {
                    return Err(format!("{LABEL} did not return a choice answer for {id}"));
                };
                ClassifierAnswer::Choice {
                    choice: choice.clone(),
                    probabilities: probabilities(answer.get("probabilities"), id)?,
                    confidence: required_number(
                        answer.get("confidence"),
                        &format!("confidence for {id}"),
                    )?,
                    calibration,
                }
            }
            ClassifierQuestion::Score { .. } => {
                if kind != Some("score") {
                    return Err(format!("{LABEL} did not return a score answer for {id}"));
                }
                ClassifierAnswer::Score {
                    score: required_number(answer.get("score"), &format!("score for {id}"))?,
                    confidence: required_number(
                        answer.get("confidence"),
                        &format!("confidence for {id}"),
                    )?,
                    probabilities: probabilities(answer.get("probabilities"), id).ok(),
                    legend: answer
                        .get("legend")
                        .and_then(Value::as_object)
                        .map(|legend| {
                            legend
                                .iter()
                                .map(|(key, text)| (key.clone(), text.clone()))
                                .collect()
                        }),
                    calibration,
                }
            }
            ClassifierQuestion::Bool { .. } => {
                if kind != Some("noul") {
                    return Err(format!("{LABEL} did not return a bool answer for {id}"));
                }
                ClassifierAnswer::Bool {
                    probability: required_number(
                        answer.get("noul"),
                        &format!("probability for {id}"),
                    )?,
                    calibration,
                }
            }
        };
        parsed.insert(id.clone(), parsed_answer);
    }
    Ok(parsed)
}

/// Usage from `{ input_tokens, output_tokens }`, priced at the model's costs, as Pi reads
/// it; none when neither is given. Maple's `cached_tokens` count among the input tokens
/// and are read out of them as cache reads, as Chat Completions usage is read.
fn parse_usage(model: &ClassifierModel, value: &Value) -> Option<Usage> {
    let usage = value.as_object()?;
    if usage.get("input_tokens").is_none() && usage.get("output_tokens").is_none() {
        return None;
    }
    let count = |key: &str| {
        usage
            .get(key)
            .and_then(Value::as_f64)
            .filter(|count| count.is_finite() && *count > 0.0)
            .map_or(0, |count| count as u64)
    };
    let cached = count("cached_tokens");
    let output = count("output_tokens");
    let all_input = count("input_tokens");
    let cache_read = cached.min(all_input);
    let mut parsed = Usage {
        input: all_input - cache_read,
        output,
        cache_read,
        total_tokens: all_input + output,
        ..Usage::default()
    };
    model.apply_cost(&mut parsed);
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::HttpResponse;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    /// A reply's status, headers and body, or a transport failure.
    type Reply = Result<(u16, Vec<(&'static str, &'static str)>, String), &'static str>;

    /// Answers each request with the next reply and records the requests.
    #[derive(Default)]
    struct Replies {
        replies: Mutex<VecDeque<Reply>>,
        requests: Mutex<Vec<HttpRequest>>,
    }

    impl Replies {
        fn new(replies: Vec<Reply>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies.into()),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn sent(&self) -> Vec<Value> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| serde_json::from_slice(&request.body).unwrap())
                .collect()
        }
    }

    #[async_trait]
    impl HttpTransport for Replies {
        async fn post(
            &self,
            request: HttpRequest,
            _cancel: CancellationToken,
        ) -> Result<HttpResponse, String> {
            self.requests.lock().unwrap().push(request);
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("an unexpected request");
            let (status, headers, body) = reply.map_err(str::to_string)?;
            Ok(HttpResponse {
                status,
                headers: headers
                    .into_iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect(),
                body: futures_util::stream::iter([Ok(Bytes::from(body))]).boxed(),
            })
        }
    }

    fn model() -> ClassifierModel {
        serde_json::from_value(json!({
            "id": "glm-5-3-flash", "name": "GLM 5.3 Flash", "api": API, "provider": "maple",
            "baseUrl": "/v1", "input": ["text", "image"], "contextWindow": 131072,
            "cost": { "input": 1.0, "output": 2.0, "cacheRead": 0.5 },
        }))
        .unwrap()
    }

    fn context() -> ClassifierContext {
        ClassifierContext {
            state: json!({ "ticket": "Charged twice" }),
            images: Vec::new(),
            questions: [
                (
                    "urgent".to_string(),
                    ClassifierQuestion::bool("Is it urgent?"),
                ),
                (
                    "intent".to_string(),
                    ClassifierQuestion::choice(
                        "What do they want?",
                        [("refund", "money back"), ("10", "ten")],
                    ),
                ),
                (
                    "frustration".to_string(),
                    ClassifierQuestion::score("How frustrated?", ["Low", "High"]),
                ),
            ]
            .into_iter()
            .collect(),
        }
    }

    const REPLY: &str = r#"{"id":"so_1","model":"glm-5-3-flash","answers":{
        "urgent":{"type":"noul","noul":0.93,"temperature":2.48,"option_mass":1.0},
        "intent":{"type":"choice","choice":"refund","probabilities":{"refund":0.86,"10":0.14},"confidence":0.6,"temperature":2.4,"option_mass":0.99},
        "frustration":{"type":"score","score":1.2,"legend":{"0":"Low","1":"High"},"probabilities":{"0":0.4,"1":0.6},"confidence":0.03,"temperature":2.48,"option_mass":0.99}
    },"usage":{"input_tokens":388,"output_tokens":3,"cached_tokens":100,"requests":3}}"#;

    async fn classify(
        transport: Arc<Replies>,
        context: ClassifierContext,
        options: ClassifierOptions,
    ) -> ClassifierResult {
        SystemOne::new(transport)
            .classify(&model(), context, options)
            .await
    }

    #[tokio::test]
    async fn a_classification_is_posted_and_answered_in_question_order() {
        let transport = Replies::new(vec![Ok((200, vec![], REPLY.to_string()))]);
        let mut context = context();
        context.images.push(crate::types::ImageContent {
            data: "AAAA".into(),
            mime_type: "image/png".into(),
        });
        let options = ClassifierOptions {
            temperature: Some(1.0),
            api_key: Some("key".into()),
            ..ClassifierOptions::default()
        };
        let result = classify(transport.clone(), context, options).await;

        assert_eq!(
            result.stop_reason,
            ClassifierStopReason::Stop,
            "{:?}",
            result.error_message
        );
        let request = transport.requests.lock().unwrap()[0].clone();
        assert_eq!(request.url, "/v1/systemone");
        assert!(
            request
                .headers
                .contains(&("authorization".into(), "Bearer key".into()))
        );
        assert_eq!(
            serde_json::to_string(&transport.sent()[0]).unwrap(),
            r#"{"model":"glm-5-3-flash","state":{"ticket":"Charged twice"},"questions":{"urgent":{"type":"noul","instructions":"Is it urgent?"},"intent":{"type":"choice","instructions":"What do they want?","criteria":{"refund":"money back","10":"ten"}},"frustration":{"type":"score","instructions":"How frustrated?","criteria":["Low","High"]}},"images":["data:image/png;base64,AAAA"],"temperature":1.0}"#
        );

        let names: Vec<&str> = result.answers.keys().map(String::as_str).collect();
        assert_eq!(names, ["urgent", "intent", "frustration"]);
        assert_eq!(
            result.answers["urgent"],
            ClassifierAnswer::Bool {
                probability: 0.93,
                calibration: Calibration {
                    temperature: Some(2.48),
                    option_mass: Some(1.0)
                },
            }
        );
        let ClassifierAnswer::Choice {
            choice,
            probabilities,
            confidence,
            ..
        } = &result.answers["intent"]
        else {
            panic!("{:?}", result.answers["intent"])
        };
        assert_eq!(choice, "refund");
        assert_eq!(probabilities.keys().collect::<Vec<_>>(), ["refund", "10"]);
        assert_eq!(*confidence, 0.6);
        let ClassifierAnswer::Score {
            score,
            legend,
            probabilities,
            ..
        } = &result.answers["frustration"]
        else {
            panic!()
        };
        assert_eq!(*score, 1.2);
        assert_eq!(legend.as_ref().unwrap()["1"], "High");
        assert_eq!(probabilities.as_ref().unwrap()["1"], 0.6);

        assert_eq!(result.response_id.as_deref(), Some("so_1"));
        assert_eq!(result.requests, Some(3));
        let usage = result.usage.unwrap();
        assert_eq!(
            (
                usage.input,
                usage.cache_read,
                usage.output,
                usage.total_tokens
            ),
            (288, 100, 3, 391)
        );
        assert!(usage.cost.total > 0.0);
    }

    #[tokio::test]
    async fn a_reply_missing_an_answer_fails_but_keeps_its_usage() {
        let reply = r#"{"answers":{"urgent":{"type":"noul","noul":0.5}},"usage":{"input_tokens":10,"output_tokens":1}}"#;
        let transport = Replies::new(vec![Ok((200, vec![], reply.to_string()))]);
        let result = classify(transport, context(), ClassifierOptions::default()).await;
        assert_eq!(result.stop_reason, ClassifierStopReason::Error);
        assert_eq!(
            result.error_message.as_deref(),
            Some("System One API did not return an answer for intent")
        );
        assert!(result.answers.is_empty());
        assert_eq!(result.usage.unwrap().input, 10);

        let reply = r#"{"answers":{"urgent":{"type":"choice","choice":"yes"}}}"#;
        let transport = Replies::new(vec![Ok((200, vec![], reply.to_string()))]);
        let result = classify(transport, context(), ClassifierOptions::default()).await;
        assert_eq!(
            result.error_message.as_deref(),
            Some("System One API did not return a bool answer for urgent")
        );
    }

    #[tokio::test]
    async fn failures_are_retried_twice_and_reported_as_pi_reports_them() {
        let transport = Replies::new(vec![
            Ok((503, vec![("retry-after-ms", "1")], "overloaded".to_string())),
            Ok((429, vec![("retry-after-ms", "1")], String::new())),
            Ok((200, vec![], REPLY.to_string())),
        ]);
        let result = classify(transport.clone(), context(), ClassifierOptions::default()).await;
        assert!(!result.is_failure(), "{:?}", result.error_message);
        assert_eq!(transport.sent().len(), 3);

        let transport = Replies::new(vec![
            Ok((503, vec![("retry-after-ms", "1")], "overloaded".to_string())),
            Ok((503, vec![("retry-after-ms", "1")], String::new())),
            Ok((
                503,
                vec![("retry-after-ms", "1")],
                "  still overloaded ".to_string(),
            )),
        ]);
        let result = classify(transport.clone(), context(), ClassifierOptions::default()).await;
        assert_eq!(
            result.error_message.as_deref(),
            Some("System One API error (503): still overloaded")
        );
        assert_eq!(transport.sent().len(), 3);

        let transport = Replies::new(vec![Ok((400, vec![], String::new()))]);
        let result = classify(transport, context(), ClassifierOptions::default()).await;
        assert_eq!(
            result.error_message.as_deref(),
            Some("System One API error (400): System One API returned 400")
        );

        let transport = Replies::new(vec![Ok((
            429,
            vec![("retry-after", "120")],
            "slow down".to_string(),
        ))]);
        let result = classify(transport, context(), ClassifierOptions::default()).await;
        assert_eq!(
            result.error_message.as_deref(),
            Some("Server requested 120s retry delay (max: 60s). System One API returned 429")
        );

        // A transport failure and a reply that is no JSON are not retried.
        let transport = Replies::new(vec![Err("connection refused")]);
        let result = classify(transport, context(), ClassifierOptions::default()).await;
        assert_eq!(result.error_message.as_deref(), Some("connection refused"));
        let transport = Replies::new(vec![Ok((200, vec![], "<html>".to_string()))]);
        let result = classify(transport, context(), ClassifierOptions::default()).await;
        assert_eq!(
            result.error_message.as_deref(),
            Some("System One API returned an unexpected response")
        );
    }

    #[tokio::test]
    async fn hooks_see_the_payload_and_the_response() {
        let transport = Replies::new(vec![Ok((
            200,
            vec![("X-Request-Id", "r1")],
            REPLY.to_string(),
        ))]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let options = ClassifierOptions {
            on_payload: Some(Arc::new(|mut payload: Value| {
                payload["model"] = json!("other");
                Box::pin(async move { payload })
            })),
            on_response: Some(Arc::new(move |response: ProviderResponse| {
                log.lock().unwrap().push(response);
                Box::pin(async {})
            })),
            ..ClassifierOptions::default()
        };
        let result = classify(transport.clone(), context(), options).await;
        assert!(!result.is_failure());
        assert_eq!(transport.sent()[0]["model"], "other");
        assert_eq!(
            *seen.lock().unwrap(),
            [ProviderResponse {
                status: 200,
                headers: [("x-request-id".to_string(), "r1".to_string())].into(),
            }]
        );
    }

    #[tokio::test]
    async fn a_cancelled_classification_is_aborted() {
        let transport = Replies::new(vec![Ok((
            503,
            vec![("retry-after-ms", "5000")],
            String::new(),
        ))]);
        let cancel = CancellationToken::new();
        let options = ClassifierOptions {
            cancel: cancel.clone(),
            ..ClassifierOptions::default()
        };
        let classification = tokio::spawn(classify(transport, context(), options));
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        let result = classification.await.unwrap();
        assert_eq!(result.stop_reason, ClassifierStopReason::Aborted);
    }
}
