use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

// Initialize the plugin for desktop
pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  _api: PluginApi<R, C>,
) -> crate::Result<Store<R>> {
  Ok(Store(app.clone()))
}

/// Desktop implementation of the Store plugin (does nothing)
pub struct Store<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Store<R> {
  pub fn get_region(&self) -> crate::Result<String> {
    Ok(String::from("UNKNOWN"))
  }
}