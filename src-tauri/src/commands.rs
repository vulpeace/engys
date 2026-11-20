//! Tauri commands exposed to the frontend. Everything that talks to mihomo's
//! REST API directly (proxies, connections, traffic…) lives in the frontend;
//! these cover what a webview can't do: processes, files, downloads.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::core::{self, CoreStatus};
use crate::error::{Error, Result};
use crate::processes::{self, ProcessEntry};
use crate::profiles::{name_from_path, new_profile, Profile, ProfileIndex, ProfileKind};
use crate::settings::Settings;
use crate::subscription;
use crate::util::now_secs;
use crate::AppState;

// ----- core -----------------------------------------------------------------

#[cfg(not(target_os = "android"))]
#[tauri::command]
pub fn core_status(state: State<'_, AppState>) -> CoreStatus {
    let settings = crate::settings::current(&state);
    state.core.lock().unwrap().status(&settings)
}

#[tauri::command]
pub async fn core_start(app: AppHandle) -> Result<CoreStatus> {
    core::start(app).await
}

#[tauri::command]
pub async fn core_stop(app: AppHandle) -> Result<CoreStatus> {
    core::stop(app).await
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
pub fn core_logs(state: State<'_, AppState>) -> Vec<String> {
    let core = state.core.lock().unwrap();
    let logs = core.logs.lock().unwrap();
    logs.iter().cloned().collect()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreBinaryInfo {
    path: Option<String>,
    version: Option<String>,
    error: Option<String>,
}

/// Which binary would be used and its `-v` output.
#[tauri::command]
pub async fn core_binary_info(state: State<'_, AppState>) -> Result<CoreBinaryInfo> {
    let settings = crate::settings::current(&state);
    let bin = match core::resolve_binary(&settings, &state.paths.core_bin_dir) {
        Ok(b) => b,
        Err(e) => return Ok(CoreBinaryInfo { path: None, version: None, error: Some(e.to_string()) }),
    };
    let b2 = bin.clone();
    let version = tauri::async_runtime::spawn_blocking(move || core::core_version(&b2))
        .await
        .map_err(|e| Error::msg(e.to_string()))?;
    Ok(match version {
        Ok(v) => CoreBinaryInfo { path: Some(bin.to_string_lossy().into()), version: Some(v), error: None },
        Err(e) => CoreBinaryInfo { path: Some(bin.to_string_lossy().into()), version: None, error: Some(e.to_string()) },
    })
}

#[tauri::command]
pub fn work_dir(state: State<'_, AppState>) -> String {
    state.paths.core_dir.to_string_lossy().into_owned()
}

// ----- settings ---------------------------------------------------------------

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    crate::settings::current(&state)
}

#[tauri::command]
pub fn settings_inherited(state: State<'_, AppState>) -> Settings {
    let effective = crate::settings::current(&state);
    let mut raw = state.settings.lock().unwrap().clone();
    raw.overrides.clear();
    let profiles = state.profiles.lock().unwrap();
    let content = profiles.active().and_then(|p| profiles.read_content(&p.id).ok()).unwrap_or_default();
    raw.resolve_profile(&content).unwrap_or(effective)
}

#[tauri::command]
pub fn settings_save(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    {
        let mut raw = state.settings.lock().unwrap();
        let next = raw.accept_update(&settings);
        next.save(&state.paths.settings_file)?;
        *raw = next;
    }
    Ok(crate::settings::current(&state))
}

// ----- profiles ---------------------------------------------------------------

#[tauri::command]
pub fn profiles_list(state: State<'_, AppState>) -> ProfileIndex {
    state.profiles.lock().unwrap().index.clone()
}

#[tauri::command]
pub fn profile_create_inline(state: State<'_, AppState>, name: String, content: String) -> Result<Profile> {
    subscription::validate_profile_yaml(&content)?;
    state.profiles.lock().unwrap().add(new_profile(name, ProfileKind::Inline, None), &content)
}

#[tauri::command]
pub async fn profile_import_file(state: State<'_, AppState>, path: String, name: Option<String>) -> Result<Profile> {
    let p = PathBuf::from(&path);
    #[cfg(target_os = "android")]
    let content = if path.starts_with("content://") {
        crate::android::invoke::<String>("readDocument", serde_json::json!({"yaml": path})).await?
    } else { std::fs::read_to_string(&p)? };
    #[cfg(not(target_os = "android"))]
    let content = std::fs::read_to_string(&p)?;
    subscription::validate_profile_yaml(&content)?;
    let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| name_from_path(&p));
    state.profiles.lock().unwrap().add(new_profile(name, ProfileKind::File, Some(path)), &content)
}

