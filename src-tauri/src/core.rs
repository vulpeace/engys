//! Lifecycle of the mihomo core process: config generation, validation,
//! spawning, log capture, crash detection and shutdown.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
#[cfg(test)]
use serde_yaml::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{Error, Result};
use crate::settings::Settings;
use crate::util::{now_secs, write_atomic};
use crate::AppState;

const LOG_CAPACITY: usize = 1000;
pub const RUNTIME_CONFIG: &str = "runtime.yaml";
pub const EVENT_STATUS: &str = "core-status";
pub const EVENT_LOG: &str = "core-log";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreStatus {
    pub running: bool,
    pub pid: Option<u32>,
    /// Base URL of the external controller, e.g. `http://127.0.0.1:9097`.
    pub controller: String,
    pub secret: String,
    pub started_at: Option<u64>,
    pub profile_id: Option<String>,
    pub binary: Option<String>,
    pub last_error: Option<String>,
}

struct Running {
    child: Child,
    controller: String,
    secret: String,
    started_at: u64,
    profile_id: String,
    binary: PathBuf,
}

#[derive(Default)]
pub struct CoreManager {
    running: Option<Running>,
    /// Bumped on every start/stop so stale watcher threads exit.
    generation: u64,
    last_error: Option<String>,
    pub logs: Arc<Mutex<VecDeque<String>>>,
}

impl CoreManager {
    pub fn status(&self, settings: &Settings) -> CoreStatus {
        match &self.running {
            Some(r) => CoreStatus {
                running: true,
                pid: Some(r.child.id()),
                controller: r.controller.clone(),
                secret: r.secret.clone(),
                started_at: Some(r.started_at),
                profile_id: Some(r.profile_id.clone()),
                binary: Some(r.binary.to_string_lossy().into_owned()),
                last_error: self.last_error.clone(),
            },
            None => CoreStatus {
                running: false,
                pid: None,
                controller: controller_url(settings.controller_port),
                secret: settings.secret.clone(),
                started_at: None,
                profile_id: None,
                binary: None,
                last_error: self.last_error.clone(),
            },
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.running.as_ref().map(|running| running.child.id())
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// Synchronous stop used on app exit: SIGTERM, short grace period, then
    /// kill. The grace period matters for TUN — mihomo removes its routes and
    /// ip rules on a clean shutdown.
    pub fn shutdown_blocking(&mut self) {
        self.generation += 1;
        if let Some(r) = self.running.as_mut() {
            match terminate(&mut r.child, Duration::from_secs(3)) {
                Ok(()) => { self.running = None; }
                Err(error) => {
                    log::error!("Core shutdown failed: {error}");
                    self.last_error = Some(error.to_string());
                }
            }
        }
    }
}

/// A response from an old core must never count as readiness for a new child.
fn ensure_controller_available(port: u16) -> Result<()> {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .map(drop)
        .map_err(|error| Error::msg(format!(
            "Cannot start core: controller address 127.0.0.1:{port} is unavailable ({error}). Another core may still be running. Stop it or choose a different controller port in Settings."
        )))
}

pub fn controller_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

// ---------------------------------------------------------------------------
// Binary resolution
// ---------------------------------------------------------------------------

fn exe_name() -> &'static str {
    if cfg!(windows) { "mihomo.exe" } else { "mihomo" }
}

/// Resolution order: explicit setting → app-managed core (downloaded via
/// Settings → Core) → binary bundled next to the app executable (Tauri
/// `externalBin` sidecar) → `mihomo` on `PATH`.
pub fn resolve_binary(settings: &Settings, managed_dir: &Path) -> Result<PathBuf> {
    if let Some(p) = settings.core_path.as_deref().filter(|p| !p.trim().is_empty()) {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(Error::msg(format!("Core binary not found at {}", p.display())))
        };
    }
    let managed = crate::core_update::managed_binary(managed_dir);
    if managed.is_file() {
        return Ok(managed);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        let p = dir.join(exe_name());
        if p.is_file() {
            return Ok(p);
        }
    }
    which::which("mihomo").map_err(|_| {
        Error::msg("mihomo core not found. Download it in Settings → Core, or set the path to an existing binary.")
    })
}

fn command(bin: &Path) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn core_version(bin: &Path) -> Result<String> {
    let out = command(bin).arg("-v").output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().next().unwrap_or("").trim().to_string())
}

// ---------------------------------------------------------------------------
// Runtime config
// ---------------------------------------------------------------------------

