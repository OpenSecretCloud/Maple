use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

#[cfg(desktop)]
mod agent;
#[cfg(desktop)]
mod agent_acp;
#[cfg(desktop)]
mod agent_host;
#[cfg(desktop)]
mod agent_tauri;
#[cfg(any(desktop, target_os = "ios"))]
mod legacy_tts_cleanup;
#[cfg(desktop)]
mod maple_api;
mod onnxruntime;
mod open_secret_config;
mod pdf_extractor;
mod pdf_ocr;
mod proxy;
#[cfg(desktop)]
mod updater_preferences;
mod word_extractor;

#[cfg(desktop)]
#[tauri::command]
async fn restart_for_update(app_handle: tauri::AppHandle) -> Result<(), String> {
    log::info!("User requested restart for update");
    let lifecycle = app_handle.state::<agent_host::AgentHostLifecycle>();
    lifecycle.shutdown_for_update(&app_handle).await?;
    app_handle.request_restart();
    Ok(())
}

#[cfg(desktop)]
fn reveal_main_window(app_handle: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    if let Err(error) = app_handle.show() {
        log::warn!("Failed to unhide Maple: {error}");
    }

    let Some(window) = app_handle.get_webview_window("main") else {
        log::warn!("Cannot reveal Maple because the main window is unavailable");
        return;
    };

    if let Err(error) = window.unminimize() {
        log::warn!("Failed to unminimize the main window: {error}");
    }
    if let Err(error) = window.show() {
        log::warn!("Failed to show the main window: {error}");
    }
    if let Err(error) = window.set_focus() {
        log::warn!("Failed to focus the main window: {error}");
    }
}

#[cfg(target_os = "macos")]
fn enable_main_window_frame_autosave(app_handle: &tauri::AppHandle) {
    use objc2_app_kit::NSWindow;
    use objc2_foundation::NSString;

    let Some(window) = app_handle.get_webview_window("main") else {
        log::warn!("Cannot enable frame autosave because the main window is unavailable");
        return;
    };

    let ns_window = match window.ns_window() {
        Ok(ns_window) => ns_window.cast::<NSWindow>(),
        Err(error) => {
            log::warn!("Failed to access the native main window: {error}");
            return;
        }
    };

    // SAFETY: Tauri returns the live NSWindow backing this WebviewWindow. This
    // setup callback runs on the macOS main thread and the pointer is used only
    // for the duration of this call.
    let Some(ns_window) = (unsafe { ns_window.as_ref() }) else {
        log::warn!("Cannot enable frame autosave because the native main window is null");
        return;
    };

    let autosave_name = NSString::from_str("MapleMainWindow");
    if !ns_window.setFrameUsingName(&autosave_name) {
        ns_window.center();
    }
    if !ns_window.setFrameAutosaveName(&autosave_name) {
        log::warn!("AppKit rejected the main window frame autosave name");
    }
}

#[cfg(desktop)]
#[tauri::command]
fn get_pending_update_failure() -> Result<Option<PendingUpdateFailure>, String> {
    PENDING_UPDATE_FAILURE
        .lock()
        .map_err(|e| format!("Failed to lock PENDING_UPDATE_FAILURE mutex: {e}"))
        .map(|failure| failure.clone())
}

#[cfg(desktop)]
#[tauri::command]
fn get_pending_update_install() -> Result<Option<String>, String> {
    Ok(PENDING_UPDATE
        .lock()
        .map_err(|e| format!("Failed to lock PENDING_UPDATE mutex: {e}"))?
        .as_ref()
        .map(|pending| pending.update.version.clone()))
}

#[cfg(desktop)]
#[tauri::command]
async fn install_pending_update(app_handle: tauri::AppHandle) -> Result<(), String> {
    // Hold the check lock for the whole install so the hourly check cannot
    // download and offer the same version again while the password prompt
    // is open.
    let _check_guard = UPDATE_CHECK_LOCK.lock().await;

    let pending = PENDING_UPDATE
        .lock()
        .map_err(|e| format!("Failed to lock PENDING_UPDATE mutex: {e}"))?
        .take()
        .ok_or_else(|| "No downloaded update is waiting to be installed".to_string())?;

    log::info!(
        "User requested install of downloaded update to version {}",
        pending.update.version
    );

    // Installing a deb/rpm blocks on pkexec until the user answers the
    // password prompt, so keep it off the async runtime.
    let (pending, result) = tauri::async_runtime::spawn_blocking(move || {
        let result = install_update(
            &app_handle,
            &pending.update,
            &pending.bytes,
            UpdateInstallOrigin::UserApprovedPending,
        );
        (pending, result)
    })
    .await
    .map_err(|e| format!("Update install task failed: {e}"))?;

    if result.is_err() {
        // A cancelled password prompt lands here. The bytes are already
        // signature-verified, so keep them for "Try Again".
        match PENDING_UPDATE.lock() {
            Ok(mut guard) => {
                if guard.is_none() {
                    *guard = Some(pending);
                }
            }
            Err(e) => {
                log::error!("Failed to lock PENDING_UPDATE mutex when restoring update: {e}")
            }
        }
    }

    result
}

