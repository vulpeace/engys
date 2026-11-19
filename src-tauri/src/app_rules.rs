//! App-managed process routing, kept separate from profile YAML and settings drafts.
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::core::{self, RUNTIME_CONFIG};
use crate::error::{Error, Result};
use crate::util::write_atomic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchKind {
    ProcessName,
    ProcessPath,
    ProcessNameRegex,
    ProcessPathRegex,
}

impl MatchKind {
    pub fn rule_type(self) -> &'static str {
        match self {
            Self::ProcessName => "PROCESS-NAME",
            Self::ProcessPath => "PROCESS-PATH",
            Self::ProcessNameRegex => "PROCESS-NAME-REGEX",
            Self::ProcessPathRegex => "PROCESS-PATH-REGEX",
        }
    }

    fn is_regex(self) -> bool {
        matches!(self, Self::ProcessNameRegex | Self::ProcessPathRegex)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRule {
    pub id: String,
    pub kind: MatchKind,
    pub value: String,
    pub target: String,
}

impl AppRule {
    pub fn render(&self) -> String {
        format!("{},{},{}", self.kind.rule_type(), self.value, self.target)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RulesFile {
    pub rules: Vec<AppRule>,
    pub select_through_proxy_overrides: BTreeMap<String, bool>,
    by_profile: BTreeMap<String, ProfileRules>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileRules {
    enabled: bool,
    rules: Vec<AppRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Global,
    Profile,
}

impl RulesFile {
    fn profile_rules(&self, profile_id: Option<&str>) -> Option<&ProfileRules> {
        profile_id.and_then(|id| self.by_profile.get(id))
    }

    pub fn rules_for_profile(&self, profile_id: Option<&str>) -> &[AppRule] {
        self.profile_rules(profile_id)
            .filter(|profile| profile.enabled)
            .map_or(&self.rules, |profile| &profile.rules)
    }

    fn save_rules(&mut self, scope: Scope, profile_id: Option<&str>, rules: Vec<AppRule>) -> Result<()> {
        match scope {
            Scope::Global => {
                self.rules = rules;
                if let Some(profile) = profile_id.and_then(|id| self.by_profile.get_mut(id)) {
                    profile.enabled = false;
                }
            }
            Scope::Profile => {
                let id = profile_id.ok_or_else(|| Error::msg("Select a profile before saving profile-specific app rules."))?;
                self.by_profile.insert(id.into(), ProfileRules { enabled: true, rules });
            }
        }
        Ok(())
    }

    pub fn order_override(&self, profile_id: Option<&str>) -> Option<bool> {
        self.select_through_proxy_overrides
            .get(profile_id.unwrap_or(""))
            .copied()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesView {
    pub scope: Scope,
    pub global_rules: Vec<AppRule>,
    pub profile_rules: Option<Vec<AppRule>>,
    pub targets: Vec<String>,
    pub active_profile: Option<String>,
    pub inferred_select_through_proxy: Option<bool>,
    pub order_override: Option<bool>,
}

pub fn load(path: &Path) -> Result<RulesFile> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let saved: RulesFile = serde_json::from_slice(&bytes)
                .map_err(|e| Error::msg(format!("Cannot read app rules at {}: {e}", path.display())))?;
            validate(&saved.rules)?;
            for profile in saved.by_profile.values() {
                validate(&profile.rules)?;
            }
            Ok(saved)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RulesFile::default()),
        Err(e) => Err(e.into()),
    }
}

fn persist(path: &Path, saved: &RulesFile) -> Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(saved)?)
}

/// Infer only from provable config destinations, never from group display names
/// or the first selectable member. Mixed groups and cycles remain unknown.
pub fn infer_select_through_proxy(root: &Mapping) -> Option<bool> {
    let fallback = root.get("rules").and_then(Value::as_sequence).and_then(|rules| {
        rules.iter().filter_map(Value::as_str).find_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            if fields.next()?.eq_ignore_ascii_case("MATCH") {
                fields.next()
            } else {
                None
            }
        })
    });
    fn direct(root: &Mapping, target: &str, visited: &mut HashSet<String>) -> Option<bool> {
        if target == "DIRECT" {
            return Some(true);
        }
        if !visited.insert(target.into()) {
            return None;
        }
        let result = (|| {
            if let Some(proxy) = root.get("proxies").and_then(Value::as_sequence).and_then(|items| {
                items
                    .iter()
                    .find(|p| p.get("name").and_then(Value::as_str) == Some(target))
            }) {
                return proxy
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|t| t.eq_ignore_ascii_case("direct"));
            }
            let group = root
                .get("proxy-groups")
                .and_then(Value::as_sequence)?
                .iter()
                .find(|g| g.get("name").and_then(Value::as_str) == Some(target))?;
            // Provider-backed groups cannot be classified from config alone.
            if group.get("use").is_some()
                || group.get("include-all").and_then(Value::as_bool) == Some(true)
                || group.get("include-all-proxies").and_then(Value::as_bool) == Some(true)
                || group.get("include-all-providers").and_then(Value::as_bool) == Some(true)
            {
                return None;
            }
            let members = group.get("proxies").and_then(Value::as_sequence)?;
            if members.is_empty() {
                return None;
            }
            let mut classification = None;
            for member in members {
                let current = direct(root, member.as_str()?, visited)?;
                if classification.is_some_and(|previous| previous != current) {
                    return None;
                }
                classification = Some(current);
            }
            classification
        })();
        visited.remove(target);
        result
    }
    // With no catch-all, mihomo's unmatched traffic falls back to DIRECT.
    fallback.map_or(Some(true), |target| direct(root, target, &mut HashSet::new()))
}