pub use crate::core_config::*;

/// Write `runtime.yaml` for the active profile and validate it with `mihomo -t`.
/// Returns (binary, profile id, settings).
pub async fn prepare(app: &AppHandle) -> Result<(PathBuf, String, Settings)> {
    let state = app.state::<AppState>();
    let settings = crate::settings::current(&state);
    let (profile_id, content) = {
        let store = state.profiles.lock().unwrap();
        let p = store
            .active()
            .ok_or_else(|| Error::msg("No active profile. Add one on the Profiles tab first."))?;
        (p.id.clone(), store.read_content(&p.id)?)
    };
    let bin = resolve_binary(&settings, &state.paths.core_bin_dir)?;
    let (runtime, settings) = preview_runtime_config(&state, &profile_id, &content)?;
    if settings.tun.enable {
        let perm = crate::privilege::check(&bin);
        if !perm.granted {
            return Err(Error::msg(format!(
                "TUN mode is on but the core lacks permission. {} Grant it on the Dashboard, or turn TUN off.",
                perm.detail
            )));
        }
    }
    let core_dir = state.paths.core_dir.clone();
    let candidate = core_dir.join(format!("prepare-{}.yaml", uuid::Uuid::new_v4()));
    write_atomic(&candidate, runtime.as_bytes())?;
    let validation = validate_config_file(&bin, &core_dir, &candidate).await;
    let _ = std::fs::remove_file(candidate);
    validation?;
    write_atomic(&core_dir.join(RUNTIME_CONFIG), runtime.as_bytes())?;
    Ok((bin, profile_id, settings))
}

/// Run the installed core's parser, including its own regex engine and policy checks.
pub async fn validate_config_file(bin: &Path, core_dir: &Path, path: &Path) -> Result<()> {
    let bin = bin.to_owned();
    let core_dir = core_dir.to_owned();
    let path = path.to_owned();
    let out = tauri::async_runtime::spawn_blocking(move || {
        command(&bin).arg("-t").arg("-d").arg(core_dir).arg("-f").arg(path).output()
    }).await.map_err(|e| Error::msg(e.to_string()))??;
    if !out.status.success() {
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let tail = text.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>();
        return Err(Error::msg(format!("Config validation failed:\n{}", tail.join("\n"))));
    }
    Ok(())
}

/// Payload reload avoids depending on the core's initial -f path and is also used
/// to restore a previous config if a transactional app-rule update fails.
pub async fn apply_config(status: &CoreStatus, yaml: &str) -> Result<()> {
    let response = reqwest::Client::new()
        .put(format!("{}/configs?force=true", status.controller))
        .bearer_auth(&status.secret)
        .timeout(Duration::from_secs(30))
        .json(&serde_json::json!({ "payload": yaml }))
        .send().await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!("Core rejected the config: {}", response.text().await?)));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Start / stop
// ---------------------------------------------------------------------------

fn emit_status(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = crate::settings::current(&state);
    let status = state.core.lock().unwrap().status(&settings);
    crate::desktop::update(app, status.running, status.last_error.as_deref());
    let _ = app.emit(EVENT_STATUS, status);
}

fn pump_logs<R: Read + Send + 'static>(app: AppHandle, logs: Arc<Mutex<VecDeque<String>>>, src: R) {
    std::thread::spawn(move || {
        for line in BufReader::new(src).lines().map_while(std::result::Result::ok) {
            {
                let mut l = logs.lock().unwrap();
                if l.len() >= LOG_CAPACITY {
                    l.pop_front();
                }
                l.push_back(line.clone());
            }
            let _ = app.emit(EVENT_LOG, line);
        }
    });
}

pub async fn start(app: AppHandle) -> Result<CoreStatus> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    start_locked(app.clone()).await
}

/// Caller must hold core_operation.
pub async fn start_locked(app: AppHandle) -> Result<CoreStatus> {
    let state = app.state::<AppState>();
    let result = start_inner(app.clone()).await;
    if let Err(error) = &result {
        state.core.lock().unwrap().last_error = Some(error.to_string());
        emit_status(&app);
    }
    result
}

