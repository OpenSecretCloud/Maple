use serde::de::DeserializeOwned;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_store);

// Initialize the plugin
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<Store<R>> {
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_store)?;

    #[cfg(not(target_os = "ios"))]
    let handle = {
        // Dummy handle for non-iOS platforms
        let handle: PluginHandle<R> = unsafe { std::mem::zeroed() };
        handle
    };

    Ok(Store(handle))
}

/// Access to the store APIs.
pub struct Store<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Store<R> {
    pub fn get_region(&self) -> crate::Result<String> {
        self.0
            .run_mobile_plugin("getRegion", ())
            .map_err(Into::into)
    }
}

