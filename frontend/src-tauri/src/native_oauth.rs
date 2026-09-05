use crate::open_secret_config::{configured_pcr0_environment, normalize_api_url};
use opensecret::{NativeOAuthHandoffGrant, OpenSecretClient};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Instant};
use tauri::State;
use tokio::sync::Mutex;
use uuid::Uuid;

const NATIVE_OAUTH_ATTEMPT_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const NATIVE_OAUTH_NETWORK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeginNativeOAuthRequest {
    api_url: String,
}

#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeOAuthSession {
    native_oauth_attempt: String,
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RedeemNativeOAuthRequest {
    native_session_id: String,
    handoff_grant: String,
}

#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeOAuthAuthentication {
    user_id: String,
    auth_bundle: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelNativeOAuthRequest {
    native_oauth_attempt: String,
}

struct PendingNativeOAuth {
    attempt_id: Uuid,
    started_at: Instant,
    session_id: Uuid,
    client: Arc<OpenSecretClient>,
}

struct CompletedNativeOAuth {
    attempt_id: Uuid,
    session_id: Uuid,
    grant_fingerprint: [u8; 32],
    completed_at: Instant,
    authentication: NativeOAuthAuthentication,
}

enum NativeOAuthAttempt {
    Pending(PendingNativeOAuth),
    Completed(CompletedNativeOAuth),
}

impl NativeOAuthAttempt {
    fn attempt_id(&self) -> Uuid {
        match self {
            Self::Pending(attempt) => attempt.attempt_id,
            Self::Completed(attempt) => attempt.attempt_id,
        }
    }

    fn is_expired(&self, now: Instant) -> bool {
        let started_at = match self {
            Self::Pending(attempt) => attempt.started_at,
            Self::Completed(attempt) => attempt.completed_at,
        };
        now.checked_duration_since(started_at)
            .is_none_or(|age| age > NATIVE_OAUTH_ATTEMPT_TTL)
    }
}

pub struct NativeOAuthState {
    attempt: Mutex<Option<NativeOAuthAttempt>>,
}

impl NativeOAuthState {
    pub fn new() -> Self {
        Self {
            attempt: Mutex::new(None),
        }
    }

    async fn begin(&self, request: BeginNativeOAuthRequest) -> Result<NativeOAuthSession, String> {
        // Serialize begin/redeem/cancel so a slower handshake can never publish
        // over a newer native login attempt.
        let mut current = self.attempt.lock().await;
        let api_url = normalize_api_url(&request.api_url)?;
        let client = Arc::new(
            OpenSecretClient::new_with_pcr0_environment(api_url, configured_pcr0_environment()?)
                .map_err(|error| map_sdk_error("prepare", &error))?,
        );
        let prepared = tokio::time::timeout(
            NATIVE_OAUTH_NETWORK_TIMEOUT,
            client.prepare_native_oauth_session(),
        )
        .await
        .map_err(|_| "Secure authentication setup timed out; please try again".to_string())?
        .map_err(|error| map_sdk_error("prepare", &error))?;
        let attempt_id = Uuid::new_v4();
        let response = NativeOAuthSession {
            native_oauth_attempt: attempt_id.to_string(),
            session_id: prepared.session_id.to_string(),
        };
        *current = Some(NativeOAuthAttempt::Pending(PendingNativeOAuth {
            attempt_id,
            started_at: Instant::now(),
            session_id: prepared.session_id,
            client,
        }));
        Ok(response)
    }

    async fn redeem(
        &self,
        request: RedeemNativeOAuthRequest,
    ) -> Result<NativeOAuthAuthentication, String> {
        let expected_session_id = parse_canonical_uuid(&request.native_session_id)
            .map_err(|_| "Native authentication session is invalid; restart sign-in".to_string())?;
        let grant_fingerprint: [u8; 32] = Sha256::digest(request.handoff_grant.as_bytes()).into();
        let grant = NativeOAuthHandoffGrant::new(request.handoff_grant)
            .map_err(|error| map_sdk_error("validate", &error))?;
        let mut current = self.attempt.lock().await;
        if current
            .as_ref()
            .is_some_and(|attempt| attempt.is_expired(Instant::now()))
        {
            *current = None;
            return Err("Native authentication expired; restart sign-in".to_string());
        }

        match current.as_ref() {
            Some(NativeOAuthAttempt::Completed(completed))
                if completed.session_id == expected_session_id
                    && completed.grant_fingerprint == grant_fingerprint =>
            {
                return Ok(completed.authentication.clone());
            }
            Some(NativeOAuthAttempt::Completed(_)) => {
                return Err(
                    "Native authentication callback does not match the completed sign-in"
                        .to_string(),
                );
            }
            None => {
                return Err("Native authentication is not pending; restart sign-in".to_string());
            }
            Some(NativeOAuthAttempt::Pending(pending))
                if pending.session_id != expected_session_id =>
            {
                return Err(
                    "Native authentication callback does not match the pending sign-in".to_string(),
                );
            }
            Some(NativeOAuthAttempt::Pending(_)) => {}
        }

        let pending = match current.take() {
            Some(NativeOAuthAttempt::Pending(pending)) => pending,
            _ => unreachable!("pending attempt checked above"),
        };
        let attempt_id = pending.attempt_id;
        let login = match tokio::time::timeout(
            NATIVE_OAUTH_NETWORK_TIMEOUT,
            pending.client.redeem_native_oauth_handoff(
                pending.session_id,
                pending.attempt_id,
                grant,
            ),
        )
        .await
        {
            Ok(Ok(login)) => login,
            Ok(Err(error)) => return Err(map_sdk_error("redeem", &error)),
            Err(_) => {
                return Err(
                    "Secure authentication completion timed out; restart sign-in".to_string(),
                );
            }
        };
        let auth_bundle = pending
            .client
            .export_transport_v2_auth_bundle()
            .map_err(|error| map_sdk_error("export", &error))?
            .ok_or_else(|| {
                "Secure authentication returned no resumable credentials; restart sign-in"
                    .to_string()
            })?;
        let authentication = NativeOAuthAuthentication {
            user_id: login.id.to_string(),
            auth_bundle,
        };
        *current = Some(NativeOAuthAttempt::Completed(CompletedNativeOAuth {
            attempt_id,
            session_id: pending.session_id,
            grant_fingerprint,
            completed_at: Instant::now(),
            authentication: authentication.clone(),
        }));
        Ok(authentication)
    }

