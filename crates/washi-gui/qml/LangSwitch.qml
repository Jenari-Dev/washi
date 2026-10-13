import QtQuick
import QtQuick.Controls as QQC2

// The language switch: a compact segmented control, "EN | 日本語". The names of the languages
// are written in the languages themselves and are not translated.
Item {
    id: sw

    readonly property var choices: [
        { code: "en", label: "EN", name: "English" },
        { code: "ja", label: "日本語", name: "日本語" }
    ]

    implicitWidth: row.implicitWidth + 2
    implicitHeight: 26

    Rectangle {
        anchors.fill: parent
        radius: 5
        color: "#FFFFFF"
        border.width: 1
        border.color: Theme.fieldBorder
    }

    Row {
        id: row
        x: 1
        y: 1
        height: parent.height - 2

        Repeater {
            model: sw.choices
            delegate: QQC2.AbstractButton {
                id: seg
                required property var modelData
                required property int index
                objectName: "lang-" + modelData.code
                readonly property bool selected: Lang.current === modelData.code

                height: row.height
                implicitWidth: Math.max(38, label.implicitWidth + 22)
                hoverEnabled: true
                focusPolicy: Qt.StrongFocus
                checkable: false
                onClicked: Lang.choose(modelData.code)

                Accessible.role: Accessible.RadioButton
                Accessible.name: modelData.name
                Accessible.checked: selected

                background: Rectangle {
                    radius: 4
                    color: seg.selected ? Theme.accent : (seg.down ? "#E4EAF4" : (seg.hovered ? "#EEF2F9" : "transparent"))
                    border.width: seg.visualFocus ? 2 : 0
                    border.color: Theme.accent
                }
                contentItem: Text {
                    id: label
                    text: seg.modelData.label
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    font.pixelSize: Theme.small + 1
                    font.weight: seg.selected ? Font.DemiBold : Font.Normal
                    color: seg.selected ? Theme.accentText : Theme.text
                }
            }
        }
    }

    QQC2.ToolTip.visible: hover.hovered
    QQC2.ToolTip.delay: 450
    QQC2.ToolTip.text: i18n("Language")
    HoverHandler {
        id: hover
    }
}