#[tauri::command]
pub async fn profile_add_remote(app: AppHandle, state: State<'_, AppState>, url: String, name: Option<String>, update_interval_minutes: Option<u32>, interval_overridden: Option<bool>, subscription_link: Option<String>) -> Result<Profile> {
    let links = app.state::<crate::subscription_links::SubscriptionLinks>();
    let _import = if let Some(link) = &subscription_link {
        let guard = links.import.lock().await;
        if !links.is_next(link) {
            return Err(crate::error::Error::msg("Subscription link is no longer pending"));
        }
        Some(guard)
    } else {
        None
    };
    let ua = state.settings.lock().unwrap().subscription_user_agent.clone();
    let d = subscription::download(url.trim(), &ua).await?;
    let name = name
        .filter(|n| !n.trim().is_empty())
        .or(d.suggested_name)
        .unwrap_or_else(|| host_of(&url));
    let mut p = new_profile(name, ProfileKind::Remote, Some(url.trim().to_string()));
    p.subscription = d.info;
    p.update_interval_hours = d.update_interval_hours;
    p.update_interval_minutes = update_interval_minutes.unwrap_or(0);
    if interval_overridden.unwrap_or(false) || p.update_interval_minutes != 0 {
        p.update_interval_source = crate::profiles::IntervalSource::Client;
    }
    p.last_refreshed_at = Some(now_secs());
    let profile = state.profiles.lock().unwrap().add(p, &d.content)?;
    if let Some(link) = subscription_link {
        links.complete(&app, &link);
    }
    crate::subscription_refresh::emit_profiles(&app);
    Ok(profile)
}

/// Re-download a subscription; hot-reloads the core if this is the active profile.
#[tauri::command]
pub async fn profile_update_remote(app: AppHandle, id: String, prompt: tauri::ipc::Channel<String>) -> Result<Option<Profile>> {
    crate::subscription_refresh::refresh(app, id, None, Some(prompt)).await
}

#[tauri::command]
pub fn profile_resolve_refresh(
    state: State<'_, AppState>,
    token: String,
    choice: Option<crate::routing::SourceRuleChoice>,
) -> Result<()> {
    state.subscription_refresh.resolve_rules(&token, choice)
}

#[tauri::command]
pub async fn pending_subscription_reviews(app: AppHandle) -> Result<Vec<crate::subscription_refresh::PendingReview>> {
    crate::subscription_refresh::pending_reviews(&app).await
}

#[tauri::command]
pub async fn resolve_pending_subscription(
    app: AppHandle,
    id: String,
    token: String,
    choice: Option<crate::routing::SourceRuleChoice>,
) -> Result<()> {
    crate::subscription_refresh::resolve_pending(&app, &id, &token, choice).await
}

#[tauri::command]
pub fn profile_set_update_interval(app: AppHandle, id: String, minutes: u32) -> Result<Profile> {
    let state = app.state::<AppState>();
    let updated = {
        let mut store = state.profiles.lock().unwrap();
        if store.get(&id)?.kind != ProfileKind::Remote {
            return Err(Error::msg("Only subscription profiles support automatic refresh."));
        }
        store.update(&id, |p| {
            p.update_interval_minutes = minutes;
            p.update_interval_source = crate::profiles::IntervalSource::Client;
        })?
    };
    crate::subscription_refresh::emit_profiles(&app);
    Ok(updated)
}

