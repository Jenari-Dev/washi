// SPDX-License-Identifier: GPL-2.0-or-later
use std::env;
use std::path::PathBuf;
use std::process::Command;

use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

/// Compile `po/<lang>.po` to `<lang>/LC_MESSAGES/washi.mo` with msgfmt (gettext), into OUT_DIR
/// and, so that `target/<profile>/washi-app` finds it, into `target/<profile>/share/locale`.
/// Installing puts the same files in `<prefix>/share/locale` (contrib/install-local.sh).
fn compile_catalogs() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let po_dir = manifest.join("../../po");
    println!("cargo:rerun-if-changed={}", po_dir.display());
    let Ok(entries) = std::fs::read_dir(&po_dir) else { return };
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    // OUT_DIR is target/<profile>/build/<package>-<hash>/out.
    let share = out.ancestors().nth(3).map(|p| p.join("share").join("locale"));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "po") {
            continue;
        }
        let Some(lang) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let dir = out.join("locale").join(&lang).join("LC_MESSAGES");
        std::fs::create_dir_all(&dir).expect("create the catalog directory");
        let mo = dir.join("washi.mo");
        let status = Command::new("msgfmt")
            .args(["--check-format", "--check-header", "-o"])
            .arg(&mo)
            .arg(&path)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => panic!("msgfmt rejected {}: {s}", path.display()),
            Err(e) => {
                println!(
                    "cargo:warning=msgfmt (gettext) not found ({e}): the {lang} translation is not built, the app stays in English"
                );
                return;
            }
        }
        if let Some(share) = &share {
            let target = share.join(&lang).join("LC_MESSAGES");
            if std::fs::create_dir_all(&target).is_ok() {
                let _ = std::fs::copy(&mo, target.join("washi.mo"));
            }
        }
    }
}

/// `qmake -query NAME` (`QMAKE` overrides the program; `qmake6` and `qmake` are tried), or None.
fn qmake_query(name: &str) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(q) = env::var_os("QMAKE") {
        candidates.push(q);
    }
    candidates.push("qmake6".into());
    candidates.push("qmake".into());
    for qmake in candidates {
        let Ok(out) = Command::new(&qmake).args(["-query", name]).output() else { continue };
        let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if out.status.success() && !text.is_empty() && text != "**Unknown**" {
            return Some(text);
        }
    }
    None
}

/// Where the Qt and KDE Frameworks headers are: the distribution's layout (`/usr/include/qt6`,
/// `/usr/include/KF6`) is the default; qmake and `KI18N_INCLUDE_DIR` / `QT_INCLUDE_DIR` correct it
/// for other layouts (the Flatpak runtime, a Qt installed elsewhere).
fn include_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let qt = env::var_os("QT_INCLUDE_DIR")
        .map(PathBuf::from)
        .or_else(|| qmake_query("QT_INSTALL_HEADERS").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/usr/include/qt6"));
    dirs.push(qt.join("QtQmlIntegration"));
    dirs.push(qt.join("QtQuick"));
    let mut kf6 = Vec::new();
    if let Some(d) = env::var_os("KI18N_INCLUDE_DIR") {
        kf6.push(PathBuf::from(d));
    }
    if let Some(prefix) = qmake_query("QT_INSTALL_PREFIX") {
        kf6.push(PathBuf::from(prefix).join("include/KF6/KI18n"));
    }
    kf6.push(PathBuf::from("/usr/include/KF6/KI18n"));
    // The first existing candidate; the distribution default when none exists (the compiler
    // then reports the missing header).
    let ki18n = kf6.iter().find(|d| d.join("KLocalizedString").exists()).or(kf6.last()).cloned();
    dirs.extend(ki18n);
    dirs
}

