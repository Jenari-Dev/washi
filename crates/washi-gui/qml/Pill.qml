import QtQuick
import QtQuick.Controls as QQC2

// Small white pill button ("+ Add Printer", "Order Ink").
QQC2.AbstractButton {
    id: control

    property string glyph: ""

    implicitHeight: 28
    implicitWidth: contentItem.implicitWidth + 24
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus

    background: Rectangle {
        radius: height / 2
        color: control.down ? "#E4EAF4" : (control.hovered ? "#F4F7FC" : "#FFFFFF")
        border.width: control.visualFocus ? 2 : 1
        border.color: control.visualFocus ? Theme.accent : "#B6C2D8"
        opacity: control.enabled ? 1 : 0.5
    }
    contentItem: Item {
        implicitWidth: row.implicitWidth
        implicitHeight: row.implicitHeight
        Row {
            id: row
            spacing: 6
            anchors.centerIn: parent
            Glyph {
                visible: control.glyph !== ""
                anchors.verticalCenter: parent.verticalCenter
                name: control.glyph
                size: 14
                weight: 2
                color: Theme.accent
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: control.text
                font.pixelSize: Theme.small + 1
                color: control.enabled ? Theme.text : Theme.textMuted
            }
        }
    }
}
