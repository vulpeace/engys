//! Operational state is separate from settings drafts. Exit cleanup never changes it.
use crate::error::Result;
use crate::util::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Session {
    pub core_running: bool,
    pub window: Option<Geometry>,
    #[serde(skip)]
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl Session {
    pub fn load(path: &Path) -> Self {
        let mut state: Self = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        state.path = path.to_owned();
        state
    }

    pub fn save(&self) -> Result<()> {
        write_atomic(&self.path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn set_running(&mut self, running: bool) -> Result<()> {
        let previous = self.core_running;
        self.core_running = running;
        if let Err(error) = self.save() {
            self.core_running = previous;
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remembers_running_separately_from_settings() {
        let path = std::env::temp_dir().join(format!("mihomo-session-{}.json", uuid::Uuid::new_v4()));
        let mut state = Session::load(&path);
        assert!(!state.core_running);
        state.set_running(true).unwrap();
        assert!(Session::load(&path).core_running);
        state.set_running(false).unwrap();
        assert!(!Session::load(&path).core_running);
        std::fs::remove_file(path).unwrap();
    }
}
