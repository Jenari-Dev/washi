// SPDX-License-Identifier: GPL-2.0-or-later
#include "i18n.h"

#include <KLocalizedQmlContext>
#include <KLocalizedString>
#include <QCoreApplication>
#include <QDir>
#include <QFontDatabase>
#include <QGuiApplication>
#include <QLibraryInfo>
#include <QLocale>
#include <QPointer>
#include <QQmlEngine>
#include <QTranslator>
#include <clocale>
#include <cstdio>
#include <cstring>

namespace
{
QPointer<QQmlEngine> g_engine;
// Qt's own strings (the stock file dialog, standard buttons) when Qt's translations are
// installed. Heap objects owned by the application: statics would be torn down in an order
// that does not suit QCoreApplication's list of translators.
QList<QPointer<QTranslator>> g_qtTranslators;

void setQtTranslators(const QString &lang)
{
    for (const QPointer<QTranslator> &t : std::as_const(g_qtTranslators)) {
        if (t) {
            QCoreApplication::removeTranslator(t);
            delete t;
        }
    }
    g_qtTranslators.clear();
    QCoreApplication *app = QCoreApplication::instance();
    if (lang == QLatin1String("en") || !app) {
        return;
    }
    const QString dir = QLibraryInfo::path(QLibraryInfo::TranslationsPath);
    const QLocale locale(lang);
    // Missing files are fine: Qt's own texts then stay English.
    for (const char *name : {"qtbase", "qtdeclarative"}) {
        auto *t = new QTranslator(app);
        if (t->load(locale, QString::fromLatin1(name), QStringLiteral("_"), dir)) {
            QCoreApplication::installTranslator(t);
            g_qtTranslators << t;
        } else {
            delete t;
        }
    }
}
}

// gettext (which KI18n uses) ignores the requested languages in the "C" locale, which is what a
// session without LANG, or with LANG=C.UTF-8, runs in. Message lookups then need some real
// locale; the one chosen here changes nothing but gettext's messages (LC_MESSAGES).
void washi_ensure_translatable_locale()
{
    const char *cur = setlocale(LC_MESSAGES, nullptr);
    const bool plain = !cur || !*cur || std::strcmp(cur, "C") == 0 || std::strcmp(cur, "POSIX") == 0 || std::strncmp(cur, "C.", 2) == 0;
    if (!plain) {
        return;
    }
    for (const char *name : {"en_US.UTF-8", "ja_JP.UTF-8", "en_GB.UTF-8", "en_US", "ja_JP"}) {
        if (setlocale(LC_MESSAGES, name)) {
            return;
        }
    }
    std::fprintf(stderr, "washi: no locale besides C is installed, so the interface can only be shown in English\n");
}

QStringList washi_catalog_dirs()
{
    QStringList dirs;
    const QByteArray env = qgetenv("WASHI_LOCALE_DIR");
    if (!env.isEmpty()) {
        dirs << QString::fromLocal8Bit(env);
    }
    if (QCoreApplication::instance()) {
        // A build tree has the catalogs in <target>/<profile>/share/locale, an install in
        // <prefix>/share/locale next to <prefix>/bin.
        const QString app = QCoreApplication::applicationDirPath();
        dirs << app + QStringLiteral("/share/locale") << app + QStringLiteral("/../share/locale");
    }
    QStringList existing;
    for (const QString &d : std::as_const(dirs)) {
        if (QDir(d).exists()) {
            existing << QDir(d).canonicalPath();
        }
    }
    existing.removeDuplicates();
    return existing;
}

// Chinese and Japanese share the Han characters but draw many of them differently, and the system's
// fallback for them is chosen by the session's locale (an English session gets a Korean or
// Chinese font). Washi's Japanese text should be drawn with Japanese letter shapes whatever the
// session says, so Japanese fonts come first for these scripts.
void washi_prefer_japanese_fonts()
{
    if (!qobject_cast<QGuiApplication *>(QCoreApplication::instance())) {
        return;
    }
    static const char *const families[] = {"Noto Sans CJK JP", "Noto Sans JP", "Source Han Sans JP", "IPAexGothic", "IPAGothic", "VL Gothic", "Takao PGothic", "Yu Gothic", "Meiryo", "Hiragino Sans"};
    for (QChar::Script script : {QChar::Script_Han, QChar::Script_Hiragana, QChar::Script_Katakana}) {
        for (const char *family : families) {
            QFontDatabase::addApplicationFallbackFontFamily(script, QString::fromLatin1(family));
        }
    }
}

void washi_i18n_setup(QQmlEngine *engine)
{
    g_engine = engine;
    washi_ensure_translatable_locale();
    washi_prefer_japanese_fonts();
    // Strings are looked up in the "washi" catalog (washi.mo); English source strings are shown
    // when no catalog is installed for the language.
    KLocalizedString::setApplicationDomain(QByteArrayLiteral("washi"));
    for (const QString &d : washi_catalog_dirs()) {
        KLocalizedString::addDomainLocaleDir(QByteArrayLiteral("washi"), d);
    }
    // Matches contrib/io.github.Jenari_Dev.Washi.desktop (window icon and grouping on Wayland).
    if (qobject_cast<QGuiApplication *>(QCoreApplication::instance())) {
        QGuiApplication::setDesktopFileName(QStringLiteral("io.github.Jenari_Dev.Washi"));
    }
    // Provides i18n(), i18nc(), i18np() ... to every QML file of the engine. Unlike the older
    // KLocalizedContext, bindings that call them are re-evaluated by QQmlEngine::retranslate().
    KLocalization::setupLocalizedContext(engine);
}

void washi_i18n_setup_app(QQmlApplicationEngine &engine)
{
    washi_i18n_setup(&engine);
}

void washi_apply_language(const QString &lang)
{
    const bool ja = lang == QLatin1String("ja");
    // Japanese first, English as the fallback for anything the catalog does not have;
    // "en_US" is the language of the source strings.
    KLocalizedString::setLanguages(ja ? QStringList{QStringLiteral("ja"), QStringLiteral("en_US")}
                                      : QStringList{QStringLiteral("en_US")});
    setQtTranslators(ja ? QStringLiteral("ja") : QStringLiteral("en"));
    // Numbers, dates and the like that C++ and Qt format follow the language too.
    QLocale::setDefault(ja ? QLocale(QLocale::Japanese, QLocale::Japan) : QLocale(QLocale::English, QLocale::UnitedStates));
    if (g_engine) {
        g_engine->setUiLanguage(ja ? QStringLiteral("ja") : QStringLiteral("en"));
        g_engine->retranslate();
    }
}
