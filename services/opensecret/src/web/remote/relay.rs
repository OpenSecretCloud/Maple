//! Stock iroh-relay 1.3 HTTP admission. This service credential is never in a
//! desktop build. The relay supplies the endpoint identity it authenticated.
use crate::{db::DBConnection, AppMode, AppState, AwsCredentialManager, Error};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const PURPOSE: &str = "maple-remote-relay-admission-v1";

pub(crate) fn router(state: Arc<AppState>) -> Router<()> {
    Router::new()
        .route("/v1/remote/relay-access", post(access))
        .with_state(state)
}

pub(crate) async fn load_token(
    mode: &AppMode,
    credentials: Arc<tokio::sync::RwLock<Option<AwsCredentialManager>>>,
    db: Arc<dyn DBConnection + Send + Sync>,
) -> Result<Option<Zeroizing<String>>, Error> {
    if *mode == AppMode::Local {
        return std::env::var("MAPLE_REMOTE_RELAY_AUTH_TOKEN")
            .ok()
            .map(validate_token)
            .transpose();
    }
    let Some(row) = db.get_enclave_secret_by_key("maple_remote_relay_auth_v1")? else {
        return Ok(None);
    };
    let manager = credentials
        .read()
        .await
        .clone()
        .ok_or(Error::SecretParsingError)?;
    let credentials = manager
        .get_credentials()
        .await
        .ok_or(Error::SecretParsingError)?;
    let plaintext = Zeroizing::new(
        crate::decrypt_with_kms(
            &credentials.region,
            &credentials.access_key_id,
            &credentials.secret_access_key,
            &credentials.token,
            &STANDARD.encode(row.value),
        )
        .map_err(|_| Error::SecretParsingError)?,
    );
    decode_token(&plaintext, &mode.to_string()).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    purpose: String,
    deployment: String,
    token: Zeroizing<String>,
}

fn validate_token(token: String) -> Result<Zeroizing<String>, Error> {
    let token = Zeroizing::new(token);
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::SecretParsingError);
    }
    Ok(token)
}

fn decode_token(plaintext: &[u8], deployment: &str) -> Result<Zeroizing<String>, Error> {
    // Bind the KMS-authenticated plaintext itself, not the attacker-controlled
    // DB key label. A valid credential for another purpose/environment fails.
    let credential: Credential =
        serde_json::from_slice(plaintext).map_err(|_| Error::SecretParsingError)?;
    if credential.purpose != PURPOSE
        || credential.deployment != deployment
        || credential.token.len() != 64
        || !credential.token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::SecretParsingError);
    }
    Ok(credential.token)
}

fn authorized(headers: &HeaderMap, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return false;
    };
    let Some(provided) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
    else {
        return false;
    };
    provided.len() == 64
        && bool::from(
            Sha256::digest(provided.as_bytes()).ct_eq(&Sha256::digest(expected.as_bytes())),
        )
}

pub(crate) fn registered(state: &AppState, endpoint: &str) -> bool {
    if super::public_key(endpoint).is_err() {
        return false;
    }
    let Ok(rows) = state.db.remote_relay_candidates(endpoint) else {
        return false;
    };
    for row in rows {
        let Ok(user) = state.db.get_user_by_uuid(row.user_id) else {
            continue;
        };
        let deployment = state.app_mode.to_string();
        let expected = super::binding(&user, &deployment, "device", endpoint);
        if let Ok(device) =
            expected.open::<super::types::RemoteDevice>(&state.enclave_key, &row.ciphertext)
        {
            if device.endpoint == endpoint && device.expires_at > chrono::Utc::now().timestamp() {
                return true;
            }
        }
    }
    false
}

async fn access(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> (StatusCode, &'static str) {
    if !authorized(
        &headers,
        state.remote_relay_token.as_deref().map(|s| s.as_str()),
    ) {
        return (StatusCode::FORBIDDEN, "false");
    }
    // Pinned to the actual 1.3.0 executable, whose header is X-Iroh-NodeId.
    let Some(endpoint) = headers
        .get("X-Iroh-NodeId")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return (StatusCode::FORBIDDEN, "false");
    };
    let allowed = tokio::task::spawn_blocking(move || registered(&state, &endpoint))
        .await
        .unwrap_or(false);
    if allowed {
        (StatusCode::OK, "true")
    } else {
        (StatusCode::FORBIDDEN, "false")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relay_credential_is_bound_and_missing_configuration_denies() {
        let token = "a".repeat(64);
        let credential =
            serde_json::json!({"purpose": PURPOSE, "deployment": "prod", "token": token});
        let bytes = serde_json::to_vec(&credential).unwrap();
        assert!(decode_token(&bytes, "prod").is_ok());
        assert!(decode_token(&bytes, "dev").is_err());
        assert!(decode_token(
            br#"{"purpose":"billing","deployment":"prod","token":"secret"}"#,
            "prod"
        )
        .is_err());
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert!(!authorized(&headers, None));
        assert!(authorized(&headers, Some(&token)));
        assert!(!authorized(&headers, Some(&"b".repeat(64))));
    }
}
