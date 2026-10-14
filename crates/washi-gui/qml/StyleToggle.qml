import QtQuick
import QtQuick.Controls as QQC2

// A small square switch for a text style: the letter is drawn in the style it turns on (a bold B,
// an italic I). `label` is what the tooltip and a screen reader say.
QQC2.AbstractButton {
    id: control

    property string letter: "B"
    property string label: ""
    property bool styleBold: false
    property bool styleItalic: false

    checkable: true
    implicitWidth: 34
    implicitHeight: 34
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    Accessible.role: Accessible.CheckBox
    Accessible.name: label
    Accessible.checked: checked
    QQC2.ToolTip.visible: hovered && label !== ""
    QQC2.ToolTip.text: label
    QQC2.ToolTip.delay: 400

    background: Rectangle {
        radius: 4
        color: control.checked ? Theme.accentSoft : (control.down ? "#E9E9E9" : (control.hovered ? "#F4F4F4" : "#FFFFFF"))
        border.width: control.visualFocus || control.checked ? 2 : 1
        border.color: control.visualFocus || control.checked ? Theme.accent : Theme.fieldBorder
    }
    contentItem: Text {
        text: control.letter
        textFormat: Text.PlainText
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        font.pixelSize: Theme.fontSize + 2
        font.bold: control.styleBold
        font.italic: control.styleItalic
        color: control.checked ? Theme.accent : Theme.text
    }
}
