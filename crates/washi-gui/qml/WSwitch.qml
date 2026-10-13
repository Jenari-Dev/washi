import QtQuick
import QtQuick.Controls as QQC2

// Toggle switch (track + knob); the label sits on the right.
QQC2.Switch {
    id: control

    implicitHeight: 28
    implicitWidth: indicator.width + (text !== "" ? label.implicitWidth + 10 : 0)
    hoverEnabled: true
    spacing: 10

    indicator: Rectangle {
        x: 0
        y: (control.height - height) / 2
        width: 42
        height: 22
        radius: 11
        color: control.checked ? (control.enabled ? Theme.accent : "#AFC0E0") : "#FFFFFF"
        border.width: control.checked ? 0 : 1.5
        border.color: control.enabled ? "#6B6B6B" : "#C4C4C4"
        Behavior on color { ColorAnimation { duration: 90 } }

        Rectangle {
            width: control.down ? 18 : 14
            height: 14
            radius: 7
            y: (parent.height - height) / 2
            x: control.checked ? parent.width - width - 4 : 4
            color: control.checked ? "#FFFFFF" : (control.enabled ? "#4A4A4A" : "#B5B5B5")
            Behavior on x { NumberAnimation { duration: 90 } }
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
