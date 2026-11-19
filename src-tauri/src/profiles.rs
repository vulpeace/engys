use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::subscription::SubscriptionInfo;
use crate::util::{now_secs, write_atomic};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    /// Written/edited inside the app.
    Inline,
    /// Imported from a local file (a copy is stored; `source` keeps the original path).
    File,
    /// Downloaded from a subscription URL.
    Remote,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IntervalSource {
    #[default]
    Default,
    Client,
    Subscription,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub kind: ProfileKind,
    /// File path (File) or URL (Remote).
    pub source: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub subscription: Option<SubscriptionInfo>,
    pub update_interval_hours: Option<u32>,
    /// Effective refresh cadence. Provider hints control it only after an explicit reset.
    #[serde(default)]
    pub update_interval_minutes: u32,
    #[serde(default)]
    pub update_interval_source: IntervalSource,
    #[serde(default)]
    pub last_refreshed_at: Option<u64>,
    #[serde(default)]
    pub last_refresh_attempt_at: Option<u64>,
    #[serde(default)]
    pub last_refresh_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileIndex {
    pub active: Option<String>,
    pub profiles: Vec<Profile>,
}

/// Profile metadata lives in `<dir>/index.json`, contents in `<dir>/<id>.yaml`.
pub struct ProfileStore {
    dir: PathBuf,
    pub index: ProfileIndex,
}

impl ProfileStore {
    pub fn load(dir: PathBuf) -> Self {
        let mut index: ProfileIndex = std::fs::read(dir.join("index.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for profile in &mut index.profiles {
            if profile.update_interval_source == IntervalSource::Default && profile.update_interval_minutes != 0 {
                profile.update_interval_source = IntervalSource::Client;
            }
        }
        Self { dir, index }
    }

    fn save_index(&self) -> Result<()> {
        write_atomic(&self.dir.join("index.json"), &serde_json::to_vec_pretty(&self.index)?)
    }

    pub fn content_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.yaml"))
    }

    pub fn get(&self, id: &str) -> Result<&Profile> {
        self.index
            .profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::msg(format!("Profile `{id}` not found")))
    }

    fn get_mut(&mut self, id: &str) -> Result<&mut Profile> {
        self.index
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::msg(format!("Profile `{id}` not found")))
    }

    pub fn active(&self) -> Option<&Profile> {
        let id = self.index.active.as_deref()?;
        self.index.profiles.iter().find(|p| p.id == id)
    }

    pub fn add(&mut self, mut profile: Profile, content: &str) -> Result<Profile> {
        let previous = self.index.clone();
        profile.id = uuid::Uuid::new_v4().simple().to_string();
        let now = now_secs();
        profile.created_at = now;
        profile.updated_at = now;
        write_atomic(&self.content_path(&profile.id), content.as_bytes())?;
        if self.index.active.is_none() {
            self.index.active = Some(profile.id.clone());
        }
        self.index.profiles.push(profile.clone());
        if let Err(error) = self.save_index() {
            self.index = previous;
            let _ = std::fs::remove_file(self.content_path(&profile.id));
            return Err(error);
        }
        Ok(profile)
    }

    pub fn read_content(&self, id: &str) -> Result<String> {
        self.get(id)?;
        Ok(std::fs::read_to_string(self.content_path(id))?)
    }

    pub fn write_content(&mut self, id: &str, content: &str) -> Result<()> {
        self.replace_content(id, content, |p| p.updated_at = now_secs()).map(|_| ())
    }

    /// Keep YAML and metadata together, restoring both if the index cannot be saved.
    pub fn replace_content<F: FnOnce(&mut Profile)>(&mut self, id: &str, content: &str, update: F) -> Result<Profile> {
        let previous_content = self.read_content(id)?;
        let previous_index = self.index.clone();
        write_atomic(&self.content_path(id), content.as_bytes())?;
        update(self.get_mut(id)?);
        let profile = self.get(id)?.clone();
        if let Err(error) = self.save_index() {
            self.index = previous_index;
            if let Err(rollback) = write_atomic(&self.content_path(id), previous_content.as_bytes()) {
                return Err(Error::msg(format!("{error} Restoring the previous profile also failed: {rollback}")));
            }
            return Err(error);
        }
        Ok(profile)
    }

    pub fn update<F: FnOnce(&mut Profile)>(&mut self, id: &str, f: F) -> Result<Profile> {
        let previous = self.index.clone();
        let p = self.get_mut(id)?;
        f(p);
        let p = p.clone();
        if let Err(error) = self.save_index() {
            self.index = previous;
            return Err(error);
        }
        Ok(p)
    }

    pub fn delete(&mut self, id: &str, cleanup: impl FnOnce() -> Result<()>) -> Result<()> {
        self.get(id)?;
        let previous = self.index.clone();
        self.index.profiles.retain(|p| p.id != id);
        if self.index.active.as_deref() == Some(id) {
            self.index.active = self.index.profiles.first().map(|p| p.id.clone());
        }
        if let Err(error) = self.save_index() {
            self.index = previous;
            return Err(error);
        }
        if let Err(error) = cleanup() {
            let deleted = std::mem::replace(&mut self.index, previous);
            if let Err(rollback) = self.save_index() {
                self.index = deleted;
                return Err(Error::msg(format!(
                    "{error} Restoring the profile index also failed: {rollback}"
                )));
            }
            return Err(error);
        }
        let _ = std::fs::remove_file(self.content_path(id));
        Ok(())
    }

    pub fn set_active(&mut self, id: &str) -> Result<()> {
        self.get(id)?;
        self.index.active = Some(id.to_string());
        self.save_index()
    }
}

