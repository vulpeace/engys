//! Subscription rule overlays, anchored by rule text and duplicate occurrence.
use crate::{
    AppState,
    core::{self, RUNTIME_CONFIG},
    error::{Error, Result},
    util::write_atomic,
};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub source_index: Option<usize>,
    pub rule: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Overlay {
    pub base_rules: Vec<String>,
    pub entries: Vec<Entry>,
    pub replace_rules: bool,
    /// A YAML mapping of custom rule-provider names to definitions.
    pub providers_yaml: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Store {
    pub by_profile: BTreeMap<String, Overlay>,
}
impl Store {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub source_rules: Vec<String>,
    pub entries: Vec<Entry>,
    pub replace_rules: bool,
    pub providers_yaml: String,
    pub inherited_providers_yaml: String,
    /// Exact source text for optimistic concurrency with subscription refreshes.
    pub revision: String,
}
fn root(content: &str) -> Result<Mapping> {
    match serde_yaml::from_str(content)? {
        Value::Mapping(m) => Ok(m),
        Value::Null => Ok(Mapping::new()),
        _ => Err(Error::msg("Profile must be a YAML mapping.")),
    }
}
fn source_rules(root: &Mapping) -> Result<Vec<String>> {
    match root.get("rules") {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Sequence(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::msg("Profile rules must contain strings."))
            })
            .collect(),
        _ => Err(Error::msg("Profile rules must be a YAML list.")),
    }
}
fn keys(rules: &[String]) -> Vec<(String, usize)> {
    let mut counts = HashMap::new();
    rules
        .iter()
        .map(|rule| {
            let count = counts.entry(rule.clone()).or_insert(0);
            *count += 1;
            (rule.clone(), *count)
        })
        .collect()
}
/// New subscription rules stay inherited. Changed/removed subscription rules lose their old overrides.
/// Custom rules stay before their next surviving inherited anchor, or at the end if none survives.
pub fn rebase(source: &[String], overlay: Option<&Overlay>) -> Vec<Entry> {
    let Some(old) = overlay else {
        return source
            .iter()
            .enumerate()
            .map(|(i, rule)| Entry {
                id: format!("source-{i}"),
                source_index: Some(i),
                rule: rule.clone(),
            })
            .collect();
    };
    if old.replace_rules {
        return old.entries.clone();
    }
    let old_keys = keys(&old.base_rules);
    let new_keys = keys(source);
    let positions: HashMap<_, _> = new_keys.iter().cloned().enumerate().map(|(i, k)| (k, i)).collect();
    let old_positions: HashMap<_, _> = old_keys.iter().cloned().enumerate().map(|(i, k)| (k, i)).collect();
    let edits: HashMap<_, _> = old
        .entries
        .iter()
        .filter_map(|e| e.source_index.map(|i| (i, e)))
        .collect();
    let mut custom: BTreeMap<usize, Vec<Entry>> = BTreeMap::new();
    let mut anchor = source.len();
    for entry in old.entries.iter().rev() {
        if let Some(i) = entry.source_index {
            if let Some(position) = old_keys.get(i).and_then(|key| positions.get(key)) {
                anchor = *position;
            }
        } else {
            custom.entry(anchor).or_default().push(entry.clone());
        }
    }
    for entries in custom.values_mut() {
        entries.reverse();
    }
    let saved_order: Vec<_> = old.entries.iter().filter_map(|entry| entry.source_index).collect();
    if saved_order.windows(2).any(|pair| pair[0] > pair[1]) {
        // Keep the user's explicit inherited order. New subscription rules attach before their
        // next surviving source anchor; deleted source rules do not reappear.
        let order: Vec<_> = saved_order
            .iter()
            .filter_map(|i| old_keys.get(*i).and_then(|key| positions.get(key)).copied())
            .collect();
        let retained: HashSet<_> = order.iter().copied().collect();
        let mut added: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut anchor = source.len();
        for i in (0..source.len()).rev() {
            if retained.contains(&i) {
                anchor = i;
            } else if !old_positions.contains_key(&new_keys[i]) {
                added.entry(anchor).or_default().push(i);
            }
        }
        for indices in added.values_mut() {
            indices.reverse();
        }
        let mut result = vec![];
        for i in order.into_iter().chain(std::iter::once(source.len())) {
            for new in added.remove(&i).unwrap_or_default() {
                result.push(Entry {
                    id: format!("source-{new}"),
                    source_index: Some(new),
                    rule: source[new].clone(),
                });
            }
            result.extend(custom.remove(&i).unwrap_or_default());
            if i < source.len() {
                let old_i = old_positions[&new_keys[i]];
                result.push(Entry {
                    id: format!("source-{i}"),
                    source_index: Some(i),
                    rule: edits[&old_i].rule.clone(),
                });
            }
        }
        return result;
    }
    let mut result = vec![];
    for (i, rule) in source.iter().enumerate() {
        result.extend(custom.remove(&i).unwrap_or_default());
        let value = match old_positions.get(&new_keys[i]) {
            Some(old_i) => match edits.get(old_i) {
                Some(e) => e.rule.clone(),
                None => continue,
            },
            None => rule.clone(),
        };
        result.push(Entry {
            id: format!("source-{i}"),
            source_index: Some(i),
            rule: value,
        });
    }
    result.extend(custom.remove(&source.len()).unwrap_or_default());
    result
}
fn providers(yaml: &str) -> Result<Mapping> {
    let m = root(yaml)?;
    for (name, definition) in &m {
        let name = name
            .as_str()
            .ok_or_else(|| Error::msg("Provider names must be strings."))?;
        if name.trim() != name || name.is_empty() || name.contains(',') || name.chars().any(char::is_control) {
            return Err(Error::msg("Invalid rule-provider name."));
        }
        if !definition.is_mapping() {
            return Err(Error::msg("Each rule-provider must be a YAML mapping."));
        }
    }
    Ok(m)
}
fn inject(root: &mut Mapping, overlay: &Overlay) -> Result<()> {
    let entries = rebase(&source_rules(root)?, Some(overlay));
    root.insert(
        "rules".into(),
        serde_yaml::to_value(entries.iter().map(|e| &e.rule).collect::<Vec<_>>())?,
    );
    let additions = providers(&overlay.providers_yaml)?;
    if !additions.is_empty() {
        let mut inherited = match root.get("rule-providers") {
            None | Some(Value::Null) => Mapping::new(),
            Some(Value::Mapping(m)) => m.clone(),
            _ => return Err(Error::msg("Profile rule-providers must be a mapping.")),
        };
        for (name, definition) in additions {
            if inherited.contains_key(&name) {
                return Err(Error::msg(format!(
                    "Rule-provider '{}' already exists in the subscription.",
                    name.as_str().unwrap()
                )));
            }
            inherited.insert(name, definition);
        }
        root.insert("rule-providers".into(), Value::Mapping(inherited));
    }
    Ok(())
}
pub fn apply_profile(state: &AppState, id: Option<&str>, content: &str) -> Result<String> {
    let overlay = id.and_then(|id| state.routing.lock().unwrap().by_profile.get(id).cloned());
    apply_overlay(content, overlay.as_ref())
}

