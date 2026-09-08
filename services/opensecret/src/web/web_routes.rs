//! Authenticated, provider-neutral web search and URL extraction APIs.
//!
//! The public contract intentionally keeps search and page extraction as two
//! separate operations: callers inspect bounded result metadata first and pay
//! to extract only selected pages. Kagi's experimental response format,
//! inline extraction, and arbitrary personalization rules are therefore not
//! exposed. JSON is always used at the provider boundary.

use axum::{
    http::StatusCode,
    middleware::from_fn_with_state,
    response::{IntoResponse, Response},
    routing::post,
    Extension, Json, Router,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tracing::{debug, info, warn};

use crate::{
    kagi::{
        sanitize_trace_id, ExtractPage, ExtractResponse, KagiClient, KagiError, SearchFilters,
        SearchLens, SearchOptions, SearchTimeRelative, SearchWorkflow,
    },
    models::users::User,
    web::{
        encryption_middleware::{decrypt_request, encrypt_response, Decrypted, TransportSession},
        web_safety::{compact_untrusted_markdown, normalize_public_https_url, strip_image_embeds},
    },
    ApiError, AppMode, AppState,
};

pub const WEB_SEARCH_PATH: &str = "/v1/web/search";
pub const WEB_EXTRACT_PATH: &str = "/v1/web/extract";

const MAX_QUERY_CHARS: usize = 512;
const DEFAULT_SEARCH_LIMIT: u16 = 10;
const MAX_SEARCH_LIMIT: u16 = 50;
const MAX_LENS_ID_CHARS: usize = 2_048;
const MAX_LENS_LIST_ITEMS: usize = 50;
const MAX_LENS_VALUE_CHARS: usize = 256;
const MAX_FILE_TYPE_CHARS: usize = 32;
const MAX_CATEGORY_CHARS: usize = 64;
const MAX_TITLE_CHARS: usize = 300;
const MAX_SNIPPET_CHARS: usize = 800;
const MAX_PUBLISHED_AT_CHARS: usize = 100;
const WEB_BILLING_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchWorkflow {
    #[default]
    Search,
    Images,
    Videos,
    News,
    Podcasts,
}

impl From<WebSearchWorkflow> for SearchWorkflow {
    fn from(value: WebSearchWorkflow) -> Self {
        match value {
            WebSearchWorkflow::Search => Self::Search,
            WebSearchWorkflow::Images => Self::Images,
            WebSearchWorkflow::Videos => Self::Videos,
            WebSearchWorkflow::News => Self::News,
            WebSearchWorkflow::Podcasts => Self::Podcasts,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchTimeRelative {
    Day,
    Week,
    Month,
}

impl From<WebSearchTimeRelative> for SearchTimeRelative {
    fn from(value: WebSearchTimeRelative) -> Self {
        match value {
            WebSearchTimeRelative::Day => Self::Day,
            WebSearchTimeRelative::Week => Self::Week,
            WebSearchTimeRelative::Month => Self::Month,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSearchLens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sites_included: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sites_excluded: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords_included: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords_excluded: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_relative: Option<WebSearchTimeRelative>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_region: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSearchFilters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WebSearchWorkflow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_search: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<WebSearchLens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<WebSearchFilters>,
}

/// A deliberately narrow, provider-neutral result. Provider `props` and image
/// metadata are omitted so agents receive links and bounded text, never remote
/// or data-URL image payloads through this API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub category: String,
    pub url: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub results: Vec<WebSearchResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebExtractRequest {
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebExtractPageError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebExtractPage {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WebExtractPageError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebExtractResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub pages: Vec<WebExtractPage>,
}

#[derive(Debug, Serialize)]
struct WebErrorBody {
    status: u16,
    code: &'static str,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_id: Option<String>,
}

#[derive(Debug)]
enum WebRouteError {
    InvalidRequest,
    ProviderUnavailable { trace_id: Option<String> },
    Api(ApiError),
}

impl From<ApiError> for WebRouteError {
    fn from(value: ApiError) -> Self {
        Self::Api(value)
    }
}

impl IntoResponse for WebRouteError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidRequest => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(WebErrorBody {
                    status: StatusCode::UNPROCESSABLE_ENTITY.as_u16(),
                    code: "invalid_request",
                    message: "The web request is invalid.",
                    trace_id: None,
                }),
            )
                .into_response(),
            Self::ProviderUnavailable { trace_id } => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(WebErrorBody {
                    status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                    code: "web_provider_unavailable",
                    message: "The web provider is temporarily unavailable.",
                    trace_id,
                }),
            )
                .into_response(),
            Self::Api(error) => error.into_response(),
        }
    }
}

pub fn router(app_state: Arc<AppState>) -> Router<()> {
    Router::new()
        .route(
            WEB_SEARCH_PATH,
            post(search_web).layer(from_fn_with_state(
                app_state.clone(),
                decrypt_request::<Value>,
            )),
        )
        .route(
            WEB_EXTRACT_PATH,
            post(extract_web).layer(from_fn_with_state(
                app_state.clone(),
                decrypt_request::<Value>,
            )),
        )
        .with_state(app_state)
}

async fn search_web(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Extension(session_id): Extension<TransportSession>,
    Extension(user): Extension<User>,
    Decrypted(body): Decrypted<Value>,
) -> Result<Response, WebRouteError> {
    let request = serde_json::from_value::<WebSearchRequest>(body)
        .map_err(|_| WebRouteError::InvalidRequest)?;
    let options = validate_search_request(request)?;
    let client = state.kagi_client.as_ref().ok_or_else(|| {
        warn!("Web search requested while the provider client is unavailable");
        WebRouteError::ProviderUnavailable { trace_id: None }
    })?;
    ensure_paid_web_access(&state, &user).await?;
    let response = execute_search(client, options).await?;
    encrypt_response(&state, &session_id, &response)
        .await
        .map_err(WebRouteError::from)
}

async fn extract_web(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Extension(session_id): Extension<TransportSession>,
    Extension(user): Extension<User>,
    Decrypted(body): Decrypted<Value>,
) -> Result<Response, WebRouteError> {
    let request = serde_json::from_value::<WebExtractRequest>(body)
        .map_err(|_| WebRouteError::InvalidRequest)?;
    let (urls, timeout) = validate_extract_request(request)?;
    let client = state.kagi_client.as_ref().ok_or_else(|| {
        warn!("Web extraction requested while the provider client is unavailable");
        WebRouteError::ProviderUnavailable { trace_id: None }
    })?;
    ensure_paid_web_access(&state, &user).await?;
    let response = execute_extract(client, urls, timeout).await?;
    encrypt_response(&state, &session_id, &response)
        .await
        .map_err(WebRouteError::from)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaidWebAccess {
    BillingUnavailable,
    Paid,
    Free,
    CheckFailed,
}

fn paid_web_access_decision(
    app_mode: &AppMode,
    is_guest: bool,
    access: PaidWebAccess,
) -> Result<(), ApiError> {
    match access {
        PaidWebAccess::Paid => Ok(()),
        PaidWebAccess::BillingUnavailable if *app_mode == AppMode::Local && !is_guest => Ok(()),
        PaidWebAccess::BillingUnavailable if *app_mode == AppMode::Local => {
            Err(ApiError::Unauthorized)
        }
        PaidWebAccess::BillingUnavailable => Err(ApiError::ServiceUnavailable),
        PaidWebAccess::Free => Err(ApiError::UsageLimitReached),
        PaidWebAccess::CheckFailed => Err(ApiError::ServiceUnavailable),
    }
}

async fn ensure_paid_web_access(state: &AppState, user: &User) -> Result<(), ApiError> {
    let access = if let Some(billing_client) = &state.billing_client {
        match tokio::time::timeout(
            WEB_BILLING_CHECK_TIMEOUT,
            billing_client.can_user_use_paid_features(user.uuid),
        )
        .await
        {
            Ok(Ok(true)) => PaidWebAccess::Paid,
            Ok(Ok(false)) => PaidWebAccess::Free,
            Ok(Err(_)) | Err(_) => PaidWebAccess::CheckFailed,
        }
    } else {
        PaidWebAccess::BillingUnavailable
    };

    let result = paid_web_access_decision(&state.app_mode, user.is_guest(), access);
    if result.is_err() {
        warn!(
            user_uuid = %user.uuid,
            is_guest = user.is_guest(),
            access = ?access,
            "Denied paid web-provider access"
        );
    }
    result
}

async fn execute_search(
    client: &KagiClient,
    options: SearchOptions,
) -> Result<WebSearchResponse, WebRouteError> {
    let result_limit = usize::from(options.limit);
    let response = client
        .search_with_options(&options)
        .await
        .map_err(|error| map_kagi_error("search", &error))?;

    let trace_id = meaningful_trace_id(response.meta.trace.as_deref());
    let mut results = Vec::with_capacity(result_limit);
    let mut seen_urls = HashSet::new();

    'categories: for (category, category_results) in response.data.into_categories() {
        let category = sanitize_category(&category);
        for result in category_results {
            if results.len() >= result_limit {
                break 'categories;
            }
            let Ok(url) = normalize_public_https_url(&result.url) else {
                debug!(category, "Dropped a non-public web-search result URL");
                continue;
            };
            if !seen_urls.insert(url.clone()) {
                continue;
            }

            let title = compact_untrusted_markdown(&result.title, MAX_TITLE_CHARS);
            let snippet = result
                .snippet
                .as_deref()
                .map(|value| compact_untrusted_markdown(value, MAX_SNIPPET_CHARS))
                .filter(|value| !value.is_empty());
            let published_at = result
                .time
                .as_deref()
                .map(|value| compact_untrusted_markdown(value, MAX_PUBLISHED_AT_CHARS))
                .filter(|value| !value.is_empty());

            results.push(WebSearchResult {
                category: category.clone(),
                url,
                title: if title.is_empty() {
                    "Untitled result".to_owned()
                } else {
                    title
                },
                snippet,
                published_at,
            });
        }
    }

    info!(
        result_count = results.len(),
        trace_id = trace_id.as_deref().unwrap_or("unavailable"),
        "Completed web search"
    );
    Ok(WebSearchResponse { trace_id, results })
}

fn validate_search_request(request: WebSearchRequest) -> Result<SearchOptions, WebRouteError> {
    let query = request.query.trim();
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return Err(WebRouteError::InvalidRequest);
    }
    if request.page.is_some_and(|page| !(1..=10).contains(&page)) {
        return Err(WebRouteError::InvalidRequest);
    }
    let limit = request.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err(WebRouteError::InvalidRequest);
    }
    if request
        .timeout
        .is_some_and(|timeout| !timeout.is_finite() || !(0.5..=4.0).contains(&timeout))
    {
        return Err(WebRouteError::InvalidRequest);
    }
    if request.lens_id.is_some() && request.lens.is_some() {
        return Err(WebRouteError::InvalidRequest);
    }
    if let Some(lens_id) = request.lens_id.as_deref() {
        validate_bounded_text(lens_id, MAX_LENS_ID_CHARS)?;
    }
    if let Some(lens) = request.lens.as_ref() {
        validate_lens(lens)?;
    }
    if let Some(filters) = request.filters.as_ref() {
        validate_filters(filters)?;
    }
    let relative_time = request
        .lens
        .as_ref()
        .and_then(|lens| lens.time_relative)
        .is_some();
    let absolute_time = request
        .lens
        .as_ref()
        .is_some_and(|lens| lens.time_after.is_some() || lens.time_before.is_some())
        || request
            .filters
            .as_ref()
            .is_some_and(|filters| filters.after.is_some() || filters.before.is_some());
    if relative_time && absolute_time {
        return Err(WebRouteError::InvalidRequest);
    }

    Ok(SearchOptions {
        query: query.to_owned(),
        workflow: request.workflow.unwrap_or_default().into(),
        lens_id: request.lens_id,
        lens: request.lens.map(|lens| SearchLens {
            sites_included: lens.sites_included,
            sites_excluded: lens.sites_excluded,
            keywords_included: lens.keywords_included,
            keywords_excluded: lens.keywords_excluded,
            file_type: lens.file_type,
            time_after: lens.time_after,
            time_before: lens.time_before,
            time_relative: lens.time_relative.map(Into::into),
            search_region: lens.search_region,
        }),
        timeout: request.timeout,
        page: request.page,
        limit,
        filters: request.filters.map(|filters| SearchFilters {
            region: filters.region,
            after: filters.after,
            before: filters.before,
        }),
        safe_search: request.safe_search.unwrap_or(true),
    })
}