async fn start_inner(app: AppHandle) -> Result<CoreStatus> {
    let state = app.state::<AppState>();
    if state.core.lock().unwrap().is_running() {
        return Err(Error::msg("Core is already running"));
    }
    let (bin, profile_id, settings) = prepare(&app).await?;
    let core_dir = state.paths.core_dir.clone();
    let controller = controller_url(settings.controller_port);
    ensure_controller_available(settings.controller_port)?;

    let generation = {
        let mut core = state.core.lock().unwrap();
        if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::msg("The app is shutting down."));
        }
        if core.is_running() {
            return Err(Error::msg("Core is already running"));
        }
        core.logs.lock().unwrap().clear();
        let mut child = command(&bin)
            .arg("-d")
            .arg(&core_dir)
            .arg("-f")
            .arg(core_dir.join(RUNTIME_CONFIG))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::msg(format!("Failed to launch {}: {e}", bin.display())))?;
        pump_logs(app.clone(), core.logs.clone(), child.stdout.take().unwrap());
        pump_logs(app.clone(), core.logs.clone(), child.stderr.take().unwrap());
        core.generation += 1;
        core.last_error = None;
        log::info!("Started core pid={} controller={controller}", child.id());
        core.running = Some(Running {
            child,
            controller: controller.clone(),
            secret: settings.secret.clone(),
            started_at: now_secs(),
            profile_id: profile_id.clone(),
            binary: bin,
        });
        core.generation
    };
    let remembered = state.session.lock().unwrap().set_running(true);
    if let Err(error) = remembered {
        state.core.lock().unwrap().shutdown_blocking();
        return Err(error);
    }
    spawn_watcher(app.clone(), generation);

    // Wait until the controller answers (first start may download GEO data).
    let client = reqwest::Client::new();
    let mut ready = false;
    for _ in 0..120 {
        if !state.core.lock().unwrap().is_running() {
            let err = state.core.lock().unwrap().last_error.clone();
            return Err(Error::msg(err.unwrap_or_else(|| "Core exited during startup".into())));
        }
        let ok = client
            .get(format!("{controller}/version"))
            .bearer_auth(&settings.secret)
            .timeout(Duration::from_millis(500))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if !ready {
        state.core.lock().unwrap().shutdown_blocking();
        return Err(Error::msg("Core controller did not become ready. Open the core log for details."));
    }
    if settings.tun.enable {
        let tun = client.get(format!("{controller}/configs"))
            .bearer_auth(&settings.secret).timeout(Duration::from_secs(3))
            .send().await?.error_for_status()?.json::<serde_json::Value>().await?;
        if tun.pointer("/tun/enable").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(Error::msg("The core started, but TUN could not be restored. Check TUN permissions and the core log."));
        }
    }
    if let Err(error) = crate::startup_proxies::apply(&state, &profile_id, &controller, &settings.secret).await {
        if let Err(stop_error) = stop_locked(app.clone(), false).await {
            return Err(Error::msg(format!("{error} Could not stop the core: {stop_error}")));
        }
        state.session.lock().unwrap().set_running(false)?;
        return Err(error);
    }
    emit_status(&app);
    Ok(state.core.lock().unwrap().status(&settings))
}

/// Poll the child; if it dies without us stopping it, record why and notify the UI.
fn spawn_watcher(app: AppHandle, generation: u64) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        let state = app.state::<AppState>();
        let mut core = state.core.lock().unwrap();
        if core.generation != generation {
            return;
        }
        let Some(r) = core.running.as_mut() else { return };
        if let Ok(Some(exit)) = r.child.try_wait() {
            core.running = None;
            let tail = core.logs.lock().unwrap().iter().rev().take(3).cloned().collect::<Vec<_>>();
            let tail: Vec<String> = tail.into_iter().rev().collect();
            core.last_error = Some(format!("Core exited unexpectedly ({exit}).\n{}", tail.join("\n")));
            // A crashed core is stopped when the app next shuts down. Keep this
            // write under the core lock so a later successful start wins.
            if let Err(error) = state.session.lock().unwrap().set_running(false) {
                log::warn!("Could not remember the stopped core state: {error}");
            }
            drop(core);
            emit_status(&app);
            return;
        }
    });
}

