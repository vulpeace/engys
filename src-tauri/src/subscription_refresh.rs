//! Subscription timers run in Rust, independently of the webview/core lifecycle.
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;

use crate::AppState;
use crate::core::{self, RUNTIME_CONFIG};
use crate::error::{Error, Result};
use crate::profiles::{Profile, ProfileKind, IntervalSource, subscription_interval};
use crate::subscription::{self, Downloaded};
use crate::util::{now_secs, write_atomic};

pub struct RefreshState {
    in_flight: Mutex<HashSet<String>>,
    slots: Semaphore,
}

impl RefreshState {
    pub fn new() -> Self {
        Self {
            in_flight: Mutex::new(HashSet::new()),
            slots: Semaphore::new(4),
        }
    }
}

struct RefreshGuard {
    app: AppHandle,
    id: String,
}
impl Drop for RefreshGuard {
    fn drop(&mut self) {
        if let Some(state) = self.app.try_state::<AppState>() {
            state.subscription_refresh.in_flight.lock().unwrap().remove(&self.id);
        }
    }
}

pub fn is_due(profile: &Profile, now: u64) -> bool {
    if profile.kind != ProfileKind::Remote || profile.update_interval_minutes == 0 {
        return false;
    }
    let last = profile
        .last_refreshed_at
        .unwrap_or(profile.updated_at)
        .max(profile.last_refresh_attempt_at.unwrap_or(0));
    now.saturating_sub(last) >= u64::from(profile.update_interval_minutes) * 60
}

fn update_download_metadata(profile: &mut Profile, downloaded: &Downloaded, now: u64) {
    if downloaded.info.is_some() {
        profile.subscription = downloaded.info.clone();
    }
    profile.update_interval_hours = downloaded.update_interval_hours;
    if profile.update_interval_source == IntervalSource::Subscription {
        profile.update_interval_minutes = subscription_interval(profile);
    }
    profile.updated_at = now;
    profile.last_refreshed_at = Some(now);
    profile.last_refresh_error = None;
}

pub fn emit_profiles(app: &AppHandle) {
    let index = app.state::<AppState>().profiles.lock().unwrap().index.clone();
    let _ = app.emit("profiles-changed", index);
}

pub fn start_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let state = app.state::<AppState>();
            if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let due = {
                let store = state.profiles.lock().unwrap();
                let in_flight = state.subscription_refresh.in_flight.lock().unwrap();
                store
                    .index
                    .profiles
                    .iter()
                    .filter(|p| is_due(p, now_secs()) && !in_flight.contains(&p.id))
                    .map(|p| (p.id.clone(), p.update_interval_minutes))
                    .collect::<Vec<_>>()
            };
            for (id, interval) in due {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = refresh(handle, id.clone(), Some(interval)).await {
                        log::warn!("Automatic subscription refresh failed for profile {id}: {error}");
                    }
                });
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}

/// expected_interval identifies an automatic job. Changes to the interval,
/// deletion, or an intervening manual refresh invalidate queued automatic work.
pub async fn refresh(app: AppHandle, id: String, expected_interval: Option<u32>) -> Result<Profile> {
    let state = app.state::<AppState>();
    let acquired = state.subscription_refresh.in_flight.lock().unwrap().insert(id.clone());
    if !acquired {
        return if expected_interval.is_some() {
            Ok(state.profiles.lock().unwrap().get(&id)?.clone())
        } else {
            Err(Error::msg("This subscription is already being refreshed."))
        };
    }
    let _guard = RefreshGuard {
        app: app.clone(),
        id: id.clone(),
    };
    let _slot = state
        .subscription_refresh
        .slots
        .acquire()
        .await
        .map_err(|e| Error::msg(e.to_string()))?;
    if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::msg("The app is shutting down."));
    }
    let (snapshot, old_content) = {
        let mut store = state.profiles.lock().unwrap();
        let profile = store.get(&id)?.clone();
        if profile.kind != ProfileKind::Remote {
            return Err(Error::msg("Only subscription profiles can be refreshed."));
        }
        if expected_interval
            .is_some_and(|interval| interval != profile.update_interval_minutes || !is_due(&profile, now_secs()))
        {
            return Ok(profile);
        }
        let content = store.read_content(&id)?;
        store.update(&id, |p| {
            p.last_refresh_attempt_at = Some(now_secs());
            p.last_refresh_error = None;
        })?;
        (profile, content)
    };
    emit_profiles(&app);
    let user_agent = state.settings.lock().unwrap().subscription_user_agent.clone();
    let result = async {
        let url = snapshot
            .source
            .as_deref()
            .ok_or_else(|| Error::msg("The subscription has no URL."))?;
        let downloaded = subscription::download(url, &user_agent).await?;
        apply_download(&app, &snapshot, &old_content, downloaded, expected_interval).await
    }
    .await;
    if let Err(error) = &result {
        let mut store = state.profiles.lock().unwrap();
        if store.get(&id).is_ok_and(|p| p.kind == ProfileKind::Remote) {
            if let Err(save_error) = store.update(&id, |p| p.last_refresh_error = Some(error.to_string())) {
                log::warn!("Could not save subscription refresh error for {id}: {save_error}");
            }
        }
    }
    emit_profiles(&app);
    result
}

