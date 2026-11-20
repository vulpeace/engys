//! Subscription timers run in Rust, independently of the webview/core lifecycle.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, ipc::Channel};
use tokio::sync::{Semaphore, oneshot};

use crate::AppState;
use crate::core::{self, RUNTIME_CONFIG};
use crate::error::{Error, Result};
use crate::profiles::{Profile, ProfileKind, IntervalSource, subscription_interval};
use crate::routing::{self, SourceRuleChoice};
use crate::subscription::{self, Downloaded};
use crate::util::{now_secs, write_atomic};

#[derive(Serialize, Deserialize)]
struct PendingDownload {
    token: String,
    source: Option<String>,
    revision: String,
    download: Downloaded,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReview {
    profile_id: String,
    profile_name: String,
    token: String,
}

fn pending_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.pending-refresh.json"))
}

fn read_pending(dir: &Path, id: &str) -> Result<Option<PendingDownload>> {
    match std::fs::read(pending_path(dir, id)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn stash_download(dir: &Path, profile: &Profile, revision: &str, download: Downloaded) -> Result<()> {
    let pending = PendingDownload {
        token: uuid::Uuid::new_v4().to_string(),
        source: profile.source.clone(),
        revision: revision.to_owned(),
        download,
    };
    write_atomic(&pending_path(dir, &profile.id), &serde_json::to_vec(&pending)?)
}

pub fn clear_pending(dir: &Path, id: &str) -> Result<()> {
    match std::fs::remove_file(pending_path(dir, id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn matching_pending(dir: &Path, profile: &Profile, content: &str) -> Result<Option<PendingDownload>> {
    let pending = match read_pending(dir, &profile.id) {
        Ok(pending) => pending,
        Err(Error::Json(error)) => {
            log::warn!("Discarding an invalid pending subscription download for {}: {error}", profile.id);
            clear_pending(dir, &profile.id)?;
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    if pending.as_ref().is_some_and(|p| p.source != profile.source || p.revision != content) {
        clear_pending(dir, &profile.id)?;
        return Ok(None);
    }
    Ok(pending)
}

pub async fn pending_reviews(app: &AppHandle) -> Result<Vec<PendingReview>> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    #[cfg(not(target_os = "android"))]
    if !app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false)
    }) {
        return Ok(Vec::new());
    }
    let store = state.profiles.lock().unwrap();
    let mut reviews = Vec::new();
    for profile in &store.index.profiles {
        if !pending_path(&state.paths.profiles_dir, &profile.id).exists() {
            continue;
        }
        let pending = store.read_content(&profile.id)
            .and_then(|content| matching_pending(&state.paths.profiles_dir, profile, &content));
        let pending = match pending {
            Ok(pending) => pending,
            Err(error) => {
                log::warn!("Could not read the pending subscription review for {}: {error}", profile.id);
                continue;
            }
        };
        if let Some(pending) = pending {
            reviews.push(PendingReview {
                profile_id: profile.id.clone(),
                profile_name: profile.name.clone(),
                token: pending.token,
            });
        }
    }
    Ok(reviews)
}

pub async fn resolve_pending(app: &AppHandle, id: &str, token: &str, choice: Option<SourceRuleChoice>) -> Result<()> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    let (profile, pending) = {
        let store = state.profiles.lock().unwrap();
        let profile = store.get(id)?.clone();
        let content = store.read_content(id)?;
        let pending = matching_pending(&state.paths.profiles_dir, &profile, &content)?
            .filter(|pending| pending.token == token)
            .ok_or_else(|| Error::msg("This pending update is no longer available."))?;
        (profile, pending)
    };
    if let Some(choice) = choice {
        apply_download(app, &profile, &pending.revision, pending.download, None, Some(choice)).await?;
        if let Err(error) = clear_pending(&state.paths.profiles_dir, id) {
            log::warn!("Could not remove the applied subscription download for {id}: {error}");
        }
    } else {
        state.profiles.lock().unwrap().update(id, |profile| {
            profile.last_refresh_attempt_at = Some(now_secs());
        })?;
        clear_pending(&state.paths.profiles_dir, id)?;
    }
    emit_profiles(app);
    Ok(())
}

pub struct RefreshState {
    in_flight: Mutex<HashSet<String>>,
    slots: Semaphore,
    rule_choices: Mutex<HashMap<String, oneshot::Sender<Option<SourceRuleChoice>>>>,
}

impl RefreshState {
    pub fn new() -> Self {
        Self {
            in_flight: Mutex::new(HashSet::new()),
            slots: Semaphore::new(4),
            rule_choices: Mutex::new(HashMap::new()),
        }
    }

    pub fn resolve_rules(&self, token: &str, choice: Option<SourceRuleChoice>) -> Result<()> {
        let sender = self.rule_choices.lock().unwrap().remove(token)
            .ok_or_else(|| Error::msg("This rule choice has expired. Refresh the subscription again."))?;
        sender.send(choice).map_err(|_| Error::msg("The subscription refresh has ended."))
    }

    async fn ask_about_rules(&self, prompt: &Channel<String>) -> Result<Option<SourceRuleChoice>> {
        let token = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.rule_choices.lock().unwrap().insert(token.clone(), sender);
        let result = match prompt.send(token.clone()) {
            Ok(()) => tokio::time::timeout(Duration::from_secs(600), receiver).await.ok().and_then(|result| result.ok()).flatten(),
            Err(error) => {
                self.rule_choices.lock().unwrap().remove(&token);
                return Err(Error::msg(error.to_string()));
            }
        };
        self.rule_choices.lock().unwrap().remove(&token);
        Ok(result)
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
                    .filter(|p| is_due(p, now_secs()) && !in_flight.contains(&p.id)
                        && !pending_path(&state.paths.profiles_dir, &p.id).exists())
                    .map(|p| (p.id.clone(), p.update_interval_minutes))
                    .collect::<Vec<_>>()
            };
            for (id, interval) in due {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = refresh(handle, id.clone(), Some(interval), None).await {
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
pub async fn refresh(
    app: AppHandle,
    id: String,
    expected_interval: Option<u32>,
    prompt: Option<Channel<String>>,
) -> Result<Option<Profile>> {
    let state = app.state::<AppState>();
    let acquired = state.subscription_refresh.in_flight.lock().unwrap().insert(id.clone());
    if !acquired {
        return if expected_interval.is_some() {
            Ok(Some(state.profiles.lock().unwrap().get(&id)?.clone()))
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
            return Ok(Some(profile));
        }
        let content = store.read_content(&id)?;
        if matching_pending(&state.paths.profiles_dir, &profile, &content)?.is_some() {
            if expected_interval.is_some() {
                return Ok(Some(profile));
            }
            return Err(Error::msg("This subscription already has an update awaiting review."));
        }
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
        let downloaded = subscription::download(&app, url, &user_agent).await?;
        drop(_slot);
        let mut rule_choice = None;
        if let Some(prompt) = &prompt {
            let conflict = routing::source_rule_conflict(
                &old_content, &downloaded.content, state.routing.lock().unwrap().by_profile.get(&id),
            )?;
            if conflict {
                rule_choice = state.subscription_refresh.ask_about_rules(prompt).await?;
                if rule_choice.is_none() {
                    return Ok(None);
                }
            }
        }
        let _operation = state.core_operation.lock().await;
        apply_download(&app, &snapshot, &old_content, downloaded, expected_interval, rule_choice)
            .await.map(Some)
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
    rule_choice: Option<SourceRuleChoice>,
) -> Result<Profile> {
    let state = app.state::<AppState>();
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
    let previous_routing = state.routing.lock().unwrap().clone();
    let overlay = previous_routing.by_profile.get(&snapshot.id);
    let mut next_routing = None;
    if routing::source_rule_conflict(old_content, &downloaded.content, overlay)? {
        if let Some(interval) = expected_interval {
            let store = state.profiles.lock().unwrap();
            let current = store.get(&snapshot.id)?;
            if current.update_interval_minutes != interval {
                return Ok(current.clone());
            }
            stash_download(&state.paths.profiles_dir, current, old_content, downloaded)?;
            return Ok(current.clone());
        }
        let choice = rule_choice.ok_or_else(|| Error::msg("Rules changed during the refresh. Try refreshing again."))?;
        if let Some(overlay) = overlay {
            let mut next = previous_routing.clone();
            next.by_profile.insert(snapshot.id.clone(), routing::reconcile_source(old_content, &downloaded.content, overlay, choice)?);
            next_routing = Some(next);
        }
    }
    let routed_content = routing::apply_overlay(
        &downloaded.content, next_routing.as_ref().unwrap_or(&previous_routing).by_profile.get(&snapshot.id),
    )?;
    let runtime = core::build_runtime_config_with_groups(
        &routed_content,
        &effective,
        if active { apps.rules_for_profile(Some(&snapshot.id)) } else { &[] },
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
    let restore_profile = || {
        state.profiles.lock().unwrap().replace_content(&snapshot.id, old_content, |p| {
            p.subscription = previous.subscription.clone();
            p.update_interval_hours = previous.update_interval_hours;
            if p.update_interval_source == IntervalSource::Subscription {
                p.update_interval_minutes = subscription_interval(p);
            }
            p.updated_at = previous.updated_at;
            p.last_refreshed_at = previous.last_refreshed_at;
        })
    };
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
    if let Some(next) = &next_routing {
        if let Err(error) = next.save(&state.paths.routing_file) {
            restore_profile().map_err(|rollback| Error::msg(format!("{error} Restoring the previous subscription failed: {rollback}")))?;
            return Err(error);
        }
    }
    if live {
        let applied = async {
            write_atomic(&runtime_file, runtime.as_bytes())?;
            core::apply_config(&status, &runtime).await
        }
        .await;
        if let Err(error) = applied {
            let mut failures = Vec::new();
            if let Err(e) = restore_profile() {
                failures.push(e.to_string());
            }
            if next_routing.is_some() {
                if let Err(e) = previous_routing.save(&state.paths.routing_file) {
                    failures.push(e.to_string());
                }
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
    if let Some(next) = next_routing {
        *state.routing.lock().unwrap() = next;
    }
    Ok(state.profiles.lock().unwrap().get(&snapshot.id)?.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::new_profile;

    #[test]
    fn corrupt_pending_download_does_not_block_refresh() {
        let dir = std::env::temp_dir().join(format!("mihomo-pending-review-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let mut profile = new_profile("Subscription".into(), ProfileKind::Remote, None);
        profile.id = "subscription".into();
        let path = pending_path(&dir, &profile.id);
        std::fs::write(&path, b"{").unwrap();

        let pending = matching_pending(&dir, &profile, "rules: []");
        let removed = !path.exists();
        std::fs::remove_dir_all(dir).unwrap();

        assert!(pending.unwrap().is_none());
        assert!(removed);
    }

    #[test]
    fn pending_download_survives_reopening_and_is_invalidated_by_source_edits() {
        use crate::profiles::ProfileStore;

        let dir = std::env::temp_dir().join(format!("mihomo-pending-review-{}", uuid::Uuid::new_v4()));
        let before = "rules: ['MATCH,DIRECT']";
        let after = "rules: ['MATCH,REJECT']";
        let mut store = ProfileStore::load(dir.clone());
        let profile = store.add(new_profile(
            "Subscription".into(), ProfileKind::Remote, Some("https://example.invalid/profile".into()),
        ), before).unwrap();
        stash_download(&dir, &profile, before, Downloaded {
            content: after.into(),
            info: None,
            suggested_name: None,
            update_interval_hours: Some(12),
        }).unwrap();

        let mut reopened = ProfileStore::load(dir.clone());
        let content = reopened.read_content(&profile.id).unwrap();
        assert_eq!(content, before);
        let pending = matching_pending(&dir, reopened.get(&profile.id).unwrap(), &content).unwrap().unwrap();
        assert_eq!(pending.download.content, after);
        assert_eq!(pending.download.update_interval_hours, Some(12));

        let edited = "rules: ['MATCH,GLOBAL']";
        reopened.write_content(&profile.id, edited).unwrap();
        assert!(matching_pending(&dir, reopened.get(&profile.id).unwrap(), edited).unwrap().is_none());
        assert!(read_pending(&dir, &profile.id).unwrap().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
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