/// Ask the core to exit (SIGTERM on Unix) so it can tear down TUN routes,
/// then force-kill after `grace`.
fn terminate(child: &mut Child, grace: Duration) -> Result<()> {
    let pid = child.id();
    if let Some(status) = child.try_wait()? {
        log::info!("Core pid={pid} already exited: {status}");
        return Ok(());
    }
    #[cfg(unix)]
    // SAFETY: plain signal to our own child's pid.
    if unsafe { libc::kill(pid as i32, libc::SIGTERM) } != 0 {
        let error = std::io::Error::last_os_error();
        if child.try_wait()?.is_some() { return Ok(()); }
        return Err(Error::msg(format!("Could not stop core pid={pid}: {error}")));
    }
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            log::info!("Stopped core pid={pid}: {status}");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    log::warn!("Core pid={pid} did not stop within the grace period; killing it");
    if let Err(error) = child.kill() {
        if child.try_wait()?.is_some() { return Ok(()); }
        return Err(Error::msg(format!("Could not kill core pid={pid}: {error}")));
    }
    let status = child.wait()?;
    log::info!("Killed core pid={pid}: {status}");
    Ok(())
}

pub async fn stop(app: AppHandle) -> Result<CoreStatus> {
    stop_inner(app, true).await
}

/// Maintenance restarts must not turn a remembered running core into a stopped one.
pub async fn stop_preserving_state(app: AppHandle) -> Result<CoreStatus> {
    stop_inner(app, false).await
}

async fn stop_inner(app: AppHandle, remember: bool) -> Result<CoreStatus> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    stop_locked(app.clone(), remember).await
}

/// Caller must hold core_operation.
pub async fn stop_locked(app: AppHandle, remember: bool) -> Result<CoreStatus> {
    let state = app.state::<AppState>();
    if remember { state.session.lock().unwrap().set_running(false)?; }
    let running = {
        let mut core = state.core.lock().unwrap();
        core.generation += 1;
        core.last_error = None;
        core.running.take()
    };
    if let Some(mut r) = running {
        let (r, result) = tauri::async_runtime::spawn_blocking(move || {
            let result = terminate(&mut r.child, Duration::from_secs(3));
            (r, result)
        }).await.map_err(|e| Error::msg(e.to_string()))?;
        if let Err(error) = result {
            let generation = {
                let mut core = state.core.lock().unwrap();
                core.last_error = Some(error.to_string());
                core.running = Some(r);
                core.generation
            };
            if remember {
                if let Err(save_error) = state.session.lock().unwrap().set_running(true) {
                    log::error!("Could not restore core session after failed stop: {save_error}");
                }
            }
            spawn_watcher(app.clone(), generation);
            emit_status(&app);
            return Err(error);
        }
    }
    emit_status(&app);
    let settings = crate::settings::current(&state);
    Ok(state.core.lock().unwrap().status(&settings))
}

/// Regenerate the runtime config and hot-reload it into the running core.
/// No-op when the core is stopped (the next start picks up the changes).
pub async fn reload(app: AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    reload_locked(app.clone()).await
}

/// Caller must hold core_operation across profile mutation and reload.
pub async fn reload_locked(app: AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let (controller, secret) = {
        let core = state.core.lock().unwrap();
        match &core.running {
            Some(r) => (r.controller.clone(), r.secret.clone()),
            None => return Ok(()),
        }
    };
    let (_, profile_id, _) = prepare(&app).await?;
    let resp = reqwest::Client::new()
        .put(format!("{controller}/configs?force=true"))
        .bearer_auth(&secret)
        .json(&serde_json::json!({ "path": "", "payload": "" }))
        .send()
        .await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::msg(format!("Core rejected the config: {body}")));
    }
    notify_reloaded(&app, &profile_id);
    Ok(())
}