#[cfg(desktop)]
#[tauri::command]
async fn check_for_updates_manually(
    app_handle: tauri::AppHandle,
) -> Result<UpdateCheckResult, String> {
    check_for_updates(app_handle, UpdateCheckTrigger::Manual).await
}

#[cfg(desktop)]
fn handle_desktop_run_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    if matches!(&event, tauri::RunEvent::Ready) {
        reveal_main_window(app_handle);
        return;
    }

    #[cfg(target_os = "macos")]
    if matches!(&event, tauri::RunEvent::Reopen { .. }) {
        reveal_main_window(app_handle);
        return;
    }

    let tauri::RunEvent::ExitRequested { code, api, .. } = event else {
        return;
    };

    // Update restart is explicitly drained by restart_for_update. Tauri does
    // not allow restart ExitRequested events to be prevented.
    if code == Some(tauri::RESTART_EXIT_CODE) || AGENT_EXIT_CLEANUP_COMPLETE.load(Ordering::SeqCst)
    {
        return;
    }

    api.prevent_exit();
    if AGENT_EXIT_CLEANUP_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let lifecycle = app_handle.state::<agent_host::AgentHostLifecycle>();
        if let Err(error) = lifecycle.shutdown_for_exit(&app_handle).await {
            log::error!("Failed to stop Agent services during app exit: {error}");
        }
        AGENT_EXIT_CLEANUP_COMPLETE.store(true, Ordering::SeqCst);
        app_handle.exit(code.unwrap_or_default());
    });
}

