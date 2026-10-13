import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

PageScroll {
    id: page

    Component.onCompleted: Store.refreshOthers()

    property string removeHost: ""
    property string removeName: ""
    // What the list says about the last removal: a message source (see Fmt.text()).
    property var noticeSource: ""
    readonly property string notice: Fmt.text(noticeSource)

    ConfirmDialog {
        id: removeDlg
        objectName: "removeDialog"
        title: i18n("Remove Printer")
        acceptText: i18n("Remove")
        glyph: "trash"
        text: i18n("Remove %1 from the list? Washi forgets it; nothing is changed on the printer itself. You can add it again with “Add Printer”.", page.removeName)
        onAccepted: Store.forgetPrinter(page.removeHost, (ok, body) => {
            const r = ok ? Store.parse(body) : null;
            if (!ok) {
                page.noticeSource = Fmt.errorSource(body);
            } else if (r && !r.persistent) {
                page.noticeSource = () => i18n("Removed for now. This printer comes from the command line or config.toml, or the service is read-only, so it returns when the service restarts.");
            } else {
                page.noticeSource = "";
            }
        })
    }

    InfoBar {
        objectName: "listNotice"
        Layout.fillWidth: true
        visible: page.notice !== ""
        kind: "info"
        text: page.notice
    }

    RowLayout {
        Layout.fillWidth: true
        WText {
            Layout.fillWidth: true
            kind: "title"
            text: i18n("Printer List")
        }
        WButton {
            kind: "primary"
            glyph: "plus"
            text: i18n("Add Printer")
            onClicked: Nav.dialog("discover")
        }
    }

    WCard {
        Layout.fillWidth: true
        implicitHeight: Math.max(list.implicitHeight, 72)

        WText {
            visible: Store.printers.length === 0
            anchors.centerIn: parent
            kind: "dim"
            text: i18n("No printers yet. Use “Add Printer” to find one.")
        }
        Column {
            id: list
            width: parent.width
            Repeater {
                model: Store.printers
                delegate: Item {
                    id: row
                    required property var modelData
                    required property int index
                    readonly property string kind: row.modelData.power === "on" ? "online" : (row.modelData.power === "sleeping" ? "sleeping" : (row.modelData.power === "offline" ? "offline" : "unknown"))
                    objectName: "printer-" + index
                    width: list.width
                    height: 60

                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: 3
                        radius: 4
                        color: rowArea.pressed ? "#E4EBF7" : (rowArea.containsMouse ? "#F3F6FB" : "transparent")
                    }
                    Rectangle {
                        visible: row.index > 0
                        x: 16
                        width: parent.width - 32
                        height: 1
                        color: Theme.line
                    }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 20
                        anchors.rightMargin: 20
                        spacing: 14
                        Item {
                            Layout.preferredWidth: 24
                            Layout.preferredHeight: 24
                            Glyph {
                                visible: row.modelData.active
                                anchors.centerIn: parent
                                name: "check"
                                size: 22
                                weight: 2.2
                                color: Theme.accent
                            }
                        }
                        StatusDot {
                            kind: row.kind
                            size: 18
                        }
                        Glyph {
                            name: "printer"
                            size: 30
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            WText {
                                Layout.fillWidth: true
                                text: Store.listLabel(row.modelData)
                                font.weight: row.modelData.active ? Font.DemiBold : Font.Normal
                                elide: Text.ElideRight
                            }
                            WText {
                                Layout.fillWidth: true
                                kind: "small"
                                text: row.modelData.host
                                elide: Text.ElideRight
                            }
                        }
                        WText {
                            Layout.rightMargin: 44
                            kind: "small"
                            text: {
                                switch (row.kind) {
                                case "online": return i18n("Online");
                                case "sleeping": return i18n("Sleeping");
                                case "offline": return i18n("Offline");
                                default: return "";
                                }
                            }
                        }
                    }
                    MouseArea {
                        id: rowArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        activeFocusOnTab: true
                        onClicked: {
                            if (!row.modelData.active) {
                                Store.setActive(row.modelData.host, null);
                            }
                        }
                        Keys.onReturnPressed: clicked(null)
                        Keys.onSpacePressed: clicked(null)
                    }
                    WButton {
                        objectName: "removePrinter-" + row.index
                        anchors.right: parent.right
                        anchors.rightMargin: 14
                        anchors.verticalCenter: parent.verticalCenter
                        kind: "quiet"
                        glyph: "trash"
                        implicitWidth: 36
                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.text: i18n("Remove this printer from the list")
                        onClicked: {
                            page.removeHost = row.modelData.host;
                            page.removeName = Store.listLabel(row.modelData);
                            removeDlg.open();
                        }
                    }
                }
            }
        }
    }
    WText {
        Layout.fillWidth: true
        kind: "muted"
        wrapMode: Text.WordWrap
        visible: Store.printers.length > 1
        text: i18n("Click a printer to make it the one Washi works with.")
    }
}
