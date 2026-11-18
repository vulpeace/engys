//! Android owns the core in its foreground service, independently of the activity.
pub use crate::core_config::*;
use crate::{AppState, android, error::Result, settings::Settings, util::write_atomic};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager};
pub const RUNTIME_CONFIG: &str = "runtime.yaml";
pub const EVENT_STATUS: &str = "core-status";
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub controller: String,
    pub secret: String,
    pub started_at: Option<u64>,
    pub profile_id: Option<String>,
    pub binary: Option<String>,
    pub last_error: Option<String>,
}
#[derive(Default)]
pub struct CoreManager {
    pub current: CoreStatus,
    pub logs: Arc<Mutex<VecDeque<String>>>,
}
impl CoreManager {
    pub fn status(&self, settings: &Settings) -> CoreStatus {
        if self.current.running {
            self.current.clone()
        } else {
            CoreStatus {
                controller: controller_url(settings.controller_port),
                secret: settings.secret.clone(),
                ..self.current.clone()
            }
        }
    }
    pub fn pid(&self) -> Option<u32> {
        self.current.pid
    }
    pub fn is_running(&self) -> bool {
        self.current.running
    }
}
pub fn controller_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}
pub fn resolve_binary(_: &Settings, _: &Path) -> Result<PathBuf> {
    Ok("bundled-mihomo".into())
}
pub fn core_version(_: &Path) -> Result<String> {
    android::call("version", json!({}))
}
pub async fn refresh(app: &AppHandle) -> Result<CoreStatus> {
    let status: CoreStatus = android::invoke("status", json!({})).await?;
    let state = app.state::<AppState>();
    let changed = {
        let mut core = state.core.lock().unwrap();
        let changed = core.current.controller != status.controller
            || core.current.secret != status.secret
            || core.current.running != status.running
            || core.current.started_at != status.started_at
            || core.current.last_error != status.last_error;
        core.current = status.clone();
        changed
    };
    if changed {
        let _ = app.emit(EVENT_STATUS, &status);
    }
    Ok(status)
}
pub async fn prepare(app: &AppHandle) -> Result<(PathBuf, String, Settings)> {
    let state = app.state::<AppState>();
    let (id, content) = {
        let store = state.profiles.lock().unwrap();
        let p = store
            .active()
            .ok_or_else(|| crate::error::Error::msg("Select a profile first."))?;
        (p.id.clone(), store.read_content(&p.id)?)
    };
    let (runtime, settings) = preview_runtime_config(&state, &id, &content)?;
    android::invoke::<serde_json::Value>(
        "validate",
        json!({"yaml": runtime, "home": state.paths.core_dir}),
    )
    .await?;
    write_atomic(
        &state.paths.core_dir.join(RUNTIME_CONFIG),
        runtime.as_bytes(),
    )?;
    Ok(("bundled-mihomo".into(), id, settings))
}
pub async fn validate_config_file(_: &Path, home: &Path, path: &Path) -> Result<()> {
    let yaml = std::fs::read_to_string(path)?;
    android::invoke::<serde_json::Value>("validate", json!({"yaml": yaml, "home": home})).await?;
    Ok(())
}
pub async fn apply_config(_: &CoreStatus, yaml: &str) -> Result<()> {
    android::invoke::<serde_json::Value>("configure", json!({"yaml": yaml})).await?;
    Ok(())
}
pub async fn start(app: AppHandle) -> Result<CoreStatus> {
    let state = app.state::<AppState>();
    let _guard = state.core_operation.lock().await;
    start_locked(app.clone()).await
}
pub async fn start_locked(app: AppHandle) -> Result<CoreStatus> {
    let (_, id, _) = prepare(&app).await?;
    let state = app.state::<AppState>();
    android::invoke::<serde_json::Value>(
        "start",
        json!({
            "yaml": std::fs::read_to_string(state.paths.core_dir.join(RUNTIME_CONFIG))?,
            "home": state.paths.core_dir, "profileId": id,
        }),
    )
    .await?;
    refresh(&app).await
}
pub async fn stop(app: AppHandle) -> Result<CoreStatus> {
    let state = app.state::<AppState>();
    let _guard = state.core_operation.lock().await;
    stop_locked(app.clone(), true).await
}
pub async fn stop_locked(app: AppHandle, _: bool) -> Result<CoreStatus> {
    android::invoke::<serde_json::Value>("stop", json!({})).await?;
    refresh(&app).await
}
pub async fn reload(app: AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let _guard = state.core_operation.lock().await;
    reload_locked(app.clone()).await
}
pub async fn reload_locked(app: AppHandle) -> Result<()> {
    if !refresh(&app).await?.running {
        return Ok(());
    }
    let (_, id, _) = prepare(&app).await?;
    let state = app.state::<AppState>();
    let yaml = std::fs::read_to_string(state.paths.core_dir.join(RUNTIME_CONFIG))?;
    let status = state.core.lock().unwrap().current.clone();
    apply_config(&status, &yaml).await?;
    android::invoke::<serde_json::Value>("setProfile", json!({"profileId": id})).await?;
    refresh(&app).await?;
    Ok(())
}
pub fn notify_reloaded(app: &AppHandle, profile_id: &str) {
    app.state::<AppState>()
        .core
        .lock()
        .unwrap()
        .current
        .profile_id = Some(profile_id.into());
    let handle = app.clone();
    let id = profile_id.to_owned();
    tauri::async_runtime::spawn(async move {
        let _ = android::invoke::<serde_json::Value>("setProfile", json!({"profileId": id})).await;
        let _ = refresh(&handle).await;
    });
}