// This handles incoming deep links
fn handle_deep_link_event(url: &str, app: &tauri::AppHandle) {
    // OAuth callbacks carry bearer tokens in the query string, so never log the raw URL.
    log::info!("[Deep Link] Received callback");
    #[cfg(desktop)]
    reveal_main_window(app);
    // Forward the URL to the frontend
    match app.emit_to("main", "deep-link-received", url.to_string()) {
        Ok(_) => log::info!("[Deep Link] Event emitted successfully"),
        Err(e) => log::error!("[Deep Link] Failed to emit event: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(desktop)]
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // argv can contain a custom-scheme callback URL with OAuth bearer tokens.
            log::info!("Single instance detected for {}", app.package_info().name);
            reveal_main_window(app);
        }))
        .plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(agent_acp::AgentAcpState::new())
        .manage(agent_host::AgentHostLifecycle::new())
        .manage(maple_api::MapleApiAuthState::new())
        .manage(proxy::ProxyState::new())
        .invoke_handler(tauri::generate_handler![
            agent_tauri::agent_get_runtime_status,
            agent_tauri::agent_start_runtime,
            agent_tauri::agent_stop_runtime,
            agent_tauri::agent_restart_runtime,
            agent_tauri::agent_load_config,
            agent_tauri::agent_save_config,
            agent_tauri::agent_list_mcp_servers,
            agent_tauri::agent_save_mcp_servers,
            agent_tauri::agent_list_recent_project_roots,
            agent_tauri::agent_save_recent_project_root,
            agent_tauri::agent_remove_project_root,
            agent_tauri::agent_get_project_trust,
            agent_tauri::agent_set_project_trust,
            agent_tauri::agent_save_project_root_order,
            agent_tauri::agent_create_session,
            agent_tauri::agent_list_sessions,
            agent_tauri::agent_load_session,
            agent_tauri::agent_load_image_attachment,
            agent_tauri::agent_rename_session,
            agent_tauri::agent_list_session_mcp_servers,
            agent_tauri::agent_set_session_mcp_server_enabled,
            agent_tauri::agent_delete_session,
            agent_tauri::agent_send_message,
            agent_tauri::agent_cancel_queued_message,
            agent_tauri::agent_unqueue_message_for_edit,
            agent_tauri::agent_begin_queued_message_edit,
            agent_tauri::agent_end_queued_message_edit,
            agent_tauri::agent_update_queued_message,
            agent_tauri::agent_cancel_run,
            agent_tauri::agent_set_permission_mode,
            agent_tauri::agent_permission_respond,
            agent_tauri::agent_clear_user_history,
            agent_tauri::agent_clear_user_data,
            agent_acp::agent_acp_load_config,
            agent_acp::agent_acp_save_config,
            agent_acp::agent_acp_start,
            agent_acp::agent_acp_restore_enabled,
            agent_acp::agent_acp_stop,
            agent_acp::agent_acp_get_status,
            maple_api::maple_api_set_auth,
            maple_api::maple_api_get_auth,
            maple_api::maple_api_clear_auth,
            proxy::start_proxy,
            proxy::stop_proxy,
            proxy::stop_and_reset_proxy,
            proxy::get_proxy_status,
            proxy::load_proxy_config,
            proxy::save_proxy_settings,
            proxy::test_proxy_port,
            pdf_extractor::extract_document_content,
            restart_for_update,
            get_pending_update_failure,
            get_pending_update_install,
            install_pending_update,
            check_for_updates_manually,
            updater_preferences::load_updater_preferences,
            updater_preferences::save_updater_preferences,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            enable_main_window_frame_autosave(app.handle());

            legacy_tts_cleanup::schedule(app.handle());

            let service = agent_host::build_service(app.handle())?;
            if !app.manage(service) {
                return Err("Maple Agent service was already initialized".into());
            }

            // Initialize proxy auto-start
            {
                let app_handle_proxy = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Small delay to ensure app is fully initialized
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                    // Create a new State wrapper for the async context
                    if let Err(e) = proxy::init_proxy_on_startup_simple(app_handle_proxy).await {
                        log::error!("Failed to initialize proxy: {e}");
                    }
                });
            }

            // Set up the deep link handler
            // Use a cloned handle with 'static lifetime
            let app_handle = app.handle().clone();
            // Register the deep link handler
            app.deep_link().on_open_url(move |event| {
                if let Some(url) = event.urls().first() {
                    // Use the cloned app_handle
                    handle_deep_link_event(url.as_ref(), &app_handle);
                }
            });
            // Optionally register the scheme at runtime
            #[cfg(desktop)]
            if let Err(e) = app.deep_link().register("cloud.opensecret.maple") {
                log::error!("[Deep Link] Failed to register scheme: {e}");
            }
            // Windows startup diagnostic: confirm the OS has our custom scheme
            // pointed at the running exe. Most "OAuth callback does nothing"
            // reports trace back to a missing/stale
            // HKCU\Software\Classes\cloud.opensecret.maple key (no installer run,
            // a deleted dev build, or a stale path), so log it once at startup.
            #[cfg(target_os = "windows")]
            match app.deep_link().is_registered("cloud.opensecret.maple") {
                Ok(true) => log::info!("[Deep Link] scheme 'cloud.opensecret.maple' is registered"),
                Ok(false) => log::warn!(
                    "[Deep Link] scheme 'cloud.opensecret.maple' is NOT registered; OAuth/payment deep links will not reach the app"
                ),
                Err(e) => log::error!("[Deep Link] is_registered check failed: {e}"),
            }
            // Create the application menu with update options
            #[cfg(desktop)]
            {
                log::info!("Setting up updater");

                // Check on startup and hourly while automatic updates remain enabled.
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Wait for app to fully initialize (use async sleep to not block the thread)
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                    if let Err(e) = check_for_updates(
                        app_handle.clone(),
                        UpdateCheckTrigger::Automatic,
                    )
                    .await
                    {
                        log::error!("Automatic update check failed: {e}");
                    }

                    // Set up hourly update checks
                    let hourly_app_handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        // Define one hour in seconds
                        let one_hour = std::time::Duration::from_secs(3600);

                        loop {
                            // Wait one hour before checking again
                            tokio::time::sleep(one_hour).await;
                            if let Err(e) = check_for_updates(
                                hourly_app_handle.clone(),
                                UpdateCheckTrigger::Automatic,
                            )
                            .await
                            {
                                log::error!("Scheduled update check failed: {e}");
                            }
                        }
                    });
                });

                // Create the application menu (macOS only).
                //
                // On Windows/Linux this menu renders as an in-window bar at the top of
                // the window. Its edit items (undo/redo/cut/copy/paste/select-all) are
                // handled natively by the webview regardless of the menu, About lives in
                // the in-app account menu, and the cross-platform manual update action
                // lives in Settings. The bar adds only clutter, so we omit it entirely
                // on those platforms for a cleaner window.
                #[cfg(target_os = "macos")]
                {
                    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

                    // Define menu item ID for "Check for Updates"
                    let check_updates_id = "check-for-updates";

                    // Get app handle for menu operations
                    let handle = app.handle();

                    let check_updates_item =
                        MenuItemBuilder::with_id(check_updates_id, "Check for Updates")
                            .build(handle)?;

                    // For macOS, we need to create a proper submenu structure
                    // First create the app submenu (first submenu becomes the application menu)
                    let app_submenu = SubmenuBuilder::new(handle, &app.package_info().name)
                        // Add about menu item (standard macOS menu item)
                        .about(None)
                        // Add our update checker to the app menu
                        .item(&check_updates_item)
                        .separator()
                        .hide()
                        .hide_others()
                        .show_all()
                        .separator()
                        .quit()
                        .build()?;

                    // Create edit submenu with standard clipboard operations
                    let edit_submenu = SubmenuBuilder::new(handle, "Edit")
                        .undo()
                        .redo()
                        .separator()
                        .cut()
                        .copy()
                        .paste()
                        .separator()
                        .select_all()
                        .build()?;

                    // Create the main menu and add our app submenu and edit submenu
                    let menu = MenuBuilder::new(handle)
                        .items(&[&app_submenu, &edit_submenu])
                        .build()?;

                    // Set as the application menu
                    app.set_menu(menu)?;

                    log::info!("Setting up macOS menu with app submenu and edit submenu (copy/paste)");

                    // Handle menu events
                    let app_handle_for_menu = app.handle().clone();
                    let check_updates_item_for_menu = check_updates_item.clone();
                    app.on_menu_event(move |_window, event| {
                        // Menu event handler receives events for all menu items
                        log::info!("Menu event received: {:?}", event.id());

                        // Check for our menu ID - works the same on all platforms now
                        if event.id().0 == check_updates_id {
                            log::info!(
                                "Check for updates menu item clicked - triggering a user-requested update check..."
                            );

                            // Clone the app handle to use in the async task
                            let app_handle_clone = app_handle_for_menu.clone();
                            let check_updates_item_clone = check_updates_item_for_menu.clone();

                            if let Err(e) =
                                check_updates_item_for_menu.set_text("Checking for Updates…")
                            {
                                log::error!("Failed to update check menu item text: {e}");
                            }
                            if let Err(e) = check_updates_item_for_menu.set_enabled(false) {
                                log::error!("Failed to disable check menu item: {e}");
                            }

                            // Spawn a new async task to check for updates (non-blocking)
                            tauri::async_runtime::spawn(async move {
                                match check_for_updates(
                                    app_handle_clone.clone(),
                                    UpdateCheckTrigger::Manual,
                                )
                                .await
                                {
                                    Ok(result) => {
                                        log::info!("Update check completed successfully");
                                        emit_manual_update_check_result(
                                            &app_handle_clone,
                                            &result,
                                        );
                                    }
                                    Err(e) => {
                                        log::error!("Update check failed: {e}");
                                        emit_manual_update_check_error(&app_handle_clone);
                                    }
                                }

                                if let Err(e) =
                                    check_updates_item_clone.set_text("Check for Updates")
                                {
                                    log::error!("Failed to restore check menu item text: {e}");
                                }
                                if let Err(e) = check_updates_item_clone.set_enabled(true) {
                                    log::error!("Failed to re-enable check menu item: {e}");
                                }
                            });
                        }
                    });
                }
            }

            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build());

    // Mobile (iOS and Android) configuration
    #[cfg(not(desktop))]
    let mut builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_fs::init());

    // Only add the Apple Sign In plugin on iOS
    #[cfg(all(not(desktop), target_os = "ios"))]
    {
        builder = builder.plugin(tauri_plugin_sign_in_with_apple::init());
    }

    // Android-specific configuration
    #[cfg(all(not(desktop), target_os = "android"))]
    let app = builder
        .invoke_handler(tauri::generate_handler![
            pdf_extractor::extract_document_content,
        ])
        .setup(|app| {
            // Set up the deep link handler for mobile
            let app_handle = app.handle().clone();

            // Register deep link handler - note that iOS does not support runtime registration
            // but the handler for incoming URLs still works
            app.deep_link().on_open_url(move |event| {
                if let Some(url) = event.urls().first() {
                    handle_deep_link_event(url.as_ref(), &app_handle);
                }
            });

            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build());

    // iOS-specific configuration
    #[cfg(all(not(desktop), target_os = "ios"))]
    let app = builder
        .invoke_handler(tauri::generate_handler![
            pdf_extractor::extract_document_content,
        ])
        .setup(|app| {
            legacy_tts_cleanup::schedule(app.handle());

            // Set up the deep link handler for mobile
            let app_handle = app.handle().clone();

            // Register deep link handler - note that iOS does not support runtime registration
            // but the handler for incoming URLs still works
            app.deep_link().on_open_url(move |event| {
                if let Some(url) = event.urls().first() {
                    handle_deep_link_event(url.as_ref(), &app_handle);
                }
            });

            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build());

    #[cfg(desktop)]
    app.build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(handle_desktop_run_event);

    #[cfg(not(desktop))]
    app.run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(desktop)]
pub fn run_acp_connector() -> Result<(), String> {
    agent_acp::run_acp_connector()
}

// Create a global variable to track if an update is already prepared and notified
#[cfg(desktop)]
use once_cell::sync::Lazy;
#[cfg(desktop)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(desktop)]
use std::sync::Mutex;

#[cfg(desktop)]
static UPDATE_DOWNLOADED: AtomicBool = AtomicBool::new(false);
#[cfg(desktop)]
static AGENT_EXIT_CLEANUP_STARTED: AtomicBool = AtomicBool::new(false);
#[cfg(desktop)]
static AGENT_EXIT_CLEANUP_COMPLETE: AtomicBool = AtomicBool::new(false);
#[cfg(desktop)]
static CURRENT_VERSION: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
#[cfg(desktop)]
static FAILED_UPDATE_VERSION: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
#[cfg(desktop)]
static PENDING_UPDATE_FAILURE: Lazy<Mutex<Option<PendingUpdateFailure>>> =
    Lazy::new(|| Mutex::new(None));
#[cfg(desktop)]
static UPDATE_CHECK_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

/// A downloaded and signature-verified update that waits for the user to
/// approve the install.
#[cfg(desktop)]
struct PendingUpdate {
    update: tauri_plugin_updater::Update,
    bytes: Vec<u8>,
}

#[cfg(desktop)]
static PENDING_UPDATE: Lazy<Mutex<Option<PendingUpdate>>> = Lazy::new(|| Mutex::new(None));

#[cfg(desktop)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateCheckTrigger {
    Automatic,
    Manual,
}

