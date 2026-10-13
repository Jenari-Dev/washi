import QtQuick
import QtQuick.Controls as QQC2

// A radio button with a small drawing before its text (Portrait / Landscape).
QQC2.RadioButton {
    id: control

    property string glyph: ""

    implicitHeight: 32
    implicitWidth: indicator.width + spacing + 20 + 4 + label.implicitWidth + 4
    hoverEnabled: true
    spacing: 8

    indicator: Rectangle {
        x: 0
        y: (control.height - height) / 2
        width: 20
        height: 20
        radius: 10
        color: "#FFFFFF"
        border.width: control.checked ? 2 : 1.5
        border.color: !control.enabled ? "#C4C4C4" : (control.checked ? Theme.accent : "#6B6B6B")
        Rectangle {
            anchors.centerIn: parent
            width: control.down ? 12 : 10
            height: width
            radius: width / 2
            visible: control.checked
            color: control.enabled ? Theme.accent : "#AFC0E0"
        }
    }
    contentItem: Item {
        Glyph {
            id: g
            x: control.indicator.width + control.spacing
            anchors.verticalCenter: parent.verticalCenter
            name: control.glyph
            size: 20
            weight: 1.5
            color: control.checked ? Theme.accent : (control.enabled ? Theme.navy : Theme.textMuted)
        }
        Text {
            id: label
            x: g.x + g.width + 4
            anchors.verticalCenter: parent.verticalCenter
            text: control.text
            font.pixelSize: Theme.fontSize
            color: control.enabled ? Theme.text : Theme.textMuted
        }
    }
}
