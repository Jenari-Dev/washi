import QtQuick

// Status marker: a coloured dot, or a warning / error icon.
Item {
    id: dot

    property string kind: "unknown"     // online sleeping offline busy warning error unknown none
    property real size: 16

    implicitWidth: size
    implicitHeight: size

    readonly property bool icon: kind === "warning" || kind === "error"

    Rectangle {
        visible: !dot.icon
        anchors.centerIn: parent
        width: dot.size * 0.7
        height: width
        radius: width / 2
        color: {
            switch (dot.kind) {
            case "online": return Theme.ok;
            case "sleeping": return Theme.sleeping;
            case "busy": return Theme.accent;
            case "offline": return Theme.offline;
            default: return "#C4C4C4";
            }
        }
    }
    Glyph {
        visible: dot.icon
        anchors.centerIn: parent
        name: dot.kind
        size: dot.size
        weight: 2
        color: dot.kind === "error" ? Theme.danger : Theme.warn
    }
}