#[cfg(desktop)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateInstallOrigin {
    Automatic,
    ManualCheck,
    UserApprovedPending,
}

#[cfg(desktop)]
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdateFailureOrigin {
    Automatic,
    Manual,
}

#[cfg(desktop)]
#[derive(Clone, serde::Serialize)]
struct PendingUpdateFailure {
    version: String,
    origin: UpdateFailureOrigin,
}

#[cfg(desktop)]
fn install_failure_retryability(origin: UpdateInstallOrigin) -> Option<bool> {
    match origin {
        UpdateInstallOrigin::Automatic => Some(false),
        UpdateInstallOrigin::ManualCheck => None,
        UpdateInstallOrigin::UserApprovedPending => Some(true),
    }
}

#[cfg(desktop)]
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum UpdateCheckResult {
    AutomaticUpdatesDisabled,
    UpToDate,
    ReadyToRestart { version: String },
    ReadyToInstall { version: String },
    InstallFailed { version: String },
}

/// Linux deb/rpm installs run `pkexec`, which opens a system password dialog.
/// That dialog must not appear without the user asking for it, so those
/// installs wait for approval. AppImage, macOS and Windows install silently.
#[cfg(desktop)]
fn install_needs_user_approval() -> bool {
    use tauri::utils::{config::BundleType, platform::bundle_type};

    cfg!(target_os = "linux") && matches!(bundle_type(), Some(BundleType::Deb | BundleType::Rpm))
}

