use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener;

mod proxy;
mod pdf_extractor;

// This handles incoming deep links
fn handle_deep_link_event(url: &str, app: &tauri::AppHandle) {
    log::info!("[Deep Link] Received: {}", url);
    // Forward the URL to the frontend
    match app.emit_to("main", "deep-link-received", url.to_string()) {
        Ok(_) => log::info!("[Deep Link] Event emitted successfully"),
        Err(e) => log::error!("[Deep Link] Failed to emit event: {}", e),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(desktop)]
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            log::info!("Single instance detected: {}, {argv:?}, {cwd}", app.package_info().name);
        }))
        .plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_fs::init())
        .manage(proxy::ProxyState::new())
        .invoke_handler(tauri::generate_handler![
            proxy::start_proxy,
            proxy::stop_proxy,
            proxy::get_proxy_status,
            proxy::load_proxy_config,
            proxy::save_proxy_settings,
            proxy::test_proxy_port,
            pdf_extractor::extract_document_content,
        ])
        .setup(|app| {
            // Initialize proxy auto-start
            {
                let app_handle_proxy = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Small delay to ensure app is fully initialized
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                    // Create a new State wrapper for the async context
                    if let Err(e) = proxy::init_proxy_on_startup_simple(app_handle_proxy).await {
                        log::error!("Failed to initialize proxy: {}", e);
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
                    handle_deep_link_event(&url.to_string(), &app_handle);
                }
            });
            // Optionally register the scheme at runtime
            #[cfg(desktop)]
            if let Err(e) = app.deep_link().register("cloud.opensecret.maple") {
                log::error!("[Deep Link] Failed to register scheme: {}", e);
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

                    // This will check for updates, download if available, and install them
                    // The update will be applied silently in the background
                    // It will take effect the next time the application is started
                    let _ = check_for_updates(app_handle.clone()).await;

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
                            let _ = check_for_updates(hourly_app_handle.clone()).await;
                        }
                    });
                });

                // Create a native menu with a "Check for Updates" option
                {
                    #[cfg(target_os = "macos")]
                    use tauri::menu::{MenuBuilder, SubmenuBuilder};

                    #[cfg(not(target_os = "macos"))]
                    use tauri::menu::MenuBuilder;

                    // Define menu item ID for "Check for Updates"
                    let check_updates_id = "check-for-updates";

                    // Get app handle for menu operations
                    let handle = app.handle();

                    // Build platform-specific menus
                    #[cfg(target_os = "macos")]
                    {
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
                        let menu = MenuBuilder::new(handle).items(&[&app_submenu, &edit_submenu]).build()?;

                        // Set as the application menu
                        app.set_menu(menu)?;

                        // Log that we're setting up the menu
                        log::info!(
                            "Setting up macOS menu with app submenu and edit submenu (copy/paste)"
                        );
                    }

                    #[cfg(not(target_os = "macos"))]
                    {
                        // For Windows/Linux, we need to include edit functionality while keeping a simpler structure
                        let menu = MenuBuilder::new(handle)
                            .about(None)
                            .text(check_updates_id, "Check for Updates")
                            .separator()
                            // Add standard edit operations
                            .undo()
                            .redo()
                            .separator()
                            .cut()
                            .copy()
                            .paste()
                            .select_all()
                            .separator()
                            .quit()
                            .build()?;

                        app.set_menu(menu)?;

                        log::info!("Setting up Windows/Linux menu with About, Check for Updates, and Edit options");
                    }

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

                            // Clear the dismissal flags to ensure the user always gets prompted
                            // even if they previously dismissed an update
                            UPDATE_DOWNLOADED.store(false, Ordering::SeqCst);
                            {
                                match CURRENT_VERSION.lock() {
                                    Ok(mut version) => version.clear(),
                                    Err(e) => log::error!("Failed to lock CURRENT_VERSION mutex when clearing: {}", e)
                                }
                            }
                            log::info!("Dismissal flags cleared - user will be prompted for any available updates");

                            // Clone the app handle to use in the async task
                            let app_handle_clone = app_handle_for_menu.clone();

                            // Spawn a new async task to check for updates (non-blocking)
                            tauri::async_runtime::spawn(async move {
                                match check_for_updates(app_handle_clone).await {
                                    Ok(_) => log::info!("Update check completed successfully"),
                                    Err(e) => log::error!("Update check failed: {}", e),
                                }
                            });
                        }
                    });
                }
            }

            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build());

    #[cfg(not(desktop))]
    let mut builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_fs::init());

    // Only add the Apple Sign In plugin on iOS
    #[cfg(all(not(desktop), target_os = "ios"))]
    {
        builder = builder.plugin(tauri_plugin_sign_in_with_apple::init());
    }

    #[cfg(not(desktop))]
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
                    handle_deep_link_event(&url.to_string(), &app_handle);
                }
            });

            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build());

    app.run(tauri::generate_context!())
        .expect("error while running tauri application");
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
static CURRENT_VERSION: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