fn validate_lens(lens: &WebSearchLens) -> Result<(), WebRouteError> {
    for values in [
        &lens.sites_included,
        &lens.sites_excluded,
        &lens.keywords_included,
        &lens.keywords_excluded,
    ] {
        validate_bounded_list(values)?;
    }
    if let Some(file_type) = lens.file_type.as_deref() {
        validate_bounded_text(file_type, MAX_FILE_TYPE_CHARS)?;
        if !file_type
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        {
            return Err(WebRouteError::InvalidRequest);
        }
    }
    validate_date_range(lens.time_after.as_deref(), lens.time_before.as_deref())?;
    if let Some(region) = lens.search_region.as_deref() {
        validate_region(region, true)?;
    }
    Ok(())
}

fn validate_filters(filters: &WebSearchFilters) -> Result<(), WebRouteError> {
    if let Some(region) = filters.region.as_deref() {
        validate_region(region, false)?;
    }
    validate_date_range(filters.after.as_deref(), filters.before.as_deref())
}

fn validate_bounded_list(values: &Option<Vec<String>>) -> Result<(), WebRouteError> {
    let Some(values) = values else {
        return Ok(());
    };
    if values.is_empty() || values.len() > MAX_LENS_LIST_ITEMS {
        return Err(WebRouteError::InvalidRequest);
    }
    for value in values {
        validate_bounded_text(value, MAX_LENS_VALUE_CHARS)?;
    }
    Ok(())
}

