//! Minimal client for Kagi's v1 Search and Extract APIs.
//!
//! Search intentionally does not request inline extraction. Callers can inspect
//! the search results and then explicitly extract only the pages they need.

use reqwest::{header, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use std::{collections::BTreeMap, fmt, sync::Arc, time::Duration};
use url::Url;

use crate::web::web_safety::normalize_public_https_url;

const KAGI_API_BASE: &str = "https://kagi.com/api/v1/";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const KAGI_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const SEARCH_RESPONSE_LIMIT_BYTES: usize = 1024 * 1024;
const EXTRACT_RESPONSE_LIMIT_BYTES: usize = 5 * 1024 * 1024;
const ERROR_MESSAGE_LIMIT_CHARS: usize = 4 * 1024;
pub(crate) const MAX_EXTRACT_URLS: usize = 10;
const TRACE_HEADER: &str = "x-kagi-trace";
const TRACE_ID_LIMIT_CHARS: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum KagiError {
    #[error("Kagi API key cannot be empty")]
    InvalidApiKey,

    #[error("Kagi search query cannot be empty")]
    InvalidQuery,

    #[error("Kagi search parameter `{field}` is invalid")]
    InvalidSearchParameter { field: &'static str },

    #[error("Kagi extract requires between 1 and {MAX_EXTRACT_URLS} URLs (received {count})")]
    InvalidUrlCount { count: usize },

    #[error("Kagi extract URL at index {index} is invalid: {reason}")]
    InvalidUrl { index: usize, reason: String },

    #[error("invalid Kagi API base URL: {0}")]
    InvalidBaseUrl(#[from] url::ParseError),

    #[error("Kagi {operation} request failed: {source}")]
    Request {
        operation: &'static str,
        #[source]
        source: reqwest::Error,
    },

    #[error("Kagi {operation} response exceeded {limit_bytes} bytes (trace ID: {trace_id})")]
    ResponseTooLarge {
        operation: &'static str,
        limit_bytes: usize,
        trace_id: String,
    },

    #[error("Kagi {operation} API returned HTTP {status} (trace ID: {trace_id})")]
    Api {
        operation: &'static str,
        status: StatusCode,
        message: String,
        trace_id: String,
    },

    #[error("invalid Kagi {operation} response (trace ID: {trace_id})")]
    InvalidResponse {
        operation: &'static str,
        message: String,
        trace_id: String,
    },
}

/// Kagi API client with a reusable connection pool and redacted credentials.
#[derive(Clone)]
pub struct KagiClient {
    client: reqwest::Client,
    api_key: Arc<str>,
    base_url: Url,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SearchWorkflow {
    #[default]
    Search,
    Images,
    Videos,
    News,
    Podcasts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SearchTimeRelative {
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SearchLens {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sites_included: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sites_excluded: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords_included: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords_excluded: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_relative: Option<SearchTimeRelative>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_region: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SearchFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchOptions {
    pub query: String,
    pub workflow: SearchWorkflow,
    pub lens_id: Option<String>,
    pub lens: Option<SearchLens>,
    pub timeout: Option<f32>,
    pub page: Option<u8>,
    pub limit: u16,
    pub filters: Option<SearchFilters>,
    pub safe_search: bool,
}

impl SearchOptions {
    pub fn basic(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            workflow: SearchWorkflow::Search,
            lens_id: None,
            lens: None,
            timeout: None,
            page: None,
            limit: 10,
            filters: None,
            safe_search: true,
        }
    }
}

impl KagiClient {
    pub fn new(api_key: String) -> Result<Self, KagiError> {
        Self::new_with_base_url_inner(api_key, KAGI_API_BASE)
    }

    #[cfg(test)]
    pub(crate) fn new_with_base_url_for_test(
        api_key: String,
        base_url: &str,
    ) -> Result<Self, KagiError> {
        Self::new_with_base_url_inner(api_key, base_url)
    }

    fn new_with_base_url_inner(api_key: String, base_url: &str) -> Result<Self, KagiError> {
        let api_key = api_key.trim().to_owned();
        if api_key.is_empty() {
            return Err(KagiError::InvalidApiKey);
        }

        let base_url = Url::parse(&format!("{}/", base_url.trim_end_matches('/')))?;
        let client = crate::http_client::client_builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(KAGI_REQUEST_TIMEOUT)
            .pool_max_idle_per_host(100)
            .user_agent("OpenSecret/0.1.0")
            .build()
            .map_err(|source| KagiError::Request {
                operation: "client initialization",
                source,
            })?;

        Ok(Self {
            client,
            api_key: Arc::from(api_key),
            base_url,
        })
    }

    /// Search Kagi for web and news results without fetching page contents.
    pub async fn search(&self, query: &str) -> Result<SearchResponse, KagiError> {
        self.search_with_options(&SearchOptions::basic(query)).await
    }

    /// Search Kagi with validated options while always requesting stable JSON
    /// output and never enabling inline page extraction.
    pub(crate) async fn search_with_options(
        &self,
        options: &SearchOptions,
    ) -> Result<SearchResponse, KagiError> {
        let query = options.query.trim();
        if query.is_empty() {
            return Err(KagiError::InvalidQuery);
        }
        validate_search_options(options)?;

        let request = SearchRequest {
            query,
            workflow: options.workflow,
            format: "json",
            lens_id: options.lens_id.as_deref(),
            lens: options.lens.as_ref(),
            timeout: options.timeout,
            page: options.page,
            limit: options.limit,
            filters: options.filters.as_ref(),
            safe_search: options.safe_search,
        };

        let response = self
            .send_json("search", self.endpoint("search")?, &request)
            .await?;

        parse_response(response, "search", SEARCH_RESPONSE_LIMIT_BYTES).await
    }

    /// Extract Markdown from one to ten public HTTPS URLs.
    pub async fn extract(&self, urls: &[String]) -> Result<ExtractResponse, KagiError> {
        self.extract_with_timeout(urls, None).await
    }

    pub(crate) async fn extract_with_timeout(
        &self,
        urls: &[String],
        timeout: Option<f32>,
    ) -> Result<ExtractResponse, KagiError> {
        let urls = normalize_extract_urls(urls)?;
        if timeout.is_some_and(|value| !(0.5..=10.0).contains(&value) || !value.is_finite()) {
            return Err(KagiError::InvalidSearchParameter {
                field: "extract.timeout",
            });
        }

        let request = ExtractRequest {
            pages: urls.iter().map(|url| PageInput { url }).collect(),
            timeout,
            format: "json",
        };

        let response = self
            .send_json("extract", self.endpoint("extract")?, &request)
            .await?;

        parse_response(response, "extract", EXTRACT_RESPONSE_LIMIT_BYTES).await
    }

    async fn send_json<T: Serialize + ?Sized>(
        &self,
        operation: &'static str,
        endpoint: Url,
        payload: &T,
    ) -> Result<reqwest::Response, KagiError> {
        self.client
            .post(endpoint)
            .bearer_auth(self.api_key.as_ref())
            .header(header::ACCEPT, "application/json")
            .json(payload)
            .timeout(KAGI_REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|source| KagiError::Request { operation, source })
    }

    fn endpoint(&self, path: &'static str) -> Result<Url, KagiError> {
        Ok(self.base_url.join(path)?)
    }
}

impl fmt::Debug for KagiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KagiClient")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Meta {
    #[serde(default, deserialize_with = "deserialize_optional_trace")]
    pub trace: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchResponse {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub meta: Meta,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub data: SearchData,
}

#[derive(Debug, Clone, Default)]
pub struct SearchData {
    pub search: Vec<SearchResult>,
    pub news: Vec<SearchResult>,
    pub categories: BTreeMap<String, Vec<SearchResult>>,
}

impl SearchData {
    pub(crate) fn into_categories(self) -> Vec<(String, Vec<SearchResult>)> {
        let mut categories = Vec::with_capacity(self.categories.len() + 2);
        if !self.search.is_empty() {
            categories.push(("search".to_owned(), self.search));
        }
        if !self.news.is_empty() {
            categories.push(("news".to_owned(), self.news));
        }
        categories.extend(self.categories);
        categories
    }
}

impl<'de> Deserialize<'de> for SearchData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let Some(object) = value.as_object() else {
            return Err(serde::de::Error::custom(
                "Kagi search data must be an object",
            ));
        };

        let mut data = Self::default();
        for (category, value) in object {
            let results = value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| serde_json::from_value::<SearchResult>(item.clone()).ok())
                .collect::<Vec<_>>();

            match category.as_str() {
                "search" => data.search = results,
                "news" => data.news = results,
                _ if !results.is_empty() => {
                    data.categories.insert(category.clone(), results);
                }
                _ => {}
            }
        }

        Ok(data)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExtractResponse {
    pub meta: Meta,
    pub data: Vec<ExtractPage>,
    pub errors: Vec<ErrorDetail>,
}

impl<'de> Deserialize<'de> for ExtractResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireResponse {
            #[serde(default, deserialize_with = "deserialize_null_default")]
            meta: Meta,
            #[serde(default, deserialize_with = "deserialize_null_default")]
            data: Vec<ExtractPage>,
            #[serde(default, deserialize_with = "deserialize_null_default")]
            errors: Vec<ErrorDetail>,
            #[serde(
                default,
                rename = "error",
                deserialize_with = "deserialize_null_default"
            )]
            error: Vec<ErrorDetail>,
        }

        let mut wire = WireResponse::deserialize(deserializer)?;
        wire.errors.append(&mut wire.error);
        Ok(Self {
            meta: wire.meta,
            data: wire.data,
            errors: wire.errors,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExtractPage {
    pub url: String,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ErrorDetail {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Serialize)]
struct SearchRequest<'a> {
    query: &'a str,
    workflow: SearchWorkflow,
    format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    lens_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lens: Option<&'a SearchLens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<u8>,
    limit: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    filters: Option<&'a SearchFilters>,
    safe_search: bool,
}

#[derive(Serialize)]
struct ExtractRequest<'a> {
    pages: Vec<PageInput<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<f32>,
    format: &'static str,
}

#[derive(Serialize)]
struct PageInput<'a> {
    url: &'a str,
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_optional_trace<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.map(|trace| sanitize_trace_id(&trace)))
}

pub(crate) fn sanitize_trace_id(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
        })
        .take(TRACE_ID_LIMIT_CHARS)
        .collect();
    if sanitized.is_empty() {
        "unavailable".to_owned()
    } else {
        sanitized
    }
}

