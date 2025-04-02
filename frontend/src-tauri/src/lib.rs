#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      app.handle().plugin(
        tauri_plugin_log::Builder::default()
          .level(log::LevelFilter::Info)
          .build(),
      )?;
      Ok(())
    })
    .plugin(tauri_plugin_updater::Builder::new().build())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
