import QtQuick
import QtQuick.Layouts

// Add Printer: search the network (the service browses mDNS for about 3 seconds) or type an address.
WDialog {
    id: dlg

    property var knownBefore: []
    // A message source (see Fmt.text()).
    property var manualErrorSource: ""
    readonly property string manualError: Fmt.text(manualErrorSource)

    title: i18n("Add Printer")
    dialogWidth: 520

    function search() {
        knownBefore = Store.printers.map(p => p.host);
        Store.discover(null);
    }

    onAboutToShow: {
        manualErrorSource = "";
        manualField.text = "";
        search();
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 10
        Spinner {
            visible: Store.discovering
            size: 22
        }
        WText {
            Layout.fillWidth: true
            kind: "dim"
            wrapMode: Text.WordWrap
            text: Store.discovering ? i18n("Looking for printers on your network…") : (Store.printers.length === 0 ? i18n("No printer found. Check that it is on and on this network, or enter its address below.") : i18np("%1 printer known.", "%1 printers known.", Store.printers.length))
        }
    }

    // Found printers
    Repeater {
        model: Store.printers
        delegate: WCard {
            id: row
            required property var modelData
            Layout.fillWidth: true
            implicitHeight: 56
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 14
                anchors.rightMargin: 12
                spacing: 12
                Glyph {
                    name: "printer"
                    size: 28
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    WText {
                        Layout.fillWidth: true
                        text: Store.listLabel(row.modelData)
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }
                    WText {
                        Layout.fillWidth: true
                        kind: "small"
                        text: row.modelData.host + (dlg.knownBefore.indexOf(row.modelData.host) < 0 ? "  ·  " + i18n("new") : "")
                        elide: Text.ElideRight
                    }
                }
                WText {
                    visible: row.modelData.active
                    kind: "small"
                    color: Theme.ok
                    text: i18n("In use")
                }
                WButton {
                    visible: !row.modelData.active
                    text: i18n("Use this printer")
                    onClicked: {
                        Store.setActive(row.modelData.host, null);
                        dlg.close();
                    }
                }
            }
        }
    }

    // Manual address
    WText {
        Layout.topMargin: 6
        font.weight: Font.DemiBold
        text: i18n("Enter an address")
    }
    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        WField {
            id: manualField
            Layout.fillWidth: true
            placeholderText: i18n("Host name or IP address")
            onAccepted: addButton.clicked()
        }
        WButton {
            id: addButton
            text: i18n("Add")
            enabled: manualField.text.trim() !== ""
            onClicked: {
                Store.setActive(manualField.text.trim(), (ok, body) => {
                    if (ok) {
                        dlg.close();
                    } else {
                        dlg.manualErrorSource = Fmt.errorSource(body);
                    }
                });
            }
        }
    }
    WText {
        visible: dlg.manualError !== ""
        Layout.fillWidth: true
        color: Theme.danger
        wrapMode: Text.WordWrap
        text: dlg.manualError
    }

    buttons: [
        WButton {
            text: i18n("Search Again")
            glyph: "refresh"
            enabled: !Store.discovering
            onClicked: dlg.search()
        },
        WButton {
            text: i18n("Close")
            onClicked: dlg.close()
        }
    ]
}