#[cfg(desktop)]
fn pending_update_version() -> Option<String> {
    match PENDING_UPDATE.lock() {
        Ok(guard) => guard.as_ref().map(|pending| pending.update.version.clone()),
        Err(e) => {
            log::error!("Failed to lock PENDING_UPDATE mutex: {e}");
            None
        }
    }
}

#[cfg(desktop)]
fn downloaded_update_version() -> Option<String> {
    if !UPDATE_DOWNLOADED.load(Ordering::SeqCst) {
        return None;
    }

    match CURRENT_VERSION.lock() {
        Ok(version) if !version.is_empty() => Some(version.clone()),
        Ok(_) => None,
        Err(e) => {
            log::error!("Failed to lock CURRENT_VERSION mutex: {e}");
            None
        }
    }
}

#[cfg(desktop)]
fn prepared_update_result(
    downloaded_version: Option<String>,
    pending_install_version: Option<String>,
) -> Option<UpdateCheckResult> {
    downloaded_version
        .map(|version| UpdateCheckResult::ReadyToRestart { version })
        .or_else(|| {
            pending_install_version.map(|version| UpdateCheckResult::ReadyToInstall { version })
        })
}

#[cfg(desktop)]
fn emit_update_available(app_handle: &tauri::AppHandle, version: &str) {
    #[derive(Clone, serde::Serialize)]
    struct UpdateAvailablePayload<'a> {
        version: &'a str,
    }

    if let Err(e) = app_handle.emit("update-available", UpdateAvailablePayload { version }) {
        log::error!("Failed to emit update-available event: {e}");
    } else {
        log::info!("Emitted update-available event for version {version}");
    }
}

#[cfg(desktop)]
fn emit_update_ready(app_handle: &tauri::AppHandle, version: &str) {
    #[derive(Clone, serde::Serialize)]
    struct UpdateReadyPayload<'a> {
        version: &'a str,
    }

    if let Err(e) = app_handle.emit("update-ready", UpdateReadyPayload { version }) {
        log::error!("Failed to emit update-ready event: {e}");
    } else {
        log::info!("Emitted update-ready event for version {version}");
    }
}

/// The macOS menu does not have an inline result surface like Settings. Emit
/// only outcomes that are not already covered by the existing update events.
#[cfg(desktop)]
fn emit_manual_update_check_result(app_handle: &tauri::AppHandle, result: &UpdateCheckResult) {
    if result == &UpdateCheckResult::UpToDate {
        if let Err(e) = app_handle.emit("manual-update-check-up-to-date", ()) {
            log::error!("Failed to emit manual-update-check-up-to-date event: {e}");
        }
    }
}

