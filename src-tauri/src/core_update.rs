//! Download / update the app-managed mihomo core from the official
//! MetaCubeX GitHub releases into `<data>/core-bin/`.
//!
//! Replacing the binary drops any Linux file capabilities, so TUN permission
//! has to be granted again after an update (the UI shows the grant button).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::core;
use crate::error::{Error, Result};
use crate::AppState;

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest";
pub const EVENT_PROGRESS: &str = "core-update-progress";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize, Clone)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// e.g. "v1.19.32" (None if no core found or version unparseable).
    pub current_version: Option<String>,
    pub current_path: Option<String>,
    /// Whether the core in use is the app-managed one.
    pub managed: bool,
    pub latest_version: String,
    pub asset_name: String,
    pub update_available: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: u64,
}

pub fn managed_dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.core_bin_dir.clone()
}

pub fn managed_binary(dir: &Path) -> PathBuf {
    dir.join(if cfg!(windows) { "mihomo.exe" } else { "mihomo" })
}

/// "Mihomo Meta v1.19.32 linux amd64 …" -> "v1.19.32"
pub fn parse_version(s: &str) -> Option<String> {
    s.split_whitespace()
        .find(|t| t.starts_with('v') && t[1..].starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string)
}

fn version_tuple(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.trim_start_matches('v').split(['.', '-']).map(|p| p.parse::<u32>());
    Some((it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

/// Asset name prefix for this platform, e.g. "mihomo-linux-amd64-".
fn asset_prefix() -> Result<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "darwin",
        other => return Err(Error::msg(format!("Unsupported OS: {other}"))),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => {
            // The plain amd64 build targets x86-64-v3 (AVX2); older CPUs need "compatible".
            #[cfg(target_arch = "x86_64")]
            let v3 = std::is_x86_feature_detected!("avx2")
                && std::is_x86_feature_detected!("bmi2")
                && std::is_x86_feature_detected!("fma");
            #[cfg(not(target_arch = "x86_64"))]
            let v3 = true;
            if v3 { "amd64".to_string() } else { "amd64-compatible".to_string() }
        }
        "aarch64" => "arm64".to_string(),
        other => return Err(Error::msg(format!("Unsupported CPU architecture: {other}"))),
    };
    Ok(format!("mihomo-{os}-{arch}-"))
}

fn pick_asset(release: &Release) -> Result<Asset> {
    let prefix = asset_prefix()?;
    let ext = if cfg!(windows) { ".zip" } else { ".gz" };
    // Exact "<prefix><tag><ext>" — skips -v1/-v2/-goXXX variants and packages.
    let want = format!("{prefix}{}{ext}", release.tag_name);
    release
        .assets
        .iter()
        .find(|a| a.name == want)
        .cloned()
        .ok_or_else(|| Error::msg(format!("No release asset named {want}")))
}

/// The running core's local HTTP-capable proxy port (mixed-port, else port),
/// as the core itself reports it — the profile may set it, not the app.
async fn core_proxy_port(app: &AppHandle) -> Option<u16> {
    let status = {
        let state = app.state::<AppState>();
        let settings = state.settings.lock().unwrap().clone();
        state.core.lock().unwrap().status(&settings)
    };
    if !status.running {
        return None;
    }
    let cfg: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/configs", status.controller))
        .bearer_auth(&status.secret)
        .timeout(Duration::from_secs(3))
        .send()
        .await
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

/// HTTP client; downloads through the running core's proxy port when there is
/// one, since GitHub may be slow or blocked without a proxy.
async fn client(app: &AppHandle) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .user_agent("mihomo-client")
        .connect_timeout(Duration::from_secs(15));
    if let Some(port) = core_proxy_port(app).await {
        b = b.proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))?);
    }
    Ok(b.build()?)
}