#[tauri::command]
pub fn profile_reset_update_interval(app: AppHandle, id: String) -> Result<Profile> {
    let state = app.state::<AppState>();
    let updated = {
        let mut store = state.profiles.lock().unwrap();
        if store.get(&id)?.kind != ProfileKind::Remote {
            return Err(Error::msg("Only subscription profiles support automatic refresh."));
        }
        store.update(&id, |p| {
            p.update_interval_source = crate::profiles::IntervalSource::Subscription;
            p.update_interval_minutes = crate::profiles::subscription_interval(p);
        })?
    };
    crate::subscription_refresh::emit_profiles(&app);
    Ok(updated)
}

#[tauri::command]
pub fn profile_read(state: State<'_, AppState>, id: String) -> Result<String> {
    state.profiles.lock().unwrap().read_content(&id)
}

#[tauri::command]
pub fn profile_read_final(state: State<'_, AppState>, id: String) -> Result<String> {
    let content = state.profiles.lock().unwrap().read_content(&id)?;
    core::preview_runtime_config(&state, &id, &content).map(|(runtime, _)| runtime)
}

#[tauri::command]
pub async fn profile_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    revision: String,
    content: String,
    rule_choice: Option<crate::routing::SourceRuleChoice>,
) -> Result<bool> {
    let _operation = state.core_operation.lock().await;
    subscription::validate_profile_yaml(&content)?;
    let (is_active, updated_at) = {
        let store = state.profiles.lock().unwrap();
        if store.read_content(&id)? != revision {
            return Err(Error::msg("The profile changed while editing. Reopen the profile before saving."));
        }
        (store.index.active.as_deref() == Some(id.as_str()), store.get(&id)?.updated_at)
    };
    if revision == content {
        return Ok(true);
    }
    let previous = state.routing.lock().unwrap().clone();
    let overlay = previous.by_profile.get(&id);
    let next = if crate::routing::source_rule_conflict(&revision, &content, overlay)? {
        let Some(choice) = rule_choice else { return Ok(false); };
        let mut next = previous.clone();
        if let Some(overlay) = overlay {
            next.by_profile.insert(id.clone(), crate::routing::reconcile_source(&revision, &content, overlay, choice)?);
        }
        Some(next)
    } else {
        None
    };
    let saved = next.as_ref().unwrap_or(&previous);
    let routed = crate::routing::apply_overlay(&content, saved.by_profile.get(&id))?;
    let (runtime, settings) = core::preview_routed_runtime_config(&state, &id, &content, &routed)?;
    let status = state.core.lock().unwrap().status(&settings);
    let binary = status.binary.as_ref().map(PathBuf::from)
        .or_else(|| core::resolve_binary(&settings, &state.paths.core_bin_dir).ok());
    if let Some(binary) = binary {
        let candidate = state.paths.core_dir.join(format!("profile-{}.yaml", uuid::Uuid::new_v4()));
        crate::util::write_atomic(&candidate, runtime.as_bytes())?;
        let validation = core::validate_config_file(&binary, &state.paths.core_dir, &candidate).await;
        let _ = std::fs::remove_file(candidate);
        validation?;
    }
    {
        let mut store = state.profiles.lock().unwrap();
        store.write_content(&id, &content)?;
        if let Some(next) = &next {
            if let Err(error) = next.save(&state.paths.routing_file) {
                store.replace_content(&id, &revision, |p| p.updated_at = updated_at)
                    .map_err(|rollback| Error::msg(format!("{error} Restoring the previous profile failed: {rollback}")))?;
                return Err(error);
            }
            *state.routing.lock().unwrap() = next.clone();
        }
    }
    if is_active {
        if let Err(error) = core::reload_locked(app.clone()).await {
            let mut failures = Vec::new();
            if let Err(e) = state.profiles.lock().unwrap().replace_content(&id, &revision, |p| p.updated_at = updated_at) {
                failures.push(e.to_string());
            }
            if next.is_some() {
                match previous.save(&state.paths.routing_file) {
                    Ok(()) => *state.routing.lock().unwrap() = previous,
                    Err(e) => failures.push(e.to_string()),
                }
            }
            if let Err(e) = core::reload_locked(app.clone()).await {
                failures.push(e.to_string());
            }
            return Err(if failures.is_empty() { error } else {
                Error::msg(format!("{error} Restoring the previous profile failed: {}", failures.join("; ")))
            });
        }
    }
    if let Err(error) = crate::subscription_refresh::clear_pending(&state.paths.profiles_dir, &id) {
        log::warn!("Could not remove an outdated subscription download for {id}: {error}");
    }
    crate::subscription_refresh::emit_profiles(&app);
    Ok(true)
}