#[cfg(desktop)]
fn emit_manual_update_install_failed(app_handle: &tauri::AppHandle, version: &str) {
    #[derive(Clone, serde::Serialize)]
    struct ManualInstallFailedPayload<'a> {
        version: &'a str,
    }

    if let Err(e) = app_handle.emit(
        "manual-update-install-failed",
        ManualInstallFailedPayload { version },
    ) {
        log::error!("Failed to emit manual-update-install-failed event: {e}");
    }
}

#[cfg(desktop)]
fn emit_manual_update_check_error(app_handle: &tauri::AppHandle) {
    if let Err(e) = app_handle.emit("manual-update-check-failed", ()) {
        log::error!("Failed to emit manual-update-check-failed event: {e}");
    }
}

#[cfg(desktop)]
async fn automatic_updates_enabled(app_handle: &tauri::AppHandle) -> Result<bool, String> {
    updater_preferences::load(app_handle)
        .await
        .map(|preferences| preferences.automatic_updates)
        .map_err(|error| format!("Failed to load updater preferences: {error}"))
}

#[cfg(desktop)]
async fn automatic_update_may_continue(
    app_handle: &tauri::AppHandle,
    trigger: UpdateCheckTrigger,
) -> Result<bool, String> {
    if trigger == UpdateCheckTrigger::Manual {
        return Ok(true);
    }

    automatic_updates_enabled(app_handle)
        .await
        .map(|enabled| update_trigger_allows(trigger, enabled))
}

#[cfg(desktop)]
fn update_trigger_allows(trigger: UpdateCheckTrigger, automatic_updates_enabled: bool) -> bool {
    trigger == UpdateCheckTrigger::Manual || automatic_updates_enabled
}

