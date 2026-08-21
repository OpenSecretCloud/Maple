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
fn get_pending_update_failure() -> Result<Option<String>, String> {
    let version = FAILED_UPDATE_VERSION
        .lock()
        .map_err(|e| format!("Failed to lock FAILED_UPDATE_VERSION mutex: {e}"))?
        .clone();

    Ok((!version.is_empty()).then_some(version))
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
        let result = install_update(&app_handle, &pending.update, &pending.bytes, true);
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
                // Set up a simple updater handler
                log::info!("Setting up automatic updater");

                // Setup update check on startup with delay and hourly checks
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Wait for app to fully initialize (use async sleep to not block the thread)
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    log::info!("Performing automatic update check on startup");

                    // This checks for updates, downloads the matching platform bundle,
                    // and invokes its installer. The update takes effect after restart.
                    if let Err(e) = check_for_updates(app_handle.clone(), false).await {
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
                            log::info!("Performing scheduled hourly update check");

                            // Check for updates
                            let _ = check_for_updates(hourly_app_handle.clone(), false).await;
                        }
                    });
                });

                // Create the application menu (macOS only).
                //
                // On Windows/Linux this menu renders as an in-window bar at the top of
                // the window. Its edit items (undo/redo/cut/copy/paste/select-all) are
                // handled natively by the webview regardless of the menu, About lives in
                // the in-app account menu, and updates are applied by the automatic check
                // (startup + hourly, above). The bar adds only clutter, so we omit it
                // entirely on those platforms for a cleaner window.
                #[cfg(target_os = "macos")]
                {
                    use tauri::menu::{MenuBuilder, SubmenuBuilder};

                    // Define menu item ID for "Check for Updates"
                    let check_updates_id = "check-for-updates";

                    // Get app handle for menu operations
                    let handle = app.handle();

                    // For macOS, we need to create a proper submenu structure
                    // First create the app submenu (first submenu becomes the application menu)
                    let app_submenu = SubmenuBuilder::new(handle, &app.package_info().name)
                        // Add about menu item (standard macOS menu item)
                        .about(None)
                        // Add our update checker to the app menu
                        .text(check_updates_id, "Check for Updates")
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
                    app.on_menu_event(move |_window, event| {
                        // Menu event handler receives events for all menu items
                        log::info!("Menu event received: {:?}", event.id());

                        // Check for our menu ID - works the same on all platforms now
                        if event.id().0 == check_updates_id {
                            log::info!(
                                "Check for updates menu item clicked - clearing dismissal flags and triggering update check..."
                            );

                            // Clone the app handle to use in the async task
                            let app_handle_clone = app_handle_for_menu.clone();

                            // Spawn a new async task to check for updates (non-blocking)
                            tauri::async_runtime::spawn(async move {
                                match check_for_updates(app_handle_clone, true).await {
                                    Ok(_) => log::info!("Update check completed successfully"),
                                    Err(e) => log::error!("Update check failed: {e}"),
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
fn clear_pending_update() {
    match PENDING_UPDATE.lock() {
        Ok(mut guard) => *guard = None,
        Err(e) => log::error!("Failed to lock PENDING_UPDATE mutex when clearing: {e}"),
    }
}

/// Check for updates silently in the background
#[cfg(desktop)]
async fn check_for_updates(app_handle: tauri::AppHandle, force_retry: bool) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let _check_guard = UPDATE_CHECK_LOCK.lock().await;

    if force_retry {
        UPDATE_DOWNLOADED.store(false, Ordering::SeqCst);
        match CURRENT_VERSION.lock() {
            Ok(mut version) => version.clear(),
            Err(e) => log::error!("Failed to lock CURRENT_VERSION mutex when clearing: {e}"),
        }
        match FAILED_UPDATE_VERSION.lock() {
            Ok(mut version) => version.clear(),
            Err(e) => {
                log::error!("Failed to lock FAILED_UPDATE_VERSION mutex when clearing: {e}")
            }
        }
        clear_pending_update();
        log::info!("Update state cleared for user-requested retry");
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
                return Ok(());
            }

            if pending_update_version().as_deref() == Some(update.version.as_str()) {
                log::info!(
                    "Update to version {} is downloaded and waiting for user approval, skipping redundant download",
                    update.version
                );
                return Ok(());
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
                return Ok(());
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

                        #[derive(Clone, serde::Serialize)]
                        struct UpdateAvailablePayload {
                            version: String,
                        }

                        if let Err(e) = app_handle.emit(
                            "update-available",
                            UpdateAvailablePayload {
                                version: version.clone(),
                            },
                        ) {
                            log::error!("Failed to emit update-available event: {e}");
                        } else {
                            log::info!("Emitted update-available event for version {version}");
                        }

                        return Ok(());
                    }

                    install_update(&app_handle, &update, &bytes, false)
                }
                Err(e) => {
                    log::error!("Failed to download update: {e}");
                    Err(format!("Failed to download update: {e}"))
                }
            }
        }
        Ok(None) => {
            log::info!("No updates available");
            Ok(())
        }
        Err(e) => {
            log::error!("Failed to check for updates: {e}");
            Err(format!("Failed to check for updates: {e}"))
        }
    }
}

/// Install a downloaded update and tell the frontend the result.
///
/// `user_approved` is true when the user clicked "Install Now". That path
/// always reports the result, and a failure (for example a cancelled password
/// prompt) does not block the version for the rest of the session.
#[cfg(desktop)]
fn install_update(
    app_handle: &tauri::AppHandle,
    update: &tauri_plugin_updater::Update,
    bytes: &[u8],
    user_approved: bool,
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
            if already_notified && !user_approved {
                log::info!("Update notification already shown, not showing another one");
                return Ok(());
            }

            // Emit event to frontend for toast notification
            #[derive(Clone, serde::Serialize)]
            struct UpdateReadyPayload {
                version: String,
            }

            if let Err(e) = app_handle.emit(
                "update-ready",
                UpdateReadyPayload {
                    version: update.version.clone(),
                },
            ) {
                log::error!("Failed to emit update-ready event: {e}");
            } else {
                log::info!("Emitted update-ready event for version {}", update.version);
            }

            Ok(())
        }
        Err(e) => {
            let error = format!("Failed to install update: {e}");
            log::error!("{error}");

            if user_approved {
                log::info!("User-approved install failed; the update stays available to retry");
            } else {
                match FAILED_UPDATE_VERSION.lock() {
                    Ok(mut version) => *version = update.version.clone(),
                    Err(lock_error) => log::error!(
                        "Failed to lock FAILED_UPDATE_VERSION mutex when recording failure: {lock_error}"
                    ),
                }
            }

            #[derive(Clone, serde::Serialize)]
            struct UpdateFailedPayload {
                version: String,
                retryable: bool,
            }

            if let Err(emit_error) = app_handle.emit(
                "update-failed",
                UpdateFailedPayload {
                    version: update.version.clone(),
                    retryable: user_approved,
                },
            ) {
                log::error!("Failed to emit update-failed event: {emit_error}");
            }

            Err(error)
        }
    }
}
