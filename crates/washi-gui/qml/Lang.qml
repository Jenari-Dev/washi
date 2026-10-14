pragma Singleton

import QtQuick

// The interface language ("en" or "ja"). The Backend object (or a fake in the tests) keeps it
// and does the switching: apply it to the engine, save [ui] language, no restart. Everything
// that looks at `current` or at the strings of i18n() updates by itself when it changes.
QtObject {
    id: lang

    readonly property string current: Store.transport && Store.transport.language ? Store.transport.language : "en"
    readonly property bool ja: current === "ja"
    // For numbers and dates: Qt.locale() with no argument would not change with the language.
    readonly property var locale: Qt.locale(ja ? "ja_JP" : "en_US")

    // "en", "ja" or "system" (follow the session's locale).
    function choose(code) {
        if (Store.transport && Store.transport.chooseLanguage) {
            Store.transport.chooseLanguage(code);
        }
    }

    // The daemon words its notifications and messages in the same language, and the printer's
    // own names for papers and so on are asked for in it.
    onCurrentChanged: {
        Store.syncLanguage();
        Store.loadPrinterStrings();
    }
}
