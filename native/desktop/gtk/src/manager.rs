use std::hash::{Hash, Hasher};
use std::sync::Arc;

use flume::Receiver;
use maple_core::{AppAction, AppReconciler, AppState, AppUpdate, FfiApp};

const KEYRING_SERVICE: &str = "cloud.opensecret.maple.desktop";
const KEYRING_ACCESS: &str = "access_token";
const KEYRING_REFRESH: &str = "refresh_token";

#[derive(Clone)]
pub struct AppManager {
    ffi: Arc<FfiApp>,
    update_rx: Receiver<AppUpdate>,
}

impl Hash for AppManager {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.ffi).hash(state);
    }
}

struct DesktopReconciler {
    tx: flume::Sender<AppUpdate>,
}

impl AppReconciler for DesktopReconciler {
    fn reconcile(&self, update: AppUpdate) {
        let _ = self.tx.send(update);
    }
}

impl AppManager {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs_next::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("Maple")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&data_dir);

        let _ = dotenvy::dotenv();
        let api_url = configured_api_url();
        let client_id = std::env::var("CLIENT_ID")
            .unwrap_or_else(|_| "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6".to_string());

        let ffi = FfiApp::new(api_url, client_id, data_dir);
        let (notify_tx, update_rx) = flume::unbounded();
        ffi.listen_for_updates(Box::new(DesktopReconciler { tx: notify_tx }));

        let manager = Self { ffi, update_rx };
        manager.restore_or_complete_startup();
        Ok(manager)
    }

    pub fn state(&self) -> AppState {
        self.ffi.state()
    }

    pub fn dispatch(&self, action: AppAction) {
        self.ffi.dispatch(action);
    }

    pub fn subscribe_updates(&self) -> Receiver<AppUpdate> {
        self.update_rx.clone()
    }

    pub fn persist_side_effect(update: &AppUpdate) {
        if let AppUpdate::SessionTokens {
            access_token,
            refresh_token,
            ..
        } = update
        {
            save_tokens_to_keyring(access_token, refresh_token);
        }
    }

    fn restore_or_complete_startup(&self) {
        if let Some((access_token, refresh_token)) = load_tokens_from_keyring() {
            self.dispatch(AppAction::RestoreSession {
                access_token,
                refresh_token,
            });
        } else {
            clear_tokens_from_keyring();
            self.dispatch(AppAction::CompleteStartup);
        }
    }
}

fn configured_api_url() -> String {
    std::env::var("OPEN_SECRET_API_URL")
        .ok()
        .or_else(|| option_env!("OPEN_SECRET_API_URL").map(str::to_owned))
        .unwrap_or_else(maple_core::default_api_url)
}

fn save_tokens_to_keyring(access_token: &str, refresh_token: &str) {
    if access_token.is_empty() || refresh_token.is_empty() {
        clear_tokens_from_keyring();
        return;
    }

    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCESS) {
        let _ = entry.set_password(access_token);
    }
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_REFRESH) {
        let _ = entry.set_password(refresh_token);
    }
}

fn load_tokens_from_keyring() -> Option<(String, String)> {
    let access_token = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCESS)
        .ok()?
        .get_password()
        .ok()?;
    let refresh_token = keyring::Entry::new(KEYRING_SERVICE, KEYRING_REFRESH)
        .ok()?
        .get_password()
        .ok()?;
    if access_token.is_empty() || refresh_token.is_empty() {
        clear_tokens_from_keyring();
        return None;
    }
    Some((access_token, refresh_token))
}

fn clear_tokens_from_keyring() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCESS) {
        let _ = entry.delete_credential();
    }
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_REFRESH) {
        let _ = entry.delete_credential();
    }
}