fn rule_position(rule: &Value) -> bool {
    rule.as_str()
        .and_then(|line| line.split(',').next())
        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("MATCH"))
}

/// Reject ambiguous rule strings before interpolation. Regex syntax is validated
/// by mihomo itself, whose regexp2 engine supports constructs JS/Rust do not.
pub fn validate(rules: &[AppRule]) -> Result<()> {
    if rules.len() > 500 {
        return Err(Error::msg("At most 500 application rules are supported."));
    }
    let mut ids = HashSet::new();
    let mut matches = HashSet::new();
    for (index, rule) in rules.iter().enumerate() {
        let error = |message: &str| Error::msg(format!("Application rule {}: {message}", index + 1));
        if rule.id.is_empty() || !ids.insert(&rule.id) {
            return Err(error("duplicate or missing rule ID."));
        }
        if rule.value.trim().is_empty() || rule.value.len() > 4096 || rule.value.chars().any(char::is_control) {
            return Err(error(
                "enter a name, path, or pattern of at most 4096 bytes without control characters.",
            ));
        }
        if rule.value.trim() != rule.value {
            return Err(error("names, paths, and patterns cannot start or end with whitespace."));
        }
        if !rule.kind.is_regex() && rule.value.contains(',') {
            return Err(error(
                "exact names and paths cannot contain commas. Use an anchored regex with \\x2c instead.",
            ));
        }
        if rule.kind.is_regex() && rule.value.split(',').any(|part| part.trim_matches(' ') != part) {
            return Err(error("use \\x2c for literal commas next to spaces in a regex."));
        }
        if rule.target.is_empty()
            || rule.target.trim() != rule.target
            || rule.target.contains(',')
            || rule.target.chars().any(char::is_control)
        {
            return Err(error("choose a routing target without commas or control characters."));
        }
        let match_key = (
            rule.kind,
            if rule.kind.is_regex() {
                rule.value.clone()
            } else {
                rule.value.to_lowercase()
            },
        );
        if !matches.insert(match_key) {
            return Err(error("this process match is already in the list."));
        }
    }
    Ok(())
}

