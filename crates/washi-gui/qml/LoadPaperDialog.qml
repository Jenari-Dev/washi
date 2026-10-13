import QtQuick
import QtQuick.Layouts

// "How to load paper": short instructions of our own for the rear tray, the cassette and the
// disc tray. The first print asks once (Prefs.hideLoadPaperHint remembers "do not show again").
WDialog {
    id: dlg

    // "rear" | "cassette" | "disc"
    property string where: "rear"
    property bool photo: false
    // When opened as a reminder before printing, "Print" continues.
    property bool reminder: false
    signal proceed()

    title: where === "disc" ? i18n("Load the disc tray") : i18n("How to load paper")
    dialogWidth: 520

    function openFor(tray, isPhoto, asReminder) {
        where = tray;
        photo = isPhoto;
        reminder = asReminder;
        dontShow.checked = false;
        open();
    }

    readonly property var steps: {
        if (where === "disc") {
            return [
                i18n("Pull out the disc tray guide from the printer's front, the way the printer's display shows."),
                i18n("Put the printable disc in the tray with the printable side up and press it flat."),
                i18n("Slide the tray in until it stops, then press Print here."),
                i18n("Do not touch the printed side until the ink has dried.")
            ];
        }
        if (where === "cassette") {
            return [
                i18n("Pull out the cassette and slide the paper guides apart."),
                i18n("Put plain paper in with the print side down, and slide the guides against the paper."),
                i18n("Push the cassette back in until it clicks and set the paper size on the printer if it asks.")
            ];
        }
        return [
            i18n("Open the rear tray's cover and pull out its support, then slide the paper guides apart."),
            photo
                ? i18n("Put photo paper in with the glossy (print) side facing you, and slide the guides against it.")
                : i18n("Put the paper in with the print side facing you, and slide the guides against it."),
            i18n("Tell the printer the paper size and type if its display asks.")
        ];
    }

    Repeater {
        model: dlg.steps
        delegate: RowLayout {
            required property string modelData
            required property int index
            Layout.fillWidth: true
            spacing: 12
            Rectangle {
                Layout.alignment: Qt.AlignTop
                Layout.preferredWidth: 26
                Layout.preferredHeight: 26
                radius: 13
                color: Theme.accent
                WText {
                    anchors.centerIn: parent
                    text: index + 1
                    color: "#FFFFFF"
                    font.weight: Font.Bold
                    font.pixelSize: 13
                }
            }
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: modelData
            }
        }
    }
    WText {
        Layout.fillWidth: true
        Layout.topMargin: 4
        kind: "muted"
        wrapMode: Text.WordWrap
        text: i18n("Your printer's manual has pictures for these steps. Washi does not send anything to Canon to show them.")
    }
    WSwitch {
        id: dontShow
        objectName: "dontShowAgain"
        visible: dlg.reminder
        text: i18n("Don't show this next time")
    }
    buttons: [
        WButton {
            visible: !dlg.reminder
            text: i18n("Open the Manual")
            glyph: "external"
            onClicked: Nav.openLink("help")
        },
        WButton {
            text: dlg.reminder ? i18n("Cancel") : i18n("Close")
            onClicked: dlg.close()
        },
        WButton {
            objectName: "loadedButton"
            visible: dlg.reminder
            kind: "primary"
            text: i18n("Paper is loaded")
            onClicked: {
                if (dontShow.checked) {
                    Prefs.hideLoadPaperHint = true;
                }
                dlg.close();
                dlg.proceed();
            }
        }
    ]
}
