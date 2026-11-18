use crate::error::{Error, Result};
use serde::Serialize;
use tauri::AppHandle;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current_version: Option<String>,
    current_path: Option<String>,
    managed: bool,
    latest_version: String,
    asset_name: String,
    update_available: bool,
}
pub async fn check(_: AppHandle) -> Result<UpdateInfo> {
    Err(Error::msg(
        "Android core updates are included in application updates.",
    ))
}
pub async fn install(_: AppHandle) -> Result<String> {
    Err(Error::msg(
        "Update the Android application to update its bundled core.",
    ))
}
