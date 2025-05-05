use tauri::{
  plugin::{Builder, TauriPlugin},
  Manager, Runtime,
};

#[cfg(not(feature = "mobile"))]
mod desktop;
#[cfg(feature = "mobile")]
mod mobile;

mod commands;
mod error;

pub use error::{Error, Result};

#[cfg(not(feature = "mobile"))]
use desktop::Store;
#[cfg(feature = "mobile")]
use mobile::Store;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to access the store APIs.
pub trait StoreExt<R: Runtime> {
  fn store(&self) -> &Store<R>;
}

impl<R: Runtime, T: Manager<R>> crate::StoreExt<R> for T {
  fn store(&self) -> &Store<R> {
    self.state::<Store<R>>().inner()
  }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("store")
    .invoke_handler(tauri::generate_handler![commands::get_region])
    .setup(|app, api| {
      #[cfg(feature = "mobile")]
      let store = mobile::init(app, api)?;
      #[cfg(not(feature = "mobile"))]
      let store = desktop::init(app, api)?;
      app.manage(store);
      Ok(())
    })
    .build()
}