import QtQuick
import QtQuick.Controls as QQC2

// Round grey icon button (banner arrows).
QQC2.AbstractButton {
    id: control

    property string glyph: "chevleft"
    property real size: 32

    implicitWidth: size
    implicitHeight: size
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus

    background: Rectangle {
        radius: control.size / 2
        color: control.down ? "#AEB6C4" : (control.hovered ? "#BCC3CF" : "#CBD1DB")
        opacity: control.enabled ? 1 : 0.4
        border.width: control.visualFocus ? 2 : 0
        border.color: Theme.accent
    }
    contentItem: Glyph {
        anchors.centerIn: parent
        name: control.glyph
        size: control.size * 0.56
        weight: 2.2
        color: "#FFFFFF"
        opacity: control.enabled ? 1 : 0.6
    }
}