/// Check for updates. Automatic checks honor the persisted preference at each
/// network/install boundary; a user-requested check always proceeds.
#[cfg(desktop)]
async fn check_for_updates(
    app_handle: tauri::AppHandle,
    trigger: UpdateCheckTrigger,
) -> Result<UpdateCheckResult, String> {
    use tauri_plugin_updater::UpdaterExt;

    let _check_guard = UPDATE_CHECK_LOCK.lock().await;

    if !automatic_update_may_continue(&app_handle, trigger).await? {
        log::info!("Skipping automatic update check because automatic updates are disabled");
        return Ok(UpdateCheckResult::AutomaticUpdatesDisabled);
    }

    if trigger == UpdateCheckTrigger::Manual {
        // A manual check should resurface already prepared work instead of
        // discarding it and downloading or installing the same version again.
        if let Some(prepared) =
            prepared_update_result(downloaded_update_version(), pending_update_version())
        {
            match &prepared {
                UpdateCheckResult::ReadyToRestart { version } => {
                    emit_update_ready(&app_handle, version)
                }
                UpdateCheckResult::ReadyToInstall { version } => {
                    emit_update_available(&app_handle, version)
                }
                _ => unreachable!("prepared update result must be actionable"),
            }
            return Ok(prepared);
        }

        // An explicit check is also an explicit retry of a version whose
        // automatic install failed earlier in this session.
        match FAILED_UPDATE_VERSION.lock() {
            Ok(mut version) => version.clear(),
            Err(e) => {
                log::error!("Failed to lock FAILED_UPDATE_VERSION mutex when clearing: {e}")
            }
        }
        match PENDING_UPDATE_FAILURE.lock() {
            Ok(mut failure) => *failure = None,
            Err(e) => {
                log::error!("Failed to lock PENDING_UPDATE_FAILURE mutex when clearing: {e}")
            }
        }
        log::info!("Automatic install failure suppression cleared for user-requested retry");
    }

    log::info!("Checking for updates...");

    // Get the updater
    let updater = match app_handle.updater() {
        Ok(u) => u,
        Err(e) => {
            log::error!("Failed to get updater: {e}");
            return Err(format!("Failed to get updater: {e}"));
        }
    };

    // Check for updates
    match updater.check().await {
        Ok(Some(update)) => {
            // Check if we've already downloaded this specific version
            let current_downloaded_version = match CURRENT_VERSION.lock() {
                Ok(guard) => guard.clone(),
                Err(e) => {
                    log::error!("Failed to lock CURRENT_VERSION mutex: {e}");
                    String::new() // Use empty string if lock fails
                }
            };

            if UPDATE_DOWNLOADED.load(Ordering::SeqCst)
                && current_downloaded_version == update.version
            {
                log::info!(
                    "Update to version {} already downloaded, skipping redundant download",
                    update.version
                );
                return Ok(UpdateCheckResult::ReadyToRestart {
                    version: update.version,
                });
            }

            if pending_update_version().as_deref() == Some(update.version.as_str()) {
                log::info!(
                    "Update to version {} is downloaded and waiting for user approval, skipping redundant download",
                    update.version
                );
                return Ok(UpdateCheckResult::ReadyToInstall {
                    version: update.version,
                });
            }

            let failed_update_version = match FAILED_UPDATE_VERSION.lock() {
                Ok(guard) => guard.clone(),
                Err(e) => {
                    log::error!("Failed to lock FAILED_UPDATE_VERSION mutex: {e}");
                    String::new()
                }
            };

            if failed_update_version == update.version {
                log::info!(
                    "Update to version {} already failed to install in this session, skipping automatic retry",
                    update.version
                );
                return Ok(UpdateCheckResult::InstallFailed {
                    version: update.version,
                });
            }

            if !automatic_update_may_continue(&app_handle, trigger).await? {
                log::info!(
                    "Automatic updates were disabled while checking; not downloading version {}",
                    update.version
                );
                return Ok(UpdateCheckResult::AutomaticUpdatesDisabled);
            }

            log::info!("Update available, attempting to download and install");

            // Download the update
            let mut downloaded = 0_u64;
            let progress_fn = move |chunk_length: usize, total: Option<u64>| {
                downloaded = downloaded.saturating_add(chunk_length as u64);
                if let Some(total) = total {
                    log::info!("Download progress: {downloaded}/{total} bytes");
                } else {
                    log::info!("Download progress: {downloaded} bytes");
                }
            };

            let download_complete = || {
                log::info!("Download stream received; verifying signature");
            };

            match update.download(progress_fn, download_complete).await {
                Ok(bytes) => {
                    log::info!("Update downloaded and signature verified");

                    // Keep the preference save lock through the final automatic
                    // action. If disabling wins the lock, this action stops; if
                    // an already-authorized action wins, save(false) does not
                    // report success until the action has finished.
                    let _automatic_action_guard = if trigger == UpdateCheckTrigger::Automatic {
                        let (preferences, guard) = updater_preferences::load_and_lock(&app_handle)
                            .await
                            .map_err(|error| {
                                format!("Failed to load updater preferences: {error}")
                            })?;
                        if !preferences.automatic_updates {
                            log::info!(
                                "Automatic updates were disabled while downloading; not installing version {}",
                                update.version
                            );
                            return Ok(UpdateCheckResult::AutomaticUpdatesDisabled);
                        }
                        Some(guard)
                    } else {
                        None
                    };

                    if install_needs_user_approval() {
                        let version = update.version.clone();
                        match PENDING_UPDATE.lock() {
                            Ok(mut guard) => *guard = Some(PendingUpdate { update, bytes }),
                            Err(e) => {
                                let error = format!(
                                    "Failed to lock PENDING_UPDATE mutex when storing update: {e}"
                                );
                                log::error!("{error}");
                                return Err(error);
                            }
                        }

                        emit_update_available(&app_handle, &version);

                        return Ok(UpdateCheckResult::ReadyToInstall { version });
                    }

                    let version = update.version.clone();
                    let install_origin = match trigger {
                        UpdateCheckTrigger::Automatic => UpdateInstallOrigin::Automatic,
                        UpdateCheckTrigger::Manual => UpdateInstallOrigin::ManualCheck,
                    };
                    match install_update(&app_handle, &update, &bytes, install_origin) {
                        Ok(()) => Ok(UpdateCheckResult::ReadyToRestart { version }),
                        Err(error) if trigger == UpdateCheckTrigger::Manual => {
                            log::error!(
                                "Manual update check downloaded version {version}, but installation failed: {error}"
                            );
                            emit_manual_update_install_failed(&app_handle, &version);
                            Ok(UpdateCheckResult::InstallFailed { version })
                        }
                        Err(error) => Err(error),
                    }
                }
                Err(e) => {
                    log::error!("Failed to download update: {e}");
                    Err(format!("Failed to download update: {e}"))
                }
            }
        }
        Ok(None) => {
            log::info!("No updates available");
            Ok(UpdateCheckResult::UpToDate)
        }
        Err(e) => {
            log::error!("Failed to check for updates: {e}");
            Err(format!("Failed to check for updates: {e}"))
        }
    }
}