fn main() {
    compile_catalogs();
    println!("cargo:rerun-if-env-changed=QMAKE");
    println!("cargo:rerun-if-env-changed=QT_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=KI18N_INCLUDE_DIR");
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("io.github.jenari_dev.washi")
            .qml_file(QmlFile::from("qml/Theme.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Icons.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Fmt.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Prefs.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Lang.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Nav.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Store.qml").singleton(true))
            .qml_file(QmlFile::from("qml/Img.qml").singleton(true))
            .qml_file(QmlFile::from("qml/PrintModel.qml").singleton(true))
            .qml_file(QmlFile::from("qml/ScanModel.qml").singleton(true))
            .qml_file(QmlFile::from("qml/DiscMath.qml").singleton(true))
            .qml_files([
                "qml/Main.qml",
                "qml/HomePage.qml",
                "qml/FunctionsPage.qml",
                "qml/ManagementPage.qml",
                "qml/ManagementTabs.qml",
                "qml/LinkHost.qml",
                "qml/FirmwareHost.qml",
                "qml/NoPrinter.qml",
                "qml/ConsumablesTab.qml",
                "qml/StatusTab.qml",
                "qml/DeviceInfoTab.qml",
                "qml/ConnectionDiagram.qml",
                "qml/UtilitiesTab.qml",
                "qml/UtilityCard.qml",
                "qml/PrinterListPage.qml",
                "qml/AppSettingsPage.qml",
                "qml/FormRow.qml",
                "qml/PrintPage.qml",
                "qml/ScanDiagram.qml",
                "qml/ScanPage.qml",
                "qml/RecommendedPage.qml",
                "qml/MorePage.qml",
                "qml/DiscLabelPage.qml",
                "qml/ServiceDown.qml",
                "qml/NavRail.qml",
                "qml/LangSwitch.qml",
                "qml/TopBar.qml",
                "qml/PageScroll.qml",
                "qml/InfoBar.qml",
                "qml/TileGrid.qml",
                "qml/StatusDot.qml",
                "qml/PrinterArt.qml",
                "qml/PrinterImage.qml",
                "qml/InkChip.qml",
                "qml/RoundButton.qml",
                "qml/Pill.qml",
                "qml/PrinterBanner.qml",
                "qml/Glyph.qml",
                "qml/WText.qml",
                "qml/WButton.qml",
                "qml/WCard.qml",
                "qml/WSwitch.qml",
                "qml/WRadio.qml",
                "qml/WField.qml",
                "qml/WCombo.qml",
                "qml/WSlider.qml",
                "qml/WSpin.qml",
                "qml/WDialog.qml",
                "qml/Spinner.qml",
                "qml/ConfirmDialog.qml",
                "qml/InkModelsDialog.qml",
                "qml/LicenseDialog.qml",
                "qml/DiscoverDialog.qml",
                "qml/MaintenanceHost.qml",
                "qml/SettingDialog.qml",
                "qml/ManualAlignHost.qml",
                "qml/WakeButton.qml",
                "qml/WCheck.qml",
                "qml/GlyphRadio.qml",
                "qml/SidePanel.qml",
                "qml/PhotoSheet.qml",
                "qml/DocSheet.qml",
                "qml/RasterSheet.qml",
                "qml/DocPreview.qml",
                "qml/PrintJobDialog.qml",
                "qml/LoadPaperDialog.qml",
                "qml/ScanFlow.qml",
                "qml/ScanSetupView.qml",
                "qml/ScanPreviewView.qml",
                "qml/ScanCropView.qml",
                "qml/ScanCorrectView.qml",
                "qml/CropOverlay.qml",
                "qml/ColorPicker.qml",
                "qml/DiscCanvas.qml",
                "qml/StyleToggle.qml",
            ]),
    )
    .qt_module("Quick")
    .files(["src/backend.rs", "src/i18n.rs", "src/imaging.rs"])
    .include_dir("src")
    .cpp_file("src/i18n.cpp")
    .cpp_file("src/shot.cpp");
    // SAFETY: only adds include paths for the KF6 I18n and Qt QML integration headers.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            for dir in include_dirs() {
                cc.include(dir);
            }
        })
    };
    builder.build();
    println!("cargo:rustc-link-lib=KF6I18nQml");
    println!("cargo:rustc-link-lib=KF6I18n");
    println!("cargo:rerun-if-changed=src/i18n.cpp");
    println!("cargo:rerun-if-changed=src/i18n.h");
}