fn validate_search_options(options: &SearchOptions) -> Result<(), KagiError> {
    if options.limit == 0 || options.limit > 1_024 {
        return Err(KagiError::InvalidSearchParameter { field: "limit" });
    }
    if options.page.is_some_and(|page| !(1..=10).contains(&page)) {
        return Err(KagiError::InvalidSearchParameter { field: "page" });
    }
    if options
        .timeout
        .is_some_and(|timeout| !(0.5..=4.0).contains(&timeout) || !timeout.is_finite())
    {
        return Err(KagiError::InvalidSearchParameter { field: "timeout" });
    }
    if options.lens_id.is_some() && options.lens.is_some() {
        return Err(KagiError::InvalidSearchParameter { field: "lens" });
    }
    Ok(())
}

fn normalize_extract_urls(urls: &[String]) -> Result<Vec<String>, KagiError> {
    if urls.is_empty() || urls.len() > MAX_EXTRACT_URLS {
        return Err(KagiError::InvalidUrlCount { count: urls.len() });
    }

    urls.iter()
        .enumerate()
        .map(|(index, raw_url)| {
            normalize_public_https_url(raw_url).map_err(|error| KagiError::InvalidUrl {
                index,
                reason: error.to_string(),
            })
        })
        .collect()
}

async fn parse_response<T: DeserializeOwned>(
    response: reqwest::Response,
    operation: &'static str,
    response_limit: usize,
) -> Result<T, KagiError> {
    let status = response.status();
    let header_trace = response
        .headers()
        .get(TRACE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(sanitize_trace_id);
    let body = read_bounded_body(response, operation, response_limit, &header_trace).await?;
    let body_trace = trace_from_body(&body);
    let trace_id = body_trace
        .or(header_trace)
        .unwrap_or_else(|| "unavailable".to_owned());

    if !status.is_success() {
        return Err(KagiError::Api {
            operation,
            status,
            message: api_error_message(&body),
            trace_id,
        });
    }

    serde_json::from_slice(&body).map_err(|error| KagiError::InvalidResponse {
        operation,
        message: error.to_string(),
        trace_id,
    })
}

async fn read_bounded_body(
    mut response: reqwest::Response,
    operation: &'static str,
    response_limit: usize,
    trace: &Option<String>,
) -> Result<Vec<u8>, KagiError> {
    if response
        .content_length()
        .is_some_and(|length| length > response_limit as u64)
    {
        return Err(KagiError::ResponseTooLarge {
            operation,
            limit_bytes: response_limit,
            trace_id: trace.clone().unwrap_or_else(|| "unavailable".to_owned()),
        });
    }

    let initial_capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0)
        .min(response_limit);
    let mut body = Vec::with_capacity(initial_capacity);

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|source| KagiError::Request { operation, source })?
    {
        if body.len().saturating_add(chunk.len()) > response_limit {
            return Err(KagiError::ResponseTooLarge {
                operation,
                limit_bytes: response_limit,
                trace_id: trace.clone().unwrap_or_else(|| "unavailable".to_owned()),
            });
        }
        body.extend_from_slice(&chunk);
    }

    Ok(body)
}

