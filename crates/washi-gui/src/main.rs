// SPDX-License-Identifier: GPL-2.0-or-later
//! Washi desktop app. All printer access goes through the washid daemon over D-Bus.

mod backend;
mod i18n;
mod imaging;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

fn main() {
    // `washi doctor` asks the app for its version; this must work without a display.
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("washi-app {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // The Basic style has no platform theming: every control in the UI is styled in QML.
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        QQuickStyle::set_style(&QString::from("Basic"));
    }
    let mut app = QGuiApplication::new();
    if let Some(mut app) = app.as_mut() {
        app.as_mut().set_application_name(&QString::from("Washi"));
        app.as_mut().set_application_version(&QString::from(env!("CARGO_PKG_VERSION")));
        app.as_mut().set_organization_name(&QString::from("JenAriDev"));
        app.as_mut().set_organization_domain(&QString::from("jenari-dev.github.io"));
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        i18n::ffi::washi_i18n_setup_app(engine.as_mut());
        // The language of the last run (or of the session, the first time), before any text
        // is shown.
        let lang = i18n::startup_setting().resolve();
        i18n::set_started_in(lang);
        i18n::apply(lang);
        engine.load(&QUrl::from("qrc:/qt/qml/io/github/jenari_dev/washi/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        let code = app.exec();
        imaging::cleanup();
        std::process::exit(code);
    }
}