async fn latest_release(app: &AppHandle) -> Result<Release> {
    Ok(client(app).await?
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

fn current(app: &AppHandle) -> (Option<PathBuf>, Option<String>) {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let Ok(bin) = core::resolve_binary(&settings, &managed_dir(app)) else { return (None, None) };
    let version = core::core_version(&bin).ok().and_then(|v| parse_version(&v));
    (Some(bin), version)
}

pub async fn check(app: AppHandle) -> Result<UpdateInfo> {
    let release = latest_release(&app).await?;
    let asset = pick_asset(&release)?;
    let app2 = app.clone();
    let (path, version) = tauri::async_runtime::spawn_blocking(move || current(&app2))
        .await
        .map_err(|e| Error::msg(e.to_string()))?;
    let managed = path.as_deref() == Some(managed_binary(&managed_dir(&app)).as_path());
    let update_available = match (version.as_deref().and_then(version_tuple), version_tuple(&release.tag_name)) {
        (Some(cur), Some(latest)) => cur < latest,
        _ => version.as_deref() != Some(release.tag_name.as_str()),
    };
    Ok(UpdateInfo {
        current_version: version,
        current_path: path.map(|p| p.to_string_lossy().into_owned()),
        managed,
        latest_version: release.tag_name,
        asset_name: asset.name,
        update_available,
    })
}

fn extract(name: &str, data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if name.ends_with(".gz") {
        flate2::read::GzDecoder::new(data).read_to_end(&mut out)?;
    } else if name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data))
            .map_err(|e| Error::msg(format!("Bad zip: {e}")))?;
        let idx = (0..zip.len())
            .find(|&i| zip.by_index(i).map(|f| f.name().ends_with(".exe")).unwrap_or(false))
            .ok_or_else(|| Error::msg("No .exe in the release archive"))?;
        zip.by_index(idx)
            .map_err(|e| Error::msg(e.to_string()))?
            .read_to_end(&mut out)?;
    } else {
        return Err(Error::msg(format!("Unknown archive type: {name}")));
    }
    Ok(out)
}

/// Download the latest release into the managed location. If the core is
/// running it is stopped for the swap and restarted afterwards.
pub async fn install(app: AppHandle) -> Result<String> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    let release = latest_release(&app).await?;
    let asset = pick_asset(&release)?;
    let dir = managed_dir(&app);
    std::fs::create_dir_all(&dir)?;

    // Download with progress events.
    let mut resp = client(&app).await?
        .get(&asset.browser_download_url)
        .timeout(Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?;
    let total = resp.content_length().unwrap_or(asset.size);
    let mut data = Vec::with_capacity(total as usize);
    let mut last_emit = 0u64;
    while let Some(chunk) = resp.chunk().await? {
        data.extend_from_slice(&chunk);
        let n = data.len() as u64;
        if n - last_emit > 256 * 1024 || n == total {
            last_emit = n;
            let _ = app.emit(EVENT_PROGRESS, Progress { downloaded: n, total });
        }
    }
    if data.len() as u64 != asset.size {
        return Err(Error::msg(format!(
            "Download incomplete ({} of {} bytes)",
            data.len(),
            asset.size
        )));
    }

    // Extract to a temp file and make sure it runs and is the version we asked for.
    let binary = extract(&asset.name, &data)?;
    let target = managed_binary(&dir);
    let tmp = dir.join(if cfg!(windows) { "mihomo.new.exe" } else { "mihomo.new" });
    std::fs::write(&tmp, &binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    let tmp2 = tmp.clone();
    let reported = tauri::async_runtime::spawn_blocking(move || core::core_version(&tmp2))
        .await
        .map_err(|e| Error::msg(e.to_string()))?;
    match reported {
        Ok(v) if parse_version(&v).as_deref() == Some(release.tag_name.as_str()) => {}
        Ok(v) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::msg(format!("Downloaded core reports `{v}`, expected {}", release.tag_name)));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::msg(format!("Downloaded core doesn't run on this system: {e}")));
        }
    }

    // Rename preserves the previous binary's Linux file capabilities.
    let backup = dir.join(if cfg!(windows) { "mihomo.previous.exe" } else { "mihomo.previous" });
    let previous_path = state.settings.lock().unwrap().core_path.clone();
    let can_restore = core::resolve_binary(&crate::settings::current(&state), &dir).is_ok();
    if !target.exists() && backup.exists() { std::fs::remove_file(&backup)?; }
    let was_running = state.core.lock().unwrap().is_running();
    if was_running {
        core::stop_locked(app.clone(), false).await?;
    }
    if let Err(error) = replace_binary(&tmp, &target, &backup) {
        if was_running {
            core::start_locked(app.clone()).await.map_err(|restart| Error::msg(format!(
                "{error}. Restarting the previous core also failed: {restart}"
            )))?;
        }
        return Err(error);
    }
    // Explicit external paths otherwise mask the newly installed managed core.
    let saved = {
        let mut settings = state.settings.lock().unwrap();
        settings.core_path = None;
        settings.save(&state.paths.settings_file)
    };
    let startup = match saved {
        Err(error) => Err(error),
        Ok(()) if was_running => core::start_locked(app.clone()).await.map(|_| ()),
        Ok(()) => {
            let has_profile = state.profiles.lock().unwrap().active().is_some();
            if has_profile { core::prepare(&app).await.map(|_| ()) } else { Ok(()) }
        }
    };
    log::info!("installed core {} at {}", release.tag_name, target.display());
    if let Err(error) = startup {
        // A failed readiness/TUN check may leave a live child holding the binary.
        core::stop_locked(app.clone(), false).await?;
        let message = format!("Core {} could not start with the current configuration:\n\n{error}", release.tag_name);
        if can_restore {
            let handle = app.clone();
            let prompt = format!("{message}\n\nRestore the previous core version?{}",
                if was_running { " It will be started again." } else { " The core will remain stopped." });
            let restore = tauri::async_runtime::spawn_blocking(move || {
                handle.dialog().message(prompt)
                    .title("Core update failed")
                    .kind(MessageDialogKind::Warning)
                    .buttons(MessageDialogButtons::OkCancelCustom("Restore previous version".into(), "Keep update".into()))
                    .blocking_show()
            }).await.map_err(|e| Error::msg(e.to_string()))?;
            if restore {
                if backup.exists() {
                    replace_binary(&backup, &target, &tmp)?;
                } else {
                    std::fs::remove_file(&target)?;
                }
                {
                    let mut settings = state.settings.lock().unwrap();
                    settings.core_path = previous_path;
                    settings.save(&state.paths.settings_file)?;
                }
                if was_running {
                    core::start_locked(app.clone()).await.map_err(|restart| Error::msg(format!(
                        "Previous core restored, but restarting it failed: {restart}"
                    )))?;
                }
                return Err(Error::msg(format!("{message}\n\nThe previous core version was restored.")));
            }
        }
        return Err(Error::msg(message));
    }
    Ok(release.tag_name)
}