/// Check for updates silently in the background
#[cfg(desktop)]
async fn check_for_updates(app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    log::info!("Checking for updates...");

    // Get the updater
    let updater = match app_handle.updater() {
        Ok(u) => u,
        Err(e) => {
            log::error!("Failed to get updater: {}", e);
            return Err(format!("Failed to get updater: {}", e));
        }
    };

    // Check for updates
    match updater.check().await {
        Ok(Some(update)) => {
            // Check if we've already downloaded this specific version
            let current_downloaded_version = match CURRENT_VERSION.lock() {
                Ok(guard) => guard.clone(),
                Err(e) => {
                    log::error!("Failed to lock CURRENT_VERSION mutex: {}", e);
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

            log::info!("Update available, attempting to download and install");

            // Download the update
            let progress_fn = |downloaded: usize, total: Option<u64>| {
                if let Some(total) = total {
                    log::info!("Download progress: {}/{} bytes", downloaded, total);
                } else {
                    log::info!("Download progress: {} bytes", downloaded);
                }
            };

            let download_complete = || {
                log::info!("Download complete!");
            };

            match update.download(progress_fn, download_complete).await {
                Ok(bytes) => {
                    log::info!("Update downloaded successfully");
                    log::info!("Installing update to version {}", update.version);

                    // Try to install the update immediately
                    match update.install(bytes) {
                        Ok(_) => {
                            // Log that the update is ready
                            log::info!("Update installed successfully. Will be applied on next application restart.");

                            // Mark this version as downloaded to prevent redundant downloads/notifications
                            {
                                match CURRENT_VERSION.lock() {
                                    Ok(mut version) => *version = update.version.clone(),
                                    Err(e) => log::error!("Failed to lock CURRENT_VERSION mutex when updating version: {}", e)
                                }
                            }

                            // If we've already shown a notification, don't show another one
                            if UPDATE_DOWNLOADED.load(Ordering::SeqCst) {
                                log::info!(
                                    "Update notification already shown, not showing another one"
                                );
                                return Ok(());
                            }

                            // Mark as downloaded to prevent further update dialogs
                            UPDATE_DOWNLOADED.store(true, Ordering::SeqCst);

                            // Show a dialog prompting the user to restart
                            let message = format!(
                                "An update to version {} has been downloaded and is ready to install. \
                                Would you like to restart the application now to apply the update?", 
                                update.version
                            );

                            use tauri_plugin_dialog::{
                                DialogExt, MessageDialogButtons, MessageDialogKind,
                            };
                            let dialog = app_handle.dialog();

                            // Show a friendly info dialog with Yes/No buttons
                            dialog
                                .message(message)
                                .title("Maple Update")
                                .kind(MessageDialogKind::Info) // Use info icon for a friendlier look
                                .buttons(MessageDialogButtons::OkCancelCustom(
                                    "Yes".to_string(),
                                    "No".to_string(),
                                ))
                                .show(move |should_restart| {
                                    if should_restart {
                                        log::info!("User chose to restart now for update");

                                        // Restart the application instead of just exiting
                                        // This will automatically apply the update
                                        app_handle.restart();
                                    } else {
                                        log::info!("User chose to postpone update restart");
                                    }
                                });
                        }
                        Err(e) => {
                            log::error!("Failed to install update: {}", e);
                        }
                    }

                    Ok(())
                }
                Err(e) => {
                    log::error!("Failed to download update: {}", e);
                    Err(format!("Failed to download update: {}", e))
                }
            }
        }
        Ok(None) => {
            log::info!("No updates available");
            Ok(())
        }
        Err(e) => {
            log::error!("Failed to check for updates: {}", e);
            Err(format!("Failed to check for updates: {}", e))
        }
    }
}
