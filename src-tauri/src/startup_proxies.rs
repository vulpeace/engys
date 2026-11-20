//! Pending proxy selections for the next start of each profile.
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use serde::Serialize;
use serde_yaml::Value;
use tauri::{AppHandle, Manager};

use crate::{
    AppState, core,
    error::{Error, Result},
    util::write_atomic,
};

type Selections = BTreeMap<String, String>;
type Pending = BTreeMap<String, Selections>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupProxies {
    profile_id: Option<String>,
    groups: Vec<Group>,
    selections: Selections,
}

#[derive(Serialize)]
struct Group {
    name: String,
    members: Vec<String>,
}

fn path(state: &AppState) -> PathBuf {
    state
        .paths
        .settings_file
        .with_file_name("startup-proxies.json")
}

fn read(state: &AppState) -> Result<Pending> {
    match std::fs::read(path(state)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Pending::new()),
        Err(err) => Err(err.into()),
    }
}

fn save(state: &AppState, pending: &Pending) -> Result<()> {
    write_atomic(&path(state), &serde_json::to_vec_pretty(pending)?)
}

fn items<'a>(root: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> + Clone {
    root.get(key)
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
}

fn groups(root: &Value) -> Vec<Group> {
    let mut nodes: Vec<_> = items(root, "proxies")
        .filter_map(|p| p["name"].as_str())
        .collect();
    let configured = items(root, "proxy-groups");
    let mut groups = Vec::new();
    if !configured
        .clone()
        .any(|g| g["name"].as_str() == Some("GLOBAL"))
    {
        groups.push(Group {
            name: "GLOBAL".into(),
            members: ["DIRECT", "REJECT"]
                .into_iter()
                .chain(nodes.iter().copied())
                .chain(configured.clone().filter_map(|g| g["name"].as_str()))
                .map(str::to_owned)
                .collect(),
        });
    }
    nodes.sort_unstable();
    for group in configured {
        let Some(name) = group["name"].as_str() else {
            continue;
        };
        if !matches!(
            group["type"].as_str(),
            Some("select" | "url-test" | "fallback")
        ) || (name != "GLOBAL" && group["hidden"].as_bool() == Some(true))
        {
            continue;
        }
        let mut members: Vec<_> = items(group, "proxies").filter_map(Value::as_str).collect();
        let includes_nodes = ["include-all", "include-all-proxies"]
            .into_iter()
            .any(|key| group[key].as_bool() == Some(true));
        // Mihomo resolves filtered and provider-backed membership at runtime.
        if includes_nodes && group["filter"].as_str().unwrap_or_default().is_empty() {
            members.extend(nodes.iter().copied());
        }
        let mut seen = std::collections::HashSet::new();
        members.retain(|name| seen.insert(*name));
        if !members.is_empty() {
            groups.push(Group {
                name: name.into(),
                members: members.into_iter().map(str::to_owned).collect(),
            });
        }
    }
    groups
}

fn view(state: &AppState, pending: &Pending) -> Result<StartupProxies> {
    let (id, content) = {
        let profiles = state.profiles.lock().unwrap();
        let Some(profile) = profiles.active() else {
            return Ok(StartupProxies {
                profile_id: None,
                groups: vec![],
                selections: Selections::new(),
            });
        };
        (profile.id.clone(), profiles.read_content(&profile.id)?)
    };
    let (runtime, _) = core::preview_runtime_config(state, &id, &content)?;
    Ok(StartupProxies {
        groups: groups(&serde_yaml::from_str(&runtime)?),
        selections: pending.get(&id).cloned().unwrap_or_default(),
        profile_id: Some(id),
    })
}

#[tauri::command]
pub async fn startup_proxies_get(app: AppHandle) -> Result<StartupProxies> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    view(&state, &read(&state)?)
}

#[tauri::command]
pub async fn startup_proxies_save(
    app: AppHandle,
    profile_id: String,
    group: Option<String>,
    member: Option<String>,
) -> Result<StartupProxies> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    #[cfg(target_os = "android")]
    core::refresh(&app).await?;
    if state.core.lock().unwrap().is_running() {
        return Err(Error::msg(
            "The core is running. Select a proxy in the live groups.",
        ));
    }
    let mut pending = read(&state)?;
    let mut current = view(&state, &pending)?;
    if current.profile_id.as_deref() != Some(&profile_id) {
        return Err(Error::msg(
            "The active profile changed. Select the proxy again.",
        ));
    }
    match (group, member) {
        (Some(group), Some(member)) => {
            if !current
                .groups
                .iter()
                .any(|g| g.name == group && g.members.contains(&member))
            {
                return Err(Error::msg(
                    "That proxy is no longer available in the group.",
                ));
            }
            current.selections.insert(group, member);
        }
        (Some(group), None) => {
            current.selections.remove(&group);
        }
        (None, None) => current.selections.clear(),
        (None, Some(_)) => return Err(Error::msg("Select a proxy group first.")),
    }
    if current.selections.is_empty() {
        pending.remove(&profile_id);
    } else {
        pending.insert(profile_id, current.selections.clone());
    }
    save(&state, &pending)?;
    Ok(current)
}

/// Called under core_operation once the controller is ready.
pub async fn apply(
    state: &AppState,
    profile_id: &str,
    controller: &str,
    secret: &str,
) -> Result<()> {
    let mut pending = read(state)?;
    let Some(selections) = pending.remove(profile_id) else {
        return Ok(());
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    let base = reqwest::Url::parse(controller).map_err(|e| Error::msg(e.to_string()))?;
    for (group, member) in selections {
        let mut url = base.clone();
        url.path_segments_mut()
            .map_err(|_| Error::msg("Invalid controller URL"))?
            .pop_if_empty()
            .push("proxies")
            .push(&group);
        client.put(url).bearer_auth(secret).json(&serde_json::json!({ "name": member }))
            .send().await.and_then(reqwest::Response::error_for_status)
            .map_err(|e| Error::msg(format!(
                "Could not select {member} in {group} at startup ({e}). Update or clear the startup selection on Proxies."
            )))?;
    }
    save(state, &pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_static_members_and_respects_explicit_global() {
        let root = serde_yaml::from_str(
            r#"
proxies:
  - {name: Tokyo, type: direct}
  - {name: Amsterdam, type: direct}
proxy-groups:
  - {name: Pick, type: select, proxies: [DIRECT, Tokyo], include-all-proxies: true}
  - {name: Auto, type: url-test, proxies: [Tokyo, Amsterdam]}
  - {name: Balanced, type: load-balance, proxies: [Tokyo, Amsterdam]}
  - {name: GLOBAL, type: select, proxies: [Pick]}
"#,
        )
        .unwrap();
        let result = groups(&root);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].name, "Pick");
        assert_eq!(result[0].members, ["DIRECT", "Tokyo", "Amsterdam"]);
        assert_eq!(result[1].name, "Auto");
        assert_eq!(result[2].name, "GLOBAL");
        assert_eq!(result[2].members, ["Pick"]);
    }

    #[test]
    fn synthesizes_global_in_config_order_for_standalone_proxies() {
        let root = serde_yaml::from_str(
            "proxies: [{name: Tokyo}, {name: Amsterdam}]\nproxy-groups: [{name: Pick, type: select, proxies: [Tokyo]}]",
        )
        .unwrap();
        let result = groups(&root);
        assert_eq!(result[0].name, "GLOBAL");
        assert_eq!(
            result[0].members,
            ["DIRECT", "REJECT", "Tokyo", "Amsterdam", "Pick"]
        );
    }
}
