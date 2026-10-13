// SPDX-License-Identifier: GPL-2.0-or-later
//! On-disk configuration: `~/.config/washi/config.toml`.
//!
//! ```toml
//! [printer]
//! host = "printer.local"
//!
//! # washid only
//! [daemon]
//! poll_interval_secs = 30
//!
//! # the language of the app and of the daemon's notifications (a missing key = "system")
//! [ui]
//! language = "system"   # "en", "ja" or "system" (follow the session's locale)
//!
//! [notifications]
//! enabled = true   # master switch; the three below pick which events notify
//! ink = true
//! errors = true
//! online = true
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::lang::LanguageSetting;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub printer: PrinterConfig,
    #[serde(default, skip_serializing_if = "NotificationConfig::is_default")]
    pub notifications: NotificationConfig,
    #[serde(default, skip_serializing_if = "DaemonConfig::is_default")]
    pub daemon: DaemonConfig,
    #[serde(default, skip_serializing_if = "UiConfig::is_default")]
    pub ui: UiConfig,
}

/// `[ui]`: shared by the desktop app and the daemon.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    /// The interface language; unknown values are read as `system`.
    #[serde(default)]
    pub language: LanguageSetting,
}

impl UiConfig {
    fn is_default(&self) -> bool {
        *self == UiConfig::default()
    }
}

fn yes() -> bool {
    true
}

/// Desktop notifications sent by `washid`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Master switch.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Ink reaching low / empty.
    #[serde(default = "yes")]
    pub ink: bool,
    /// A new printer error (support code).
    #[serde(default = "yes")]
    pub errors: bool,
    /// The printer coming back online after being unreachable.
    #[serde(default = "yes")]
    pub online: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        NotificationConfig { enabled: true, ink: true, errors: true, online: true }
    }
}

impl NotificationConfig {
    fn is_default(&self) -> bool {
        *self == NotificationConfig::default()
    }
}

/// Smallest polling interval `washid` accepts (the printer is a small embedded device).
pub const MIN_POLL_INTERVAL_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// How often the active printer is polled.
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
}

fn default_poll() -> u64 {
    30
}

impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig { poll_interval_secs: default_poll() }
    }
}

impl DaemonConfig {
    fn is_default(&self) -> bool {
        *self == DaemonConfig::default()
    }