/// Install a downloaded update and tell the frontend the result.
///
/// A staged install the user approved always reports the result, and a failure
/// (for example a cancelled password prompt) does not block the version for the
/// rest of the session. Manual-check failures are returned to their caller,
/// while automatic failures emit the global fallback notification.
#[cfg(desktop)]
fn install_update(
    app_handle: &tauri::AppHandle,
    update: &tauri_plugin_updater::Update,
    bytes: &[u8],
    origin: UpdateInstallOrigin,
) -> Result<(), String> {
    log::info!("Installing update to version {}", update.version);

    match update.install(bytes) {
        Ok(_) => {
            // Log that the update is ready
            log::info!(
                "Update installed successfully. Will be applied on next application restart."
            );

            match FAILED_UPDATE_VERSION.lock() {
                Ok(mut version) => version.clear(),
                Err(e) => {
                    log::error!("Failed to lock FAILED_UPDATE_VERSION mutex when clearing: {e}")
                }
            }
            match PENDING_UPDATE_FAILURE.lock() {
                Ok(mut failure) => *failure = None,
                Err(e) => {
                    log::error!("Failed to lock PENDING_UPDATE_FAILURE mutex when clearing: {e}")
                }
            }

            // Mark this version as downloaded to prevent redundant downloads/notifications
            match CURRENT_VERSION.lock() {
                Ok(mut version) => *version = update.version.clone(),
                Err(e) => {
                    log::error!("Failed to lock CURRENT_VERSION mutex when updating version: {e}")
                }
            }

            // The silent path shows the restart toast once per session. An
            // install the user asked for always gets feedback.
            let already_notified = UPDATE_DOWNLOADED.swap(true, Ordering::SeqCst);
            if already_notified && origin != UpdateInstallOrigin::UserApprovedPending {
                log::info!("Update notification already shown, not showing another one");
                return Ok(());
            }

            emit_update_ready(app_handle, &update.version);

            Ok(())
        }
        Err(e) => {
            let error = format!("Failed to install update: {e}");
            log::error!("{error}");

            if origin == UpdateInstallOrigin::UserApprovedPending {
                log::info!("User-approved install failed; the update stays available to retry");
            } else {
                match FAILED_UPDATE_VERSION.lock() {
                    Ok(mut version) => *version = update.version.clone(),
                    Err(lock_error) => log::error!(
                        "Failed to lock FAILED_UPDATE_VERSION mutex when recording failure: {lock_error}"
                    ),
                }

                let failure_origin = match origin {
                    UpdateInstallOrigin::Automatic => UpdateFailureOrigin::Automatic,
                    UpdateInstallOrigin::ManualCheck => UpdateFailureOrigin::Manual,
                    UpdateInstallOrigin::UserApprovedPending => unreachable!(
                        "user-approved pending failures do not enter suppression state"
                    ),
                };
                match PENDING_UPDATE_FAILURE.lock() {
                    Ok(mut failure) => {
                        *failure = Some(PendingUpdateFailure {
                            version: update.version.clone(),
                            origin: failure_origin,
                        });
                    }
                    Err(lock_error) => log::error!(
                        "Failed to lock PENDING_UPDATE_FAILURE mutex when recording failure: {lock_error}"
                    ),
                }
            }

            #[derive(Clone, serde::Serialize)]
            struct UpdateFailedPayload {
                version: String,
                retryable: bool,
            }

            if let Some(retryable) = install_failure_retryability(origin) {
                if let Err(emit_error) = app_handle.emit(
                    "update-failed",
                    UpdateFailedPayload {
                        version: update.version.clone(),
                        retryable,
                    },
                ) {
                    log::error!("Failed to emit update-failed event: {emit_error}");
                }
            }

            Err(error)
        }
    }
}

#[cfg(all(test, desktop))]
mod updater_policy_tests {
    use super::*;

    #[test]
    fn automatic_trigger_honors_the_preference_while_manual_checks_bypass_it() {
        assert!(!update_trigger_allows(UpdateCheckTrigger::Automatic, false));
        assert!(update_trigger_allows(UpdateCheckTrigger::Automatic, true));
        assert!(update_trigger_allows(UpdateCheckTrigger::Manual, false));
    }

    #[test]
    fn manual_checks_resurface_prepared_updates_before_hitting_the_network() {
        assert_eq!(
            prepared_update_result(Some("4.5.6".to_string()), Some("4.5.5".to_string())),
            Some(UpdateCheckResult::ReadyToRestart {
                version: "4.5.6".to_string()
            })
        );
        assert_eq!(
            prepared_update_result(None, Some("4.5.5".to_string())),
            Some(UpdateCheckResult::ReadyToInstall {
                version: "4.5.5".to_string()
            })
        );
        assert_eq!(prepared_update_result(None, None), None);
    }

    #[test]
    fn install_failure_feedback_matches_the_recovery_path() {
        assert_eq!(
            install_failure_retryability(UpdateInstallOrigin::Automatic),
            Some(false)
        );
        assert_eq!(
            install_failure_retryability(UpdateInstallOrigin::ManualCheck),
            None
        );
        assert_eq!(
            install_failure_retryability(UpdateInstallOrigin::UserApprovedPending),
            Some(true)
        );
    }

    #[test]
    fn pending_failure_origin_preserves_manual_recovery_copy() {
        let failure = PendingUpdateFailure {
            version: "4.5.6".to_string(),
            origin: UpdateFailureOrigin::Manual,
        };

        assert_eq!(
            serde_json::to_value(failure).unwrap(),
            serde_json::json!({
                "version": "4.5.6",
                "origin": "manual"
            })
        );
    }
}
