use crate::{
    provider_client::{ProviderClient, ProviderRequest},
    AppState,
};
use axum::{http::StatusCode, Router};
use axum::{routing::get, Json};
use reqwest::Method;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

const API_VERSION: &str = "v1";

pub fn router_with_state(state: Arc<AppState>) -> Router<()> {
    Router::new()
        .route("/health-check", get(health_check))
        .route("/health-check-extended", get(health_check_extended))
        .with_state(state)
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct ExtendedHealthResponse {
    pub status: String,
    pub version: String,
    pub outbound_connectivity: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_check: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HealthResponse {
    /// Fabricate a status: pass response without checking database connectivity
    pub fn new_ok() -> Self {
        Self {
            status: String::from("pass"),
            version: String::from(API_VERSION),
        }
    }
}

/// Health check endpoint following the IETF draft standard
/// <https://datatracker.ietf.org/doc/html/draft-inadarei-api-health-check>
pub async fn health_check() -> Result<Json<HealthResponse>, (StatusCode, String)> {
    Ok(Json(HealthResponse::new_ok()))
}

/// Extended health check that tests outbound connectivity via model listing
pub async fn health_check_extended(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<Json<ExtendedHealthResponse>, (StatusCode, String)> {
    let tinfoil_proxy = state.proxy_router.get_tinfoil_proxy();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        fetch_models_directly(&state.provider_client, &tinfoil_proxy),
    )
    .await;

    match result {
        Ok(Ok(model_count)) => Ok(Json(ExtendedHealthResponse {
            status: "pass".to_string(),
            version: API_VERSION.to_string(),
            outbound_connectivity: true,
            model_check: Some(format!(
                "Successfully fetched {} models from {}",
                model_count, tinfoil_proxy.provider_name
            )),
            error: None,
        })),
        Ok(Err(e)) => {
            // Failed to fetch models
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "Failed to fetch models from {}: {}",
                    tinfoil_proxy.provider_name, e
                ),
            ))
        }
        Err(_) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "Model fetch from {} timed out after 5 seconds",
                tinfoil_proxy.provider_name
            ),
        )),
    }
}

/// Helper function to fetch models directly without caching
async fn fetch_models_directly(
    client: &ProviderClient,
    proxy_config: &crate::proxy_config::ProxyConfig,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let res = client
        .send(
            proxy_config,
            ProviderRequest::new(Method::GET, "/v1/models", Duration::from_secs(5)),
        )
        .await?;

    if !res.is_success() {
        let status = res.status_code();
        let body_bytes = res.bytes().await?;
        let body_str = String::from_utf8_lossy(&body_bytes);
        return Err(format!("HTTP {}: {}", status, body_str).into());
    }

    let body_bytes = res.bytes().await?;
    let models_response: serde_json::Value = serde_json::from_slice(&body_bytes)?;

    // Count the models
    let model_count = models_response
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0);

    Ok(model_count)
}
