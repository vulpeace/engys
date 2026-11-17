use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::util::write_atomic;

/// Persistent application settings. Fields that map onto mihomo config keys
/// are inherited from the active profile unless explicitly marked as overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Explicit path to the mihomo binary. `None` = auto-detect.
    pub core_path: Option<String>,
    pub overrides: BTreeSet<String>,
    pub controller_port: u16,
    pub secret: String,
    /// Stable app fallback, separate from a user override or inherited secret.
    pub fallback_secret: String,
    /// `None` = keep whatever the profile sets (or no mixed port at all).
    pub mixed_port: Option<u16>,
    pub allow_lan: bool,
    pub ipv6: bool,
    pub log_level: String,
    pub mode: String,
    pub delay_test_url: String,
    pub delay_test_timeout: u32,
    pub request_history_limit: u32,
    pub subscription_user_agent: String,
    pub theme: String,
    /// Hide the main window on minimize/close; tray Quit still exits.
    pub minimize_to_tray: bool,
    pub tun: TunSettings,
}

/// TUN mode options. Written into the runtime config's `tun:` block, on top of
/// whatever `tun:` options the profile has.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TunSettings {
    pub enable: bool,
    /// system | gvisor | mixed | mips
    pub stack: String,
    pub auto_route: bool,
    pub strict_route: bool,
    pub auto_detect_interface: bool,
    pub dns_hijack: Vec<String>,
    /// `tun.dns-mode`: "" (core default) | disabled | native | hijack.
    /// Linux defaults to "disabled": native/hijack make the core call
    /// `resolvectl` as the user, which triggers polkit password prompts on
    /// every TUN start. Auto-route already sends all port-53 traffic into the
    /// TUN on Linux, so dns-hijack keeps working without it.
    pub dns_mode: String,
    /// Empty = core default.
    pub device: String,
    /// None = core default.
    pub mtu: Option<u32>,
    /// Nameservers for the DNS block injected when TUN is on but the profile
    /// has no enabled `dns:` section (hijacked DNS would otherwise fail).
    pub dns_nameservers: Vec<String>,
}

impl Default for TunSettings {
    fn default() -> Self {
        Self {
            enable: false,
            stack: "mixed".into(),
            auto_route: true,
            strict_route: false,
            auto_detect_interface: true,
            dns_hijack: vec!["any:53".into(), "tcp://any:53".into()],
            dns_mode: if cfg!(target_os = "linux") {
                "disabled".into()
            } else {
                String::new()
            },
            device: String::new(),
            mtu: None,
            // IP-based DoH: needs no bootstrap resolver and can't loop through
            // the system resolver (whose own queries TUN would hijack).
            dns_nameservers: vec!["https://1.1.1.1/dns-query".into(), "https://8.8.8.8/dns-query".into()],
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            core_path: None,
            overrides: BTreeSet::new(),
            controller_port: 9097,
            secret: String::new(),
            fallback_secret: String::new(),
            mixed_port: None,
            allow_lan: false,
            ipv6: false,
            log_level: "info".into(),
            mode: "rule".into(),
            delay_test_url: "https://www.gstatic.com/generate_204".into(),
            delay_test_timeout: 5000,
            request_history_limit: 2000,
            subscription_user_agent: "clash.meta".into(),
            theme: "system".into(),
            minimize_to_tray: false,
            tun: TunSettings::default(),
        }
    }
}

/// (YAML path, serialized Settings JSON pointer). Only these fields have a
/// subscription counterpart; local application preferences are never inherited.
pub const PROFILE_FIELDS: &[(&str, &str)] = &[
    ("external-controller", "/controllerPort"),
    ("secret", "/secret"),
    ("mixed-port", "/mixedPort"),
    ("allow-lan", "/allowLan"),
    ("ipv6", "/ipv6"),
    ("log-level", "/logLevel"),
    ("mode", "/mode"),
    ("tun.enable", "/tun/enable"),
    ("tun.stack", "/tun/stack"),
    ("tun.auto-route", "/tun/autoRoute"),
    ("tun.strict-route", "/tun/strictRoute"),
    ("tun.auto-detect-interface", "/tun/autoDetectInterface"),
    ("tun.dns-hijack", "/tun/dnsHijack"),
    ("tun.dns-mode", "/tun/dnsMode"),
    ("tun.device", "/tun/device"),
    ("tun.mtu", "/tun/mtu"),
    ("dns.nameserver", "/tun/dnsNameservers"),
];

fn yaml_field<'a>(root: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    let mut parts = key.split('.');
    let mut value = root.get(parts.next()?)?;
    for part in parts {
        value = value.get(part)?;
    }
    Some(value)
}

