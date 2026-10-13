// SPDX-License-Identifier: GPL-2.0-or-later
//! The user interface language, shared by the desktop app and the daemon.
//!
//! `config.toml` holds the user's choice in `[ui] language`: `"en"`, `"ja"` or `"system"`.
//! `"system"` (also what a missing key means, so a first run) follows the locale of the
//! session: Japanese for `ja_*`, English for everything else.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A language Washi is translated into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Language {
    #[default]
    En,
    Ja,
}

impl Language {
    /// `"en"` or `"ja"`.
    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Ja => "ja",
        }
    }

    /// `"en"`, `"ja"` and locale names such as `ja_JP.UTF-8` or `en-US`. Anything else is `None`.
    pub fn from_locale(name: &str) -> Option<Language> {
        let lang =
            name.trim().split(['_', '-', '.', '@']).next().unwrap_or("").to_ascii_lowercase();
        match lang.as_str() {
            "en" => Some(Language::En),
            "ja" => Some(Language::Ja),
            _ => None,
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// What `[ui] language` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum LanguageSetting {
    /// Follow the session's locale.
    #[default]
    System,
    En,
    Ja,
}

impl LanguageSetting {
    /// `"en"`, `"ja"` or `"system"`.
    pub fn code(self) -> &'static str {
        match self {
            LanguageSetting::System => "system",
            LanguageSetting::En => "en",
            LanguageSetting::Ja => "ja",
        }
    }

    /// Lenient: unknown values mean `System`, so a typo (or a language from a newer version)
    /// never stops the daemon from reading its configuration.
    pub fn parse(s: &str) -> LanguageSetting {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" => LanguageSetting::En,
            "ja" => LanguageSetting::Ja,
            _ => LanguageSetting::System,
        }
    }

    /// Like [`LanguageSetting::parse`], but `None` for something that is not a setting at all.
    pub fn parse_strict(s: &str) -> Option<LanguageSetting> {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" => Some(LanguageSetting::En),
            "ja" => Some(LanguageSetting::Ja),
            "system" => Some(LanguageSetting::System),
            _ => None,
        }
    }

    /// The language to show, given the process environment.
    pub fn resolve(self) -> Language {
        self.resolve_with(|k| std::env::var(k).ok())
    }

    /// [`LanguageSetting::resolve`] with an explicit environment (for tests).
    pub fn resolve_with(self, env: impl Fn(&str) -> Option<String>) -> Language {
        match self {
            LanguageSetting::En => Language::En,
            LanguageSetting::Ja => Language::Ja,
            LanguageSetting::System => system_language(env),
        }
    }
}

impl Serialize for LanguageSetting {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for LanguageSetting {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(LanguageSetting::parse(&s))
    }
}

/// The language of the session: the first of `LANGUAGE` (a colon-separated preference list, as
/// gettext reads it) that is one we speak, else the locale (`LC_ALL`, `LC_MESSAGES`, `LANG`),
/// else English. The C and POSIX locales are English and ignore `LANGUAGE`, like gettext does.
pub fn system_language(env: impl Fn(&str) -> Option<String>) -> Language {
    let get = |k: &str| env(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| get(k));
    if matches!(locale.as_deref(), None | Some("C" | "POSIX")) {
        return Language::En;
    }
    if let Some(list) = get("LANGUAGE") {
        if let Some(l) = list.split(':').find_map(Language::from_locale) {
            return l;
        }
    }
    locale.as_deref().and_then(Language::from_locale).unwrap_or(Language::En)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn locale_names() {
        for (name, want) in [
            ("ja_JP.UTF-8", Some(Language::Ja)),
            ("ja", Some(Language::Ja)),
            ("JA_jp", Some(Language::Ja)),
            ("ja-JP", Some(Language::Ja)),
            ("en_US.UTF-8", Some(Language::En)),
            ("en", Some(Language::En)),
            ("de_DE", None),
            ("", None),
            ("jam", None),
            ("C", None),
        ] {
            assert_eq!(Language::from_locale(name), want, "{name:?}");
        }
    }

    #[test]
    fn the_session_decides_when_the_setting_is_system() {
        let s = LanguageSetting::System;
        assert_eq!(s.resolve_with(env(&[("LANG", "ja_JP.UTF-8")])), Language::Ja);
        assert_eq!(s.resolve_with(env(&[("LANG", "en_US.UTF-8")])), Language::En);
        assert_eq!(s.resolve_with(env(&[("LANG", "de_DE.UTF-8")])), Language::En);
        assert_eq!(s.resolve_with(env(&[])), Language::En);
        // LC_ALL beats LC_MESSAGES beats LANG.
        assert_eq!(
            s.resolve_with(env(&[("LC_ALL", "ja_JP.UTF-8"), ("LANG", "en_US.UTF-8")])),
            Language::Ja
        );
        assert_eq!(
            s.resolve_with(env(&[("LC_MESSAGES", "en_US.UTF-8"), ("LANG", "ja_JP.UTF-8")])),
            Language::En
        );
        // Empty values do not count.
        assert_eq!(s.resolve_with(env(&[("LC_ALL", ""), ("LANG", "ja_JP.UTF-8")])), Language::Ja);
        // LANGUAGE is a preference list that only matters outside the C locale.
        assert_eq!(
            s.resolve_with(env(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "ja:en")])),
            Language::Ja
        );
        assert_eq!(
            s.resolve_with(env(&[("LANG", "ja_JP.UTF-8"), ("LANGUAGE", "de:en_US")])),
            Language::En
        );
        assert_eq!(
            s.resolve_with(env(&[("LANG", "de_DE.UTF-8"), ("LANGUAGE", "fr:de:ja")])),
            Language::Ja
        );
        assert_eq!(
            s.resolve_with(env(&[("LANG", "C"), ("LANGUAGE", "ja")])),
            Language::En,
            "gettext ignores LANGUAGE in the C locale"
        );
        assert_eq!(s.resolve_with(env(&[("LANG", "POSIX")])), Language::En);
    }

    #[test]
    fn an_explicit_choice_ignores_the_session() {
        let ja_session = env(&[("LANG", "ja_JP.UTF-8")]);
        assert_eq!(LanguageSetting::En.resolve_with(&ja_session), Language::En);
        assert_eq!(LanguageSetting::Ja.resolve_with(env(&[("LANG", "en_US.UTF-8")])), Language::Ja);
    }

    #[test]
    fn settings_parse_leniently_and_round_trip() {
        assert_eq!(LanguageSetting::parse("ja"), LanguageSetting::Ja);
        assert_eq!(LanguageSetting::parse(" EN "), LanguageSetting::En);
        assert_eq!(LanguageSetting::parse("system"), LanguageSetting::System);
        assert_eq!(LanguageSetting::parse("fr"), LanguageSetting::System);
        assert_eq!(LanguageSetting::parse_strict("fr"), None);
        assert_eq!(LanguageSetting::parse_strict("ja"), Some(LanguageSetting::Ja));
        for s in [LanguageSetting::System, LanguageSetting::En, LanguageSetting::Ja] {
            assert_eq!(LanguageSetting::parse(s.code()), s);
        }
    }
}
