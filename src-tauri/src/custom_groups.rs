//! Profile-scoped, app-owned proxy groups. Subscription files remain untouched.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::core::{self, RUNTIME_CONFIG};
use crate::error::{Error, Result};
use crate::util::write_atomic;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GroupKind {
    Select,
    UrlTest,
    Fallback,
    LoadBalance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    ConsistentHashing,
    RoundRobin,
    StickySessions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomGroup {
    pub id: String,
    pub name: String,
    pub kind: GroupKind,
    pub proxies: Vec<String>,
    pub providers: Vec<String>,
    pub include_all_proxies: bool,
    pub include_all_providers: bool,
    pub filter: String,
    pub url: String,
    pub interval: u32,
    pub strategy: Strategy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GroupsStore {
    pub by_profile: BTreeMap<String, Vec<CustomGroup>>,
}

impl GroupsStore {
    pub fn for_profile(&self, id: Option<&str>) -> &[CustomGroup] {
        id.and_then(|id| self.by_profile.get(id))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn load(path: &Path) -> Result<Self> {
        let saved: Self = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        for groups in saved.by_profile.values() {
            validate_definitions(groups)?;
        }
        Ok(saved)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupsView {
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub groups: Vec<CustomGroup>,
    pub members: Vec<String>,
    pub providers: Vec<String>,
}

const RESERVED: &[&str] = &[
    "DIRECT",
    "REJECT",
    "REJECT-DROP",
    "PASS",
    "PASS-RULE",
    "COMPATIBLE",
    "GLOBAL",
];
fn named_items(root: &Mapping, key: &str) -> Vec<String> {
    root.get(key)
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect()
}
fn provider_names(root: &Mapping) -> Vec<String> {
    root.get("proxy-providers")
        .and_then(Value::as_mapping)
        .into_iter()
        .flat_map(|m| m.keys())
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name.trim() == name
        && !name.contains(',')
        && !name.chars().any(char::is_control)
}
fn validate_definitions(groups: &[CustomGroup]) -> Result<()> {
    if groups.len() > 100 {
        return Err(Error::msg("At most 100 custom groups per profile are supported."));
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for group in groups {
        if group.id.is_empty() || !ids.insert(&group.id) {
            return Err(Error::msg("Custom groups have duplicate or missing IDs."));
        }
        if !safe_name(&group.name) || RESERVED.contains(&group.name.as_str()) || !names.insert(&group.name) {
            return Err(Error::msg(format!(
                "Invalid, duplicate, or reserved custom group name: '{}'",
                group.name
            )));
        }
        if group.proxies.len() > 1000
            || group.providers.len() > 1000
            || group.filter.len() > 4096
            || group.filter.chars().any(char::is_control)
        {
            return Err(Error::msg(format!(
                "Custom group '{}' has too many members or an invalid filter.",
                group.name
            )));
        }
        if group.proxies.is_empty()
            && group.providers.is_empty()
            && !group.include_all_proxies
            && !group.include_all_providers
        {
            return Err(Error::msg(format!(
                "Choose at least one member or provider for '{}'.",
                group.name
            )));
        }
        let parsed = reqwest::Url::parse(&group.url).ok();
        if !parsed.is_some_and(|u| matches!(u.scheme(), "https" | "http") && u.host_str().is_some())
            || group.url.chars().any(char::is_control)
        {
            return Err(Error::msg(format!("Enter an HTTP(S) test URL for '{}'.", group.name)));
        }
        if group.interval == 0 {
            return Err(Error::msg("The group test interval must be at least one second."));
        }
        if group.proxies.iter().collect::<HashSet<_>>().len() != group.proxies.len()
            || group.providers.iter().collect::<HashSet<_>>().len() != group.providers.len()
        {
            return Err(Error::msg(format!(
                "Remove duplicate members/providers from '{}'.",
                group.name
            )));
        }
    }
    Ok(())
}

fn as_yaml(group: &CustomGroup) -> Value {
    let mut value = Mapping::new();
    value.insert("name".into(), group.name.clone().into());
    value.insert("type".into(), serde_yaml::to_value(group.kind).unwrap());
    if !group.proxies.is_empty() {
        value.insert("proxies".into(), serde_yaml::to_value(&group.proxies).unwrap());
    }
    if !group.providers.is_empty() && !group.include_all_providers {
        value.insert("use".into(), serde_yaml::to_value(&group.providers).unwrap());
    }
    if group.include_all_proxies {
        value.insert("include-all-proxies".into(), true.into());
    }
    if group.include_all_providers {
        value.insert("include-all-providers".into(), true.into());
    }
    if !group.filter.is_empty() {
        value.insert("filter".into(), group.filter.clone().into());
    }
    value.insert("url".into(), group.url.clone().into());
    if group.kind != GroupKind::Select {
        value.insert("interval".into(), group.interval.into());
    }
    if group.kind == GroupKind::LoadBalance {
        value.insert("strategy".into(), serde_yaml::to_value(&group.strategy).unwrap());
    }
    Value::Mapping(value)
}

/// Append groups, checking references and cycles before the core parser sees them.
/// Do not silently remove nodes that disappeared in a subscription update.
pub fn inject(root: &mut Mapping, groups: &[CustomGroup]) -> Result<()> {
    validate_definitions(groups)?;
    if groups.is_empty() {
        return Ok(());
    }
    let source_groups = named_items(root, "proxy-groups");
    let source_nodes = named_items(root, "proxies");
    let mut available: HashSet<String> = RESERVED
        .iter()
        .map(|s| s.to_string())
        .chain(source_nodes)
        .chain(source_groups)
        .collect();
    let providers: HashSet<String> = provider_names(root).into_iter().collect();
    for group in groups {
        if !available.insert(group.name.clone()) || providers.contains(&group.name) {
            return Err(Error::msg(format!(
                "Custom group '{}' conflicts with an existing proxy, group, or provider.",
                group.name
            )));
        }
    }
    for group in groups {
        for member in &group.proxies {
            if member == "GLOBAL" || !available.contains(member) {
                return Err(Error::msg(format!(
                    "Member '{}' is unavailable for custom group '{}'. Edit its members on Proxies.",
                    member, group.name
                )));
            }
        }
        if !group.include_all_providers {
            for provider in &group.providers {
                if !providers.contains(provider) {
                    return Err(Error::msg(format!(
                        "Provider '{}' is unavailable for custom group '{}'.",
                        provider, group.name
                    )));
                }
            }
        }
    }
    let mut merged = match root.get("proxy-groups") {
        Some(Value::Sequence(items)) => items.clone(),
        Some(Value::Null) | None => Vec::new(),
        _ => return Err(Error::msg("Profile proxy-groups must be a YAML list.")),
    };
    merged.extend(groups.iter().map(as_yaml));
    let graph: HashMap<String, Vec<String>> = merged
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_owned();
            let members = item
                .get("proxies")
                .and_then(Value::as_sequence)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            Some((name, members))
        })
        .collect();
    fn visit(
        name: &str,
        graph: &HashMap<String, Vec<String>>,
        visiting: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name.into()) {
            return Err(Error::msg(format!("Proxy group cycle involving '{name}'.")));
        }
        if let Some(members) = graph.get(name) {
            for member in members {
                if graph.contains_key(member) {
                    visit(member, graph, visiting, done)?;
                }
            }
        }
        visiting.remove(name);
        done.insert(name.into());
        Ok(())
    }
    let mut done = HashSet::new();
    for name in graph.keys() {
        visit(name, &graph, &mut HashSet::new(), &mut done)?;
    }
    root.insert("proxy-groups".into(), Value::Sequence(merged));
    Ok(())
}

pub fn view(app: &AppHandle) -> Result<GroupsView> {
    let state = app.state::<AppState>();
    let (profile_id, profile_name, content) = {
        let profiles = state.profiles.lock().unwrap();
        match profiles.active() {
            Some(p) => (Some(p.id.clone()), Some(p.name.clone()), profiles.read_content(&p.id)?),
            None => (None, None, String::new()),
        }
    };
    let root = match serde_yaml::from_str::<Value>(&content)? {
        Value::Mapping(root) => root,
        Value::Null => Mapping::new(),
        _ => return Err(Error::msg("Profile is not a YAML mapping.")),
    };
    let groups = state
        .custom_groups
        .lock()
        .unwrap()
        .for_profile(profile_id.as_deref())
        .to_vec();
    let mut members = vec!["DIRECT".into(), "REJECT".into()];
    for name in named_items(&root, "proxies")
        .into_iter()
        .chain(named_items(&root, "proxy-groups"))
        .chain(groups.iter().map(|g| g.name.clone()))
    {
        if name != "GLOBAL" && !members.contains(&name) {
            members.push(name);
        }
    }
    Ok(GroupsView {
        profile_id,
        profile_name,
        groups,
        members,
        providers: provider_names(&root),
    })
}

pub async fn save(app: AppHandle, profile_id: String, groups: Vec<CustomGroup>) -> Result<GroupsView> {
    validate_definitions(&groups)?;
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    let content = {
        let profiles = state.profiles.lock().unwrap();
        if profiles.index.active.as_deref() != Some(profile_id.as_str()) {
            return Err(Error::msg("The active profile changed. Reopen the group editor."));
        }
        profiles.read_content(&profile_id)?
    };
    let previous = state.custom_groups.lock().unwrap().clone();
    let mut next = previous.clone();
    if groups.is_empty() {
        next.by_profile.remove(&profile_id);
    } else {
        next.by_profile.insert(profile_id.clone(), groups.clone());
    }
    let mut settings = crate::settings::current(&state);
    let status = state.core.lock().unwrap().status(&settings);
    if status.running {
        settings.controller_port = reqwest::Url::parse(&status.controller)
            .ok()
            .and_then(|u| u.port_or_known_default())
            .ok_or_else(|| Error::msg("The controller address is invalid."))?;
        settings.secret = status.secret.clone();
        settings
            .overrides
            .extend(["external-controller".into(), "secret".into()]);
    }
    let apps = state.app_rules.lock().unwrap().clone();
    let content = crate::routing::apply_profile(&state, Some(&profile_id), &content)?;
    let runtime = core::build_runtime_config_with_groups(
        &content,
        &settings,
        apps.rules_for_profile(Some(&profile_id)),
        apps.order_override(Some(&profile_id)),
        &groups,
    )?;
    let runtime_file = state.paths.core_dir.join(RUNTIME_CONFIG);
    let mut previous_runtime = match std::fs::read(&runtime_file) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if status.running {
        if let Some(bytes) = &previous_runtime {
            let mut root: Mapping = serde_yaml::from_slice(bytes)?;
            root.insert("mode".into(), settings.mode.clone().into());
            previous_runtime = Some(serde_yaml::to_string(&root)?.into_bytes());
        }
    }
    if !groups.is_empty() || !previous.for_profile(Some(&profile_id)).is_empty() || status.running {
        let bin = status
            .binary
            .as_ref()
            .filter(|_| status.running)
            .map(std::path::PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| core::resolve_binary(&settings, &state.paths.core_bin_dir))?;
        let candidate = state
            .paths
            .core_dir
            .join(format!("groups-{}.yaml", uuid::Uuid::new_v4()));
        write_atomic(&candidate, runtime.as_bytes())?;
        let valid = core::validate_config_file(&bin, &state.paths.core_dir, &candidate).await;
        let _ = std::fs::remove_file(candidate);
        valid?;
    }
    if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::msg("The app is shutting down."));
    }
    next.save(&state.paths.custom_groups_file)?;
    let committed = async {
        write_atomic(&runtime_file, runtime.as_bytes())?;
        if status.running {
            core::apply_config(&status, &runtime).await?;
        }
        Ok::<(), Error>(())
    }
    .await;
    if let Err(error) = committed {
        let mut failures = Vec::new();
        if let Err(e) = previous.save(&state.paths.custom_groups_file) {
            failures.push(e.to_string());
        }
        let restored = match &previous_runtime {
            Some(bytes) => write_atomic(&runtime_file, bytes),
            None => match std::fs::remove_file(&runtime_file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            },
        };
        if let Err(e) = restored {
            failures.push(e.to_string());
        }
        if status.running {
            if let Some(bytes) = &previous_runtime {
                if let Err(e) = core::apply_config(&status, &String::from_utf8_lossy(bytes)).await {
                    failures.push(e.to_string());
                }
            } else {
                failures.push("The previous live config is unavailable.".into());
            }
        }
        return Err(if failures.is_empty() {
            error
        } else {
            Error::msg(format!(
                "{error} Restoring the previous groups failed: {}",
                failures.join("; ")
            ))
        });
    }
    *state.custom_groups.lock().unwrap() = next;
    if status.running {
        core::notify_reloaded(&app, &profile_id);
    }
    let _ = app.emit("custom-groups-changed", &profile_id);
    view(&app)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn group(name: &str, kind: GroupKind, members: &[&str]) -> CustomGroup {
        CustomGroup {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            kind,
            proxies: members.iter().map(|p| p.to_string()).collect(),
            providers: vec![],
            include_all_proxies: false,
            include_all_providers: false,
            filter: String::new(),
            url: "https://www.gstatic.com/generate_204".into(),
            interval: 300,
            strategy: Strategy::ConsistentHashing,
        }
    }
    fn source() -> Mapping {
        serde_yaml::from_str("proxies: [{name: Node, type: socks5, server: 127.0.0.1, port: 1}]\nproxy-groups: [{name: Existing, type: select, proxies: [Node]}]\nrules: ['MATCH,Existing']").unwrap()
    }
    #[test]
    fn appends_groups_without_changing_source_groups_or_rules() {
        let mut root = source();
        let before = root.clone();
        let groups = vec![
            group("Manual", GroupKind::Select, &["Existing", "DIRECT"]),
            group("Fast", GroupKind::UrlTest, &["Node"]),
            group("Backup", GroupKind::Fallback, &["Node", "DIRECT"]),
            group("Balanced", GroupKind::LoadBalance, &["Node"]),
        ];
        inject(&mut root, &groups).unwrap();
        assert_eq!(root["rules"], before["rules"]);
        assert_eq!(root["proxy-groups"][0], before["proxy-groups"][0]);
        assert_eq!(root["proxy-groups"].as_sequence().unwrap().len(), 5);
        assert_eq!(root["proxy-groups"][2]["type"], Value::from("url-test"));
        assert_eq!(root["proxy-groups"][4]["strategy"], Value::from("consistent-hashing"));
        assert_eq!(root["proxy-groups"][3]["proxies"][1], Value::from("DIRECT"));
        assert!(crate::app_rules::targets(&root).contains(&"Fast".into()));
    }
    #[test]
    fn rejects_collisions_missing_references_and_cycles() {
        assert!(inject(&mut source(), &[group("Existing", GroupKind::Select, &["Node"])]).is_err());
        assert!(inject(&mut source(), &[group("DIRECT", GroupKind::Select, &["Node"])]).is_err());
        assert!(inject(&mut source(), &[group("New", GroupKind::Select, &["Absent"])]).is_err());
        assert!(inject(&mut source(), &[group("New", GroupKind::Select, &["GLOBAL"])]).is_err());
        assert!(
            inject(
                &mut source(),
                &[
                    group("A", GroupKind::Select, &["B"]),
                    group("B", GroupKind::Select, &["A"])
                ]
            )
            .is_err()
        );
        assert!(inject(&mut source(), &[group("A", GroupKind::Select, &["A"])]).is_err());
        let mut provider = group("New", GroupKind::Select, &[]);
        provider.providers.push("Absent".into());
        assert!(inject(&mut source(), &[provider]).is_err());
    }
    #[test]
    fn provider_groups_survive_changing_provider_nodes_and_store_is_profile_scoped() {
        let mut root = source();
        root.insert(
            "proxy-providers".into(),
            serde_yaml::from_str("Sub: {type: inline, payload: []}").unwrap(),
        );
        let mut dynamic = group("Dynamic", GroupKind::UrlTest, &[]);
        dynamic.providers.push("Sub".into());
        dynamic.filter = "(?i)node".into();
        inject(&mut root, &[dynamic.clone()]).unwrap();
        assert_eq!(root["proxy-groups"][1]["use"][0], Value::from("Sub"));
        let path = std::env::temp_dir().join(format!("mihomo-groups-{}.json", uuid::Uuid::new_v4()));
        let mut store = GroupsStore::default();
        store.by_profile.insert("profile-a".into(), vec![dynamic.clone()]);
        store.save(&path).unwrap();
        let restored = GroupsStore::load(&path).unwrap();
        assert_eq!(restored.for_profile(Some("profile-a")), &[dynamic]);
        assert!(restored.for_profile(Some("profile-b")).is_empty());
        assert!(restored.for_profile(None).is_empty());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn app_rules_can_use_custom_groups_and_prevent_deleting_referenced_groups() {
        let profile = serde_yaml::to_string(&source()).unwrap();
        let group = group("Apps", GroupKind::Select, &["Node"]);
        let app = crate::app_rules::AppRule {
            id: "app".into(),
            kind: crate::app_rules::MatchKind::ProcessName,
            value: "curl".into(),
            target: "Apps".into(),
        };
        let settings = crate::settings::Settings::default();
        assert!(core::build_runtime_config_with_groups(&profile, &settings, &[app.clone()], None, &[group]).is_ok());
        assert!(core::build_runtime_config_with_groups(&profile, &settings, &[app], None, &[]).is_err());
    }
    #[test]
    fn installed_core_validates_all_group_types_and_rejects_bad_filters() {
        let Some(binary) = std::env::var_os("MIHOMO_TEST_BINARY") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("mihomo-group-core-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("runtime.yaml");
        let profile = serde_yaml::to_string(&source()).unwrap();
        let mut groups = vec![
            group("Manual", GroupKind::Select, &["Node", "DIRECT"]),
            group("Fast", GroupKind::UrlTest, &["Node"]),
            group("Backup", GroupKind::Fallback, &["Node", "DIRECT"]),
            group("Balanced", GroupKind::LoadBalance, &["Node"]),
        ];
        let run = |groups: &[CustomGroup]| {
            let runtime = core::build_runtime_config_with_groups(
                &profile,
                &crate::settings::Settings::default(),
                &[],
                None,
                groups,
            )
            .unwrap();
            std::fs::write(&file, runtime).unwrap();
            std::process::Command::new(&binary)
                .arg("-t")
                .arg("-d")
                .arg(&dir)
                .arg("-f")
                .arg(&file)
                .output()
                .unwrap()
        };
        let valid = run(&groups);
        assert!(
            valid.status.success(),
            "{}{}",
            String::from_utf8_lossy(&valid.stdout),
            String::from_utf8_lossy(&valid.stderr)
        );
        groups[0].include_all_proxies = true;
        groups[0].filter = "[".into();
        assert!(!run(&groups).status.success());
        assert!(run(&[]).status.success());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