pub fn new_profile(name: String, kind: ProfileKind, source: Option<String>) -> Profile {
    Profile {
        id: String::new(),
        name,
        kind,
        source,
        created_at: 0,
        updated_at: 0,
        subscription: None,
        update_interval_hours: None,
        update_interval_minutes: 0,
        update_interval_source: IntervalSource::Default,
        last_refreshed_at: None,
        last_refresh_attempt_at: None,
        last_refresh_error: None,
    }
}

pub fn subscription_interval(profile: &Profile) -> u32 {
    profile.update_interval_hours.unwrap_or(0).saturating_mul(60)
}

/// "clash.yaml" -> "clash"
pub fn name_from_path(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported profile".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_profiles_remain_manual_only_despite_provider_interval_hints() {
        let mut old = serde_json::to_value(new_profile("Old subscription".into(), ProfileKind::Remote, None)).unwrap();
        let object = old.as_object_mut().unwrap();
        for key in ["updateIntervalMinutes", "updateIntervalSource", "lastRefreshedAt", "lastRefreshAttemptAt", "lastRefreshError"] {
            object.remove(key);
        }
        object.insert("updateIntervalHours".into(), serde_json::json!(24));
        let profile: Profile = serde_json::from_value(old).unwrap();
        assert_eq!(profile.update_interval_minutes, 0);
        assert!(profile.last_refreshed_at.is_none());
        assert_eq!(profile.update_interval_hours, Some(24));
    }

    #[test]
    fn index_failure_restores_content_and_metadata_and_interval_survives_restart() {
        let dir = std::env::temp_dir().join(format!("mihomo-profile-test-{}", uuid::Uuid::new_v4()));
        let mut store = ProfileStore::load(dir.clone());
        let profile = store.add(new_profile("Sub".into(), ProfileKind::Remote, Some("https://example.invalid".into())), "rules: []\n").unwrap();
        store.update(&profile.id, |p| p.update_interval_minutes = 17).unwrap();
        assert_eq!(ProfileStore::load(dir.clone()).get(&profile.id).unwrap().update_interval_minutes, 17);
        // Prevent atomic index writes, while leaving profile YAML writable.
        std::fs::create_dir(dir.join("index.tmp")).unwrap();
        assert!(store.replace_content(&profile.id, "rules: ['MATCH,REJECT']\n", |p| p.updated_at = 1).is_err());
        assert_eq!(store.read_content(&profile.id).unwrap(), "rules: []\n");
        assert_eq!(store.get(&profile.id).unwrap().updated_at, profile.updated_at);
        assert!(store.update(&profile.id, |p| p.update_interval_minutes = 0).is_err());
        assert_eq!(store.get(&profile.id).unwrap().update_interval_minutes, 17);
        assert_eq!(ProfileStore::load(dir.clone()).get(&profile.id).unwrap().update_interval_minutes, 17);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
