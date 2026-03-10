use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

use base64::{
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::{DateTime, Local, Utc};
use flume::{Receiver, Sender};
use opensecret::attestation::{AttestationDocument, AttestationVerifier};
use opensecret::crypto::{self, PublicKey};
use opensecret::types::{
    AttestationResponse, ConversationContent, ConversationItem, EncryptedRequest,
    EncryptedResponse, KeyExchangeRequest, KeyExchangeResponse, RefreshRequest, RefreshResponse,
    UserResponse,
};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_cbor::Value as CborValue;
use serde_json::Value as JsonValue;

uniffi::setup_scaffolding!();

const DEFAULT_API_URL: &str = "http://0.0.0.0:3000";

#[uniffi::export]
pub fn default_api_url() -> String {
    DEFAULT_API_URL.to_string()
}

const PAGE_SIZE: i64 = 20;

fn format_relative_time(epoch_secs: i64) -> String {
    let msg_time = DateTime::from_timestamp(epoch_secs, 0)
        .unwrap_or_else(Utc::now)
        .with_timezone(&Local);
    let now = Local::now();
    let diff = now.signed_duration_since(msg_time);

    let secs = diff.num_seconds();
    if secs < 0 {
        return "now".to_string();
    }
    if secs < 60 {
        return "now".to_string();
    }

    let mins = diff.num_minutes();
    if mins == 1 {
        return "1 min ago".to_string();
    }
    if mins < 60 {
        return format!("{mins} min ago");
    }

    let hours = diff.num_hours();
    if hours == 1 {
        return "1 hour ago".to_string();
    }
    if hours < 24 {
        return format!("{hours} hours ago");
    }

    // Check if it was yesterday
    let msg_date = msg_time.date_naive();
    let today = now.date_naive();
    let days = (today - msg_date).num_days();

    if days == 1 {
        return format!("Yesterday, {}", msg_time.format("%-I:%M %p"));
    }

    // Within the last week: show day name + time
    if days < 7 {
        return msg_time.format("%A, %-I:%M %p").to_string();
    }

    // Older: show full date + time
    msg_time.format("%b %-d, %Y, %-I:%M %p").to_string()
}

fn parse_conversation_item(item: &ConversationItem) -> Option<ChatMessage> {
    match item {
        ConversationItem::Message {
            id,
            role,
            content,
            created_at,
            ..
        } => {
            let is_user = role == "user";
            let text: String = content
                .iter()
                .filter_map(|c| match c {
                    ConversationContent::Text { text }
                    | ConversationContent::InputText { text }
                    | ConversationContent::OutputText { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            if text.is_empty() {
                return None;
            }

            let ts = created_at.unwrap_or_else(|| Utc::now().timestamp());

            Some(ChatMessage {
                id: id.to_string(),
                content: text,
                is_user,
                is_streaming: false,
                timestamp: ts,
                timestamp_display: format_relative_time(ts),
                show_sender: true,
                show_timestamp: true,
            })
        }
        _ => None,
    }
}

enum ErrorContext {
    Auth,
    History,
    Agent,
    Delete,
}

fn is_recoverable_secure_session_error(error: &opensecret::Error) -> bool {
    matches!(
        error,
        opensecret::Error::Session(_)
            | opensecret::Error::Encryption(_)
            | opensecret::Error::Decryption(_)
            | opensecret::Error::Api { status: 400, .. }
    )
}

fn is_recoverable_secure_session_error_text(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();

    normalized.contains("session error")
        || normalized.contains("encryption error")
        || normalized.contains("decryption error")
        || normalized.contains("failed to decrypt session key")
        || normalized.contains("aead::error")
        || normalized.contains("no active session")
}

fn user_visible_error(context: ErrorContext, error: &str) -> String {
    if !is_recoverable_secure_session_error_text(error) {
        return error.to_string();
    }

    match context {
        ErrorContext::Auth => "Couldn't complete the secure sign-in. Please try again.".into(),
        ErrorContext::History => "Couldn't refresh the conversation. Please try again.".into(),
        ErrorContext::Agent => "Secure connection interrupted. Please send that again.".into(),
        ErrorContext::Delete => "Couldn't complete that securely. Please try again.".into(),
    }
}

async fn perform_attestation_handshake_with_retry(
    os: &opensecret::OpenSecretClient,
) -> Result<(), opensecret::Error> {
    match os.perform_attestation_handshake().await {
        Ok(()) => Ok(()),
        Err(error) if is_recoverable_secure_session_error(&error) => {
            eprintln!("Retrying transient attestation failure: {error}");
            os.perform_attestation_handshake().await
        }
        Err(error) => Err(error),
    }
}

fn api_uses_mock_attestation(api_url: &str) -> bool {
    api_url.contains("localhost")
        || api_url.contains("127.0.0.1")
        || api_url.contains("0.0.0.0")
        || api_url.contains("10.0.2.2")
}

fn jwt_payload(token: &str) -> Option<JsonValue> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).or_else(|_| {
        let mut padded = payload.to_string();
        while padded.len() % 4 != 0 {
            padded.push('=');
        }
        URL_SAFE.decode(padded)
    });

    serde_json::from_slice(&decoded.ok()?).ok()
}

fn token_fingerprint(token: &str) -> String {
    let checksum = token.bytes().fold(0u64, |acc, byte| {
        acc.wrapping_mul(131).wrapping_add(byte as u64)
    });
    format!("{checksum:016x}")
}

fn token_debug_summary(label: &str, token: &str) -> String {
    let mut pieces = vec![
        format!("len={}", token.len()),
        format!("fp={}", token_fingerprint(token)),
        format!("segments={}", token.split('.').count()),
    ];

    if let Some(payload) = jwt_payload(token) {
        if let Some(exp) = payload.get("exp").and_then(|value| value.as_i64()) {
            pieces.push(format!("exp_delta={}s", exp - Utc::now().timestamp()));
        }

        if let Some(token_type) = payload
            .get("type")
            .or_else(|| payload.get("token_type"))
            .or_else(|| payload.get("typ"))
            .and_then(|value| value.as_str())
        {
            pieces.push(format!("type={token_type}"));
        }
    } else {
        pieces.push("jwt=unparsed".to_string());
    }

    format!("{label}{{{}}}", pieces.join(", "))
}

fn session_token_debug_summary(access_token: &str, refresh_token: &str) -> String {
    format!(
        "{} {} same={}",
        token_debug_summary("access", access_token),
        token_debug_summary("refresh", refresh_token),
        access_token == refresh_token,
    )
}

fn parse_attestation_document_for_refresh(
    api_url: &str,
    document_b64: &str,
    expected_nonce: &str,
) -> Result<AttestationDocument, opensecret::Error> {
    if !api_uses_mock_attestation(api_url) {
        return AttestationVerifier::new()
            .verify_attestation_document(document_b64, expected_nonce);
    }

    let document_bytes = BASE64.decode(document_b64)?;
    let cbor_value: CborValue = serde_cbor::from_slice(&document_bytes)?;

    let cose_sign1 = match &cbor_value {
        CborValue::Array(arr) if arr.len() == 4 => arr,
        _ => {
            return Err(opensecret::Error::AttestationVerificationFailed(
                "Invalid COSE_Sign1 structure".to_string(),
            ));
        }
    };

    let payload = match &cose_sign1[2] {
        CborValue::Bytes(bytes) => bytes,
        _ => {
            return Err(opensecret::Error::AttestationVerificationFailed(
                "Invalid payload".to_string(),
            ));
        }
    };

    let doc_cbor: CborValue = serde_cbor::from_slice(payload)?;
    let map = match &doc_cbor {
        CborValue::Map(entries) => entries,
        _ => {
            return Err(opensecret::Error::AttestationVerificationFailed(
                "Invalid attestation document format".to_string(),
            ));
        }
    };

    let mut public_key = None;
    let mut nonce = None;

    for (key, value) in map {
        if let CborValue::Text(key_str) = key {
            match key_str.as_str() {
                "public_key" => {
                    public_key = match value {
                        CborValue::Bytes(bytes) => Some(bytes.clone()),
                        _ => None,
                    };
                }
                "nonce" => {
                    nonce = match value {
                        CborValue::Bytes(bytes) => Some(bytes.clone()),
                        _ => None,
                    };
                }
                _ => {}
            }
        }
    }

    match nonce.as_deref() {
        Some(raw_nonce) if raw_nonce == expected_nonce.as_bytes() => {}
        Some(_) => {
            return Err(opensecret::Error::AttestationVerificationFailed(
                "Nonce mismatch".to_string(),
            ));
        }
        None => {
            return Err(opensecret::Error::AttestationVerificationFailed(
                "Missing nonce in attestation document".to_string(),
            ));
        }
    }

    Ok(AttestationDocument {
        module_id: "mock-module".to_string(),
        timestamp: 0,
        digest: "SHA384".to_string(),
        pcrs: std::collections::HashMap::new(),
        certificate: vec![],
        cabundle: vec![],
        public_key,
        user_data: None,
        nonce,
    })
}

async fn response_to_api_error(response: reqwest::Response) -> opensecret::Error {
    let status = response.status().as_u16();
    let message = response
        .text()
        .await
        .unwrap_or_else(|_| "Unknown error".to_string());

    opensecret::Error::Api { status, message }
}

async fn refresh_session_tokens_without_auth(
    api_url: &str,
    refresh_token: String,
) -> Result<SessionTokenPair, opensecret::Error> {
    let client = reqwest::Client::new();
    let base_url = api_url.trim_end_matches('/');
    let nonce = uuid::Uuid::new_v4().to_string();

    let attestation_response = client
        .get(format!("{base_url}/attestation/{nonce}"))
        .send()
        .await?;
    if !attestation_response.status().is_success() {
        eprintln!(
            "Session restore refresh path: attestation failed with status {}",
            attestation_response.status()
        );
        return Err(response_to_api_error(attestation_response).await);
    }

    let attestation_doc: AttestationResponse = attestation_response.json().await?;
    let doc = parse_attestation_document_for_refresh(
        api_url,
        &attestation_doc.attestation_document,
        &nonce,
    )?;
    let server_public_key_bytes = doc.public_key.ok_or_else(|| {
        opensecret::Error::AttestationVerificationFailed(
            "No public key in attestation document".to_string(),
        )
    })?;

    let (secret, public_key) = crypto::generate_static_keypair();
    let key_exchange_request = KeyExchangeRequest {
        client_public_key: BASE64.encode(public_key.as_bytes()),
        nonce,
    };

    let key_exchange_response = client
        .post(format!("{base_url}/key_exchange"))
        .header(CONTENT_TYPE, "application/json")
        .json(&key_exchange_request)
        .send()
        .await?;
    if !key_exchange_response.status().is_success() {
        eprintln!(
            "Session restore refresh path: key_exchange failed with status {}",
            key_exchange_response.status()
        );
        return Err(response_to_api_error(key_exchange_response).await);
    }

    let key_exchange: KeyExchangeResponse = key_exchange_response.json().await?;
    let server_public_key = PublicKey::from(
        <[u8; 32]>::try_from(server_public_key_bytes.as_slice()).map_err(|_| {
            opensecret::Error::KeyExchange("Invalid server public key length".to_string())
        })?,
    );
    let shared_secret = crypto::perform_static_key_exchange(&secret, &server_public_key);
    let session_key =
        crypto::decrypt_session_key(&shared_secret, &key_exchange.encrypted_session_key)?;
    let session_id = uuid::Uuid::parse_str(&key_exchange.session_id).map_err(|error| {
        opensecret::Error::Session(format!("Invalid session ID format: {error}"))
    })?;

    let encrypted_request = EncryptedRequest {
        encrypted: BASE64.encode(crypto::encrypt_data(
            &session_key,
            serde_json::to_string(&RefreshRequest { refresh_token })?.as_bytes(),
        )?),
    };

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "x-session-id",
        HeaderValue::from_str(&session_id.to_string())
            .map_err(|error| opensecret::Error::Session(format!("Invalid session ID: {error}")))?,
    );

    let refresh_response = client
        .post(format!("{base_url}/refresh"))
        .headers(headers)
        .json(&encrypted_request)
        .send()
        .await?;
    if !refresh_response.status().is_success() {
        eprintln!(
            "Session restore refresh path: /refresh failed with status {}",
            refresh_response.status()
        );
        return Err(response_to_api_error(refresh_response).await);
    }

    let encrypted_response: EncryptedResponse<RefreshResponse> = refresh_response.json().await?;
    let decrypted =
        crypto::decrypt_data(&session_key, &BASE64.decode(&encrypted_response.encrypted)?)?;
    let refreshed: RefreshResponse = serde_json::from_slice(&decrypted)?;

    eprintln!(
        "Session restore refresh path: refresh succeeded with {}",
        session_token_debug_summary(&refreshed.access_token, &refreshed.refresh_token)
    );

    Ok((refreshed.access_token, refreshed.refresh_token))
}

async fn restore_user_session(
    os: &opensecret::OpenSecretClient,
    api_url: &str,
) -> Result<UserResponse, opensecret::Error> {
    match os.get_user().await {
        Ok(response) => Ok(response),
        Err(opensecret::Error::Api {
            status: 401,
            message,
        }) => {
            let refresh_token = match os.get_refresh_token()? {
                Some(token) if !token.trim().is_empty() => token,
                _ => {
                    return Err(opensecret::Error::Api {
                        status: 401,
                        message,
                    });
                }
            };

            eprintln!(
                "Session restore got 401 from /protected/user; attempting token refresh: {message}"
            );

            let (access_token, refresh_token) =
                refresh_session_tokens_without_auth(api_url, refresh_token).await?;
            eprintln!("Session restore refreshed tokens successfully; retrying /protected/user");

            os.set_tokens(access_token, Some(refresh_token))?;
            os.get_user().await
        }
        Err(error) => Err(error),
    }
}

type SessionTokenPair = (String, String);

fn current_session_tokens(os: &opensecret::OpenSecretClient) -> Option<SessionTokenPair> {
    let access_token = os
        .get_access_token()
        .ok()
        .flatten()
        .map(|token| token.trim().to_string())?;
    let refresh_token = os
        .get_refresh_token()
        .ok()
        .flatten()
        .map(|token| token.trim().to_string())?;

    if access_token.is_empty() || refresh_token.is_empty() {
        return None;
    }

    Some((access_token, refresh_token))
}

fn emit_session_tokens_if_changed(
    update_tx: &Sender<AppUpdate>,
    rev: &mut u64,
    last_synced_session_tokens: &mut Option<SessionTokenPair>,
    tokens: Option<SessionTokenPair>,
) {
    if *last_synced_session_tokens == tokens {
        return;
    }

    match tokens.as_ref() {
        Some((access_token, refresh_token)) => {
            eprintln!(
                "Session tokens updated: {}",
                session_token_debug_summary(access_token, refresh_token)
            );
        }
        None => eprintln!("Session tokens cleared"),
    }

    *rev += 1;
    let emitted_rev = *rev;
    let (access_token, refresh_token) = tokens
        .clone()
        .unwrap_or_else(|| (String::new(), String::new()));

    *last_synced_session_tokens = tokens;

    let _ = update_tx.send(AppUpdate::SessionTokens {
        rev: emitted_rev,
        access_token,
        refresh_token,
    });
}

fn sync_session_tokens_from_client(
    os: &opensecret::OpenSecretClient,
    update_tx: &Sender<AppUpdate>,
    rev: &mut u64,
    last_synced_session_tokens: &mut Option<SessionTokenPair>,
) {
    emit_session_tokens_if_changed(
        update_tx,
        rev,
        last_synced_session_tokens,
        current_session_tokens(os),
    );
}

// ── State ───────────────────────────────────────────────────────────────────

#[derive(uniffi::Enum, Clone, Debug)]
pub enum AuthState {
    Initializing,
    Ready,
    LoggingIn,
    SigningUp,
    LoggedIn {
        user_id: String,
        email: Option<String>,
        name: Option<String>,
    },
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum OAuthProvider {
    Github,
    Google,
    Apple,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct ChatMessage {
    pub id: String,
    pub content: String,
    pub is_user: bool,
    pub is_streaming: bool,
    pub timestamp: i64,
    pub timestamp_display: String,
    pub show_sender: bool,
    pub show_timestamp: bool,
}

const TIMESTAMP_GAP_SECS: i64 = 300; // 5 minutes

fn recompute_display_flags(messages: &mut [ChatMessage]) {
    let len = messages.len();
    for i in 0..len {
        // Show sender label when sender changes from previous message
        let prev = if i > 0 { Some(&messages[i - 1]) } else { None };
        messages[i].show_sender = match prev {
            Some(p) => messages[i].is_user != p.is_user,
            None => true,
        };

        // Show timestamp on the LAST message before a sender change or time gap
        let next = if i + 1 < len {
            Some(&messages[i + 1])
        } else {
            None
        };
        messages[i].show_timestamp = match next {
            Some(n) => {
                messages[i].is_user != n.is_user
                    || (n.timestamp - messages[i].timestamp).abs() >= TIMESTAMP_GAP_SECS
            }
            None => true, // always show on the last message
        };
    }
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct AppState {
    pub rev: u64,
    pub auth: AuthState,
    pub pending_auth_url: Option<String>,
    pub router: Router,
    pub messages: Vec<ChatMessage>,
    pub is_agent_typing: bool,
    pub is_loading_history: bool,
    pub has_older_messages: bool,
    pub compose_text: String,
    pub toast: Option<String>,
    pub show_settings: bool,
    pub confirm_delete_agent: bool,
    pub is_deleting_agent: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Router {
    pub default_screen: Screen,
    pub screen_stack: Vec<Screen>,
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum Screen {
    Loading,
    Login,
    Chat,
}

impl AppState {
    fn initial() -> Self {
        Self {
            rev: 0,
            auth: AuthState::Initializing,
            pending_auth_url: None,
            router: Router {
                default_screen: Screen::Loading,
                screen_stack: vec![],
            },
            messages: vec![],
            is_agent_typing: false,
            is_loading_history: false,
            has_older_messages: false,
            compose_text: String::new(),
            toast: None,
            show_settings: false,
            confirm_delete_agent: false,
            is_deleting_agent: false,
        }
    }
}

// ── Actions & Updates ───────────────────────────────────────────────────────

#[derive(uniffi::Enum, Clone, Debug)]
pub enum AppAction {
    // Auth
    LoginWithEmail {
        email: String,
        password: String,
    },
    SignUpWithEmail {
        email: String,
        password: String,
        name: String,
    },
    InitiateOAuth {
        provider: OAuthProvider,
        invite_code: Option<String>,
    },
    HandleOAuthCallback {
        provider: OAuthProvider,
        code: String,
        state: String,
        invite_code: String,
    },
    RestoreSession {
        access_token: String,
        refresh_token: String,
    },
    CompleteStartup,
    ClearPendingAuthUrl,
    Logout,

    // Chat
    SendMessage {
        content: String,
    },
    LoadOlderMessages,
    RefreshTimestamps,
    ClearToast,

    // Settings
    ToggleSettings,
    RequestDeleteAgent,
    ConfirmDeleteAgent,
    CancelDeleteAgent,
}

#[derive(uniffi::Enum, Clone, Debug)]
pub enum AppUpdate {
    FullState(AppState),
    SessionTokens {
        rev: u64,
        access_token: String,
        refresh_token: String,
    },
}

// ── Callback interface ──────────────────────────────────────────────────────

#[uniffi::export(callback_interface)]
pub trait AppReconciler: Send + Sync + 'static {
    fn reconcile(&self, update: AppUpdate);
}

// ── Internal events (async results, not FFI-visible) ────────────────────────

enum InternalEvent {
    LoginSuccess {
        user_id: String,
        email: Option<String>,
        name: Option<String>,
        access_token: String,
        refresh_token: String,
    },
    SessionRestored {
        user_id: String,
        email: Option<String>,
        name: Option<String>,
    },
    SessionRestoreFailed(String),
    AuthFailed(String),
    OAuthUrlReady {
        url: String,
    },
    // History loading
    HistoryLoaded {
        messages: Vec<ChatMessage>,
        has_more: bool,
        is_initial: bool,
    },
    HistoryLoadFailed(String),
    // Agent streaming events
    AgentMessageChunk {
        messages: Vec<String>,
        step: usize,
    },
    AgentDone,
    AgentError(String),
    // Settings
    AgentDeleted,
    AgentDeleteFailed(String),
}

enum CoreMsg {
    Action(AppAction),
    Internal(Box<InternalEvent>),
}

// ── FFI entry point ─────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct FfiApp {
    core_tx: Sender<CoreMsg>,
    update_rx: Receiver<AppUpdate>,
    listening: AtomicBool,
    shared_state: Arc<RwLock<AppState>>,
}

#[uniffi::export]
impl FfiApp {
    #[uniffi::constructor]
    pub fn new(api_url: String, client_id: String, data_dir: String) -> Arc<Self> {
        let _ = data_dir;

        let (update_tx, update_rx) = flume::unbounded();
        let (core_tx, core_rx) = flume::unbounded::<CoreMsg>();
        let shared_state = Arc::new(RwLock::new(AppState::initial()));

        let shared_for_core = shared_state.clone();
        let core_tx_async = core_tx.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            let client_uuid = uuid::Uuid::parse_str(&client_id).expect("Invalid client_id UUID");

            let os_client = Arc::new(
                opensecret::OpenSecretClient::new(&api_url)
                    .expect("Failed to create OpenSecret client"),
            );

            let mut state = AppState::initial();
            let mut rev: u64 = 0;
            let mut next_msg_id: u64 = 0;
            let mut oldest_cursor: Option<String> = None;
            let mut last_synced_session_tokens: Option<SessionTokenPair> = None;

            let emit =
                |state: &AppState, shared: &Arc<RwLock<AppState>>, tx: &Sender<AppUpdate>| {
                    let snapshot = state.clone();
                    match shared.write() {
                        Ok(mut g) => *g = snapshot.clone(),
                        Err(p) => *p.into_inner() = snapshot.clone(),
                    }
                    let _ = tx.send(AppUpdate::FullState(snapshot));
                };

            emit(&state, &shared_for_core, &update_tx);

            // Helper: spawn a history fetch (initial or older pages)
            macro_rules! spawn_history_fetch {
                ($cursor:expr, $is_initial:expr) => {{
                    let os = os_client.clone();
                    let tx = core_tx_async.clone();
                    let cursor = $cursor;
                    let is_initial: bool = $is_initial;
                    rt.spawn(async move {
                        let params = opensecret::types::AgentItemsListParams {
                            limit: Some(PAGE_SIZE),
                            order: Some("desc".to_string()),
                            after: cursor.and_then(|c: String| uuid::Uuid::parse_str(&c).ok()),
                            include: None,
                        };
                        let event = match os.list_main_agent_items(Some(params)).await {
                            Ok(resp) => {
                                let mut msgs: Vec<ChatMessage> = resp
                                    .data
                                    .iter()
                                    .filter_map(|item| parse_conversation_item(item))
                                    .collect();
                                // API returns desc order; reverse to chronological
                                msgs.reverse();
                                InternalEvent::HistoryLoaded {
                                    messages: msgs,
                                    has_more: resp.has_more,
                                    is_initial,
                                }
                            }
                            Err(e) => InternalEvent::HistoryLoadFailed(e.to_string()),
                        };
                        let _ = tx.send(CoreMsg::Internal(Box::new(event)));
                    });
                }};
            }

            while let Ok(msg) = core_rx.recv() {
                let mut should_sync_session_tokens = true;

                match msg {
                    CoreMsg::Internal(event) => match *event {
                        InternalEvent::LoginSuccess {
                            user_id,
                            email,
                            name,
                            access_token,
                            refresh_token,
                        } => {
                            state.auth = AuthState::LoggedIn {
                                user_id,
                                email,
                                name,
                            };
                            state.router = Router {
                                default_screen: Screen::Chat,
                                screen_stack: vec![],
                            };
                            state.messages.clear();
                            state.pending_auth_url = None;
                            state.is_loading_history = true;
                            oldest_cursor = None;

                            emit_session_tokens_if_changed(
                                &update_tx,
                                &mut rev,
                                &mut last_synced_session_tokens,
                                Some((access_token, refresh_token)),
                            );
                            should_sync_session_tokens = false;

                            spawn_history_fetch!(None::<String>, true);
                        }
                        InternalEvent::SessionRestored {
                            user_id,
                            email,
                            name,
                        } => {
                            state.auth = AuthState::LoggedIn {
                                user_id,
                                email,
                                name,
                            };
                            state.router = Router {
                                default_screen: Screen::Chat,
                                screen_stack: vec![],
                            };
                            state.pending_auth_url = None;
                            state.messages.clear();
                            state.is_loading_history = true;
                            oldest_cursor = None;
                            spawn_history_fetch!(None::<String>, true);
                        }
                        InternalEvent::SessionRestoreFailed(error) => {
                            eprintln!("Session restore failed: {error}");
                            state.auth = AuthState::Ready;
                            state.router = Router {
                                default_screen: Screen::Login,
                                screen_stack: vec![],
                            };
                            state.toast = Some(user_visible_error(ErrorContext::Auth, &error));
                        }
                        InternalEvent::AuthFailed(error) => {
                            state.auth = AuthState::Ready;
                            state.toast = Some(user_visible_error(ErrorContext::Auth, &error));
                            state.pending_auth_url = None;
                        }
                        InternalEvent::OAuthUrlReady { url } => {
                            state.pending_auth_url = Some(url);
                        }
                        InternalEvent::HistoryLoaded {
                            messages,
                            has_more,
                            is_initial,
                        } => {
                            state.is_loading_history = false;
                            state.has_older_messages = has_more;

                            if !messages.is_empty() {
                                oldest_cursor = Some(messages[0].id.clone());

                                if is_initial {
                                    state.messages = messages;
                                } else {
                                    let mut combined = messages;
                                    combined.append(&mut state.messages);
                                    state.messages = combined;
                                }
                                recompute_display_flags(&mut state.messages);
                            }
                        }
                        InternalEvent::HistoryLoadFailed(error) => {
                            state.is_loading_history = false;
                            state.toast = Some(user_visible_error(ErrorContext::History, &error));
                        }
                        InternalEvent::AgentMessageChunk { messages, step } => {
                            // Each event carries new messages for this step.
                            // Finalize any currently-streaming message, then
                            // append all new messages (last one stays streaming).
                            eprintln!("[agent] step={step} messages={messages:?}");

                            // Finalize previous streaming message
                            if let Some(msg) = state.messages.last_mut().filter(|m| m.is_streaming)
                            {
                                msg.is_streaming = false;
                            }

                            for (i, text) in messages.iter().enumerate() {
                                let is_last = i == messages.len() - 1;
                                next_msg_id += 1;
                                let ts = Utc::now().timestamp();
                                state.messages.push(ChatMessage {
                                    id: format!("agent_s{step}_{next_msg_id}"),
                                    content: text.clone(),
                                    is_user: false,
                                    is_streaming: is_last,
                                    timestamp: ts,
                                    timestamp_display: format_relative_time(ts),
                                    show_sender: true,
                                    show_timestamp: true,
                                });
                            }
                            recompute_display_flags(&mut state.messages);
                            state.is_agent_typing = true;
                        }
                        InternalEvent::AgentDone => {
                            // Finalize any streaming message
                            if let Some(msg) = state.messages.last_mut().filter(|m| m.is_streaming)
                            {
                                msg.is_streaming = false;
                            }
                            state.is_agent_typing = false;
                        }
                        InternalEvent::AgentError(error) => {
                            // Finalize any streaming message
                            if let Some(msg) = state.messages.last_mut().filter(|m| m.is_streaming)
                            {
                                msg.is_streaming = false;
                            }
                            state.is_agent_typing = false;
                            state.toast = Some(user_visible_error(ErrorContext::Agent, &error));
                        }
                        InternalEvent::AgentDeleted => {
                            state.is_deleting_agent = false;
                            state.show_settings = false;
                            state.messages.clear();
                            state.is_agent_typing = false;
                            state.has_older_messages = false;
                            oldest_cursor = None;
                            state.toast = Some("Agent conversation deleted".to_string());
                        }
                        InternalEvent::AgentDeleteFailed(error) => {
                            state.is_deleting_agent = false;
                            state.toast = Some(user_visible_error(ErrorContext::Delete, &error));
                        }
                    },

                    CoreMsg::Action(action) => match action {
                        // ── Auth ─────────────────────────────────────────
                        AppAction::LoginWithEmail { email, password } => {
                            state.auth = AuthState::LoggingIn;
                            let os = os_client.clone();
                            let tx = core_tx_async.clone();
                            let cid = client_uuid;
                            rt.spawn(async move {
                                let event = match os.login(email, password, cid).await {
                                    Ok(resp) => InternalEvent::LoginSuccess {
                                        user_id: resp.id.to_string(),
                                        email: resp.email,
                                        name: None,
                                        access_token: resp.access_token,
                                        refresh_token: resp.refresh_token,
                                    },
                                    Err(e) => {
                                        InternalEvent::AuthFailed(format!("Login failed: {e}"))
                                    }
                                };
                                let _ = tx.send(CoreMsg::Internal(Box::new(event)));
                            });
                        }

                        AppAction::SignUpWithEmail {
                            email,
                            password,
                            name,
                        } => {
                            state.auth = AuthState::SigningUp;
                            let os = os_client.clone();
                            let tx = core_tx_async.clone();
                            let cid = client_uuid;
                            let name_opt = if name.is_empty() { None } else { Some(name) };
                            rt.spawn(async move {
                                let event = match os.register(email, password, cid, name_opt).await
                                {
                                    Ok(resp) => InternalEvent::LoginSuccess {
                                        user_id: resp.id.to_string(),
                                        email: resp.email,
                                        name: None,
                                        access_token: resp.access_token,
                                        refresh_token: resp.refresh_token,
                                    },
                                    Err(e) => {
                                        InternalEvent::AuthFailed(format!("Sign up failed: {e}"))
                                    }
                                };
                                let _ = tx.send(CoreMsg::Internal(Box::new(event)));
                            });
                        }

                        AppAction::InitiateOAuth {
                            provider,
                            invite_code,
                        } => {
                            state.auth = AuthState::LoggingIn;
                            let os = os_client.clone();
                            let tx = core_tx_async.clone();
                            let cid = client_uuid;
                            rt.spawn(async move {
                                let result = match provider {
                                    OAuthProvider::Github => os
                                        .initiate_github_auth(cid, invite_code)
                                        .await
                                        .map(|r| r.auth_url),
                                    OAuthProvider::Google => os
                                        .initiate_google_auth(cid, invite_code)
                                        .await
                                        .map(|r| r.auth_url),
                                    OAuthProvider::Apple => os
                                        .initiate_apple_auth(cid, invite_code)
                                        .await
                                        .map(|r| r.auth_url),
                                };
                                let event = match result {
                                    Ok(url) => InternalEvent::OAuthUrlReady { url },
                                    Err(e) => InternalEvent::AuthFailed(format!(
                                        "OAuth initiation failed: {e}"
                                    )),
                                };
                                let _ = tx.send(CoreMsg::Internal(Box::new(event)));
                            });
                        }

                        AppAction::HandleOAuthCallback {
                            provider,
                            code,
                            state: oauth_state,
                            invite_code,
                        } => {
                            state.auth = AuthState::LoggingIn;
                            state.pending_auth_url = None;
                            let os = os_client.clone();
                            let tx = core_tx_async.clone();
                            rt.spawn(async move {
                                let result = match provider {
                                    OAuthProvider::Github => {
                                        os.handle_github_callback(code, oauth_state, invite_code)
                                            .await
                                    }
                                    OAuthProvider::Google => {
                                        os.handle_google_callback(code, oauth_state, invite_code)
                                            .await
                                    }
                                    OAuthProvider::Apple => {
                                        os.handle_apple_callback(code, oauth_state, invite_code)
                                            .await
                                    }
                                };
                                let event = match result {
                                    Ok(resp) => InternalEvent::LoginSuccess {
                                        user_id: resp.id.to_string(),
                                        email: resp.email,
                                        name: None,
                                        access_token: resp.access_token,
                                        refresh_token: resp.refresh_token,
                                    },
                                    Err(e) => InternalEvent::AuthFailed(format!(
                                        "OAuth callback failed: {e}"
                                    )),
                                };
                                let _ = tx.send(CoreMsg::Internal(Box::new(event)));
                            });
                        }

                        AppAction::RestoreSession {
                            access_token,
                            refresh_token,
                        } => {
                            eprintln!(
                                "Attempting session restore with {}",
                                session_token_debug_summary(&access_token, &refresh_token)
                            );
                            state.auth = AuthState::Initializing;
                            state.router = Router {
                                default_screen: Screen::Loading,
                                screen_stack: vec![],
                            };
                            state.pending_auth_url = None;
                            let os = os_client.clone();
                            let tx = core_tx_async.clone();
                            if let Err(e) = os.set_tokens(access_token, Some(refresh_token)) {
                                let _ = tx.send(CoreMsg::Internal(Box::new(
                                    InternalEvent::SessionRestoreFailed(format!(
                                        "Failed to set tokens: {e}"
                                    )),
                                )));
                            } else {
                                let restore_api_url = api_url.clone();
                                rt.spawn(async move {
                                    match restore_user_session(os.as_ref(), &restore_api_url).await
                                    {
                                        Ok(resp) => {
                                            let _ = tx.send(CoreMsg::Internal(Box::new(
                                                InternalEvent::SessionRestored {
                                                    user_id: resp.user.id.to_string(),
                                                    email: resp.user.email,
                                                    name: resp.user.name,
                                                },
                                            )));
                                        }
                                        Err(e) => {
                                            let _ = tx.send(CoreMsg::Internal(Box::new(
                                                InternalEvent::SessionRestoreFailed(e.to_string()),
                                            )));
                                        }
                                    }
                                });
                            }
                        }

                        AppAction::CompleteStartup => {
                            if matches!(state.auth, AuthState::Initializing) {
                                state.auth = AuthState::Ready;
                                state.router = Router {
                                    default_screen: Screen::Login,
                                    screen_stack: vec![],
                                };
                                state.pending_auth_url = None;
                            }
                        }

                        AppAction::ClearPendingAuthUrl => {
                            state.pending_auth_url = None;
                        }

                        AppAction::Logout => {
                            let os = os_client.clone();
                            rt.spawn(async move {
                                let _ = os.logout().await;
                            });
                            state.auth = AuthState::Ready;
                            state.router = Router {
                                default_screen: Screen::Login,
                                screen_stack: vec![],
                            };
                            state.messages.clear();
                            state.is_agent_typing = false;
                            state.is_loading_history = false;
                            state.has_older_messages = false;
                            state.compose_text.clear();
                            state.show_settings = false;
                            state.confirm_delete_agent = false;
                            state.is_deleting_agent = false;
                            oldest_cursor = None;

                            emit_session_tokens_if_changed(
                                &update_tx,
                                &mut rev,
                                &mut last_synced_session_tokens,
                                None,
                            );
                            should_sync_session_tokens = false;
                        }

                        // ── Chat ─────────────────────────────────────────
                        AppAction::LoadOlderMessages => {
                            if state.is_loading_history || !state.has_older_messages {
                                continue;
                            }
                            state.is_loading_history = true;
                            spawn_history_fetch!(oldest_cursor.clone(), false);
                        }
                        AppAction::SendMessage { content } => {
                            let content = content.trim().to_string();
                            if content.is_empty() || state.is_agent_typing {
                                continue;
                            }

                            // Add user message to state
                            next_msg_id += 1;
                            let ts = Utc::now().timestamp();
                            state.messages.push(ChatMessage {
                                id: format!("user_{next_msg_id}"),
                                content: content.clone(),
                                is_user: true,
                                is_streaming: false,
                                timestamp: ts,
                                timestamp_display: format_relative_time(ts),
                                show_sender: true,
                                show_timestamp: true,
                            });
                            recompute_display_flags(&mut state.messages);
                            state.is_agent_typing = true;
                            state.compose_text.clear();

                            // Spawn agent_chat streaming task
                            let os = os_client.clone();
                            let tx = core_tx_async.clone();
                            rt.spawn(async move {
                                use futures::StreamExt;
                                let mut retried_session = false;

                                'agent_stream: loop {
                                    match os.agent_chat(&content).await {
                                        Ok(mut stream) => {
                                            let mut received_message = false;

                                            while let Some(event) = stream.next().await {
                                                match event {
                                                    Ok(opensecret::types::AgentSseEvent::Message(
                                                        msg,
                                                    )) => {
                                                        received_message = true;
                                                        let _ = tx.send(CoreMsg::Internal(Box::new(
                                                            InternalEvent::AgentMessageChunk {
                                                                messages: msg.messages,
                                                                step: msg.step,
                                                            },
                                                        )));
                                                    }
                                                    Ok(opensecret::types::AgentSseEvent::Typing(_)) => {
                                                        continue;
                                                    }
                                                    Ok(opensecret::types::AgentSseEvent::Done(_)) => {
                                                        let _ = tx.send(CoreMsg::Internal(Box::new(
                                                            InternalEvent::AgentDone,
                                                        )));
                                                        return;
                                                    }
                                                    Ok(opensecret::types::AgentSseEvent::Error(
                                                        err,
                                                    )) => {
                                                        if !received_message
                                                            && !retried_session
                                                            && is_recoverable_secure_session_error_text(
                                                                &err.error,
                                                            )
                                                        {
                                                            retried_session = true;
                                                            match perform_attestation_handshake_with_retry(
                                                                os.as_ref(),
                                                            )
                                                            .await
                                                            {
                                                                Ok(()) => continue 'agent_stream,
                                                                Err(handshake_error) => {
                                                                    let _ = tx.send(CoreMsg::Internal(
                                                                        Box::new(
                                                                            InternalEvent::AgentError(
                                                                                handshake_error
                                                                                    .to_string(),
                                                                            ),
                                                                        ),
                                                                    ));
                                                                    return;
                                                                }
                                                            }
                                                        }

                                                        let _ = tx.send(CoreMsg::Internal(Box::new(
                                                            InternalEvent::AgentError(err.error),
                                                        )));
                                                        return;
                                                    }
                                                    Err(error) => {
                                                        if !received_message
                                                            && !retried_session
                                                            && is_recoverable_secure_session_error(
                                                                &error,
                                                            )
                                                        {
                                                            retried_session = true;
                                                            match perform_attestation_handshake_with_retry(
                                                                os.as_ref(),
                                                            )
                                                            .await
                                                            {
                                                                Ok(()) => continue 'agent_stream,
                                                                Err(handshake_error) => {
                                                                    let _ = tx.send(CoreMsg::Internal(
                                                                        Box::new(
                                                                            InternalEvent::AgentError(
                                                                                handshake_error
                                                                                    .to_string(),
                                                                            ),
                                                                        ),
                                                                    ));
                                                                    return;
                                                                }
                                                            }
                                                        }

                                                        let _ = tx.send(CoreMsg::Internal(Box::new(
                                                            InternalEvent::AgentError(
                                                                error.to_string(),
                                                            ),
                                                        )));
                                                        return;
                                                    }
                                                }
                                            }

                                            let _ = tx.send(CoreMsg::Internal(Box::new(
                                                InternalEvent::AgentDone,
                                            )));
                                            return;
                                        }
                                        Err(error)
                                            if !retried_session
                                                && is_recoverable_secure_session_error(&error) =>
                                        {
                                            retried_session = true;
                                            match perform_attestation_handshake_with_retry(
                                                os.as_ref(),
                                            )
                                            .await
                                            {
                                                Ok(()) => continue,
                                                Err(handshake_error) => {
                                                    let _ = tx.send(CoreMsg::Internal(Box::new(
                                                        InternalEvent::AgentError(
                                                            handshake_error.to_string(),
                                                        ),
                                                    )));
                                                    return;
                                                }
                                            }
                                        }
                                        Err(error) => {
                                            let _ = tx.send(CoreMsg::Internal(Box::new(
                                                InternalEvent::AgentError(error.to_string()),
                                            )));
                                            return;
                                        }
                                    }
                                }
                            });
                        }
                        AppAction::RefreshTimestamps => {
                            for msg in &mut state.messages {
                                msg.timestamp_display = format_relative_time(msg.timestamp);
                            }
                        }
                        AppAction::ClearToast => {
                            state.toast = None;
                        }

                        // ── Settings ─────────────────────────────────
                        AppAction::ToggleSettings => {
                            state.show_settings = !state.show_settings;
                            state.confirm_delete_agent = false;
                        }
                        AppAction::RequestDeleteAgent => {
                            state.confirm_delete_agent = true;
                        }
                        AppAction::CancelDeleteAgent => {
                            state.confirm_delete_agent = false;
                        }
                        AppAction::ConfirmDeleteAgent => {
                            state.is_deleting_agent = true;
                            state.confirm_delete_agent = false;
                            let os = os_client.clone();
                            let tx = core_tx_async.clone();
                            rt.spawn(async move {
                                match os.delete_main_agent().await {
                                    Ok(_) => {
                                        let _ = tx.send(CoreMsg::Internal(Box::new(
                                            InternalEvent::AgentDeleted,
                                        )));
                                    }
                                    Err(e) => {
                                        let _ = tx.send(CoreMsg::Internal(Box::new(
                                            InternalEvent::AgentDeleteFailed(e.to_string()),
                                        )));
                                    }
                                }
                            });
                        }
                    },
                }

                if should_sync_session_tokens {
                    sync_session_tokens_from_client(
                        os_client.as_ref(),
                        &update_tx,
                        &mut rev,
                        &mut last_synced_session_tokens,
                    );
                }

                rev += 1;
                state.rev = rev;
                emit(&state, &shared_for_core, &update_tx);
            }
        });

        Arc::new(Self {
            core_tx,
            update_rx,
            listening: AtomicBool::new(false),
            shared_state,
        })
    }

    pub fn state(&self) -> AppState {
        match self.shared_state.read() {
            Ok(g) => g.clone(),
            Err(poison) => poison.into_inner().clone(),
        }
    }

    pub fn dispatch(&self, action: AppAction) {
        let _ = self.core_tx.send(CoreMsg::Action(action));
    }

    pub fn listen_for_updates(&self, reconciler: Box<dyn AppReconciler>) {
        if self
            .listening
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let rx = self.update_rx.clone();
        thread::spawn(move || {
            while let Ok(update) = rx.recv() {
                reconciler.reconcile(update);
            }
        });
    }
}
