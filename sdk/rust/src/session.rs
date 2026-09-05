use crate::error::{Error, Result};
use crate::transport_v2::V2Session;
use crate::types::{SessionState, TokenPair};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct UserAuthEpoch {
    pub(crate) generation: u64,
    pub(crate) principal: Option<String>,
}

#[derive(Clone)]
pub(crate) struct CredentialSnapshot {
    pub(crate) tokens: Option<TokenPair>,
    pub(crate) api_key: Option<String>,
    pub(crate) generation: u64,
    pub(crate) auth_epoch: UserAuthEpoch,
    pub(crate) user_session: Option<Arc<V2Session>>,
}

#[derive(Default)]
struct CredentialState {
    tokens: Option<TokenPair>,
    token_principal: Option<String>,
    user_session: Option<Arc<V2Session>>,
    api_key: Option<String>,
    generation: u64,
    token_generation: u64,
}

#[derive(Clone)]
pub struct SessionManager {
    session: Arc<RwLock<Option<SessionState>>>,
    credentials: Arc<RwLock<CredentialState>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            session: Arc::new(RwLock::new(None)),
            credentials: Arc::new(RwLock::new(CredentialState::default())),
        }
    }

    pub fn new_with_api_key(api_key: String) -> Self {
        Self {
            session: Arc::new(RwLock::new(None)),
            credentials: Arc::new(RwLock::new(CredentialState {
                api_key: Some(api_key),
                generation: 1,
                ..CredentialState::default()
            })),
        }
    }

    pub fn set_api_key(&self, api_key: String) -> Result<()> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        credentials.api_key = Some(api_key);
        credentials.generation = credentials.generation.wrapping_add(1);
        Ok(())
    }

    pub fn get_api_key(&self) -> Result<Option<String>> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;

        Ok(credentials.api_key.clone())
    }

    pub fn clear_api_key(&self) -> Result<()> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        credentials.api_key = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        Ok(())
    }

    pub fn set_session(&self, session_id: Uuid, session_key: [u8; 32]) -> Result<()> {
        let mut session_guard = self
            .session
            .write()
            .map_err(|e| Error::Session(format!("Failed to acquire session write lock: {}", e)))?;

        *session_guard = Some(SessionState {
            session_id,
            session_key,
        });

        Ok(())
    }

    pub fn get_session(&self) -> Result<Option<SessionState>> {
        let session_guard = self
            .session
            .read()
            .map_err(|e| Error::Session(format!("Failed to acquire session read lock: {}", e)))?;

        Ok(session_guard.clone())
    }

    pub fn clear_session(&self) -> Result<()> {
        let mut session_guard = self
            .session
            .write()
            .map_err(|e| Error::Session(format!("Failed to acquire session write lock: {}", e)))?;

        *session_guard = None;
        Ok(())
    }

    pub fn set_tokens(&self, access_token: String, refresh_token: Option<String>) -> Result<()> {
        self.replace_user_tokens(access_token, refresh_token, None)
            .map(|_| ())
    }

    pub(crate) fn replace_user_tokens(
        &self,
        access_token: String,
        refresh_token: Option<String>,
        principal: Option<String>,
    ) -> Result<UserAuthEpoch> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        credentials.tokens = Some(TokenPair {
            access_token,
            refresh_token,
        });
        credentials.token_principal = principal;
        credentials.user_session = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);

        Ok(user_auth_epoch(&credentials))
    }

    pub(crate) fn replace_user_tokens_and_session_if_epoch(
        &self,
        expected: &UserAuthEpoch,
        access_token: String,
        refresh_token: Option<String>,
        principal: String,
        session: Arc<V2Session>,
    ) -> Result<Option<UserAuthEpoch>> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        if user_auth_epoch(&credentials) != *expected {
            return Ok(None);
        }

        credentials.tokens = Some(TokenPair {
            access_token,
            refresh_token,
        });
        credentials.token_principal = Some(principal);
        credentials.user_session = Some(session);
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        Ok(Some(user_auth_epoch(&credentials)))
    }

    pub(crate) fn replace_user_tokens_if_epoch(
        &self,
        expected: &UserAuthEpoch,
        access_token: String,
        refresh_token: Option<String>,
        principal: String,
    ) -> Result<Option<UserAuthEpoch>> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        if user_auth_epoch(&credentials) != *expected {
            return Ok(None);
        }

        credentials.tokens = Some(TokenPair {
            access_token,
            refresh_token,
        });
        credentials.token_principal = Some(principal);
        credentials.user_session = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        Ok(Some(user_auth_epoch(&credentials)))
    }

    pub(crate) fn get_credential_snapshot(&self) -> Result<CredentialSnapshot> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;

        Ok(credential_snapshot(&credentials))
    }

    pub(crate) fn get_credential_snapshot_if_auth_epoch(
        &self,
        expected: &UserAuthEpoch,
    ) -> Result<Option<CredentialSnapshot>> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;

        if user_auth_epoch(&credentials) != *expected {
            return Ok(None);
        }

        Ok(Some(credential_snapshot(&credentials)))
    }

    pub(crate) fn credential_generation_matches(&self, expected: u64) -> Result<bool> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;
        Ok(credentials.generation == expected)
    }

    pub(crate) fn get_user_session(&self) -> Result<Option<Arc<V2Session>>> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;
        Ok(credentials.user_session.clone())
    }

    pub(crate) fn clear_user_session(&self) -> Result<()> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;
        credentials.user_session = None;
        Ok(())
    }

    pub(crate) fn clear_user_session_if(&self, expected: &Arc<V2Session>) -> Result<()> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;
        if credentials
            .user_session
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, expected))
        {
            credentials.user_session = None;
        }
        Ok(())
    }

    pub fn get_tokens(&self) -> Result<Option<TokenPair>> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;

        Ok(credentials.tokens.clone())
    }

    pub fn get_access_token(&self) -> Result<Option<String>> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;

        Ok(credentials
            .tokens
            .as_ref()
            .map(|tokens| tokens.access_token.clone()))
    }

    pub fn get_refresh_token(&self) -> Result<Option<String>> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;

        Ok(credentials
            .tokens
            .as_ref()
            .and_then(|tokens| tokens.refresh_token.clone()))
    }

    pub fn update_access_token(&self, access_token: String) -> Result<()> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        if let Some(tokens) = credentials.tokens.as_mut() {
            tokens.access_token = access_token;
        } else {
            return Err(Error::Authentication("No tokens to update".to_string()));
        }

        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        credentials.token_principal = None;
        credentials.user_session = None;
        Ok(())
    }

    pub fn clear_tokens(&self) -> Result<()> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        credentials.tokens = None;
        credentials.token_principal = None;
        credentials.user_session = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        Ok(())
    }

    pub(crate) fn invalidate_user_auth_if_epoch(&self, expected: &UserAuthEpoch) -> Result<bool> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;
        if user_auth_epoch(&credentials) != *expected {
            return Ok(false);
        }
        credentials.tokens = None;
        credentials.token_principal = None;
        credentials.user_session = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        Ok(true)
    }

    pub fn clear_all(&self) -> Result<()> {
        self.clear_session()?;
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;
        credentials.tokens = None;
        credentials.token_principal = None;
        credentials.user_session = None;
        credentials.api_key = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        Ok(())
    }

    pub(crate) fn clear_all_if_generation(&self, expected_generation: u64) -> Result<bool> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        if credentials.generation != expected_generation {
            return Ok(false);
        }

        self.clear_session()?;
        credentials.tokens = None;
        credentials.token_principal = None;
        credentials.user_session = None;
        credentials.api_key = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        Ok(true)
    }
}

