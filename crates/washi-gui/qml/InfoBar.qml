import QtQuick
import QtQuick.Layouts

// A one-line notice: kind info, warning or error.
Rectangle {
    id: bar

    property string kind: "info"
    property string text: ""
    default property alias extra: extraRow.data

    implicitHeight: Math.max(40, row.implicitHeight + 16)
    radius: 6
    color: kind === "error" ? Theme.dangerBg : (kind === "warning" ? Theme.warnBg : Theme.accentSoft)
    border.width: 1
    border.color: kind === "error" ? "#E8B4B4" : (kind === "warning" ? "#EBD08A" : "#C4D6F0")

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 10
        Glyph {
            name: bar.kind === "info" ? "info" : bar.kind
            size: 20
            color: bar.kind === "error" ? Theme.danger : (bar.kind === "warning" ? Theme.warn : Theme.accent)
        }
        WText {
            Layout.fillWidth: true
            text: bar.text
            wrapMode: Text.WordWrap
        }
        RowLayout {
            id: extraRow
            spacing: 8
        }
    }
}