    /// The polling interval, never below [`MIN_POLL_INTERVAL_SECS`].
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_secs.max(MIN_POLL_INTERVAL_SECS))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterConfig {
    /// Default printer: a host name or IP address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

fn cfg_err(path: &Path, msg: impl ToString) -> Error {
    Error::Config { path: path.display().to_string(), msg: msg.to_string() }
}

impl Config {
    /// `<config dir>/washi/config.toml`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("washi").join("config.toml"))
    }

    /// Load the default file; a missing file is an empty configuration.
    pub fn load() -> Result<Config> {
        match Self::default_path() {
            Some(p) => Self::load_from(&p),
            None => Ok(Config::default()),
        }
    }

    pub fn load_from(path: &Path) -> Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| cfg_err(path, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(cfg_err(path, e)),
        }
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::default_path()
            .ok_or_else(|| Error::Invalid("no configuration directory on this system".into()))?;
        self.save_to(&path)?;
        Ok(path)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| cfg_err(path, e))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| cfg_err(path, e))?;
        std::fs::write(path, text).map_err(|e| cfg_err(path, e))
    }

    /// Write `[notifications]` into the file at `path`, leaving every other key and table as it
    /// is (unknown keys survive; comments do not). A missing file is created. Defaults are
    /// written out explicitly so that the file says what the daemon does.
    pub fn save_notifications(path: &Path, n: &NotificationConfig) -> Result<()> {
        let mut t = toml::Table::new();
        t.insert("enabled".into(), n.enabled.into());
        t.insert("ink".into(), n.ink.into());
        t.insert("errors".into(), n.errors.into());
        t.insert("online".into(), n.online.into());
        Self::replace_table(path, "notifications", t)
    }

    /// Write `[ui] language` into the file at `path`, leaving everything else as it is (see
    /// [`Config::save_notifications`]). The choice is written out explicitly, `system` included.
    pub fn save_ui_language(path: &Path, language: LanguageSetting) -> Result<()> {
        let mut t = toml::Table::new();
        t.insert("language".into(), language.code().into());
        Self::replace_table(path, "ui", t)
    }

    /// Replace one top-level table of the file at `path`. A file that does not parse is left
    /// alone and reported.
    fn replace_table(path: &Path, name: &str, new: toml::Table) -> Result<()> {
        let mut table: toml::Table = match std::fs::read_to_string(path) {
            Ok(text) => text.parse().map_err(|e| cfg_err(path, e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
            Err(e) => return Err(cfg_err(path, e)),
        };
        // Keep keys of the table that this version does not know about.
        let merged = match table.remove(name) {
            Some(toml::Value::Table(mut old)) => {
                old.extend(new);
                old
            }
            _ => new,
        };
        table.insert(name.into(), toml::Value::Table(merged));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| cfg_err(path, e))?;
        }
        let text = toml::to_string_pretty(&table).map_err(|e| cfg_err(path, e))?;
        std::fs::write(path, text).map_err(|e| cfg_err(path, e))
    }

    /// Set the default host after a sanity check.
    pub fn set_host(&mut self, host: &str) -> Result<()> {
        let host = host.trim();
        if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c.is_control() || c == '/')
        {
            return Err(Error::Invalid(format!("{host:?} is not a host name or address")));
        }
        self.printer.host = Some(host.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("washi").join("config.toml");
        assert_eq!(Config::load_from(&path).unwrap(), Config::default());

        let mut c = Config::default();
        c.set_host("printer.local").unwrap();
        c.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[printer]") && text.contains("host = \"printer.local\""), "{text}");
        assert_eq!(Config::load_from(&path).unwrap(), c);
    }

    #[test]
    fn invalid_toml_and_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        std::fs::write(&path, "this is = not [valid").unwrap();
        assert!(matches!(Config::load_from(&path), Err(Error::Config { .. })));
        std::fs::write(&path, "future = 1\n[printer]\nhost = \"a\"\nnew = 2\n").unwrap();
        assert_eq!(Config::load_from(&path).unwrap().printer.host.as_deref(), Some("a"));
    }

    #[test]
    fn daemon_and_notification_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        assert_eq!(Config::default().daemon.poll_interval(), Duration::from_secs(30));
        assert!(Config::default().notifications.enabled);

        std::fs::write(
            &path,
            "[notifications]\nenabled = false\nink = false\n[daemon]\npoll_interval_secs = 1\n",
        )
        .unwrap();
        let c = Config::load_from(&path).unwrap();
        assert!(!c.notifications.enabled && !c.notifications.ink);
        assert!(c.notifications.errors && c.notifications.online, "unset keys default to on");
        assert_eq!(c.daemon.poll_interval(), Duration::from_secs(MIN_POLL_INTERVAL_SECS));

        // A partial section keeps the other defaults; defaults are not written back out.
        std::fs::write(&path, "[notifications]\nonline = false\n").unwrap();
        let c = Config::load_from(&path).unwrap();
        assert!(c.notifications.enabled && !c.notifications.online);
        let mut plain = Config::default();
        plain.set_host("p.local").unwrap();
        plain.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("notifications") && !text.contains("daemon"), "{text}");
    }

    #[test]
    fn notifications_are_saved_without_touching_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("washi").join("config.toml");
        let off = NotificationConfig { enabled: true, ink: false, errors: true, online: false };
        Config::save_notifications(&path, &off).unwrap();
        assert_eq!(Config::load_from(&path).unwrap().notifications, off);

        std::fs::write(
            &path,
            "future = 7\n[printer]\nhost = \"p.local\"\n[daemon]\npoll_interval_secs = 60\n",
        )
        .unwrap();
        Config::save_notifications(&path, &off).unwrap();
        let c = Config::load_from(&path).unwrap();
        assert_eq!(c.notifications, off);
        assert_eq!(c.printer.host.as_deref(), Some("p.local"));
        assert_eq!(c.daemon.poll_interval_secs, 60);
        assert!(std::fs::read_to_string(&path).unwrap().contains("future = 7"));
        std::fs::write(&path, "not [valid").unwrap();
        assert!(
            Config::save_notifications(&path, &off).is_err(),
            "a broken file is not overwritten"
        );
    }

    #[test]
    fn ui_language_is_read_leniently_and_saved_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("washi").join("config.toml");
        // No file, no key: follow the system.
        assert_eq!(Config::load_from(&path).unwrap().ui.language, LanguageSetting::System);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[ui]\nlanguage = \"ja\"\n").unwrap();
        assert_eq!(Config::load_from(&path).unwrap().ui.language, LanguageSetting::Ja);
        // A value from the future must not make the configuration unreadable.
        std::fs::write(&path, "[ui]\nlanguage = \"fr\"\nfuture = 1\n").unwrap();
        assert_eq!(Config::load_from(&path).unwrap().ui.language, LanguageSetting::System);

        std::fs::write(
            &path,
            "future = 7\n[printer]\nhost = \"p.local\"\n[ui]\nfuture_ui = 2\n[notifications]\nink = false\n",
        )
        .unwrap();
        Config::save_ui_language(&path, LanguageSetting::Ja).unwrap();
        let c = Config::load_from(&path).unwrap();
        assert_eq!(c.ui.language, LanguageSetting::Ja);
        assert_eq!(c.printer.host.as_deref(), Some("p.local"));
        assert!(!c.notifications.ink);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("future = 7") && text.contains("future_ui = 2"), "{text}");
        assert!(text.contains("language = \"ja\""), "{text}");
        Config::save_ui_language(&path, LanguageSetting::En).unwrap();
        assert_eq!(Config::load_from(&path).unwrap().ui.language, LanguageSetting::En);
        // The default is not written by `save`, an explicit choice is.
        let mut plain = Config::default();
        plain.set_host("p.local").unwrap();
        plain.save_to(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("[ui]"));
        std::fs::write(&path, "not [valid").unwrap();
        assert!(
            Config::save_ui_language(&path, LanguageSetting::Ja).is_err(),
            "a broken file is not overwritten"
        );
    }

    #[test]
    fn host_validation() {
        let mut c = Config::default();
        assert!(c.set_host("").is_err());
        assert!(c.set_host("a b").is_err());
        assert!(c.set_host("http://x/y").is_err());
        assert!(c.set_host("192.0.2.9").is_ok());
        assert!(c.set_host("fe80::1%eth0").is_ok());
    }
}
