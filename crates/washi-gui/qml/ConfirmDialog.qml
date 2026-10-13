import QtQuick
import QtQuick.Layouts

// A plain question with Cancel and an accept button.
WDialog {
    id: dlg

    property string text: ""
    property string acceptText: i18n("OK")
    property string glyph: ""
    signal accepted()

    WText {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        text: dlg.text
    }
    buttons: [
        WButton {
            text: i18n("Cancel")
            onClicked: dlg.close()
        },
        WButton {
            objectName: "acceptButton"
            kind: "primary"
            glyph: dlg.glyph
            text: dlg.acceptText
            onClicked: {
                dlg.close();
                dlg.accepted();
            }
        }
    ]
}
