//! Native service bridge. All blocking plugin calls run outside the Android UI thread.
use crate::error::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
static APP: OnceLock<AppHandle> = OnceLock::new();
pub fn initialize(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(error) = crate::core::refresh(&app).await {
                log::debug!("Android service status: {error}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
}
pub fn call<T: DeserializeOwned>(method: &str, payload: impl Serialize) -> Result<T> {
    let app = APP
        .get()
        .ok_or_else(|| Error::msg("Android service bridge is not ready"))?;
    app.state::<tauri_plugin_android_vpn::Vpn<tauri::Wry>>()
        .call(method, payload)
        .map_err(Error::msg)
}
pub async fn invoke<T: DeserializeOwned + Send + 'static>(
    method: &'static str,
    payload: serde_json::Value,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || call(method, payload))
        .await
        .map_err(|e| Error::msg(e.to_string()))?
}
