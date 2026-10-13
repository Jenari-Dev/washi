pragma Singleton

import QtQuick

// Light palette and metrics. Colours are measured from public screenshots of the app Washi
// replaces; nothing here is copied from vendor files.
QtObject {
    readonly property color bg: "#F3F3F3"
    readonly property color navBg: "#EBEBEB"
    readonly property color card: "#FFFFFF"
    readonly property color cardBorder: "#DCDCDC"
    readonly property color line: "#E4E4E4"
    readonly property color fieldBorder: "#B9B9B9"

    readonly property color text: "#1F1F1F"
    readonly property color textDim: "#5A5A5A"
    readonly property color textMuted: "#8C8C8C"
    readonly property color navy: "#1B2F5E"
    readonly property color accent: "#1E50B4"
    readonly property color accentHover: "#1A459C"
    readonly property color accentPressed: "#153A85"
    readonly property color accentTint: "#D9E6F8"
    readonly property color accentSoft: "#EAF1FB"
    readonly property color accentText: "#FFFFFF"

    readonly property color ok: "#2E9E4F"
    readonly property color warn: "#D98A00"
    readonly property color danger: "#C93A3A"
    readonly property color sleeping: "#7F93B2"
    readonly property color offline: "#A0A0A0"
    readonly property color warnBg: "#FFF3CD"
    readonly property color dangerBg: "#FBE4E4"

    readonly property color bannerLeft: "#D5E7FA"
    readonly property color bannerRight: "#D6F1E4"

    // Ink swatches (display only).
    readonly property var inkColors: ({
        "BK": "#2B2B2B", "PGBK": "#46464B", "C": "#12A4DC", "M": "#E0368C", "Y": "#F2CB1D",
        "GY": "#8E8E8E", "PBK": "#46464B"
    })

    readonly property int radius: 6
    readonly property int fontSize: 14
    readonly property int small: 12
    readonly property int heading: 18
    readonly property int title: 24

    readonly property int railWidth: 48
    readonly property int railExpanded: 212
    readonly property int pageMargin: 24
}
