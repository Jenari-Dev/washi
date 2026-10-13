import QtQuick
import QtQuick.Controls as QQC2

QQC2.SpinBox {
    id: control

    implicitHeight: 34
    implicitWidth: 120
    editable: true
    font.pixelSize: Theme.fontSize

    contentItem: TextInput {
        text: control.textFromValue(control.value, control.locale)
        font: control.font
        color: control.enabled ? Theme.text : Theme.textMuted
        horizontalAlignment: Qt.AlignHCenter
        verticalAlignment: Qt.AlignVCenter
        readOnly: !control.editable
        validator: control.validator
        inputMethodHints: Qt.ImhFormattedNumbersOnly
    }
    up.indicator: Rectangle {
        x: control.width - width
        height: control.height
        width: 32
        radius: 4
        color: control.up.pressed ? "#E9E9E9" : "transparent"
        Glyph { anchors.centerIn: parent; name: "plus"; size: 14; color: control.enabled ? Theme.text : Theme.textMuted }
    }
    down.indicator: Rectangle {
        height: control.height
        width: 32
        radius: 4
        color: control.down.pressed ? "#E9E9E9" : "transparent"
        Rectangle { anchors.centerIn: parent; width: 12; height: 1.7; radius: 1; color: control.enabled ? Theme.text : Theme.textMuted }
    }
    background: Rectangle {
        radius: 4
        color: control.enabled ? "#FFFFFF" : "#F7F7F7"
        border.width: 1
        border.color: control.enabled ? Theme.fieldBorder : "#D6D6D6"
    }
}