    async fn cancel(&self, request: CancelNativeOAuthRequest) -> Result<(), String> {
        let attempt_id = parse_canonical_uuid(&request.native_oauth_attempt)
            .map_err(|_| "Native authentication state is invalid".to_string())?;
        let mut current = self.attempt.lock().await;
        if current
            .as_ref()
            .is_some_and(|attempt| attempt.attempt_id() == attempt_id)
        {
            *current = None;
        }
        Ok(())
    }
}

fn parse_canonical_uuid(value: &str) -> Result<Uuid, ()> {
    let parsed = Uuid::parse_str(value).map_err(|_| ())?;
    if parsed.is_nil() || parsed.to_string() != value {
        return Err(());
    }
    Ok(parsed)
}

fn map_sdk_error(action: &'static str, error: &opensecret::Error) -> String {
    let category = match error {
        opensecret::Error::Http(_) => "http",
        opensecret::Error::Serialization(_) => "serialization",
        opensecret::Error::Cbor(_) => "cbor",
        opensecret::Error::Crypto(_) => "crypto",
        opensecret::Error::AttestationVerificationFailed(_) => "attestation",
        opensecret::Error::Session(_) => "session",
        opensecret::Error::KeyExchange(_) => "key_exchange",
        opensecret::Error::Encryption(_) => "encryption",
        opensecret::Error::Decryption(_) => "decryption",
        opensecret::Error::Authentication(_) => "authentication",
        opensecret::Error::InvalidResponse(_) => "invalid_response",
        opensecret::Error::Api { .. } => "api",
        opensecret::Error::Configuration(_) => "configuration",
        opensecret::Error::Io(_) => "io",
        opensecret::Error::Utf8(_) => "utf8",
        opensecret::Error::Base64Decode(_) => "base64",
        opensecret::Error::Other(_) => "other",
    };
    log::warn!("Native OAuth {action} failed ({category})");
    "Secure authentication failed; restart sign-in".to_string()
}

#[tauri::command]
pub async fn native_oauth_begin(
    state: State<'_, NativeOAuthState>,
    request: BeginNativeOAuthRequest,
) -> Result<NativeOAuthSession, String> {
    state.begin(request).await
}

#[tauri::command]
pub async fn native_oauth_redeem(
    state: State<'_, NativeOAuthState>,
    request: RedeemNativeOAuthRequest,
) -> Result<NativeOAuthAuthentication, String> {
    state.redeem(request).await
}

#[tauri::command]
pub async fn native_oauth_cancel(
    state: State<'_, NativeOAuthState>,
    request: CancelNativeOAuthRequest,
) -> Result<(), String> {
    state.cancel(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_identifiers_require_lowercase_canonical_uuid_text() {
        let id = Uuid::new_v4();
        assert_eq!(parse_canonical_uuid(&id.to_string()), Ok(id));
        assert!(parse_canonical_uuid(&id.to_string().to_uppercase()).is_err());
        assert!(parse_canonical_uuid(&format!("{{{id}}}")).is_err());
        assert!(parse_canonical_uuid(&Uuid::nil().to_string()).is_err());
        assert!(parse_canonical_uuid("not-a-uuid").is_err());
    }

    #[test]
    fn sdk_errors_are_reduced_to_safe_native_messages() {
        let error = opensecret::Error::Authentication("sensitive upstream detail".to_string());
        let message = map_sdk_error("test", &error);
        assert!(!message.contains("sensitive"));
        assert_eq!(message, "Secure authentication failed; restart sign-in");
    }

    #[tokio::test]
    async fn stale_public_session_does_not_consume_the_pending_attempt() {
        let attempt_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let state = NativeOAuthState {
            attempt: Mutex::new(Some(NativeOAuthAttempt::Pending(PendingNativeOAuth {
                attempt_id,
                started_at: Instant::now(),
                session_id,
                client: Arc::new(
                    OpenSecretClient::new("http://127.0.0.1:9")
                        .expect("test client configuration must be valid"),
                ),
            }))),
        };

        let result = state
            .redeem(RedeemNativeOAuthRequest {
                native_session_id: Uuid::new_v4().to_string(),
                handoff_grant: "header.payload.signature".to_string(),
            })
            .await;
        assert!(result.is_err());

        let current = state.attempt.lock().await;
        let Some(NativeOAuthAttempt::Pending(pending)) = current.as_ref() else {
            panic!("stale callback must preserve the pending native attempt");
        };
        assert_eq!(pending.attempt_id, attempt_id);
        assert_eq!(pending.session_id, session_id);
    }
}