fn credential_snapshot(credentials: &CredentialState) -> CredentialSnapshot {
    CredentialSnapshot {
        tokens: credentials.tokens.clone(),
        api_key: credentials.api_key.clone(),
        generation: credentials.generation,
        auth_epoch: user_auth_epoch(credentials),
        user_session: credentials.user_session.clone(),
    }
}

fn user_auth_epoch(credentials: &CredentialState) -> UserAuthEpoch {
    UserAuthEpoch {
        generation: credentials.token_generation,
        principal: credentials.token_principal.clone(),
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_session(marker: u8) -> Arc<V2Session> {
        Arc::new(
            V2Session::from_master_for_test(Uuid::from_bytes([marker; 16]), [marker; 32], u64::MAX)
                .expect("test v2 session"),
        )
    }

    #[test]
    fn test_session_management() {
        let manager = SessionManager::new();

        // Initially empty
        assert!(manager.get_session().unwrap().is_none());

        // Set session
        let session_id = Uuid::new_v4();
        let session_key = [0u8; 32];
        manager.set_session(session_id, session_key).unwrap();

        // Retrieve session
        let session = manager.get_session().unwrap().unwrap();
        assert_eq!(session.session_id, session_id);
        assert_eq!(session.session_key, session_key);

        // Clear session
        manager.clear_session().unwrap();
        assert!(manager.get_session().unwrap().is_none());
    }

    #[test]
    fn test_token_management() {
        let manager = SessionManager::new();

        // Initially empty
        assert!(manager.get_tokens().unwrap().is_none());

        // Set tokens
        manager
            .set_tokens("access".to_string(), Some("refresh".to_string()))
            .unwrap();

        // Retrieve tokens
        let tokens = manager.get_tokens().unwrap().unwrap();
        assert_eq!(tokens.access_token, "access");
        assert_eq!(tokens.refresh_token, Some("refresh".to_string()));

        // Update access token
        manager
            .update_access_token("new_access".to_string())
            .unwrap();
        assert_eq!(
            manager.get_access_token().unwrap(),
            Some("new_access".to_string())
        );

        // Clear tokens
        manager.clear_tokens().unwrap();
        assert!(manager.get_tokens().unwrap().is_none());
    }

    #[test]
    fn stale_auth_commit_cannot_overwrite_or_clear_a_new_principal() {
        let manager = SessionManager::new();
        let initial = manager.get_credential_snapshot().unwrap().auth_epoch;
        let old_session = v2_session(0x11);
        let old_epoch = manager
            .replace_user_tokens_and_session_if_epoch(
                &initial,
                "old-access".to_string(),
                Some("old-refresh".to_string()),
                "old-user".to_string(),
                Arc::clone(&old_session),
            )
            .unwrap()
            .expect("install old auth");

        let new_epoch = manager
            .replace_user_tokens(
                "new-access".to_string(),
                Some("new-refresh".to_string()),
                Some("new-user".to_string()),
            )
            .unwrap();
        assert!(manager
            .replace_user_tokens_and_session_if_epoch(
                &old_epoch,
                "stale-access".to_string(),
                Some("stale-refresh".to_string()),
                "old-user".to_string(),
                old_session,
            )
            .unwrap()
            .is_none());
        assert!(!manager.invalidate_user_auth_if_epoch(&old_epoch).unwrap());

        let snapshot = manager.get_credential_snapshot().unwrap();
        assert_eq!(snapshot.auth_epoch, new_epoch);
        assert_eq!(snapshot.auth_epoch.principal.as_deref(), Some("new-user"));
        assert_eq!(
            snapshot.tokens.expect("new tokens").access_token,
            "new-access"
        );
        assert!(snapshot.user_session.is_none());
    }

    #[test]
    fn rejected_refresh_invalidates_only_its_own_auth_lifecycle() {
        let manager = SessionManager::new();
        let initial = manager.get_credential_snapshot().unwrap().auth_epoch;
        let rejected_epoch = manager
            .replace_user_tokens_and_session_if_epoch(
                &initial,
                "access".to_string(),
                Some("refresh".to_string()),
                "user".to_string(),
                v2_session(0x22),
            )
            .unwrap()
            .expect("install rejected auth");

        assert!(manager
            .invalidate_user_auth_if_epoch(&rejected_epoch)
            .unwrap());
        let signed_out = manager.get_credential_snapshot().unwrap();
        assert!(signed_out.tokens.is_none());
        assert!(signed_out.auth_epoch.principal.is_none());
        assert!(signed_out.user_session.is_none());

        let replacement = manager
            .replace_user_tokens(
                "replacement-access".to_string(),
                Some("replacement-refresh".to_string()),
                Some("replacement-user".to_string()),
            )
            .unwrap();
        assert!(!manager
            .invalidate_user_auth_if_epoch(&rejected_epoch)
            .unwrap());
        assert_eq!(
            manager.get_credential_snapshot().unwrap().auth_epoch,
            replacement
        );
    }
}
