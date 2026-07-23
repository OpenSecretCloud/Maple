use crate::error::{Error, Result};
use crate::types::{SessionState, TokenPair};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct CredentialSnapshot {
    pub(crate) tokens: Option<TokenPair>,
    pub(crate) api_key: Option<String>,
    pub(crate) generation: u64,
    pub(crate) token_generation: u64,
    pub(crate) api_key_generation: u64,
}

#[derive(Debug, Default)]
struct CredentialState {
    tokens: Option<TokenPair>,
    api_key: Option<String>,
    generation: u64,
    token_generation: u64,
    api_key_generation: u64,
}

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
                api_key_generation: 1,
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
        credentials.api_key_generation = credentials.api_key_generation.wrapping_add(1);
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
        credentials.api_key_generation = credentials.api_key_generation.wrapping_add(1);
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
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        credentials.tokens = Some(TokenPair {
            access_token,
            refresh_token,
        });
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);

        Ok(())
    }

    pub(crate) fn set_tokens_if_generation(
        &self,
        expected_token_generation: u64,
        access_token: String,
        refresh_token: Option<String>,
    ) -> Result<bool> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        if credentials.token_generation != expected_token_generation {
            return Ok(false);
        }

        credentials.tokens = Some(TokenPair {
            access_token,
            refresh_token,
        });
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        Ok(true)
    }

    pub(crate) fn get_credential_snapshot(&self) -> Result<CredentialSnapshot> {
        let credentials = self.credentials.read().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials read lock: {}", e))
        })?;

        Ok(CredentialSnapshot {
            tokens: credentials.tokens.clone(),
            api_key: credentials.api_key.clone(),
            generation: credentials.generation,
            token_generation: credentials.token_generation,
            api_key_generation: credentials.api_key_generation,
        })
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
        Ok(())
    }

    pub fn clear_tokens(&self) -> Result<()> {
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;

        credentials.tokens = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        self.clear_session()?;
        let mut credentials = self.credentials.write().map_err(|e| {
            Error::Authentication(format!("Failed to acquire credentials write lock: {}", e))
        })?;
        credentials.tokens = None;
        credentials.api_key = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        credentials.api_key_generation = credentials.api_key_generation.wrapping_add(1);
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
        credentials.api_key = None;
        credentials.generation = credentials.generation.wrapping_add(1);
        credentials.token_generation = credentials.token_generation.wrapping_add(1);
        credentials.api_key_generation = credentials.api_key_generation.wrapping_add(1);
        Ok(true)
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
}