fn validate_bounded_text(value: &str, max_chars: usize) -> Result<(), WebRouteError> {
    if value.trim().is_empty()
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
    {
        return Err(WebRouteError::InvalidRequest);
    }
    Ok(())
}

fn validate_region(region: &str, allow_no_region: bool) -> Result<(), WebRouteError> {
    if allow_no_region && region == "no_region" {
        return Ok(());
    }
    if region.len() != 2 || !region.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(WebRouteError::InvalidRequest);
    }
    Ok(())
}

fn validate_date_range(after: Option<&str>, before: Option<&str>) -> Result<(), WebRouteError> {
    let after = after
        .map(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| WebRouteError::InvalidRequest)?;
    let before = before
        .map(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| WebRouteError::InvalidRequest)?;
    if after
        .zip(before)
        .is_some_and(|(after, before)| after > before)
    {
        return Err(WebRouteError::InvalidRequest);
    }
    Ok(())
}

async fn execute_extract(
    client: &KagiClient,
    urls: Vec<String>,
    timeout: Option<f32>,
) -> Result<WebExtractResponse, WebRouteError> {
    let response = client
        .extract_with_timeout(&urls, timeout)
        .await
        .map_err(|error| map_kagi_error("extract", &error))?;
    Ok(format_extract_response(&urls, response))
}

