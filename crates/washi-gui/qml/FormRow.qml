import QtQuick
import QtQuick.Layouts

// A label above a control, for the right-hand settings panels.
ColumnLayout {
    property string label: ""
    default property alias control: slot.data

    Layout.fillWidth: true
    // Take the width the column gives (a wrapping hint text would otherwise push the row wider).
    Layout.preferredWidth: 10
    Layout.minimumWidth: 0
    spacing: 4

    WText {
        text: parent.label
        font.weight: Font.DemiBold
        kind: "small"
        color: Theme.text
    }
    ColumnLayout {
        id: slot
        Layout.fillWidth: true
        spacing: 2
    }
}