pub fn notify_reloaded(app: &AppHandle, profile_id: &str) {
    if let Some(r) = app.state::<AppState>().core.lock().unwrap().running.as_mut() {
        r.profile_id = profile_id.to_owned();
    }
    emit_status(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_rejects_an_existing_controller_instead_of_using_its_api() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let error = ensure_controller_available(port).unwrap_err().to_string();
        assert!(error.contains(&format!("127.0.0.1:{port}")));
        assert!(error.contains("Another core may still be running"));
        ensure_controller_available(0).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stopping_core_waits_for_the_child_to_exit() {
        let mut child = Command::new("sleep").arg("60").spawn().unwrap();
        terminate(&mut child, Duration::from_secs(1)).unwrap();
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn overrides_owned_keys() {
        let mut s = Settings { secret: "abc".into(), ..Default::default() };
        s.overrides.insert("secret".into());
        let out = build_runtime_config("mixed-port: 1\nsecret: x\nproxies: []\nexternal-controller-tls: 0.0.0.0:1\n", &s).unwrap();
        let v: Value = serde_yaml::from_str(&out).unwrap();
        // Mixed port unset by default: the profile's value is kept.
        assert_eq!(v["mixed-port"], Value::from(1));
        let mut s2 = Settings { mixed_port: Some(7890), ..s.clone() };
        s2.overrides.insert("mixed-port".into());
        let v2: Value = serde_yaml::from_str(&build_runtime_config("mixed-port: 1\n", &s2).unwrap()).unwrap();
        assert_eq!(v2["mixed-port"], Value::from(7890));
        assert_eq!(v["secret"], Value::from("abc"));
        assert_eq!(v["external-controller"], Value::from("127.0.0.1:9097"));
        assert!(v.get("external-controller-tls").is_none());
        assert!(v.get("proxies").is_some());
        assert_eq!(v["profile"]["store-selected"], Value::from(true));
        let cached: Value = serde_yaml::from_str(&build_runtime_config(
            "profile: {store-selected: false, store-fake-ip: true}\n", &s).unwrap()).unwrap();
        assert_eq!(cached["profile"]["store-selected"], Value::from(true));
        assert_eq!(cached["profile"]["store-fake-ip"], Value::from(true));
        // An unmodified TUN toggle follows the profile.
        let out = build_runtime_config("tun: {enable: true, route-exclude-address: [10.0.0.0/8]}\n", &s).unwrap();
        let v: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(v["tun"]["enable"], Value::from(true));
    }

    #[test]
    fn runtime_inherits_and_reset_removes_the_actual_override() {
        let profile = "external-controller: 0.0.0.0:7777\nsecret: sub-secret\nmixed-port: 1234\nmode: global\nallow-lan: true\nipv6: true\nlog-level: debug\ntun: {enable: true, stack: gvisor, auto-route: false, device: SubTun, mtu: 1400}\ndns: {enable: true, nameserver: [9.9.9.9]}";
        let mut settings = Settings::default();
        let inherited: Value = serde_yaml::from_str(&build_runtime_config(profile, &settings).unwrap()).unwrap();
        assert_eq!(inherited["external-controller"], Value::from("127.0.0.1:7777"));
        assert_eq!(inherited["secret"], Value::from("sub-secret"));
        assert_eq!(inherited["mode"], Value::from("global"));
        assert_eq!(inherited["tun"]["stack"], Value::from("gvisor"));
        assert_eq!(inherited["tun"]["auto-route"], Value::from(false));
        settings.overrides.extend(["mode".into(), "allow-lan".into(), "tun.device".into(), "tun.mtu".into(), "tun.dns-hijack".into()]);
        settings.mode = "direct".into();
        settings.tun.dns_hijack = Vec::new();
        let pinned: Value = serde_yaml::from_str(&build_runtime_config(profile, &settings).unwrap()).unwrap();
        assert_eq!(pinned["mode"], Value::from("direct"));
        assert_eq!(pinned["allow-lan"], Value::from(false));
        assert!(pinned["tun"].get("device").is_none());
        assert!(pinned["tun"].get("mtu").is_none());
        assert_eq!(pinned["tun"]["dns-hijack"], Value::Sequence(vec![]));
        settings.overrides.clear();
        let reset: Value = serde_yaml::from_str(&build_runtime_config(profile, &settings).unwrap()).unwrap();
        assert_eq!(reset, inherited);
    }

    #[test]
    fn tun_injects_dns_and_keeps_profile_tun_options() {
        let mut s = Settings::default();
        s.tun.enable = true;
        s.overrides.insert("tun.enable".into());
        let out = build_runtime_config("tun: {route-exclude-address: [10.0.0.0/8]}\n", &s).unwrap();
        let v: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(v["tun"]["enable"], Value::from(true));
        assert_eq!(v["tun"]["stack"], Value::from("mixed"));
        if cfg!(target_os = "linux") {
            assert_eq!(v["tun"]["dns-mode"], Value::from("disabled"));
        }
        assert!(v["tun"].get("route-exclude-address").is_some());
        assert_eq!(v["dns"]["enable"], Value::from(true));
        assert_eq!(v["dns"]["enhanced-mode"], Value::from("fake-ip"));

        // A profile with its own enabled DNS is left alone.
        let out = build_runtime_config("dns: {enable: true, enhanced-mode: redir-host, nameserver: [9.9.9.9]}\n", &s).unwrap();
        let v: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(v["dns"]["enhanced-mode"], Value::from("redir-host"));
        assert!(v["dns"].get("fake-ip-filter").is_none());
    }
}
