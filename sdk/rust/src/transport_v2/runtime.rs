use std::sync::{Arc, RwLock};

use tokio::sync::Mutex;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::session::SessionManager;

use super::{session::V2Session, Result, TransportV2Error};

#[derive(Clone, Eq, PartialEq, Zeroize, ZeroizeOnDrop)]
pub(crate) struct ApiKeyScope([u8; 32]);

impl ApiKeyScope {
    pub(crate) const fn new(fingerprint: [u8; 32]) -> Self {
        Self(fingerprint)
    }
}

impl std::fmt::Debug for ApiKeyScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKeyScope([REDACTED])")
    }
}

struct ApiKeySession {
    scope: ApiKeyScope,
    session: Arc<V2Session>,
}

/// Authority-scoped transport-v2 sessions owned by one SDK client.
///
/// The short synchronous locks protect only pointer replacement. Network and
/// cryptographic work is serialized by the corresponding asynchronous gate,
/// never while a state lock is held.
pub(super) struct TransportV2Runtime {
    cache_namespace_root: RwLock<[u8; 32]>,
    anonymous: RwLock<Option<Arc<V2Session>>>,
    session_manager: SessionManager,
    api_key: RwLock<Option<ApiKeySession>>,
    anonymous_gate: Mutex<()>,
    user_gate: Mutex<()>,
    api_key_gate: Mutex<()>,
}

impl TransportV2Runtime {
    pub(super) fn new(cache_namespace_root: [u8; 32], session_manager: SessionManager) -> Self {
        Self {
            cache_namespace_root: RwLock::new(cache_namespace_root),
            anonymous: RwLock::new(None),
            session_manager,
            api_key: RwLock::new(None),
            anonymous_gate: Mutex::new(()),
            user_gate: Mutex::new(()),
            api_key_gate: Mutex::new(()),
        }
    }

    pub(super) fn cache_namespace_root(&self) -> Result<[u8; 32]> {
        self.cache_namespace_root
            .read()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)
            .map(|root| *root)
    }

    pub(super) fn replace_cache_namespace_root(&self, root: [u8; 32]) -> Result<()> {
        let mut current = self
            .cache_namespace_root
            .write()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?;
        current.zeroize();
        *current = root;
        drop(current);
        self.clear_sessions()
    }

    pub(super) fn anonymous(&self) -> Result<Option<Arc<V2Session>>> {
        self.anonymous
            .read()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)
            .map(|session| session.clone())
    }

    pub(super) fn set_anonymous(&self, session: Arc<V2Session>) -> Result<()> {
        *self
            .anonymous
            .write()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)? = Some(session);
        Ok(())
    }

    pub(super) fn clear_anonymous_if(&self, expected: &Arc<V2Session>) -> Result<()> {
        let mut session = self
            .anonymous
            .write()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?;
        if session
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, expected))
        {
            *session = None;
        }
        Ok(())
    }

    pub(super) fn user(&self) -> Result<Option<Arc<V2Session>>> {
        self.session_manager
            .get_user_session()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)
    }

    pub(super) fn clear_user(&self) -> Result<()> {
        self.session_manager
            .clear_user_session()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)
    }

    pub(super) fn clear_user_if(&self, expected: &Arc<V2Session>) -> Result<()> {
        self.session_manager
            .clear_user_session_if(expected)
            .map_err(|_| TransportV2Error::SessionStateUnavailable)
    }

    pub(super) fn api_key(&self, scope: &ApiKeyScope) -> Result<Option<Arc<V2Session>>> {
        self.api_key
            .read()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)
            .map(|entry| {
                entry
                    .as_ref()
                    .filter(|entry| &entry.scope == scope)
                    .map(|entry| Arc::clone(&entry.session))
            })
    }

    pub(super) fn set_api_key(&self, scope: &ApiKeyScope, session: Arc<V2Session>) -> Result<()> {
        *self
            .api_key
            .write()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)? = Some(ApiKeySession {
            scope: scope.clone(),
            session,
        });
        Ok(())
    }

    pub(super) fn clear_api_key(&self) -> Result<()> {
        *self
            .api_key
            .write()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)? = None;
        Ok(())
    }

    pub(super) fn clear_api_key_if(
        &self,
        scope: &ApiKeyScope,
        expected: &Arc<V2Session>,
    ) -> Result<()> {
        let mut entry = self
            .api_key
            .write()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?;
        if entry.as_ref().is_some_and(|current| {
            &current.scope == scope && Arc::ptr_eq(&current.session, expected)
        }) {
            *entry = None;
        }
        Ok(())
    }

    /// Forget any cached authority slot that still references a failed
    /// session. This never retries the request; it only lets a later,
    /// independently initiated call establish fresh attested state.
    pub(super) fn clear_session_if(&self, expected: &Arc<V2Session>) -> Result<()> {
        self.clear_anonymous_if(expected)?;
        self.clear_user_if(expected)?;
        let mut entry = self
            .api_key
            .write()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?;
        if entry
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.session, expected))
        {
            *entry = None;
        }
        Ok(())
    }

    pub(super) fn clear_sessions(&self) -> Result<()> {
        *self
            .anonymous
            .write()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)? = None;
        self.clear_user()?;
        self.clear_api_key()
    }

    pub(super) fn session_manager(&self) -> SessionManager {
        self.session_manager.clone()
    }

    #[cfg(test)]
    pub(super) fn set_user_for_test(&self, session: Arc<V2Session>) -> Result<()> {
        let expected = self
            .session_manager
            .get_credential_snapshot()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?
            .auth_epoch;
        self.session_manager
            .replace_user_tokens_and_session_if_epoch(
                &expected,
                "test-access".to_string(),
                Some("test-refresh".to_string()),
                "test-user".to_string(),
                session,
            )
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?
            .ok_or(TransportV2Error::SessionStateUnavailable)
            .map(|_| ())
    }

    pub(super) fn active_session_id(&self) -> Result<Option<Uuid>> {
        if let Some(session) = self.anonymous()? {
            return Ok(Some(session.session_id()));
        }
        if let Some(session) = self.user()? {
            return Ok(Some(session.session_id()));
        }
        let api_key = self
            .api_key
            .read()
            .map_err(|_| TransportV2Error::SessionStateUnavailable)?;
        Ok(api_key.as_ref().map(|entry| entry.session.session_id()))
    }

    pub(super) const fn anonymous_gate(&self) -> &Mutex<()> {
        &self.anonymous_gate
    }

    pub(super) const fn user_gate(&self) -> &Mutex<()> {
        &self.user_gate
    }

    pub(super) const fn api_key_gate(&self) -> &Mutex<()> {
        &self.api_key_gate
    }
}

