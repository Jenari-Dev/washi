// SPDX-License-Identifier: GPL-2.0-or-later
// The QML test runner: Qt Quick Test with the same i18n setup the app has (KI18n's QML context
// and the real catalogs), so the tests see exactly what the app shows, in both languages.
//
//   runner -input <dir>            runs every tst_*.qml of the directory
//
// $WASHI_LOCALE_DIR points at the compiled catalogs (<dir>/ja/LC_MESSAGES/washi.mo); run.sh
// compiles po/*.po there first.
#include <QQmlContext>
#include <QQmlEngine>
#include <QtQuickTest>

#include "i18n.h"

// A QML-visible handle on washi_apply_language(), the function the app's Backend calls.
class L10n : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE void apply(const QString &lang) { washi_apply_language(lang); }
    // $WASHI_LOCALE_DIR/<lang>/LC_MESSAGES/washi.mo exists
    Q_INVOKABLE bool hasCatalog(const QString &lang) const
    {
        const auto dirs = washi_catalog_dirs();
        for (const QString &d : dirs) {
            if (QFile::exists(d + QLatin1Char('/') + lang + QStringLiteral("/LC_MESSAGES/washi.mo"))) {
                return true;
            }
        }
        return false;
    }
};

class Setup : public QObject
{
    Q_OBJECT
public slots:
    void qmlEngineAvailable(QQmlEngine *engine)
    {
        washi_i18n_setup(engine);
        washi_apply_language(QStringLiteral("en"));
        engine->rootContext()->setContextProperty(QStringLiteral("L10n"), new L10n(engine));
    }
};

QUICK_TEST_MAIN_WITH_SETUP(washi, Setup)

#include "runner.moc"
