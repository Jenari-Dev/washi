// SPDX-License-Identifier: GPL-2.0-or-later
//! The app's language: KDE i18n for QML (see i18n.cpp), the choice in `config.toml`, and the
//! switch at run time.

use std::path::Path;

use washi_core::{Config, Language, LanguageSetting};

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("i18n.h");
        fn washi_i18n_setup_app(engine: Pin<&mut QQmlApplicationEngine>);
        fn washi_apply_language(lang: &QString);
    }
}

/// What the app starts in. `WASHI_LANG` (en, ja or system) overrides `config.toml` for the
/// start (headless screenshots, tests); switching in the app still saves the choice.
pub fn startup_setting() -> LanguageSetting {
    startup_setting_from(std::env::var("WASHI_LANG").ok().as_deref(), Config::load())
}

fn startup_setting_from(env: Option<&str>, config: washi_core::Result<Config>) -> LanguageSetting {
    if let Some(v) = env.and_then(LanguageSetting::parse_strict) {
        return v;
    }
    match config {
        Ok(c) => c.ui.language,
        Err(e) => {
            eprintln!("washi-app: ignoring the unreadable config file: {e}");
            LanguageSetting::System
        }
    }
}

/// Make the QML engine use `lang` now.
pub fn apply(lang: Language) {
    ffi::washi_apply_language(&cxx_qt_lib::QString::from(lang.code()));
}

/// The language the app was started in (resolved and applied by `main`).
static STARTED_IN: std::sync::OnceLock<Language> = std::sync::OnceLock::new();

pub fn set_started_in(lang: Language) {
    let _ = STARTED_IN.set(lang);
}

pub fn started_in() -> Language {
    STARTED_IN.get().copied().unwrap_or_default()
}

/// Save `setting` as `[ui] language` in `config.toml` and return the language it means.
pub fn save_setting(setting: LanguageSetting) -> Result<Language, String> {
    let path = Config::default_path().ok_or("no configuration directory")?;
    save_setting_to(&path, setting)
}

fn save_setting_to(path: &Path, setting: LanguageSetting) -> Result<Language, String> {
    Config::save_ui_language(path, setting).map_err(|e| e.to_string())?;
    Ok(setting.resolve())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("washi").join("config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn the_first_run_follows_the_system() {
        // No file, no key: system.
        assert_eq!(startup_setting_from(None, Ok(Config::default())), LanguageSetting::System);
        let (_d, path) = config_with("[printer]\nhost = \"p.local\"\n");
        assert_eq!(startup_setting_from(None, Config::load_from(&path)), LanguageSetting::System);
    }

    #[test]
    fn the_saved_choice_wins_over_the_session_and_survives_a_restart() {
        let (_d, path) = config_with("[printer]\nhost = \"p.local\"\n");
        assert_eq!(save_setting_to(&path, LanguageSetting::Ja), Ok(Language::Ja));
        // "restart": read the file again
        assert_eq!(startup_setting_from(None, Config::load_from(&path)), LanguageSetting::Ja);
        assert_eq!(save_setting_to(&path, LanguageSetting::En), Ok(Language::En));
        assert_eq!(startup_setting_from(None, Config::load_from(&path)), LanguageSetting::En);
        // the rest of the file is as it was
        let c = Config::load_from(&path).unwrap();
        assert_eq!(c.printer.host.as_deref(), Some("p.local"));
        // and "system" can be chosen again
        assert!(save_setting_to(&path, LanguageSetting::System).is_ok());
        assert_eq!(startup_setting_from(None, Config::load_from(&path)), LanguageSetting::System);
    }

    #[test]
    fn the_environment_decides_the_start_but_a_bad_value_does_not() {
        let (_d, path) = config_with("[ui]\nlanguage = \"en\"\n");
        let cfg = || Config::load_from(&path);
        assert_eq!(startup_setting_from(Some("ja"), cfg()), LanguageSetting::Ja);
        assert_eq!(startup_setting_from(Some("system"), cfg()), LanguageSetting::System);
        assert_eq!(startup_setting_from(Some("klingon"), cfg()), LanguageSetting::En);
        assert_eq!(startup_setting_from(Some(""), cfg()), LanguageSetting::En);
    }

    #[test]
    fn an_unreadable_config_does_not_stop_the_app() {
        let (_d, path) = config_with("this is = not [valid");
        assert_eq!(startup_setting_from(None, Config::load_from(&path)), LanguageSetting::System);
        // and it is not overwritten by a switch of the language
        assert!(save_setting_to(&path, LanguageSetting::Ja).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "this is = not [valid");
    }

    #[test]
    fn a_choice_follows_the_session_only_when_it_says_system() {
        let ja = |k: &str| (k == "LANG").then(|| "ja_JP.UTF-8".to_string());
        assert_eq!(LanguageSetting::System.resolve_with(ja), Language::Ja);
        assert_eq!(LanguageSetting::En.resolve_with(ja), Language::En);
    }
}
