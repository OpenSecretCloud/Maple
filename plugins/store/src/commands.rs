use tauri::{AppHandle, command, Runtime};

use crate::Result;
use crate::StoreExt;

#[command]
pub(crate) async fn get_region<R: Runtime>(
    app: AppHandle<R>,
) -> Result<String> {
    app.store().get_region()
}