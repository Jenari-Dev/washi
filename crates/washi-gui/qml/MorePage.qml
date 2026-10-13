import QtQuick
import QtQuick.Layouts

PageScroll {
    id: page

    readonly property var entries: [
        { title: i18n("Manual"), text: i18n("The printer's online manual (opens in your web browser)."), glyph: "book", link: "help" },
        { title: i18n("Remote UI"), text: i18n("The printer's own web page for settings and maintenance."), glyph: "globe", link: "printer" },
        { title: i18n("App Settings"), text: i18n("Automatic printer search, notifications, version."), glyph: "gear", page: "app-settings" },
        { title: i18n("License Information"), text: i18n("Washi is free software under the GPL."), glyph: "document", dialog: "license" }
    ]

    WText {
        kind: "title"
        text: i18n("More")
    }
    WCard {
        Layout.fillWidth: true
        implicitHeight: col.implicitHeight
        Column {
            id: col
            width: parent.width
            Repeater {
                model: page.entries
                delegate: Item {
                    id: row
                    required property var modelData
                    required property int index
                    width: col.width
                    height: 66
                    readonly property bool off: modelData.link === "printer" && !Store.hasPrinter
                    opacity: off ? 0.5 : 1
                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: 3
                        radius: 4
                        color: area.containsMouse && !row.off ? "#F3F6FB" : "transparent"
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
                        spacing: 16
                        Glyph {
                            name: row.modelData.glyph
                            size: 28
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            WText {
                                text: row.modelData.title
                                font.weight: Font.DemiBold
                            }
                            WText {
                                Layout.fillWidth: true
                                kind: "dim"
                                elide: Text.ElideRight
                                text: row.modelData.text
                            }
                        }
                        Glyph {
                            name: row.modelData.link ? "external" : "chevright"
                            size: 18
                            color: Theme.textMuted
                        }
                    }
                    MouseArea {
                        id: area
                        anchors.fill: parent
                        hoverEnabled: true
                        enabled: !row.off
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            const m = row.modelData;
                            if (m.link) {
                                Nav.openLink(m.link);
                            } else if (m.page) {
                                Nav.go(m.page);
                            } else if (m.dialog) {
                                Nav.dialog(m.dialog);
                            }
                        }
                    }
                }
            }
        }
    }
}
