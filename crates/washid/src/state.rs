// SPDX-License-Identifier: GPL-2.0-or-later
//! The daemon's small state file (`~/.local/state/washi/washid.json`): which printers it knows
//! beyond the config file, and which one is active. Never holds identifiers beyond the host
//! names the user or mDNS supplied.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredPrinter {
    pub host: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StateFile {
    pub active: Option<String>,
    pub printers: Vec<StoredPrinter>,
}

impl StateFile {
    pub fn default_path() -> Option<PathBuf> {
        dirs::state_dir().or_else(dirs::data_local_dir).map(|d| d.join("washi").join("washid.json"))
    }

    /// A missing or unreadable file is an empty state.
    pub fn load(path: &Path) -> StateFile {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("washid.json");
        assert_eq!(StateFile::load(&path), StateFile::default());
        let s = StateFile {
            active: Some("a.local".into()),
            printers: vec![StoredPrinter { host: "a.local".into(), name: Some("A".into()) }],
        };
        s.save(&path).unwrap();
        assert_eq!(StateFile::load(&path), s);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(StateFile::load(&path), StateFile::default());
        std::fs::write(&path, r#"{"active": "b", "future": 1}"#).unwrap();
        assert_eq!(StateFile::load(&path).active.as_deref(), Some("b"));
    }
}