pub fn apply_overlay(content: &str, overlay: Option<&Overlay>) -> Result<String> {
    let Some(overlay) = overlay else {
        return Ok(content.into());
    };
    let mut root = root(content)?;
    inject(&mut root, overlay)?;
    Ok(serde_yaml::to_string(&root)?)
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceRuleChoice {
    Merge,
    Override,
    Subscription,
}

pub fn source_rule_conflict(old: &str, new: &str, overlay: Option<&Overlay>) -> Result<bool> {
    let previous = source_rules(&root(old)?)?;
    let next = source_rules(&root(new)?)?;
    Ok(previous != next && overlay.is_some_and(|overlay| {
        overlay.replace_rules || rebase(&previous, Some(overlay)) != rebase(&previous, None)
    }))
}

pub fn reconcile_source(
    old: &str,
    new: &str,
    overlay: &Overlay,
    choice: SourceRuleChoice,
) -> Result<Overlay> {
    let previous = source_rules(&root(old)?)?;
    let base_rules = source_rules(&root(new)?)?;
    let replace_rules = matches!(choice, SourceRuleChoice::Override);
    let mut entries = match choice {
        SourceRuleChoice::Merge => {
            let mut inherited = rebase(&base_rules, None).into_iter();
            let mut merged = Vec::new();
            for (index, mut entry) in rebase(&previous, Some(overlay)).into_iter().enumerate() {
                if entry.source_index.is_some_and(|i| previous[i] == entry.rule) {
                    continue;
                }
                let gap = index.saturating_sub(merged.len());
                merged.extend(inherited.by_ref().take(gap));
                entry.source_index = None;
                merged.push(entry);
            }
            merged.extend(inherited);
            merged
        }
        SourceRuleChoice::Override => {
            let mut entries = rebase(&previous, Some(overlay));
            for entry in &mut entries {
                entry.source_index = None;
            }
            entries
        }
        SourceRuleChoice::Subscription => rebase(&base_rules, None),
    };
    // Edited inherited rows can have IDs also generated by the new source.
    for entry in entries.iter_mut().filter(|entry| entry.source_index.is_none() && entry.id.starts_with("source-")) {
        entry.id = uuid::Uuid::new_v4().to_string();
    }
    Ok(Overlay { base_rules, entries, replace_rules, providers_yaml: overlay.providers_yaml.clone() })
}
pub fn view(app: &AppHandle) -> Result<View> {
    let state = app.state::<AppState>();
    let profiles = state.profiles.lock().unwrap();
    let (id, name, content) = match profiles.active() {
        Some(p) => (Some(p.id.clone()), Some(p.name.clone()), profiles.read_content(&p.id)?),
        None => (None, None, String::new()),
    };
    let root = root(&content)?;
    let source = source_rules(&root)?;
    let overlay = id
        .as_ref()
        .and_then(|id| state.routing.lock().unwrap().by_profile.get(id).cloned());
    Ok(View {
        profile_id: id,
        profile_name: name,
        entries: rebase(&source, overlay.as_ref()),
        replace_rules: overlay.as_ref().is_some_and(|o| o.replace_rules),
        source_rules: source,
        providers_yaml: overlay.map(|o| o.providers_yaml).unwrap_or_default(),
        inherited_providers_yaml: serde_yaml::to_string(
            root.get("rule-providers").unwrap_or(&Value::Mapping(Mapping::new())),
        )?,
        revision: content,
    })
}
fn validate_entries(source: &[String], entries: &[Entry], previous: &[Entry], edit_inherited: bool) -> Result<()> {
    if entries.len() > 100000 {
        return Err(Error::msg("Too many rules."));
    }
    let mut ids = HashSet::new();
    let mut indices = HashSet::new();
    for entry in entries {
        if entry.id.is_empty()
            || !ids.insert(&entry.id)
            || entry.rule.trim().is_empty()
            || entry.rule.chars().any(char::is_control)
        {
            return Err(Error::msg(
                "Rules need unique IDs and a nonempty, single-line definition.",
            ));
        }
        if let Some(i) = entry.source_index {
            if i >= source.len() || !indices.insert(i) {
                return Err(Error::msg("Invalid inherited rule reference."));
            }
        }
    }
    let inherited = |rows: &[Entry]| {
        rows.iter()
            .filter(|e| e.source_index.is_some())
            .map(|e| (e.source_index, e.rule.clone()))
            .collect::<Vec<_>>()
    };
    if !edit_inherited && inherited(entries) != inherited(previous) {
        return Err(Error::msg(
            "Unlock inherited rule editing before changing, deleting, or moving subscription rules.",
        ));
    }
    Ok(())
}

pub async fn save(
    app: AppHandle,
    profile_id: String,
    revision: String,
    entries: Vec<Entry>,
    providers_yaml: String,
    edit_inherited: bool,
    replace_rules: bool,
) -> Result<View> {
    let state = app.state::<AppState>();
    let _operation = state.core_operation.lock().await;
    let content = {
        let profiles = state.profiles.lock().unwrap();
        if profiles.index.active.as_deref() != Some(profile_id.as_str()) {
            return Err(Error::msg("The active profile changed. Reload Rules before saving."));
        }
        profiles.read_content(&profile_id)?
    };
    if content != revision {
        return Err(Error::msg(
            "The subscription changed while editing. Reload Rules before saving.",
        ));
    }
    let source = source_rules(&root(&content)?)?;
    let previous = state.routing.lock().unwrap().clone();
    let saved = previous.by_profile.get(&profile_id);
    let was_replacing = saved.is_some_and(|o| o.replace_rules);
    if replace_rules && !was_replacing {
        return Err(Error::msg("Choose Override subscription when changing the profile source."));
    }
    let current = rebase(&source, saved.filter(|_| replace_rules == was_replacing));
    validate_entries(&source, &entries, &current, edit_inherited)?;
    providers(&providers_yaml)?;
    let overlay = Overlay {
        base_rules: source,
        entries,
        providers_yaml,
        replace_rules,
    };
    let mut next = previous.clone();
    next.by_profile.insert(profile_id.clone(), overlay.clone());
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
    let mut edited = root(&content)?;
    inject(&mut edited, &overlay)?;
    let content = serde_yaml::to_string(&edited)?;
    let groups = state
        .custom_groups
        .lock()
        .unwrap()
        .for_profile(Some(&profile_id))
        .to_vec();
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
    {
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
            .join(format!("rules-{}.yaml", uuid::Uuid::new_v4()));
        write_atomic(&candidate, runtime.as_bytes())?;
        let valid = core::validate_config_file(&bin, &state.paths.core_dir, &candidate).await;
        let _ = std::fs::remove_file(candidate);
        valid?;
    }
    if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::msg("The app is shutting down."));
    }
    next.save(&state.paths.routing_file)?;
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
        if let Err(e) = previous.save(&state.paths.routing_file) {
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
                "{error} Restoring the previous rules failed: {}",
                failures.join("; ")
            ))
        });
    }
    *state.routing.lock().unwrap() = next;
    if status.running {
        core::notify_reloaded(&app, &profile_id);
    }
    let _ = app.emit("routing-changed", &profile_id);
    view(&app)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rules(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }
    fn custom(id: &str, rule: &str) -> Entry {
        Entry {
            id: id.into(),
            source_index: None,
            rule: rule.into(),
        }
    }
    fn overlay(base: &[String], entries: Vec<Entry>) -> Overlay {
        Overlay {
            base_rules: base.to_vec(),
            entries,
            replace_rules: false,
            providers_yaml: String::new(),
        }
    }

    #[test]
    fn source_change_choices_preserve_positions_pin_the_old_list_or_use_the_new_list() {
        let old = "rules: ['DOMAIN,a,DIRECT', 'DOMAIN,b,DIRECT', 'MATCH,DIRECT']";
        let new = "rules: ['DOMAIN,c,DIRECT', 'DOMAIN,d,DIRECT', 'MATCH,REJECT']";
        let base = source_rules(&root(old).unwrap()).unwrap();
        let mut entries = rebase(&base, None);
        entries[1].rule = "DOMAIN,b,REJECT".into();
        entries.insert(1, custom("custom", "DOMAIN,x,REJECT"));
        let saved = overlay(&base, entries);
        assert!(source_rule_conflict(old, new, Some(&saved)).unwrap());
        assert!(!source_rule_conflict(old, old, Some(&saved)).unwrap());
        assert!(!source_rule_conflict(old, new, None).unwrap());

        let merged = reconcile_source(old, new, &saved, SourceRuleChoice::Merge).unwrap();
        assert_eq!(values(&rebase(&merged.base_rules, Some(&merged))), rules(&[
            "DOMAIN,c,DIRECT", "DOMAIN,x,REJECT", "DOMAIN,b,REJECT", "DOMAIN,d,DIRECT", "MATCH,REJECT",
        ]));
        validate_entries(&merged.base_rules, &merged.entries, &merged.entries, false).unwrap();

        let pinned = reconcile_source(old, new, &saved, SourceRuleChoice::Override).unwrap();
        assert!(pinned.replace_rules);
        assert_eq!(values(&rebase(&rules(&["MATCH,GLOBAL"]), Some(&pinned))), values(&saved.entries));
        let inherited = reconcile_source(old, new, &saved, SourceRuleChoice::Subscription).unwrap();
        assert_eq!(values(&inherited.entries), inherited.base_rules);
        for overlay in [merged, pinned, inherited] {
            assert_eq!(overlay.providers_yaml, saved.providers_yaml);
        }
    }
    fn values(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.rule.clone()).collect()
    }

    #[test]
    fn inserts_before_between_and_after_inherited_without_modifying_them() {
        let base = rules(&["DOMAIN,a,DIRECT", "MATCH,DIRECT"]);
        let mut entries = rebase(&base, None);
        entries.insert(0, custom("first", "DOMAIN,first,REJECT"));
        entries.insert(2, custom("middle", "DOMAIN,middle,REJECT"));
        entries.push(custom("last", "DOMAIN,last,REJECT"));
        validate_entries(&base, &entries, &rebase(&base, None), false).unwrap();
        let mut root = root("rules: ['DOMAIN,a,DIRECT', 'MATCH,DIRECT']\n").unwrap();
        inject(&mut root, &overlay(&base, entries.clone())).unwrap();
        assert_eq!(source_rules(&root).unwrap(), values(&entries));
    }
    #[test]
    fn locked_rules_reject_edits_deletions_and_movement_but_allow_custom_movement() {
        let base = rules(&["DOMAIN,a,DIRECT", "MATCH,DIRECT"]);
        let original = rebase(&base, None);
        let mut changed = original.clone();
        changed[0].rule = "DOMAIN,a,REJECT".into();
        assert!(validate_entries(&base, &changed, &original, false).is_err());
        validate_entries(&base, &changed, &original, true).unwrap();
        // A saved override is read-only when the tab is reopened; custom rules can still be added.
        let mut added = changed.clone();
        added.push(custom("new", "DOMAIN,new,DIRECT"));
        validate_entries(&base, &added, &changed, false).unwrap();
        assert!(validate_entries(&base, &original[1..], &original, false).is_err());
        validate_entries(&base, &original[1..], &original, true).unwrap();
        changed.swap(0, 1);
        assert!(validate_entries(&base, &changed, &original, false).is_err());
        changed[1].source_index = Some(99);
        assert!(validate_entries(&base, &changed, &original, true).is_err());
        let mut duplicate = original.clone();
        duplicate.push(original[0].clone());
        assert!(validate_entries(&base, &duplicate, &original, true).is_err());
    }
    #[test]
    fn refresh_rebases_insertions_edits_deletions_and_duplicate_occurrences() {
        let base = rules(&[
            "DOMAIN,a,DIRECT",
            "DOMAIN,a,DIRECT",
            "DOMAIN,removed,DIRECT",
            "MATCH,DIRECT",
        ]);
        let mut entries = rebase(&base, None);
        entries[1].rule = "DOMAIN,a,REJECT".into();
        entries.remove(2);
        entries.insert(1, custom("before-second", "DOMAIN,custom,DIRECT"));
        entries.push(custom("end", "DOMAIN,end,DIRECT"));
        let saved = overlay(&base, entries);
        let updated = rules(&[
            "DOMAIN,new,DIRECT",
            "DOMAIN,a,DIRECT",
            "DOMAIN,a,DIRECT",
            "DOMAIN,removed,DIRECT",
            "MATCH,DIRECT",
        ]);
        assert_eq!(
            values(&rebase(&updated, Some(&saved))),
            rules(&[
                "DOMAIN,new,DIRECT",
                "DOMAIN,a,DIRECT",
                "DOMAIN,custom,DIRECT",
                "DOMAIN,a,REJECT",
                "MATCH,DIRECT",
                "DOMAIN,end,DIRECT"
            ])
        );
        let updated = rules(&["DOMAIN,new,DIRECT", "MATCH,DIRECT"]);
        assert_eq!(
            values(&rebase(&updated, Some(&saved))),
            rules(&[
                "DOMAIN,new,DIRECT",
                "DOMAIN,custom,DIRECT",
                "MATCH,DIRECT",
                "DOMAIN,end,DIRECT"
            ])
        );
        assert_eq!(
            values(&rebase(&[], Some(&saved))),
            rules(&["DOMAIN,custom,DIRECT", "DOMAIN,end,DIRECT"])
        );
    }
    #[test]
    fn reordered_inherited_rules_round_trip_and_rebase_with_custom_insertions() {
        let base = rules(&["DOMAIN,a,DIRECT", "DOMAIN,b,DIRECT", "DOMAIN,c,DIRECT", "MATCH,DIRECT"]);
        let original = rebase(&base, None);
        let entries = vec![
            original[2].clone(),
            custom("custom", "DOMAIN,custom,REJECT"),
            original[0].clone(),
            original[1].clone(),
            original[3].clone(),
        ];
        assert!(validate_entries(&base, &entries, &original, false).is_err());
        validate_entries(&base, &entries, &original, true).unwrap();
        let saved = overlay(&base, entries.clone());
        assert_eq!(rebase(&base, Some(&saved)), entries);
        let mut config =
            root("rules: ['DOMAIN,a,DIRECT', 'DOMAIN,b,DIRECT', 'DOMAIN,c,DIRECT', 'MATCH,DIRECT']").unwrap();
        inject(&mut config, &saved).unwrap();
        assert_eq!(source_rules(&config).unwrap(), values(&entries));
        let updated = rules(&[
            "DOMAIN,a,DIRECT",
            "DOMAIN,new,DIRECT",
            "DOMAIN,b,DIRECT",
            "DOMAIN,c,DIRECT",
            "MATCH,DIRECT",
        ]);
        assert_eq!(
            values(&rebase(&updated, Some(&saved))),
            rules(&[
                "DOMAIN,c,DIRECT",
                "DOMAIN,custom,REJECT",
                "DOMAIN,a,DIRECT",
                "DOMAIN,new,DIRECT",
                "DOMAIN,b,DIRECT",
                "MATCH,DIRECT"
            ])
        );
        let updated = rules(&["DOMAIN,b,DIRECT", "DOMAIN,c,DIRECT", "MATCH,DIRECT"]);
        assert_eq!(
            values(&rebase(&updated, Some(&saved))),
            rules(&[
                "DOMAIN,c,DIRECT",
                "DOMAIN,custom,REJECT",
                "DOMAIN,b,DIRECT",
                "MATCH,DIRECT"
            ])
        );
    }
    #[test]
    fn reordered_deletions_stay_deleted_and_new_tail_rules_precede_tail_custom_rules() {
        let base = rules(&["DOMAIN,a,DIRECT", "DOMAIN,b,DIRECT", "DOMAIN,c,DIRECT"]);
        let original = rebase(&base, None);
        let entries = vec![original[2].clone(), original[0].clone(), custom("tail", "MATCH,REJECT")];
        let saved = overlay(&base, entries);
        let updated = rules(&[
            "DOMAIN,a,DIRECT",
            "DOMAIN,b,DIRECT",
            "DOMAIN,c,DIRECT",
            "DOMAIN,new,DIRECT",
        ]);
        assert_eq!(
            values(&rebase(&updated, Some(&saved))),
            rules(&[
                "DOMAIN,c,DIRECT",
                "DOMAIN,a,DIRECT",
                "DOMAIN,new,DIRECT",
                "MATCH,REJECT"
            ])
        );
        assert_eq!(values(&rebase(&[], Some(&saved))), rules(&["MATCH,REJECT"]));
    }
    #[test]
    fn providers_merge_without_replacing_subscription_definitions() {
        let mut root = root("rules: ['MATCH,DIRECT']\nrule-providers: {Original: {type: inline, behavior: domain, payload: ['+.original.test']}}\n").unwrap();
        let original = root["rule-providers"]["Original"].clone();
        let mut saved = overlay(
            &source_rules(&root).unwrap(),
            rebase(&source_rules(&root).unwrap(), None),
        );
        saved.providers_yaml = "Custom: {type: inline, behavior: domain, payload: ['+.custom.test']}".into();
        inject(&mut root, &saved).unwrap();
        assert_eq!(root["rule-providers"]["Original"], original);
        assert_eq!(root["rule-providers"]["Custom"]["type"], Value::from("inline"));
        assert!(inject(&mut root, &saved).is_err());
        for yaml in ["[]", "P: []", "'bad,name': {}", "42: {}"] {
            assert!(providers(yaml).is_err());
        }
    }
    #[test]
    fn persistence_is_profile_scoped_and_rebased_overrides_survive_restart() {
        let dir = std::env::temp_dir().join(format!("routing-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("routing.json");
        let mut store = Store::load(&path).unwrap();
        let base = rules(&["MATCH,DIRECT"]);
        let mut entries = rebase(&base, None);
        entries.insert(0, custom("custom", "DOMAIN,a,REJECT"));
        store.by_profile.insert("a".into(), overlay(&base, entries.clone()));
        store.save(&path).unwrap();
        let loaded = Store::load(&path).unwrap();
        assert_eq!(rebase(&base, loaded.by_profile.get("a")), entries);
        assert_eq!(values(&rebase(&base, loaded.by_profile.get("b"))), base);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn installed_core_validates_rules_and_providers_and_rejects_bad_references() {
        let Some(binary) = std::env::var_os("MIHOMO_TEST_BINARY") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("routing-core-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = rules(&["DOMAIN,original.test,DIRECT", "MATCH,DIRECT"]);
        let mut entries = rebase(&base, None);
        entries.insert(1, custom("provider", "RULE-SET,Custom,REJECT"));
        let mut saved = overlay(&base, entries);
        saved.providers_yaml = "Custom: {type: inline, behavior: domain, payload: ['+.example.test']}".into();
        let validate = |saved: &Overlay| {
            let mut config = root("rules: ['DOMAIN,original.test,DIRECT', 'MATCH,DIRECT']").unwrap();
            inject(&mut config, saved).unwrap();
            let runtime = core::build_runtime_config_with_groups(
                &serde_yaml::to_string(&config).unwrap(),
                &crate::settings::Settings::default(),
                &[],
                None,
                &[],
            )
            .unwrap();
            let path = dir.join("test.yaml");
            std::fs::write(&path, runtime).unwrap();
            std::process::Command::new(&binary)
                .arg("-t")
                .arg("-d")
                .arg(&dir)
                .arg("-f")
                .arg(path)
                .output()
                .unwrap()
        };
        let good = validate(&saved);
        assert!(
            good.status.success(),
            "{} {}",
            String::from_utf8_lossy(&good.stdout),
            String::from_utf8_lossy(&good.stderr)
        );
        saved.entries[1].rule = "RULE-SET,Missing,DIRECT".into();
        assert!(!validate(&saved).status.success());
        saved.entries[1].rule = "DOMAIN,example.test,MissingTarget".into();
        assert!(!validate(&saved).status.success());
        saved.entries[1].rule = "NOT-A-RULE,example.test,DIRECT".into();
        assert!(!validate(&saved).status.success());
        saved.entries[1].rule = "RULE-SET,Custom,REJECT".into();
        saved.providers_yaml = "Custom: {type: unknown, behavior: domain}".into();
        assert!(!validate(&saved).status.success());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
