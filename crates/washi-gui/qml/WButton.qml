import QtQuick
import QtQuick.Controls as QQC2

// Washi's button. `kind`: primary (blue), secondary (white, grey border),
// quiet (no border, for toolbars) or danger.
QQC2.AbstractButton {
    id: control

    property string kind: "secondary"
    property string glyph: ""
    property int radius: 4

    readonly property bool solid: kind === "primary" || kind === "danger"
    readonly property color fg: !enabled ? (solid ? "#FFFFFF" : Theme.textMuted) : (solid ? Theme.accentText : Theme.text)

    implicitHeight: 34
    implicitWidth: Math.max(kind === "quiet" ? 0 : 92, contentItem.implicitWidth + 28)
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    opacity: 1

    background: Rectangle {
        radius: control.radius
        color: {
            if (control.kind === "primary") {
                return !control.enabled ? "#AFC0E0" : (control.down ? Theme.accentPressed : (control.hovered ? Theme.accentHover : Theme.accent));
            }
            if (control.kind === "danger") {
                return !control.enabled ? "#E4B5B5" : (control.down ? "#9F2B2B" : (control.hovered ? "#B03232" : Theme.danger));
            }
            if (control.kind === "quiet") {
                return control.down ? "#DCDCDC" : (control.hovered && control.enabled ? "#E6E6E6" : "transparent");
            }
            return !control.enabled ? "#F7F7F7" : (control.down ? "#E9E9E9" : (control.hovered ? "#F4F4F4" : "#FFFFFF"));
        }
        border.width: control.kind === "quiet" || control.solid ? 0 : 1
        border.color: control.visualFocus ? Theme.accent : (control.enabled ? Theme.fieldBorder : "#D6D6D6")
        Rectangle {
            visible: control.visualFocus
            anchors.fill: parent
            anchors.margins: -2
            radius: control.radius + 2
            color: "transparent"
            border.width: 2
            border.color: Theme.accent
            opacity: 0.5
        }
    }

    contentItem: Item {
        implicitWidth: row.implicitWidth
        implicitHeight: row.implicitHeight
        Row {
            id: row
            spacing: 8
            anchors.centerIn: parent
            Glyph {
                visible: control.glyph !== ""
                anchors.verticalCenter: parent.verticalCenter
                name: control.glyph
                size: 18
                weight: 1.8
                color: control.fg
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: control.text
                visible: text !== ""
                font.pixelSize: Theme.fontSize
                color: control.fg
            }
        }
    }
}
