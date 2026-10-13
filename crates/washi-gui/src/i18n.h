// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once
#include <QString>
#include <QStringList>
#include <QtQml/QQmlApplicationEngine>

// Washi's i18n glue for QML. Plain Qt and KI18n, no cxx-qt types, so the QML test runner
// (tests/qml/runner.cpp) can use exactly the code the app uses.

// Look strings up in the "washi" catalog (washi.mo), find the catalogs next to the program or in
// $WASHI_LOCALE_DIR, and give every QML file of `engine` the i18n(), i18nc(), i18np() ...
// functions. Changing the language later re-evaluates every binding that calls them.
void washi_i18n_setup(QQmlEngine *engine);

// Show the interface in `lang` ("en" or "ja") from now on: no restart, every visible string,
// tooltip and formatted value of the engine's windows is re-evaluated. Safe to call before the
// first window exists.
void washi_apply_language(const QString &lang);

// The same for the application's engine (the shape cxx-qt's bridge wants).
void washi_i18n_setup_app(QQmlApplicationEngine &engine);

// Make sure gettext can translate at all (it cannot in the "C" locale). Called by
// washi_i18n_setup().
void washi_ensure_translatable_locale();

// Draw Han and kana text with Japanese letter shapes whatever the session's locale is. Called by
// washi_i18n_setup().
void washi_prefer_japanese_fonts();

// The directories searched for <lang>/LC_MESSAGES/washi.mo besides the standard data dirs.
QStringList washi_catalog_dirs();