#[tauri::command]
pub async fn profile_duplicate(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<Profile> {
    let _operation = state.core_operation.lock().await;
    let duplicate = {
        let mut profiles = state.profiles.lock().unwrap();
        let mut copy = profiles.get(&id)?.clone();
        let content = profiles.read_content(&id)?;
        copy.name = format!("{} (copy)", copy.name);
        copy.last_refresh_attempt_at = None;
        copy.last_refresh_error = None;
        let duplicate = profiles.add(copy, &content)?;
        let mut routing = state.routing.lock().unwrap();
        let mut groups = state.custom_groups.lock().unwrap();
        let mut apps = state.app_rules.lock().unwrap();
        let mut routing_written = false;
        let mut groups_written = false;
        let result = (|| {
            if let Some(overlay) = routing.by_profile.get(&id).cloned() {
                let mut next = routing.clone();
                next.by_profile.insert(duplicate.id.clone(), overlay);
                next.save(&state.paths.routing_file)?;
                routing_written = true;
                *routing = next;
            }
            if let Some(definitions) = groups.by_profile.get(&id).cloned() {
                let mut next = groups.clone();
                next.by_profile.insert(duplicate.id.clone(), definitions);
                next.save(&state.paths.custom_groups_file)?;
                groups_written = true;
                *groups = next;
            }
            apps.copy_profile(&state.paths.app_rules_file, &id, &duplicate.id)
        })();
        if let Err(error) = result {
            profiles.delete(&duplicate.id, || {
                if routing_written {
                    let mut next = routing.clone();
                    next.by_profile.remove(&duplicate.id);
                    next.save(&state.paths.routing_file)?;
                    *routing = next;
                }
                if groups_written {
                    let mut next = groups.clone();
                    next.by_profile.remove(&duplicate.id);
                    next.save(&state.paths.custom_groups_file)?;
                    *groups = next;
                }
                Ok(())
            }).map_err(|rollback| Error::msg(format!("{error} Removing the incomplete copy failed: {rollback}")))?;
            return Err(error);
        }
        duplicate
    };
    crate::subscription_refresh::emit_profiles(&app);
    Ok(duplicate)
}

#[tauri::command]
pub fn profile_rename(state: State<'_, AppState>, id: String, name: String) -> Result<Profile> {
    state.profiles.lock().unwrap().update(&id, |p| p.name = name)
}

#[tauri::command]
pub async fn profile_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    let _operation = state.core_operation.lock().await;
    let core_profile = {
        let settings = crate::settings::current(&state);
        state.core.lock().unwrap().status(&settings).profile_id
    };
    if core_profile.as_deref() == Some(id.as_str()) {
        return Err(Error::msg("This profile is in use by the running core. Stop the core or activate another profile first."));
    }
    state.profiles.lock().unwrap().delete(&id, || {
        state.app_rules.lock().unwrap().remove_profile(&state.paths.app_rules_file, &id)
    })?;
    if let Err(error) = crate::subscription_refresh::clear_pending(&state.paths.profiles_dir, &id) {
        log::warn!("Could not remove the pending subscription download for {id}: {error}");
    }
    crate::subscription_refresh::emit_profiles(&app);
    Ok(())
}