impl Settings {
    pub fn load(path: &PathBuf) -> Settings {
        let json = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let mut s: Settings = json
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let legacy = json.as_ref().is_some_and(|v| v.get("overrides").is_none());
        if legacy {
            // Old versions did not record edits. Preserve identifiable nondefault
            // values; unchanged defaults now follow the subscription.
            let values = serde_json::to_value(&s).unwrap();
            let defaults = serde_json::to_value(Settings::default()).unwrap();
            for &(key, pointer) in PROFILE_FIELDS {
                if key == "secret" {
                    let generated = s.secret.len() == 32 && s.secret.chars().all(|c| c.is_ascii_hexdigit());
                    if !s.secret.is_empty() && !generated {
                        s.overrides.insert(key.into());
                    }
                } else if values.pointer(pointer) != defaults.pointer(pointer) {
                    s.overrides.insert(key.into());
                }
            }
        }
        let generate_fallback = s.fallback_secret.is_empty();
        if generate_fallback {
            let generated = s.secret.len() == 32 && s.secret.chars().all(|c| c.is_ascii_hexdigit());
            s.fallback_secret = if generated && !s.overrides.contains("secret") {
                s.secret.clone()
            } else {
                uuid::Uuid::new_v4().simple().to_string()
            };
        }
        let generate_secret = s.secret.is_empty() && !s.overrides.contains("secret");
        if generate_secret {
            s.secret = s.fallback_secret.clone();
        }
        if legacy || generate_secret || generate_fallback {
            let _ = s.save(path);
        }
        s
    }

    pub fn resolve_profile(&self, profile: &str) -> Result<Settings> {
        let root = match serde_yaml::from_str::<serde_yaml::Value>(profile)? {
            serde_yaml::Value::Mapping(root) => root,
            serde_yaml::Value::Null => serde_yaml::Mapping::new(),
            _ => return Err(crate::error::Error::msg("Profile is not a YAML mapping.")),
        };
        Ok(self.resolve(&root))
    }

    pub fn resolve(&self, root: &serde_yaml::Mapping) -> Settings {
        let mut result = serde_json::to_value(self).unwrap();
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        for &(key, pointer) in PROFILE_FIELDS {
            if self.overrides.contains(key) {
                continue;
            }
            let fallback = if key == "secret" {
                serde_json::Value::from(if self.fallback_secret.is_empty() {
                    self.secret.clone()
                } else {
                    self.fallback_secret.clone()
                })
            } else {
                defaults.pointer(pointer).unwrap().clone()
            };
            let value = if key == "external-controller" {
                yaml_field(root, key)
                    .and_then(serde_yaml::Value::as_str)
                    .and_then(|endpoint| endpoint.rsplit(':').next()?.parse::<u16>().ok())
                    .filter(|port| *port != 0)
                    .map(serde_json::Value::from)
            } else {
                yaml_field(root, key).and_then(|v| serde_json::to_value(v).ok())
            };
            *result.pointer_mut(pointer).unwrap() = fallback.clone();
            if let Some(value) = value {
                *result.pointer_mut(pointer).unwrap() = value;
                // Do not coerce invalid profile values into valid client overrides.
                // The core's validation still sees the original YAML for such fields.
                if serde_json::from_value::<Settings>(result.clone()).is_err() {
                    *result.pointer_mut(pointer).unwrap() = fallback;
                }
            }
        }
        serde_json::from_value(result).unwrap()
    }

    /// Incoming inherited values may be stale (a subscription can refresh while
    /// the form is open). Only explicitly marked fields become client overrides.
    pub fn accept_update(&self, incoming: &Settings) -> Settings {
        let mut value = serde_json::to_value(incoming).unwrap();
        let old = serde_json::to_value(self).unwrap();
        for &(key, pointer) in PROFILE_FIELDS {
            if !incoming.overrides.contains(key) {
                *value.pointer_mut(pointer).unwrap() = old.pointer(pointer).unwrap().clone();
            }
        }
        let mut next: Settings = serde_json::from_value(value).unwrap();
        next.fallback_secret = self.fallback_secret.clone();
        next.overrides
            .retain(|key| PROFILE_FIELDS.iter().any(|(known, _)| known == key));
        next
    }

    pub fn save(&self, path: &PathBuf) -> Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }
}