pub fn targets(root: &Mapping) -> Vec<String> {
    let mut names = vec!["GLOBAL".into(), "DIRECT".into(), "REJECT".into()];
    for key in ["proxy-groups", "proxies"] {
        if let Some(Value::Sequence(items)) = root.get(key) {
            for item in items {
                if let Some(name) = item.get("name").and_then(Value::as_str) {
                    if !name.contains(',')
                        && !name.chars().any(char::is_control)
                        && name.trim() == name
                        && !names.iter().any(|n| n == name)
                    {
                        names.push(name.into());
                    }
                }
            }
        }
    }
    names
}

#[cfg(test)]
pub fn inject(root: &mut Mapping, rules: &[AppRule]) -> Result<()> {
    inject_with_order(root, rules, None)
}

pub fn inject_with_order(root: &mut Mapping, rules: &[AppRule], order_override: Option<bool>) -> Result<()> {
    let select_through_proxy = order_override
        .or_else(|| infer_select_through_proxy(root))
        .unwrap_or(false);
    validate(rules)?;
    if rules.is_empty() {
        return Ok(());
    }
    let available = targets(root);
    for rule in rules {
        if !available.contains(&rule.target) {
            return Err(Error::msg(format!(
                "App rule target '{}' is not in the active profile. Choose another target on Proxied apps.",
                rule.target
            )));
        }
    }
    let original = match root.remove("rules") {
        Some(Value::Sequence(items)) => items,
        Some(Value::Null) | None => Vec::new(),
        _ => return Err(Error::msg("Profile rules must be a YAML list.")),
    };
    let position = if select_through_proxy {
        original.iter().position(rule_position).unwrap_or(original.len())
    } else {
        0
    };
    let mut merged = original;
    merged.splice(position..position, rules.iter().map(|r| Value::from(r.render())));
    root.insert(Value::from("rules"), Value::Sequence(merged));
    root.insert(Value::from("find-process-mode"), Value::from("always"));
    Ok(())
}

fn active_profile(state: &AppState) -> Result<(Option<String>, String)> {
    let profiles = state.profiles.lock().unwrap();
    match profiles.active() {
        Some(profile) => Ok((Some(profile.id.clone()), profiles.read_content(&profile.id)?)),
        None => Ok((None, String::new())),
    }
}

fn rules_view(saved: &RulesFile, active_profile: Option<String>, root: &Mapping) -> RulesView {
    let profile = saved.profile_rules(active_profile.as_deref());
    RulesView {
        scope: if profile.is_some_and(|profile| profile.enabled) { Scope::Profile } else { Scope::Global },
        global_rules: saved.rules.clone(),
        profile_rules: profile.map(|profile| profile.rules.clone()),
        targets: targets(root),
        order_override: saved.order_override(active_profile.as_deref()),
        inferred_select_through_proxy: infer_select_through_proxy(root),
        active_profile,
    }
}

pub async fn view(app: &AppHandle) -> Result<RulesView> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    let (active_profile, content) = active_profile(&state)?;
    let content = crate::routing::apply_profile(&state, active_profile.as_deref(), &content)?;
    let mut root = match serde_yaml::from_str::<Value>(&content)? {
        Value::Mapping(root) => root,
        Value::Null => Mapping::new(),
        _ => return Err(Error::msg("The active profile must be a YAML mapping.")),
    };
    let groups = state.custom_groups.lock().unwrap().for_profile(active_profile.as_deref()).to_vec();
    crate::custom_groups::inject(&mut root, &groups)?;
    let saved = state.app_rules.lock().unwrap();
    Ok(rules_view(&saved, active_profile, &root))
}