/// Mark a profile active; if the core is running, hot-reload it. On failure
/// the previous active profile is restored.
#[tauri::command]
pub async fn profile_activate(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    let _operation = state.core_operation.lock().await;
    let previous = {
        let mut store = state.profiles.lock().unwrap();
        let prev = store.index.active.clone();
        store.set_active(&id)?;
        prev
    };
    if let Err(e) = core::reload_locked(app.clone()).await {
        if let Some(prev) = previous {
            let _ = state.profiles.lock().unwrap().set_active(&prev);
        }
        return Err(e);
    }
    crate::subscription_refresh::emit_profiles(&app);
    Ok(())
}

/// Apply settings that only take effect via the runtime config (ports, secret…).
#[tauri::command]
pub async fn core_reload(app: AppHandle) -> Result<()> {
    core::reload(app).await
}

// ----- processes and app routing ----------------------------------------

#[cfg(not(target_os = "android"))]
#[tauri::command]
pub async fn processes_list() -> Result<Vec<ProcessEntry>> {
    tauri::async_runtime::spawn_blocking(processes::list)
        .await
        .map_err(|e| Error::msg(e.to_string()))
}

// ----- TUN ----------------------------------------------------------------------

#[cfg(not(target_os = "android"))]
#[tauri::command]
pub fn tun_permission(state: State<'_, AppState>) -> Result<crate::privilege::TunPermission> {
    let settings = crate::settings::current(&state);
    let bin = core::resolve_binary(&settings, &state.paths.core_bin_dir)?;
    Ok(crate::privilege::check(&bin))
}

/// Linux: polkit prompt to set capabilities on the core binary.
/// Windows: stop the core, relaunch the app elevated, exit this instance.
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub async fn tun_grant_permission(app: AppHandle, state: State<'_, AppState>) -> Result<crate::privilege::TunPermission> {
    let settings = crate::settings::current(&state);
    let bin = core::resolve_binary(&settings, &state.paths.core_bin_dir)?;
    if cfg!(windows) {
        let was_running = state.core.lock().unwrap().is_running();
        core::stop_preserving_state(app.clone()).await?;
        let b = bin.clone();
        let granted = tauri::async_runtime::spawn_blocking(move || crate::privilege::grant(&b))
            .await
            .map_err(|e| Error::msg(e.to_string()))
            .and_then(|result| result);
        if let Err(error) = granted {
            if was_running {
                if let Err(restart_error) = core::start(app.clone()).await {
                    return Err(Error::msg(format!("{error} Restoring the core also failed: {restart_error}")));
                }
            }
            return Err(error);
        }
        app.exit(0);
        return Ok(crate::privilege::check(&bin));
    }
    let b = bin.clone();
    tauri::async_runtime::spawn_blocking(move || crate::privilege::grant(&b))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;
    Ok(crate::privilege::check(&bin))
}

// ----- core updates -------------------------------------------------------------

#[tauri::command]
pub async fn core_check_update(app: AppHandle) -> Result<crate::core_update::UpdateInfo> {
    crate::core_update::check(app).await
}

/// Download + install the latest core; returns the installed version tag.
#[tauri::command]
pub async fn core_install_update(app: AppHandle) -> Result<String> {
    crate::core_update::install(app).await
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split(['/', '?', '#']).next())
        .filter(|h| !h.is_empty())
        .unwrap_or("Subscription")
        .to_string()
}

// Login registration is separate from settings drafts and runs off the UI thread.
#[tauri::command]
pub async fn login_status() -> Result<crate::login::LoginStatus> {
    tauri::async_runtime::spawn_blocking(crate::login::status).await
        .map_err(|e| Error::msg(e.to_string()))?
}

#[tauri::command]
pub async fn login_set(enabled: bool) -> Result<crate::login::LoginStatus> {
    tauri::async_runtime::spawn_blocking(move || crate::login::set(enabled)).await
        .map_err(|e| Error::msg(e.to_string()))?
}

#[tauri::command]
pub async fn app_rules_get(app: AppHandle) -> Result<crate::app_rules::RulesView> {
    crate::app_rules::view(&app).await
}

