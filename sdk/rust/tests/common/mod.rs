#![allow(dead_code)]

use opensecret::{OpenSecretClient, Result};
use std::env;

pub fn live_ai_enabled() -> bool {
    env::var("RUN_LIVE_AI").is_ok_and(|value| value == "1")
}

pub fn new_test_client(base_url: impl Into<String>) -> Result<OpenSecretClient> {
    OpenSecretClient::new(base_url)
}

pub fn new_test_client_with_api_key(
    base_url: impl Into<String>,
    api_key: String,
) -> Result<OpenSecretClient> {
    OpenSecretClient::new_with_api_key(base_url, api_key)
}
