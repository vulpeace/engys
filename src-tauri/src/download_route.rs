use std::time::Duration;

use reqwest::{Client, ClientBuilder, Proxy};
use tauri::{AppHandle, Manager};

use crate::{AppState, error::Result};

pub async fn client(app: &AppHandle, builder: ClientBuilder) -> Result<Client> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let builder = if settings.download_without_proxy {
        builder.no_proxy()
    } else if let Some(port) = core_proxy_port(app, &settings).await {
        builder.proxy(Proxy::all(format!("http://127.0.0.1:{port}"))?)
    } else {
        builder
    };
    Ok(builder.build()?)
}

/// The running core's local HTTP-capable proxy port (mixed-port, else port),
/// as the core itself reports it — the profile may set it, not the app.
async fn core_proxy_port(app: &AppHandle, settings: &crate::settings::Settings) -> Option<u16> {
    let status = {
        let state = app.state::<AppState>();
        state.core.lock().unwrap().status(settings)
    };
    if !status.running {
        return None;
    }
    let cfg: serde_json::Value = reqwest::Client::builder().no_proxy().build().ok()?
        .get(format!("{}/configs", status.controller))
        .bearer_auth(&status.secret)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    ["mixed-port", "port"]
        .iter()
        .filter_map(|k| cfg.get(*k).and_then(serde_json::Value::as_u64))
        .find(|&p| p > 0 && p <= u16::MAX as u64)
        .map(|p| p as u16)
}