#[tauri::command]
pub async fn app_rules_save(
    app: AppHandle,
    expected_profile: Option<String>,
    scope: crate::app_rules::Scope,
    rules: Vec<crate::app_rules::AppRule>,
    order_override: Option<bool>,
) -> Result<crate::app_rules::RulesView> {
    crate::app_rules::save(app, expected_profile, scope, rules, order_override).await
}

#[tauri::command]
pub async fn client_memory(app: AppHandle) -> Result<Option<u64>> {
    tauri::async_runtime::spawn_blocking(move || {
        let before = app.state::<AppState>().core.lock().unwrap().pid();
        processes::client_memory(before, || app.state::<AppState>().core.lock().unwrap().pid())
    }).await.map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
pub fn custom_groups_get(app: AppHandle) -> Result<crate::custom_groups::GroupsView> {
    crate::custom_groups::view(&app)
}

#[tauri::command]
pub async fn custom_groups_save(app: AppHandle, profile_id: String, groups: Vec<crate::custom_groups::CustomGroup>) -> Result<crate::custom_groups::GroupsView> {
    crate::custom_groups::save(app, profile_id, groups).await
}

#[tauri::command]
pub fn routing_get(app: AppHandle) -> Result<crate::routing::View> { crate::routing::view(&app) }
#[tauri::command]
pub async fn routing_save(app: AppHandle, profile_id: String, revision: String, entries: Vec<crate::routing::Entry>, providers_yaml: String, edit_inherited: bool, replace_rules: bool) -> Result<crate::routing::View> {
    crate::routing::save(app, profile_id, revision, entries, providers_yaml, edit_inherited, replace_rules).await
}

#[tauri::command]
pub fn platform_info() -> &'static str { std::env::consts::OS }

#[tauri::command]
pub async fn android_setup_status() -> Result<serde_json::Value> {
    #[cfg(target_os = "android")]
    return crate::android::invoke("setupStatus", serde_json::json!({})).await;
    #[cfg(not(target_os = "android"))]
    Ok(serde_json::json!({"supported": false}))
}
#[tauri::command]
pub async fn android_request_battery() -> Result<()> {
    #[cfg(target_os = "android")]
    crate::android::invoke::<serde_json::Value>("requestBattery", serde_json::json!({})).await?;
    Ok(())
}
#[tauri::command]
pub async fn android_request_notifications() -> Result<()> {
    #[cfg(target_os = "android")]
    crate::android::invoke::<serde_json::Value>("requestNotifications", serde_json::json!({})).await?;
    Ok(())
}
#[tauri::command]
pub async fn android_vpn_active() -> Result<bool> {
    #[cfg(target_os = "android")]
    return crate::android::invoke("vpnActive", serde_json::json!({})).await;
    #[cfg(not(target_os = "android"))]
    Ok(false)
}
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn core_status(app: AppHandle) -> Result<CoreStatus> { core::refresh(&app).await }
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn core_logs() -> Result<Vec<String>> { crate::android::invoke("logs", serde_json::json!({})).await }
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn processes_list() -> Result<Vec<ProcessEntry>> { crate::android::invoke("installedApps", serde_json::json!({})).await }
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn tun_permission() -> Result<serde_json::Value> { crate::android::invoke("vpnPermission", serde_json::json!({})).await }
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn tun_grant_permission() -> Result<serde_json::Value> { crate::android::invoke("requestVpn", serde_json::json!({})).await }

#[tauri::command]
pub async fn installed_app_icon(package_name: String) -> Result<Option<String>> {
    #[cfg(target_os = "android")]
    return crate::android::invoke("installedAppIcon", serde_json::json!({"packageName": package_name})).await;
    #[cfg(not(target_os = "android"))]
    { let _ = package_name; Ok(None) }
}

#[tauri::command]
pub async fn android_memory() -> Result<serde_json::Value> {
    #[cfg(target_os = "android")]
    return crate::android::invoke("memory", serde_json::json!({})).await;
    #[cfg(not(target_os = "android"))]
    Ok(serde_json::Value::Null)
}