/// Undo the first rename if installing the replacement fails.
fn replace_binary(source: &Path, target: &Path, backup: &Path) -> Result<()> {
    let had_target = target.exists();
    if had_target {
        if backup.exists() { std::fs::remove_file(backup)?; }
        std::fs::rename(target, backup)?;
    }
    if let Err(error) = std::fs::rename(source, target) {
        if had_target {
            std::fs::rename(backup, target).map_err(|restore| Error::msg(format!(
                "Installing core failed: {error}. Restoring {} failed: {restore}", backup.display()
            )))?;
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_swap_preserves_previous_and_can_restore_it() {
        let dir = std::env::temp_dir().join(format!("core-swap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("core");
        let candidate = dir.join("new");
        let backup = dir.join("previous");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&candidate, b"new").unwrap();
        std::fs::write(&backup, b"stale").unwrap();
        replace_binary(&candidate, &target, &backup).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old");
        replace_binary(&backup, &target, &candidate).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_swap_restores_previous_binary() {
        let dir = std::env::temp_dir().join(format!("core-swap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("core");
        std::fs::write(&target, b"old").unwrap();
        assert!(replace_binary(&dir.join("missing"), &target, &dir.join("backup")).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn versions() {
        assert_eq!(
            parse_version("Mihomo Meta v1.19.32 linux amd64 with go1.26.8 Wed Sep 30").as_deref(),
            Some("v1.19.32")
        );
        assert!(version_tuple("v1.19.32") < version_tuple("v1.19.34"));
        assert!(version_tuple("v1.9.0") < version_tuple("v1.19.0"));
    }

    #[test]
    fn picks_exact_asset() {
        let mk = |n: &str| Asset { name: n.into(), browser_download_url: String::new(), size: 1 };
        let tag = "v1.19.34";
        let prefix = asset_prefix().unwrap();
        let ext = if cfg!(windows) { ".zip" } else { ".gz" };
        let release = Release {
            tag_name: tag.into(),
            assets: vec![
                mk(&format!("{prefix}v1-{tag}{ext}")),
                mk(&format!("{prefix}{tag}.deb")),
                mk(&format!("{prefix}{tag}{ext}")),
            ],
        };
        assert_eq!(pick_asset(&release).unwrap().name, format!("{prefix}{tag}{ext}"));
    }
}