async fn apply_download(
    app: &AppHandle,
    snapshot: &Profile,
    old_content: &str,
    downloaded: Downloaded,
    expected_interval: Option<u32>,
) -> Result<Profile> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::msg("The app is shutting down."));
    }
    let (previous, active) = {
        let store = state.profiles.lock().unwrap();
        let current = store.get(&snapshot.id)?.clone();
        // Disabling/changing automation during the HTTP request cancels its commit.
        if expected_interval.is_some_and(|interval| interval == 0 || current.update_interval_minutes != interval) {
            return Ok(current);
        }
        if current.source != snapshot.source
            || current.kind != ProfileKind::Remote
            || current.updated_at != snapshot.updated_at
            || store.read_content(&snapshot.id)? != old_content
        {
            return Err(Error::msg(
                "This profile changed during the download. Refresh it again to replace the newer content.",
            ));
        }
        (current, store.index.active.as_deref() == Some(snapshot.id.as_str()))
    };
    let settings = crate::settings::current(&state);
    let status = state.core.lock().unwrap().status(&settings);
    let live = active && status.running;
    let mut effective = settings.clone();
    if live {
        effective.controller_port = reqwest::Url::parse(&status.controller)
            .ok()
            .and_then(|url| url.port_or_known_default())
            .ok_or_else(|| Error::msg("The controller address is invalid."))?;
        effective.secret = status.secret.clone();
        effective.overrides.extend(["external-controller".into(), "secret".into()]);
    }
    let apps = state.app_rules.lock().unwrap().clone();
    let groups = state.custom_groups.lock().unwrap().for_profile(Some(&snapshot.id)).to_vec();
    let routed_content = crate::routing::apply_profile(&state, Some(&snapshot.id), &downloaded.content)?;
    let runtime = core::build_runtime_config_with_groups(
        &routed_content,
        &effective,
        if active { &apps.rules } else { &[] },
        if active {
            apps.order_override(Some(&snapshot.id))
        } else {
            None
        },
        &groups,
    )?;
    let binary = if live {
        status.binary.as_ref().map(std::path::PathBuf::from)
    } else {
        core::resolve_binary(&settings, &state.paths.core_bin_dir).ok()
    };
    if live && effective.resolve_profile(&downloaded.content)?.tun.enable {
        if let Some(binary) = &binary {
            if !crate::privilege::check(binary).granted {
                return Err(Error::msg("The subscription enables TUN, but the core lacks permission. Grant permission on the Dashboard before refreshing."));
            }
        }
    }
    // Even inactive subscriptions are parser-validated when a core is installed.
    // Before core installation, the downloader still requires valid mapping YAML.
    if let Some(binary) = binary {
        let candidate = state
            .paths
            .core_dir
            .join(format!("subscription-{}.yaml", uuid::Uuid::new_v4()));
        write_atomic(&candidate, runtime.as_bytes())?;
        let valid = core::validate_config_file(&binary, &state.paths.core_dir, &candidate).await;
        let _ = std::fs::remove_file(candidate);
        valid?;
    } else if live {
        return Err(Error::msg(
            "The running core binary is unavailable for config validation.",
        ));
    }
    let runtime_file = state.paths.core_dir.join(RUNTIME_CONFIG);
    let rollback_runtime = if live {
        let bytes = std::fs::read(&runtime_file)?;
        let mut root: serde_yaml::Mapping = serde_yaml::from_slice(&bytes)?;
        root.insert(
            serde_yaml::Value::from("mode"),
            serde_yaml::Value::from(settings.mode.clone()),
        );
        Some(serde_yaml::to_string(&root)?)
    } else {
        None
    };
    let now = now_secs();
    {
        let mut store = state.profiles.lock().unwrap();
        if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::msg("The app is shutting down."));
        }
        // Check and commit under one lock so disabling a download before this
        // point cannot race its profile write.
        let current = store.get(&snapshot.id)?;
        if expected_interval.is_some_and(|interval| current.update_interval_minutes != interval) {
            return Ok(current.clone());
        }
        store.replace_content(&snapshot.id, &downloaded.content, |p| {
            update_download_metadata(p, &downloaded, now)
        })?;
    }
    if live {
        let applied = async {
            write_atomic(&runtime_file, runtime.as_bytes())?;
            core::apply_config(&status, &runtime).await
        }
        .await;
        if let Err(error) = applied {
            let mut failures = Vec::new();
            if let Err(e) = state
                .profiles
                .lock()
                .unwrap()
                .replace_content(&snapshot.id, old_content, |p| {
                    p.subscription = previous.subscription;
                    p.update_interval_hours = previous.update_interval_hours;
                    if p.update_interval_source == IntervalSource::Subscription {
                        p.update_interval_minutes = subscription_interval(p);
                    }
                    p.updated_at = previous.updated_at;
                    p.last_refreshed_at = previous.last_refreshed_at;
                })
            {
                failures.push(e.to_string());
            }
            if let Some(previous_runtime) = rollback_runtime {
                if let Err(e) = write_atomic(&runtime_file, previous_runtime.as_bytes()) {
                    failures.push(e.to_string());
                }
                if let Err(e) = core::apply_config(&status, &previous_runtime).await {
                    failures.push(e.to_string());
                }
            }
            return Err(if failures.is_empty() {
                error
            } else {
                Error::msg(format!(
                    "{error} Restoring the previous subscription also failed: {}",
                    failures.join("; ")
                ))
            });
        }
        core::notify_reloaded(app, &snapshot.id);
    }
    Ok(state.profiles.lock().unwrap().get(&snapshot.id)?.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::new_profile;
    #[test]
    fn scheduling_respects_zero_profile_kind_and_exact_minutes() {
        let mut profile = new_profile(
            "Sub".into(),
            ProfileKind::Remote,
            Some("https://example.invalid".into()),
        );
        profile.updated_at = 100;
        assert!(!is_due(&profile, 10000));
        profile.update_interval_minutes = 2;
        assert!(!is_due(&profile, 219));
        assert!(is_due(&profile, 220));
        profile.kind = ProfileKind::File;
        assert!(!is_due(&profile, 10000));
    }
    #[test]
    fn attempts_rate_limit_failures_and_success_resets_the_schedule() {
        let mut profile = new_profile("Sub".into(), ProfileKind::Remote, None);
        profile.update_interval_minutes = 1;
        profile.last_refreshed_at = Some(100);
        profile.last_refresh_attempt_at = Some(150);
        assert!(!is_due(&profile, 209));
        assert!(is_due(&profile, 210));
        profile.last_refreshed_at = Some(170);
        assert!(!is_due(&profile, 229));
        assert!(is_due(&profile, 230));
        assert!(!is_due(&profile, 90));
        profile.update_interval_minutes = 0;
        assert!(!is_due(&profile, u64::MAX));
    }
    #[test]
    fn provider_hints_do_not_change_the_user_interval_or_reenable_zero() {
        let downloaded = Downloaded {
            content: "rules: []".into(),
            info: Some(Default::default()),
            suggested_name: None,
            update_interval_hours: Some(12),
        };
        let mut profile = new_profile("Sub".into(), ProfileKind::Remote, None);
        profile.last_refresh_error = Some("old failure".into());
        update_download_metadata(&mut profile, &downloaded, 100);
        assert_eq!(profile.update_interval_minutes, 0);
        assert_eq!(profile.update_interval_hours, Some(12));
        assert_eq!(profile.last_refreshed_at, Some(100));
        assert!(profile.last_refresh_error.is_none());
        profile.update_interval_minutes = 7;
        update_download_metadata(&mut profile, &downloaded, 200);
        assert_eq!(profile.update_interval_minutes, 7);
    }

    #[test]
    fn reset_interval_follows_new_provider_hints_including_their_removal() {
        let mut profile = new_profile("Sub".into(), ProfileKind::Remote, None);
        profile.update_interval_source = IntervalSource::Subscription;
        let mut downloaded = Downloaded { content: "rules: []".into(), info: None, suggested_name: None, update_interval_hours: Some(2) };
        update_download_metadata(&mut profile, &downloaded, 100);
        assert_eq!(profile.update_interval_minutes, 120);
        downloaded.update_interval_hours = Some(3);
        update_download_metadata(&mut profile, &downloaded, 200);
        assert_eq!(profile.update_interval_minutes, 180);
        downloaded.update_interval_hours = None;
        update_download_metadata(&mut profile, &downloaded, 300);
        assert_eq!(profile.update_interval_minutes, 0);
        profile.update_interval_source = IntervalSource::Client;
        profile.update_interval_minutes = 0;
        downloaded.update_interval_hours = Some(4);
        update_download_metadata(&mut profile, &downloaded, 400);
        assert_eq!(profile.update_interval_minutes, 0);
    }

    #[test]
    fn local_edits_do_not_postpone_a_known_last_fetch() {
        let mut profile = new_profile("Sub".into(), ProfileKind::Remote, None);
        profile.update_interval_minutes = 1;
        profile.last_refreshed_at = Some(100);
        profile.updated_at = 159;
        assert!(is_due(&profile, 160));
        profile.update_interval_minutes = u32::MAX;
        assert!(!is_due(&profile, 160));
    }
}
