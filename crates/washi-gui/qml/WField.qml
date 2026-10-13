import QtQuick
import QtQuick.Controls as QQC2

QQC2.TextField {
    id: control

    implicitHeight: 34
    implicitWidth: 200
    leftPadding: 10
    rightPadding: 10
    font.pixelSize: Theme.fontSize
    color: enabled ? Theme.text : Theme.textMuted
    placeholderTextColor: Theme.textMuted
    selectByMouse: true
    verticalAlignment: TextInput.AlignVCenter

    background: Rectangle {
        radius: 4
        color: control.enabled ? "#FFFFFF" : "#F7F7F7"
        border.width: control.activeFocus ? 2 : 1
        border.color: control.activeFocus ? Theme.accent : (control.enabled ? Theme.fieldBorder : "#D6D6D6")
    }
}
