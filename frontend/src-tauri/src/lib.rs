#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(desktop)]
    let app = tauri::Builder::default()
        .setup(|app| {
            // Set up logging
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;

            // Create the application menu with update options
            #[cfg(desktop)]
            {
                // Set up a simple updater handler
                log::info!("Setting up automatic updater");

                // Setup update check on startup with delay
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Wait for app to fully initialize (use async sleep to not block the thread)
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    log::info!("Performing automatic update check on startup");

                    // This will check for updates, download if available, and install them
                    // The update will be applied silently in the background
                    // It will take effect the next time the application is started
                    let _ = check_for_updates(app_handle).await;
                });

                // We'll add Tauri menu integration when it's more stable
            }

            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build());

    #[cfg(not(desktop))]
    let app = tauri::Builder::default()
        .setup(|app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;
            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build());

    app.run(tauri::generate_context!())
        .expect("error while running tauri application");
}

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