pub fn current(state: &crate::AppState) -> Settings {
    let raw = state.settings.lock().unwrap().clone();
    let content = {
        let profiles = state.profiles.lock().unwrap();
        profiles
            .active()
            .and_then(|p| profiles.read_content(&p.id).ok())
            .unwrap_or_default()
    };
    raw.resolve_profile(&content)
        .unwrap_or_else(|_| raw.resolve(&serde_yaml::Mapping::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    const PROFILE: &str = "external-controller: 0.0.0.0:7777\nsecret: sub-secret\nmixed-port: 3333\nallow-lan: true\nipv6: true\nlog-level: warning\nmode: global\ntun: {enable: true, stack: gvisor, auto-route: false, strict-route: true, auto-detect-interface: false, dns-hijack: ['udp://8.8.8.8:53'], dns-mode: hijack, device: SubTun, mtu: 1400}\ndns: {nameserver: [9.9.9.9]}\n";

    #[test]
    fn resetting_a_custom_secret_uses_the_stable_app_fallback_when_profile_has_none() {
        let path = std::env::temp_dir().join(format!("mihomo-secret-{}.json", uuid::Uuid::new_v4()));
        let mut raw = Settings {
            secret: "my-override".into(),
            ..Default::default()
        };
        raw.overrides.insert("secret".into());
        raw.save(&path).unwrap();
        let mut raw = Settings::load(&path);
        let fallback = raw.fallback_secret.clone();
        assert!(!fallback.is_empty());
        assert_ne!(fallback, "my-override");
        raw.overrides.remove("secret");
        assert_eq!(raw.resolve_profile("").unwrap().secret, fallback);
        assert_eq!(
            raw.resolve_profile("secret: from-subscription").unwrap().secret,
            "from-subscription"
        );
        raw.save(&path).unwrap();
        assert_eq!(Settings::load(&path).resolve_profile("").unwrap().secret, fallback);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn every_profile_field_can_be_pinned_and_independently_reset() {
        let raw = Settings {
            secret: "client-fallback".into(),
            ..Default::default()
        };
        let inherited = raw.resolve_profile(PROFILE).unwrap();
        assert_eq!(inherited.controller_port, 7777);
        assert_eq!(inherited.secret, "sub-secret");
        assert!(inherited.tun.enable);
        assert_eq!(inherited.tun.stack, "gvisor");
        assert_eq!(inherited.tun.dns_nameservers, vec!["9.9.9.9"]);
        let raw_json = serde_json::to_value(&raw).unwrap();
        let inherited_json = serde_json::to_value(&inherited).unwrap();
        for &(key, pointer) in PROFILE_FIELDS {
            let mut pinned = raw.clone();
            pinned.overrides.insert(key.into());
            let resolved = serde_json::to_value(pinned.resolve_profile(PROFILE).unwrap()).unwrap();
            assert_eq!(resolved.pointer(pointer), raw_json.pointer(pointer), "override {key}");
            pinned.overrides.remove(key);
            let resolved = serde_json::to_value(pinned.resolve_profile(PROFILE).unwrap()).unwrap();
            assert_eq!(
                resolved.pointer(pointer),
                inherited_json.pointer(pointer),
                "reset {key}"
            );
        }
    }

    #[test]
    fn reset_follows_future_subscription_values_and_preserves_other_pins() {
        let mut raw = Settings::default();
        raw.mode = "direct".into();
        raw.overrides.extend(["mode".into(), "allow-lan".into()]);
        raw.overrides.remove("allow-lan");
        let current = raw.resolve_profile(PROFILE).unwrap();
        assert!(current.allow_lan);
        assert_eq!(current.mode, "direct");
        let changed = raw.resolve_profile("allow-lan: false\nmode: global\n").unwrap();
        assert!(!changed.allow_lan);
        assert_eq!(changed.mode, "direct");
        assert!(changed.overrides.contains("mode"));
        assert!(!changed.overrides.contains("allow-lan"));
    }

    #[test]
    fn stale_inherited_form_values_do_not_become_overrides() {
        let raw = Settings::default();
        let mut old_form = raw.resolve_profile(PROFILE).unwrap();
        old_form.theme = "dark".into();
        let saved = raw.accept_update(&old_form);
        assert!(saved.overrides.is_empty());
        assert_eq!(saved.theme, "dark");
        let current = saved.resolve_profile("mode: direct\nallow-lan: false").unwrap();
        assert_eq!(current.mode, "direct");
        assert!(!current.allow_lan);
        old_form.overrides.insert("allow-lan".into());
        let pinned = raw.accept_update(&old_form);
        assert!(pinned.resolve_profile("allow-lan: false").unwrap().allow_lan);
    }

    #[test]
    fn migration_preserves_nondefault_edits_without_repinning_resets() {
        let path = std::env::temp_dir().join(format!("mihomo-settings-{}.json", uuid::Uuid::new_v4()));
        let old = Settings {
            allow_lan: true,
            mode: "global".into(),
            secret: "custom-secret".into(),
            ..Default::default()
        };
        let mut json = serde_json::to_value(&old).unwrap();
        json.as_object_mut().unwrap().remove("overrides");
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        let mut loaded = Settings::load(&path);
        assert!(loaded.overrides.contains("allow-lan"));
        assert!(loaded.overrides.contains("mode"));
        assert!(loaded.overrides.contains("secret"));
        assert!(!loaded.overrides.contains("tun.stack"));
        loaded.overrides.clear();
        loaded.save(&path).unwrap();
        let restored = Settings::load(&path);
        assert!(restored.overrides.is_empty());
        assert!(!restored.resolve_profile("allow-lan: false").unwrap().allow_lan);
        std::fs::remove_file(path).unwrap();
    }
}
