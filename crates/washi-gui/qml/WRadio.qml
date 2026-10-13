import QtQuick
import QtQuick.Controls as QQC2

QQC2.RadioButton {
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
    contentItem: Text {
        id: label
        text: control.text
        leftPadding: control.indicator.width + control.spacing
        verticalAlignment: Text.AlignVCenter
        font.pixelSize: Theme.fontSize
        color: control.enabled ? Theme.text : Theme.textMuted
    }
}
