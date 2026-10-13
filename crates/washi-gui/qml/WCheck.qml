import QtQuick
import QtQuick.Controls as QQC2

// A check box in the app's style: a square with a tick.
QQC2.CheckBox {
    id: control

    implicitHeight: 30
    // the label's own left padding already makes room for the indicator
    padding: 0
    implicitWidth: label.implicitWidth + 4
    hoverEnabled: true
    spacing: 8

    indicator: Rectangle {
        x: 0
        y: (control.height - height) / 2
        width: 20
        height: 20
        radius: 3
        color: control.checked ? (control.enabled ? Theme.accent : "#AFC0E0") : "#FFFFFF"
        border.width: control.checked ? 0 : 1.5
        border.color: control.enabled ? "#6B6B6B" : "#C4C4C4"
        Glyph {
            anchors.centerIn: parent
            visible: control.checked
            name: "check"
            size: 16
            weight: 2.4
            color: "#FFFFFF"
        }
    }
    contentItem: Text {
        id: label
        text: control.text
        leftPadding: control.indicator.width + control.spacing
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.WordWrap
        font.pixelSize: Theme.fontSize
        color: control.enabled ? Theme.text : Theme.textMuted
    }
}