impl Drop for TransportV2Runtime {
    fn drop(&mut self) {
        if let Ok(root) = self.cache_namespace_root.get_mut() {
            root.zeroize();
        }
    }
}

impl std::fmt::Debug for TransportV2Runtime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransportV2Runtime")
            .field("cache_namespace_root", &"[REDACTED]")
            .field("anonymous", &"[SESSION SLOT]")
            .field("user", &"[AUTH LIFECYCLE SESSION SLOT]")
            .field("api_key", &"[SESSION SLOT]")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(marker: u8) -> Arc<V2Session> {
        Arc::new(
            V2Session::from_master_for_test(Uuid::from_bytes([marker; 16]), [marker; 32], u64::MAX)
                .expect("test session"),
        )
    }

    #[test]
    fn failed_session_cleanup_is_pointer_scoped_across_authorities() {
        let manager = SessionManager::new();
        let runtime = TransportV2Runtime::new([0x11; 32], manager.clone());
        let failed = session(0x22);
        let replacement = session(0x33);
        let scope = ApiKeyScope::new([0x44; 32]);

        runtime.set_anonymous(Arc::clone(&failed)).unwrap();
        manager
            .replace_user_tokens_and_session_if_epoch(
                &manager.get_credential_snapshot().unwrap().auth_epoch,
                "access".to_string(),
                Some("refresh".to_string()),
                "user".to_string(),
                Arc::clone(&failed),
            )
            .unwrap()
            .expect("install failed session");
        runtime.set_api_key(&scope, Arc::clone(&failed)).unwrap();
        runtime.clear_session_if(&failed).unwrap();
        assert!(runtime.anonymous().unwrap().is_none());
        assert!(runtime.user().unwrap().is_none());
        assert!(runtime.api_key(&scope).unwrap().is_none());

        manager
            .replace_user_tokens_and_session_if_epoch(
                &manager.get_credential_snapshot().unwrap().auth_epoch,
                "new-access".to_string(),
                Some("new-refresh".to_string()),
                "user".to_string(),
                Arc::clone(&replacement),
            )
            .unwrap()
            .expect("install replacement");
        runtime.clear_session_if(&failed).unwrap();
        assert!(Arc::ptr_eq(
            &runtime.user().unwrap().expect("replacement remains"),
            &replacement,
        ));
    }
}
