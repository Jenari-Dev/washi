import QtQuick
import QtQuick.Layouts

// The ink tank model numbers of the active printer, from its snapshot.
WDialog {
    id: dlg

    title: i18n("Ink Model Number")
    dialogWidth: 460

    WText {
        Layout.fillWidth: true
        kind: "dim"
        wrapMode: Text.WordWrap
        text: i18n("Buy tanks with these model numbers for %1.", Store.displayName)
    }
    Repeater {
        model: Store.inks
        delegate: RowLayout {
            required property var modelData
            Layout.fillWidth: true
            spacing: 12
            Rectangle {
                Layout.preferredWidth: 14
                Layout.preferredHeight: 14
                radius: 3
                color: Fmt.inkColor(modelData)
                border.width: 1
                border.color: "#00000022"
            }
            WText {
                Layout.preferredWidth: 120
                text: Fmt.inkName(modelData.color)
            }
            WText {
                Layout.fillWidth: true
                font.weight: Font.DemiBold
                text: modelData.model ? Fmt.tankModel(modelData.model) : i18n("not known yet")
                color: modelData.model ? Theme.text : Theme.textMuted
            }
        }
    }
    WText {
        Layout.fillWidth: true
        visible: Store.inks.some(i => !i.model)
        kind: "muted"
        wrapMode: Text.WordWrap
        text: i18n("Model numbers appear once the printer has been awake while Washi was running.")
    }
    buttons: [
        WButton {
            text: i18n("Close")
            onClicked: dlg.close()
        }
    ]
}
