import QtQuick
import QtQuick.Layouts

// Shown instead of printer data when no printer is set up yet.
Item {
    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width - 48, 420)
        spacing: 14
        Glyph {
            Layout.alignment: Qt.AlignHCenter
            name: "printer"
            size: 56
            weight: 1.3
            color: Theme.sleeping
        }
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            kind: "heading"
            text: i18n("No printer yet")
        }
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            kind: "dim"
            wrapMode: Text.WordWrap
            text: i18n("Washi has not found a printer. Make sure it is switched on and on the same network, then add it.")
        }
        WButton {
            Layout.alignment: Qt.AlignHCenter
            kind: "primary"
            glyph: "plus"
            text: i18n("Add Printer")
            onClicked: Nav.dialog("discover")
        }
    }
}