fn validate_extract_request(
    request: WebExtractRequest,
) -> Result<(Vec<String>, Option<f32>), WebRouteError> {
    if request.urls.is_empty() || request.urls.len() > crate::kagi::MAX_EXTRACT_URLS {
        return Err(WebRouteError::InvalidRequest);
    }
    if request
        .timeout
        .is_some_and(|timeout| !timeout.is_finite() || !(0.5..=10.0).contains(&timeout))
    {
        return Err(WebRouteError::InvalidRequest);
    }

    let mut urls = Vec::with_capacity(request.urls.len());
    let mut seen = HashSet::new();
    for raw_url in request.urls {
        let url =
            normalize_public_https_url(&raw_url).map_err(|_| WebRouteError::InvalidRequest)?;
        if !seen.insert(url.clone()) {
            return Err(WebRouteError::InvalidRequest);
        }
        urls.push(url);
    }
    Ok((urls, request.timeout))
}

fn format_extract_response(urls: &[String], response: ExtractResponse) -> WebExtractResponse {
    let trace_id = meaningful_trace_id(response.meta.trace.as_deref());
    let failed_indices = extraction_error_indices(&response);
    let requested_urls = urls.iter().cloned().collect::<HashSet<_>>();
    let mut pages_by_url = HashMap::new();
    for page in response.data {
        let Ok(url) = normalize_public_https_url(&page.url) else {
            continue;
        };
        if requested_urls.contains(&url) {
            pages_by_url.entry(url).or_insert(page);
        }
    }

    let pages = urls
        .iter()
        .enumerate()
        .map(|(index, url)| {
            let page = pages_by_url.remove(url);
            format_extract_page(url, page, failed_indices.contains(&index))
        })
        .collect::<Vec<_>>();

    info!(
        page_count = pages.len(),
        failed_page_count = pages.iter().filter(|page| page.error.is_some()).count(),
        trace_id = trace_id.as_deref().unwrap_or("unavailable"),
        "Completed web extraction"
    );
    WebExtractResponse { trace_id, pages }
}

