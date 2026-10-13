import QtQuick
import QtQuick.Layouts

// A row of colour swatches and a hex field. `value` is "#rrggbb"; `picked(color)` is emitted
// when the user chooses one.
ColumnLayout {
    id: picker

    property string value: "#000000"
    signal picked(string color)

    spacing: 6

    Flow {
        Layout.fillWidth: true
        spacing: 6
        Repeater {
            model: DiscMath.swatches
            delegate: Rectangle {
                id: sw
                required property string modelData
                objectName: "swatch-" + modelData.substring(1).toLowerCase()
                width: 24
                height: 24
                radius: 12
                color: modelData
                border.width: picker.value.toLowerCase() === modelData.toLowerCase() ? 3 : 1
                border.color: picker.value.toLowerCase() === modelData.toLowerCase() ? Theme.accent : "#B0B0B0"
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: picker.picked(sw.modelData.toLowerCase())
                }
            }
        }
    }
    RowLayout {
        spacing: 8
        Rectangle {
            Layout.preferredWidth: 28
            Layout.preferredHeight: 28
            radius: 4
            color: DiscMath.isColor(picker.value) ? picker.value : "#FFFFFF"
            border.width: 1
            border.color: "#B0B0B0"
        }
        WField {
            id: hex
            objectName: "hexField"
            Layout.fillWidth: true
            text: picker.value
            maximumLength: 7
            inputMethodHints: Qt.ImhNoPredictiveText
            onEditingFinished: {
                const t = text.trim().startsWith("#") ? text.trim() : "#" + text.trim();
                if (DiscMath.isColor(t)) {
                    picker.picked(t.toLowerCase());
                } else {
                    text = picker.value;
                }
            }
        }
    }
}
