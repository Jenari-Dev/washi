import QtQuick
import QtQuick.Controls as QQC2

// Slider with a blue filled track and a round handle.
QQC2.Slider {
    id: control

    implicitHeight: 28
    implicitWidth: 240
    snapMode: QQC2.Slider.SnapAlways
    hoverEnabled: true

    background: Rectangle {
        x: control.leftPadding
        y: control.topPadding + control.availableHeight / 2 - height / 2
        width: control.availableWidth
        height: 4
        radius: 2
        color: "#CFCFCF"
        Rectangle {
            width: control.visualPosition * parent.width
            height: parent.height
            radius: 2
            color: control.enabled ? Theme.accent : "#AFC0E0"
        }
    }
    handle: Rectangle {
        x: control.leftPadding + control.visualPosition * (control.availableWidth - width)
        y: control.topPadding + control.availableHeight / 2 - height / 2
        width: 20
        height: 20
        radius: 10
        color: control.enabled ? "#FFFFFF" : "#F2F2F2"
        border.width: 2
        border.color: control.enabled ? Theme.accent : "#AFC0E0"
    }
}
