//! HTTP client for the Maple billing API. Mirrors
//! `frontend/src/billing/billingApi.ts` in the Maple web app.
//!
//! The client takes a third-party JWT as a plain string. Mint it with the
//! OpenSecret SDK (`generate_third_party_token`) with the billing base URL
//! as the audience. This crate does not depend on the enclave client.

use serde::Deserialize;

pub const DEFAULT_BILLING_API_URL: &str = "https://billing.opensecret.cloud";

/// Subset of `GET /v1/maple/subscription/status` used by clients.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BillingStatus {
    #[serde(default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub total_tokens: Option<i64>,
    #[serde(default)]
    pub used_tokens: Option<i64>,
    #[serde(default)]
    pub usage_reset_date: Option<String>,
}

#[derive(Debug)]
pub enum BillingError {
    /// The third-party token was rejected; mint a new one and retry.
    Unauthorized,
    Other(String),
}

impl std::fmt::Display for BillingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("billing API rejected the session token"),
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for BillingError {}

#[derive(Debug, Clone)]
pub struct BillingClient {
    base_url: String,
    http: reqwest::Client,
}

impl BillingClient {
    /// Build a client. A trailing slash on `base_url` is removed.
    ///
    /// Fails when the HTTP client cannot be built (for example when the TLS
    /// backend is unavailable). `reqwest::Client::default()` panics in the
    /// same conditions, so there is no silent fallback.
    pub fn new(base_url: impl Into<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|error| format!("failed to build the billing HTTP client: {error}"))?;
        let base_url = base_url.into().trim().trim_end_matches('/').to_string();
        Ok(Self { base_url, http })
    }

    /// The base URL, also the audience for the third-party token.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn subscription_status(&self, token: &str) -> Result<BillingStatus, BillingError> {
        let response = self
            .http
            .get(format!("{}/v1/maple/subscription/status", self.base_url))
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|error| BillingError::Other(format!("billing request failed: {error}")))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(BillingError::Unauthorized);
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(BillingError::Other(format!(
                "billing status failed ({status}): {body}"
            )));
        }
        response
            .json::<BillingStatus>()
            .await
            .map_err(|error| BillingError::Other(format!("billing status decode failed: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_trailing_slash() {
        assert_eq!(
            BillingClient::new("https://billing.example/ ")
                .unwrap()
                .base_url(),
            "https://billing.example"
        );
    }

    #[test]
    fn decodes_partial_status() {
        let status: BillingStatus =
            serde_json::from_str(r#"{"product_name":"Max","is_subscribed":true}"#).unwrap();
        assert_eq!(status.product_name.as_deref(), Some("Max"));
        assert_eq!(status.total_tokens, None);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "hits the live billing API"]
    async fn dummy_token_is_rejected_quickly() {
        let client = BillingClient::new(DEFAULT_BILLING_API_URL).unwrap();
        let started = std::time::Instant::now();
        let result = client.subscription_status("dummy").await;
        eprintln!("result after {:?}: {result:?}", started.elapsed());
        assert!(matches!(result, Err(BillingError::Unauthorized)));
    }
}