fn format_extract_page(
    url: &str,
    page: Option<ExtractPage>,
    diagnostic_failure: bool,
) -> WebExtractPage {
    let Some(page) = page else {
        return failed_page(url, "missing_result", "No extraction result was returned.");
    };
    if page
        .error
        .as_deref()
        .is_some_and(|error| !error.trim().is_empty())
        || diagnostic_failure
    {
        return failed_page(url, "extraction_failed", "The page could not be extracted.");
    }
    let Some(markdown) = page.markdown else {
        return failed_page(url, "no_content", "No textual page content was returned.");
    };

    let sanitized = strip_image_embeds(&markdown);
    if sanitized.trim().is_empty() {
        return failed_page(url, "no_content", "No textual page content was returned.");
    }
    WebExtractPage {
        url: url.to_owned(),
        markdown: Some(sanitized),
        error: None,
    }
}

fn failed_page(url: &str, code: &str, message: &str) -> WebExtractPage {
    WebExtractPage {
        url: url.to_owned(),
        markdown: None,
        error: Some(WebExtractPageError {
            code: code.to_owned(),
            message: message.to_owned(),
        }),
    }
}

fn extraction_error_indices(response: &ExtractResponse) -> HashSet<usize> {
    response
        .errors
        .iter()
        .filter_map(|error| error.location.as_deref())
        .filter_map(parse_page_location)
        .collect()
}

fn parse_page_location(location: &str) -> Option<usize> {
    let suffix = location.strip_prefix("pages[")?;
    let end = suffix.find(']')?;
    if !matches!(&suffix[end + 1..], "" | ".url") {
        return None;
    }
    suffix[..end].parse().ok()
}

fn sanitize_category(category: &str) -> String {
    let category = category
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(MAX_CATEGORY_CHARS)
        .collect::<String>();
    if category.is_empty() {
        "other".to_owned()
    } else {
        category
    }
}

fn meaningful_trace_id(trace_id: Option<&str>) -> Option<String> {
    trace_id
        .map(sanitize_trace_id)
        .filter(|trace_id| trace_id != "unavailable")
}