fn trace_from_body(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()?
        .get("meta")?
        .get("trace")?
        .as_str()
        .map(sanitize_trace_id)
}

fn api_error_message(body: &[u8]) -> String {
    let parsed = serde_json::from_slice::<serde_json::Value>(body).ok();
    let mut messages = Vec::new();

    if let Some(value) = parsed.as_ref() {
        collect_error_messages(value.get("error"), &mut messages);
        collect_error_messages(value.get("errors"), &mut messages);
        if messages.is_empty() {
            collect_string(value.get("message"), &mut messages);
        }
    }

    let message = if messages.is_empty() {
        let text = String::from_utf8_lossy(body).trim().to_owned();
        if text.is_empty() {
            "empty error response".to_owned()
        } else {
            text
        }
    } else {
        messages.join("; ")
    };

    truncate_chars(&message, ERROR_MESSAGE_LIMIT_CHARS)
}

fn collect_error_messages(value: Option<&serde_json::Value>, messages: &mut Vec<String>) {
    let Some(value) = value else {
        return;
    };

    match value {
        serde_json::Value::String(message) => messages.push(message.clone()),
        serde_json::Value::Array(errors) => {
            for error in errors {
                collect_error_messages(Some(error), messages);
            }
        }
        serde_json::Value::Object(error) => {
            let code = error.get("code").and_then(serde_json::Value::as_str);
            let message = error.get("message").and_then(serde_json::Value::as_str);
            match (code, message) {
                (Some(code), Some(message)) => messages.push(format!("{code}: {message}")),
                (Some(code), None) => messages.push(code.to_owned()),
                (None, Some(message)) => messages.push(message.to_owned()),
                (None, None) => {}
            }
        }
        _ => {}
    }
}

