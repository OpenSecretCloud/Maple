#![allow(dead_code)]

use opensecret::{Error, OpenSecretClient, Pcr0Environment, Result};
use std::env::{self, VarError};

const PCR_ENVIRONMENT_VARIABLE: &str = "VITE_OPEN_SECRET_PCR_ENVIRONMENT";
const PCR_ENVIRONMENT_ERROR: &str =
    "VITE_OPEN_SECRET_PCR_ENVIRONMENT must be either \"production\" or \"development\"";

pub fn live_ai_enabled() -> bool {
    env::var("RUN_LIVE_AI").is_ok_and(|value| value == "1")
}

pub fn parse_pcr0_environment(
    value: Option<&str>,
) -> std::result::Result<Pcr0Environment, &'static str> {
    match value {
        None | Some("production") => Ok(Pcr0Environment::Production),
        Some("development") => Ok(Pcr0Environment::Development),
        Some(_) => Err(PCR_ENVIRONMENT_ERROR),
    }
}

pub fn selected_pcr0_environment() -> Result<Pcr0Environment> {
    let configured = match env::var(PCR_ENVIRONMENT_VARIABLE) {
        Ok(value) => Some(value),
        Err(VarError::NotPresent) => None,
        Err(VarError::NotUnicode(_)) => {
            return Err(Error::Configuration(PCR_ENVIRONMENT_ERROR.to_string()));
        }
    };

    parse_pcr0_environment(configured.as_deref())
        .map_err(|message| Error::Configuration(message.to_string()))
}

pub fn new_test_client(base_url: impl Into<String>) -> Result<OpenSecretClient> {
    OpenSecretClient::new_with_pcr0_environment(base_url, selected_pcr0_environment()?)
}

pub fn new_test_client_with_api_key(
    base_url: impl Into<String>,
    api_key: String,
) -> Result<OpenSecretClient> {
    OpenSecretClient::new_with_api_key_and_pcr0_environment(
        base_url,
        api_key,
        selected_pcr0_environment()?,
    )
}