fn map_kagi_error(operation: &'static str, error: &KagiError) -> WebRouteError {
    let trace_id = match error {
        KagiError::Api { trace_id, .. }
        | KagiError::ResponseTooLarge { trace_id, .. }
        | KagiError::InvalidResponse { trace_id, .. } => meaningful_trace_id(Some(trace_id)),
        _ => None,
    };
    let error_kind = match error {
        KagiError::InvalidApiKey => "invalid_api_key",
        KagiError::InvalidQuery => "invalid_query",
        KagiError::InvalidSearchParameter { .. } => "invalid_parameter",
        KagiError::InvalidUrlCount { .. } => "invalid_url_count",
        KagiError::InvalidUrl { .. } => "invalid_url",
        KagiError::InvalidBaseUrl(_) => "invalid_base_url",
        KagiError::Request { source, .. } if source.is_timeout() => "request_timeout",
        KagiError::Request { source, .. } if source.is_connect() => "request_connect",
        KagiError::Request { .. } => "request_failed",
        KagiError::ResponseTooLarge { .. } => "response_too_large",
        KagiError::Api { status, .. } if status.as_u16() == 429 => "rate_limited",
        KagiError::Api { .. } => "api_error",
        KagiError::InvalidResponse { .. } => "invalid_response",
    };
    warn!(
        operation,
        error_kind,
        trace_id = trace_id.as_deref().unwrap_or("unavailable"),
        "Web provider request failed"
    );

    match error {
        KagiError::InvalidQuery
        | KagiError::InvalidSearchParameter { .. }
        | KagiError::InvalidUrlCount { .. }
        | KagiError::InvalidUrl { .. } => WebRouteError::InvalidRequest,
        KagiError::Api { status, .. } if matches!(status.as_u16(), 400 | 422) => {
            WebRouteError::InvalidRequest
        }
        _ => WebRouteError::ProviderUnavailable { trace_id },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        http::{HeaderMap, Uri},
        routing::post,
        Json, Router,
    };
    use serde_json::json;
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
    async fn search_forwards_bounded_options_and_sanitizes_dynamic_categories() {
        async fn handler(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer secret");
            assert_eq!(body["query"], "rust release");
            assert_eq!(body["workflow"], "images");
            assert_eq!(body["page"], 2);
            assert_eq!(body["limit"], 2);
            assert_eq!(body["safe_search"], false);
            assert_eq!(body["filters"]["region"], "US");
            assert_eq!(body["format"], "json");
            assert!(body.get("extract").is_none());
            assert!(body.get("personalizations").is_none());
            Json(json!({
                "meta": { "trace": "search-trace" },
                "data": {
                    "image": [{
                        "url": "https://example.com/image#fragment",
                        "title": "![remote](https://images.example/title.png) Result",
                        "snippet": "<img src='data:image/png;base64,AAAA'> Useful snippet",
                        "image": { "url": "https://images.example/omitted.png" },
                        "props": { "thumbnail": "https://images.example/also-omitted.png" }
                    }],
                    "future_category": [{
                        "url": "https://example.org/future",
                        "title": "Future result"
                    }],
                    "zzz_future_category": [{
                        "url": "https://example.net/future",
                        "title": "Must be capped"
                    }],
                    "search": [{
                        "url": "https://localhost/private",
                        "title": "Dropped"
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

        let options = validate_search_request(WebSearchRequest {
            query: " rust release ".to_owned(),
            workflow: Some(WebSearchWorkflow::Images),
            page: Some(2),
            limit: Some(2),
            safe_search: Some(false),
            timeout: None,
            lens_id: None,
            lens: None,
            filters: Some(WebSearchFilters {
                region: Some("US".to_owned()),
                after: None,
                before: None,
            }),
        })
        .unwrap();
        let response = execute_search(&client, options).await.unwrap();

        assert_eq!(response.trace_id.as_deref(), Some("search-trace"));
        assert_eq!(response.results.len(), 2);
        let image_result = response
            .results
            .iter()
            .find(|result| result.category == "image")
            .unwrap();
        assert_eq!(image_result.url, "https://example.com/image");
        assert_eq!(image_result.title, "remote Result");
        assert_eq!(image_result.snippet.as_deref(), Some("Useful snippet"));
        assert!(response
            .results
            .iter()
            .any(|result| result.category == "future_category"));
        assert!(!response
            .results
            .iter()
            .any(|result| result.category == "zzz_future_category"));
        assert!(!serde_json::to_string(&response)
            .unwrap()
            .contains("images.example"));
        server.abort();
    }

    #[tokio::test]
    async fn extract_returns_sanitized_partial_results_without_raw_provider_errors() {
        async fn handler(Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(body["timeout"], 2.5);
            assert_eq!(body["format"], "json");
            Json(json!({
                "meta": { "trace": "extract-trace" },
                "data": [{
                    "url": "https://example.com/one",
                    "markdown": "![chart](data:image/png;base64,AAAA) [Source](https://example.org/source) useful text"
                }, {
                    "url": "https://example.com/two",
                    "error": "private provider diagnostic with echoed input"
                }],
                "error": [{
                    "code": "crawler.private_detail",
                    "url": "https://kagi.com/errors/private",
                    "message": "private provider diagnostic",
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
        let (urls, timeout) = validate_extract_request(WebExtractRequest {
            urls: vec![
                "https://example.com/one#fragment".to_owned(),
                "https://example.com/two".to_owned(),
            ],
            timeout: Some(2.5),
        })
        .unwrap();
        let response = execute_extract(&client, urls, timeout).await.unwrap();

        assert_eq!(response.trace_id.as_deref(), Some("extract-trace"));
        assert_eq!(response.pages[0].url, "https://example.com/one");
        let markdown = response.pages[0].markdown.as_deref().unwrap();
        assert!(markdown.contains("chart"));
        assert!(markdown.contains("[Source](https://example.org/source)"));
        assert!(!markdown.contains("data:image"));
        assert_eq!(
            response.pages[1].error.as_ref().unwrap().code,
            "extraction_failed"
        );
        let serialized = serde_json::to_string(&response).unwrap();
        assert!(!serialized.contains("private provider diagnostic"));
        assert!(!serialized.contains("crawler.private_detail"));
        server.abort();
    }

    #[test]
    fn validation_rejects_unsupported_or_dangerous_requests_with_422_mapping() {
        assert!(serde_json::from_value::<WebSearchRequest>(json!({
            "query": "x",
            "extract": { "count": 10 }
        }))
        .is_err());
        assert!(matches!(
            validate_extract_request(WebExtractRequest {
                urls: vec!["https://169.254.169.254/latest/meta-data".to_owned()],
                timeout: None,
            }),
            Err(WebRouteError::InvalidRequest)
        ));
        let conflicting_time = serde_json::from_value::<WebSearchRequest>(json!({
            "query": "x",
            "lens": { "time_relative": "week" },
            "filters": { "after": "2026-07-01" }
        }))
        .unwrap();
        assert!(matches!(
            validate_search_request(conflicting_time),
            Err(WebRouteError::InvalidRequest)
        ));
        let response = WebRouteError::InvalidRequest.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn provider_client_errors_map_to_422_without_exposing_diagnostics() {
        for status in [400, 422] {
            let error = KagiError::Api {
                operation: "search",
                status: reqwest::StatusCode::from_u16(status).unwrap(),
                message: "private provider diagnostic".to_owned(),
                trace_id: "trace".to_owned(),
            };
            let response = map_kagi_error("search", &error).into_response();
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let body = axum::body::to_bytes(response.into_body(), 8 * 1024)
                .await
                .unwrap();
            let body = String::from_utf8(body.to_vec()).unwrap();
            assert!(body.contains("invalid_request"));
            assert!(!body.contains("private provider diagnostic"));
        }
    }

    #[test]
    fn extraction_preserves_full_sanitized_markdown_for_callers() {
        let content = "x".repeat(100_000);
        let response = format_extract_response(
            &["https://example.com/large".to_owned()],
            ExtractResponse {
                meta: Default::default(),
                data: vec![ExtractPage {
                    url: "https://example.com/large".to_owned(),
                    markdown: Some(content.clone()),
                    error: None,
                }],
                errors: Vec::new(),
            },
        );

        assert_eq!(
            response.pages[0].markdown.as_deref(),
            Some(content.as_str())
        );
        assert!(response.pages[0].error.is_none());
    }

    #[test]
    fn paid_access_guard_is_fail_closed_but_keeps_local_non_guest_development() {
        assert!(paid_web_access_decision(&AppMode::Prod, false, PaidWebAccess::Paid).is_ok());
        assert!(paid_web_access_decision(&AppMode::Prod, true, PaidWebAccess::Paid).is_ok());
        assert!(paid_web_access_decision(
            &AppMode::Local,
            false,
            PaidWebAccess::BillingUnavailable
        )
        .is_ok());
        assert!(matches!(
            paid_web_access_decision(&AppMode::Local, true, PaidWebAccess::BillingUnavailable),
            Err(ApiError::Unauthorized)
        ));
        for mode in [
            AppMode::Dev,
            AppMode::Preview,
            AppMode::Prod,
            AppMode::Custom("test".to_owned()),
        ] {
            assert!(matches!(
                paid_web_access_decision(&mode, false, PaidWebAccess::BillingUnavailable),
                Err(ApiError::ServiceUnavailable)
            ));
        }
        assert!(matches!(
            paid_web_access_decision(&AppMode::Prod, false, PaidWebAccess::Free),
            Err(ApiError::UsageLimitReached)
        ));
        assert!(matches!(
            paid_web_access_decision(&AppMode::Prod, false, PaidWebAccess::CheckFailed),
            Err(ApiError::ServiceUnavailable)
        ));
    }

    #[test]
    fn route_paths_live_in_provider_neutral_v1_namespace() {
        assert_eq!(
            WEB_SEARCH_PATH.parse::<Uri>().unwrap().path(),
            "/v1/web/search"
        );
        assert_eq!(
            WEB_EXTRACT_PATH.parse::<Uri>().unwrap().path(),
            "/v1/web/extract"
        );
    }
}