fn collect_string(value: Option<&serde_json::Value>, messages: &mut Vec<String>) {
    if let Some(message) = value.and_then(serde_json::Value::as_str) {
        messages.push(message.to_owned());
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::State,
        http::{HeaderMap, StatusCode as AxumStatusCode},
        response::Response,
        routing::post,
        Json, Router,
    };
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;

    async fn test_server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{address}"), task)
    }

    #[tokio::test]
    async fn search_sends_v1_contract_and_parses_web_and_news() {
        async fn handler(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer secret");
            assert_eq!(
                body,
                json!({
                    "query": "current rust release",
                    "workflow": "search",
                    "format": "json",
                    "limit": 10,
                    "safe_search": true
                })
            );
            Json(json!({
                "meta": { "trace": "search-trace" },
                "data": {
                    "search": [{
                        "url": "https://www.rust-lang.org/",
                        "title": "Rust",
                        "snippet": "A language",
                        "time": "2026-07-01"
                    }],
                    "news": [{
                        "url": "https://blog.rust-lang.org/",
                        "title": "Rust blog"
                    }]
                }
            }))
        }

        let router = Router::new().route("/api/v1/search", post(handler));
        let (base_url, server) = test_server(router).await;
        let client = KagiClient::new_with_base_url_for_test(
            "secret".to_owned(),
            &format!("{base_url}/api/v1"),
        )
        .unwrap();

        let response = client.search(" current rust release ").await.unwrap();
        assert_eq!(response.meta.trace.as_deref(), Some("search-trace"));
        assert_eq!(response.data.search.len(), 1);
        assert_eq!(response.data.search[0].title, "Rust");
        assert_eq!(response.data.news.len(), 1);
        assert_eq!(response.data.news[0].snippet, None);
        server.abort();
    }

    #[tokio::test]
    async fn extract_parses_page_and_top_level_errors() {
        async fn handler(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer secret");
            assert_eq!(
                body,
                json!({
                    "pages": [
                        { "url": "https://example.com/one" },
                        { "url": "https://example.com/two" }
                    ],
                    "format": "json"
                })
            );
            Json(json!({
                "meta": { "trace": "extract-trace" },
                "data": [
                    {
                        "url": "https://example.com/one",
                        "markdown": "# One"
                    },
                    {
                        "url": "https://example.com/two",
                        "error": "No data returned from crawlers"
                    }
                ],
                "errors": [{
                    "code": "crawler.empty",
                    "url": "https://kagi.com/docs/errors/crawler.empty",
                    "message": "One page failed",
                    "location": "pages[1]"
                }]
            }))
        }

        let router = Router::new().route("/api/v1/extract", post(handler));
        let (base_url, server) = test_server(router).await;
        let client = KagiClient::new_with_base_url_for_test(
            "secret".to_owned(),
            &format!("{base_url}/api/v1"),
        )
        .unwrap();
        let urls = vec![
            "https://example.com/one".to_owned(),
            "https://example.com/two".to_owned(),
        ];

        let response = client.extract(&urls).await.unwrap();
        assert_eq!(response.meta.trace.as_deref(), Some("extract-trace"));
        assert_eq!(response.data[0].markdown.as_deref(), Some("# One"));
        assert_eq!(
            response.data[1].error.as_deref(),
            Some("No data returned from crawlers")
        );
        assert_eq!(response.errors[0].code, "crawler.empty");
        server.abort();
    }

    #[tokio::test]
    async fn api_errors_include_message_and_body_trace() {
        async fn handler() -> (AxumStatusCode, Json<Value>) {
            (
                AxumStatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "meta": { "trace": "rate-trace" },
                    "error": [{
                        "code": "rate_limit",
                        "message": "Too many requests"
                    }]
                })),
            )
        }

        let router = Router::new().route("/api/v1/search", post(handler));
        let (base_url, server) = test_server(router).await;
        let client = KagiClient::new_with_base_url_for_test(
            "secret".to_owned(),
            &format!("{base_url}/api/v1"),
        )
        .unwrap();

        let error = client.search("anything").await.unwrap_err();
        match error {
            KagiError::Api {
                status,
                message,
                trace_id,
                ..
            } => {
                assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(message, "rate_limit: Too many requests");
                assert_eq!(trace_id, "rate-trace");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        server.abort();
    }

    #[tokio::test]
    async fn rejects_oversized_responses_before_deserializing() {
        #[derive(Clone)]
        struct LargeBody(Arc<Vec<u8>>);

        async fn handler(State(body): State<LargeBody>) -> Response {
            Response::builder()
                .header(TRACE_HEADER, "large-trace")
                .body(Body::from(body.0.as_ref().clone()))
                .unwrap()
        }

        let body = LargeBody(Arc::new(vec![b'x'; SEARCH_RESPONSE_LIMIT_BYTES + 1]));
        let router = Router::new()
            .route("/api/v1/search", post(handler))
            .with_state(body);
        let (base_url, server) = test_server(router).await;
        let client = KagiClient::new_with_base_url_for_test(
            "secret".to_owned(),
            &format!("{base_url}/api/v1"),
        )
        .unwrap();

        let error = client.search("anything").await.unwrap_err();
        match error {
            KagiError::ResponseTooLarge {
                limit_bytes,
                trace_id,
                ..
            } => {
                assert_eq!(limit_bytes, SEARCH_RESPONSE_LIMIT_BYTES);
                assert_eq!(trace_id, "large-trace");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        server.abort();
    }

    #[tokio::test]
    async fn extract_rejects_invalid_urls_before_sending() {
        let client = KagiClient::new_with_base_url_for_test(
            "secret".to_owned(),
            "http://127.0.0.1:1/api/v1",
        )
        .unwrap();

        assert!(matches!(
            client.extract(&[]).await,
            Err(KagiError::InvalidUrlCount { count: 0 })
        ));
        assert!(matches!(
            client.extract(&["http://example.com".to_owned()]).await,
            Err(KagiError::InvalidUrl { .. })
        ));
        assert!(matches!(
            client
                .extract(&["https://user:password@example.com".to_owned()])
                .await,
            Err(KagiError::InvalidUrl { .. })
        ));
        assert!(matches!(
            client
                .extract(&["https://169.254.169.254/latest/meta-data".to_owned()])
                .await,
            Err(KagiError::InvalidUrl { .. })
        ));
    }

    #[tokio::test]
    async fn transient_status_is_returned_after_one_attempt() {
        async fn transient_handler(State(attempts): State<Arc<AtomicUsize>>) -> Response {
            attempts.fetch_add(1, Ordering::SeqCst);
            Response::builder()
                .status(AxumStatusCode::SERVICE_UNAVAILABLE)
                .header("retry-after", "0")
                .body(Body::from(r#"{"error":[{"code":"temporary"}]}"#))
                .unwrap()
        }

        let attempts = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .route("/api/v1/search", post(transient_handler))
            .with_state(attempts.clone());
        let (base_url, server) = test_server(router).await;
        let client = KagiClient::new_with_base_url_for_test(
            "secret".to_owned(),
            &format!("{base_url}/api/v1"),
        )
        .unwrap();

        assert!(matches!(
            client.search("anything").await,
            Err(KagiError::Api { .. })
        ));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[test]
    fn debug_output_redacts_api_key() {
        let client = KagiClient::new("top-secret-value".to_owned()).unwrap();
        let debug = format!("{client:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("top-secret-value"));
    }

    #[test]
    fn nullable_optional_collections_decode_as_empty() {
        let search: SearchResponse = serde_json::from_value(json!({
            "meta": null,
            "data": { "search": null, "news": null }
        }))
        .unwrap();
        assert!(search.data.search.is_empty());
        assert!(search.data.news.is_empty());

        let extract: ExtractResponse = serde_json::from_value(json!({
            "meta": null,
            "data": null,
            "errors": null
        }))
        .unwrap();
        assert!(extract.data.is_empty());
        assert!(extract.errors.is_empty());
    }

    #[test]
    fn non_object_search_data_is_rejected() {
        for data in [json!("broken"), json!([]), json!(42)] {
            assert!(serde_json::from_value::<SearchResponse>(json!({
                "meta": {},
                "data": data
            }))
            .is_err());
        }
    }

    #[test]
    fn trace_ids_are_sanitized_and_bounded_during_deserialization() {
        let response: SearchResponse = serde_json::from_value(json!({
            "meta": { "trace": format!("trace\n{}", "x".repeat(200)) },
            "data": {}
        }))
        .unwrap();
        let trace = response.meta.trace.unwrap();
        assert!(!trace.contains('\n'));
        assert!(trace.chars().count() <= TRACE_ID_LIMIT_CHARS);
        assert_eq!(sanitize_trace_id("\n\t"), "unavailable");
    }

    #[test]
    fn api_error_display_does_not_expose_response_message() {
        let error = KagiError::Api {
            operation: "search",
            status: StatusCode::BAD_REQUEST,
            message: "private query echoed by provider".to_string(),
            trace_id: "safe-trace".to_string(),
        };
        let display = error.to_string();
        assert!(display.contains("HTTP 400"));
        assert!(display.contains("safe-trace"));
        assert!(!display.contains("private query"));
    }
}
