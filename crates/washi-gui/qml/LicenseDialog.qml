import QtQuick
import QtQuick.Layouts

WDialog {
    id: dlg

    title: i18n("License Information")
    dialogWidth: 520

    WText {
        Layout.fillWidth: true
        font.weight: Font.DemiBold
        text: i18n("Washi %1", Qt.application.version)
    }
    WText {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        text: i18n("Washi is free software. You can redistribute it and change it under the terms of the GNU General Public License, version 2 or (at your option) any later version. It comes with no warranty.")
    }
    WText {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        kind: "dim"
        text: i18n("Washi is an independent project. It is not affiliated with, endorsed by or sponsored by Canon Inc. Canon, PIXUS, PIXMA and Canon PRINT are trademarks of Canon Inc., named here only to say which printers and which app Washi works with. No Canon code, firmware or artwork is included.")
    }
    buttons: [
        WButton {
            glyph: "external"
            text: i18n("Read the License")
            onClicked: Nav.openUrl("https://www.gnu.org/licenses/old-licenses/gpl-2.0.html")
        },
        WButton {
            kind: "primary"
            text: i18n("Close")
            onClicked: dlg.close()
        }
    ]
}