/// Validate first, then commit disk and live config as one serialized operation.
/// Rejected rules leave the previous runtime and saved rules intact.
pub async fn save(
    app: AppHandle,
    expected_profile: Option<String>,
    scope: Scope,
    rules: Vec<AppRule>,
    order_override: Option<bool>,
) -> Result<RulesView> {
    validate(&rules)?;
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    let (profile_id, content) = active_profile(&state)?;
    if profile_id != expected_profile {
        return Err(Error::msg("The active profile changed. Reload Proxied apps before saving."));
    }
    let settings = crate::settings::current(&state);
    let status = state.core.lock().unwrap().status(&settings);
    let mut effective = settings.clone();
    if status.running {
        // Settings can contain pending controller changes that explicitly need
        // a restart. Updating app rules must not disconnect this controller.
        effective.controller_port = reqwest::Url::parse(&status.controller)
            .ok()
            .and_then(|url| url.port_or_known_default())
            .ok_or_else(|| Error::msg("The running core's controller address is invalid."))?;
        effective.secret = status.secret.clone();
        effective.overrides.extend(["external-controller".into(), "secret".into()]);
    }
    let previous = state.app_rules.lock().unwrap().clone();
    let mut next = previous.clone();
    next.save_rules(scope, profile_id.as_deref(), rules)?;
    let profile_key = profile_id.as_deref().unwrap_or("").to_owned();
    match order_override {
        Some(value) => {
            next.select_through_proxy_overrides.insert(profile_key, value);
        }
        None => {
            next.select_through_proxy_overrides.remove(&profile_key);
        }
    }
    let groups = state.custom_groups.lock().unwrap().for_profile(profile_id.as_deref()).to_vec();
    let content = crate::routing::apply_profile(&state, profile_id.as_deref(), &content)?;
    let rules = next.rules_for_profile(profile_id.as_deref());
    let runtime = core::build_runtime_config_with_groups(&content, &effective, rules, order_override, &groups)?;
    let root: Mapping = serde_yaml::from_str(&runtime)?;
    let view = rules_view(&next, profile_id.clone(), &root);
    let runtime_file = state.paths.core_dir.join(RUNTIME_CONFIG);
    let mut previous_runtime = match std::fs::read(&runtime_file) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if status.running {
        if let Some(bytes) = &previous_runtime {
            // Mode changes are patched live without rewriting runtime.yaml.
            // Preserve the latest saved mode if a rollback becomes necessary.
            let mut root: Mapping = serde_yaml::from_slice(bytes)?;
            root.insert(Value::from("mode"), Value::from(settings.mode.clone()));
            previous_runtime = Some(serde_yaml::to_string(&root)?.into_bytes());
        }
    }
    // Use a disposable candidate. Invalid regexes never replace runtime.yaml.
    if !rules.is_empty() || status.running {
        let bin = core::resolve_binary(&settings, &state.paths.core_bin_dir)?;
        let candidate = state
            .paths
            .core_dir
            .join(format!("app-rules-{}.yaml", uuid::Uuid::new_v4()));
        write_atomic(&candidate, runtime.as_bytes())?;
        let validation = core::validate_config_file(&bin, &state.paths.core_dir, &candidate).await;
        let _ = std::fs::remove_file(candidate);
        validation?;
    }
    if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::msg("The app is shutting down."));
    }
    persist(&state.paths.app_rules_file, &next)?;
    let commit = async {
        write_atomic(&runtime_file, runtime.as_bytes())?;
        if status.running {
            core::apply_config(&status, &runtime).await?;
        }
        Ok::<(), Error>(())
    }
    .await;
    if let Err(error) = commit {
        let mut failures = Vec::new();
        if let Err(e) = persist(&state.paths.app_rules_file, &previous) {
            failures.push(e.to_string());
        }
        let restore = match &previous_runtime {
            Some(bytes) => write_atomic(&runtime_file, bytes),
            None => match std::fs::remove_file(&runtime_file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            },
        };
        if let Err(e) = restore {
            failures.push(e.to_string());
        }
        // A timed-out controller request might still have applied; explicitly
        // restore its previous config rather than assuming rejection was atomic.
        if status.running {
            if let Some(bytes) = &previous_runtime {
                if let Err(e) = core::apply_config(&status, &String::from_utf8_lossy(bytes)).await {
                    failures.push(e.to_string());
                }
            } else {
                failures.push("the previous live config is unavailable".into());
            }
        }
        return Err(if failures.is_empty() {
            error
        } else {
            Error::msg(format!(
                "{error} Could not fully restore the previous configuration: {}",
                failures.join("; ")
            ))
        });
    }
    *state.app_rules.lock().unwrap() = next;
    if status.running {
        if let Some(id) = &profile_id {
            core::notify_reloaded(&app, id);
        }
    }
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rule(kind: MatchKind, value: &str, target: &str) -> AppRule {
        AppRule {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            value: value.into(),
            target: target.into(),
        }
    }
    #[test]
    fn prepends_all_match_types_without_changing_profile_rules() {
        let mut root: Mapping =
            serde_yaml::from_str("find-process-mode: off\nproxy-groups: [{name: Work}]\nrules: ['MATCH,DIRECT']\n")
                .unwrap();
        let rules = vec![
            rule(MatchKind::ProcessPath, r"C:\Program Files\App\app.exe", "Work"),
            rule(MatchKind::ProcessName, "curl", "DIRECT"),
            rule(MatchKind::ProcessNameRegex, r"(?i)^(firefox|chrome)\.exe$", "REJECT"),
            rule(MatchKind::ProcessPathRegex, r"/opt/app[0-9]{1,3}/.*", "GLOBAL"),
        ];
        inject(&mut root, &rules).unwrap();
        let generated = root["rules"].as_sequence().unwrap();
        assert_eq!(generated.len(), 5);
        assert_eq!(
            generated[0],
            Value::from(r"PROCESS-PATH,C:\Program Files\App\app.exe,Work")
        );
        assert_eq!(
            generated[3],
            Value::from(r"PROCESS-PATH-REGEX,/opt/app[0-9]{1,3}/.*,GLOBAL")
        );
        assert_eq!(generated[4], Value::from("MATCH,DIRECT"));
        assert_eq!(root["find-process-mode"], Value::from("always"));
    }
    #[test]
    fn automatic_order_follows_fallback_and_manual_override_wins() {
        let rules = vec![rule(MatchKind::ProcessName, "curl", "GLOBAL")];
        let direct: Mapping = serde_yaml::from_str("rules: ['DOMAIN,example.com,DIRECT', 'MATCH,DIRECT']").unwrap();
        assert_eq!(infer_select_through_proxy(&direct), Some(true));
        let mut root = direct.clone();
        inject_with_order(&mut root, &rules, None).unwrap();
        assert_eq!(root["rules"][0], Value::from("DOMAIN,example.com,DIRECT"));
        assert_eq!(root["rules"][1], Value::from("PROCESS-NAME,curl,GLOBAL"));
        assert_eq!(root["rules"][2], Value::from("MATCH,DIRECT"));
        let mut root = direct;
        inject_with_order(&mut root, &rules, Some(false)).unwrap();
        assert_eq!(root["rules"][0], Value::from("PROCESS-NAME,curl,GLOBAL"));
        let mut root: Mapping = serde_yaml::from_str(
            "proxies: [{name: Proxy, type: socks5}]\nrules: ['DOMAIN,example.com,DIRECT', 'MATCH,Proxy']",
        )
        .unwrap();
        assert_eq!(infer_select_through_proxy(&root), Some(false));
        inject_with_order(&mut root, &rules, None).unwrap();
        assert_eq!(root["rules"][0], Value::from("PROCESS-NAME,curl,GLOBAL"));
    }

    #[test]
    fn late_rules_stay_reachable_and_preserve_the_profile_order() {
        let rules = vec![
            rule(MatchKind::ProcessName, "one", "DIRECT"),
            rule(MatchKind::ProcessName, "two", "REJECT"),
        ];
        let mut root: Mapping =
            serde_yaml::from_str("rules: ['DOMAIN,a,DIRECT', 'MATCH,GLOBAL', 'DOMAIN,unreachable,DIRECT']").unwrap();
        inject_with_order(&mut root, &rules, Some(true)).unwrap();
        assert_eq!(root["rules"][0], Value::from("DOMAIN,a,DIRECT"));
        assert_eq!(root["rules"][1], Value::from("PROCESS-NAME,one,DIRECT"));
        assert_eq!(root["rules"][2], Value::from("PROCESS-NAME,two,REJECT"));
        assert_eq!(root["rules"][3], Value::from("MATCH,GLOBAL"));
        assert_eq!(root["rules"][4], Value::from("DOMAIN,unreachable,DIRECT"));
        let mut root: Mapping = serde_yaml::from_str("rules: ['DOMAIN,a,DIRECT']").unwrap();
        assert_eq!(infer_select_through_proxy(&root), Some(true));
        inject_with_order(&mut root, &rules, None).unwrap();
        assert_eq!(root["rules"][2], Value::from("PROCESS-NAME,two,REJECT"));
    }

    #[test]
    fn inference_handles_direct_aliases_and_ambiguous_or_cyclic_groups() {
        let root: Mapping = serde_yaml::from_str("proxies: [{name: Bypass, type: direct}]\nproxy-groups: [{name: Local, proxies: [DIRECT, Bypass]}]\nrules: ['MATCH,Local']").unwrap();
        assert_eq!(infer_select_through_proxy(&root), Some(true));
        let mixed: Mapping =
            serde_yaml::from_str("proxy-groups: [{name: Mixed, proxies: [DIRECT, GLOBAL]}]\nrules: ['MATCH,Mixed']")
                .unwrap();
        assert_eq!(infer_select_through_proxy(&mixed), None);
        let cycle: Mapping = serde_yaml::from_str(
            "proxy-groups: [{name: A, proxies: [B]}, {name: B, proxies: [A]}]\nrules: ['MATCH,A']",
        )
        .unwrap();
        assert_eq!(infer_select_through_proxy(&cycle), None);
    }

    #[test]
    fn no_rules_leaves_original_process_mode_and_rules_unchanged() {
        let mut root: Mapping = serde_yaml::from_str("find-process-mode: off\nrules: ['MATCH,DIRECT']").unwrap();
        let original = root.clone();
        inject(&mut root, &[]).unwrap();
        assert_eq!(root, original);
    }
    #[test]
    fn rejects_ambiguous_payloads_missing_targets_and_duplicate_matches() {
        assert!(validate(&[rule(MatchKind::ProcessName, "app,DIRECT", "REJECT")]).is_err());
        assert!(validate(&[rule(MatchKind::ProcessNameRegex, "foo\n.*", "DIRECT")]).is_err());
        assert!(validate(&[rule(MatchKind::ProcessNameRegex, "app", "group,DIRECT")]).is_err());
        assert!(
            validate(&[
                rule(MatchKind::ProcessName, "App.EXE", "DIRECT"),
                rule(MatchKind::ProcessName, "app.exe", "REJECT")
            ])
            .is_err()
        );
        assert!(inject(&mut Mapping::new(), &[rule(MatchKind::ProcessName, "app", "missing")]).is_err());
        let mut root: Mapping = serde_yaml::from_str("rules: invalid").unwrap();
        assert!(inject(&mut root, &[rule(MatchKind::ProcessName, "app", "DIRECT")]).is_err());
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn profile_scope_replaces_global_rules_and_keeps_its_list_when_disabled() {
        let mut config: RulesFile = serde_json::from_value(serde_json::json!({
            "rules": [{ "id": "global", "kind": "processName", "value": "browser", "target": "GLOBAL" }]
        })).unwrap();
        let root = Mapping::new();
        let initial = rules_view(&config, Some("profile-a".into()), &root);
        assert_eq!(initial.scope, Scope::Global);
        assert_eq!(initial.global_rules, config.rules);
        assert!(initial.profile_rules.is_none());

        let mut copied = initial.global_rules;
        copied[0].target = "DIRECT".into();
        config.save_rules(Scope::Profile, Some("profile-a"), copied.clone()).unwrap();
        assert_eq!(config.rules_for_profile(Some("profile-a")), copied);
        assert_eq!(config.rules_for_profile(Some("profile-b")), config.rules);

        config.save_rules(Scope::Profile, Some("profile-b"), vec![]).unwrap();
        let global = config.rules.clone();
        config.save_rules(Scope::Global, Some("profile-a"), global.clone()).unwrap();
        let restored: RulesFile = serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        let view = rules_view(&restored, Some("profile-a".into()), &root);
        assert_eq!(view.scope, Scope::Global);
        assert_eq!(view.profile_rules, Some(copied));
        assert_eq!(restored.rules_for_profile(Some("profile-a")), global);
        assert_eq!(rules_view(&restored, Some("profile-b".into()), &root).scope, Scope::Profile);
        assert!(restored.rules_for_profile(Some("profile-b")).is_empty());
    }

    #[test]
    fn saves_and_clears_rules_independently_of_profile_and_settings() {
        let dir = std::env::temp_dir().join(format!("mihomo-app-rules-{}", uuid::Uuid::new_v4()));
        let path = dir.join("app-rules.json");
        assert!(load(&path).unwrap().rules.is_empty());
        let rules = vec![AppRule {
            id: "one".into(),
            kind: MatchKind::ProcessPath,
            value: r"C:\Apps\Mihomo test\app.exe".into(),
            target: "GLOBAL".into(),
        }];
        persist(
            &path,
            &RulesFile {
                rules: rules.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(load(&path).unwrap().rules, rules);
        let mut config = load(&path).unwrap();
        config.select_through_proxy_overrides.insert("profile-a".into(), true);
        config.select_through_proxy_overrides.insert("profile-b".into(), false);
        persist(&path, &config).unwrap();
        let restored = load(&path).unwrap();
        assert_eq!(restored.order_override(Some("profile-a")), Some(true));
        assert_eq!(restored.order_override(Some("profile-b")), Some(false));
        assert_eq!(restored.order_override(Some("profile-c")), None);
        // Existing rule files without an override continue to infer from their profile.
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({ "rules": rules })).unwrap(),
        )
        .unwrap();
        assert_eq!(load(&path).unwrap().order_override(Some("profile-a")), None);
        persist(&path, &RulesFile::default()).unwrap();
        assert!(load(&path).unwrap().rules.is_empty());
        std::fs::write(&path, b"broken").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Set MIHOMO_TEST_BINARY to opt into real parser validation without a GUI,
    /// a network listener, user profiles, or changes to the installed app's data.
    #[test]
    fn installed_core_validates_generated_rules_and_rejects_invalid_regex() {
        let Some(binary) = std::env::var_os("MIHOMO_TEST_BINARY") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("mihomo-app-core-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("candidate.yaml");
        let profile = "proxy-groups: [{name: Work, type: select, proxies: [DIRECT]}]\nrules: ['MATCH,DIRECT']\n";
        let kinds = [
            MatchKind::ProcessPath,
            MatchKind::ProcessName,
            MatchKind::ProcessNameRegex,
            MatchKind::ProcessPathRegex,
        ];
        let values = [
            r"C:\Program Files\App\app.exe",
            "curl",
            r"(?i)^(firefox|chrome)\.exe$",
            r"/opt/app[0-9]{1,3}/.*",
        ];
        let mut rules = kinds
            .into_iter()
            .zip(values)
            .enumerate()
            .map(|(i, (kind, value))| AppRule {
                id: i.to_string(),
                kind,
                value: value.into(),
                target: "Work".into(),
            })
            .collect::<Vec<_>>();
        let run = |rules: &[AppRule]| {
            let yaml =
                core::build_runtime_config_with_apps(profile, &crate::settings::Settings::default(), rules).unwrap();
            std::fs::write(&config, yaml).unwrap();
            std::process::Command::new(&binary)
                .arg("-t")
                .arg("-d")
                .arg(&dir)
                .arg("-f")
                .arg(&config)
                .output()
                .unwrap()
        };
        let valid = run(&rules);
        assert!(
            valid.status.success(),
            "{}{}",
            String::from_utf8_lossy(&valid.stdout),
            String::from_utf8_lossy(&valid.stderr)
        );
        rules[2].value = "[".into();
        let invalid = run(&rules);
        assert!(!invalid.status.success(), "mihomo accepted an invalid regex");
        let cleared = run(&[]);
        assert!(cleared.status.success());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
