import QtQuick
import QtQuick.Layouts

PageScroll {
    id: tab

    readonly property string source: Store.snap.ink_source || "none"
    readonly property bool lowAny: Store.inks.some(i => i.status === "low" || i.status === "empty")

    NoPrinter {
        visible: !Store.hasPrinter
        Layout.fillWidth: true
        Layout.preferredHeight: 320
    }

    ColumnLayout {
        visible: Store.hasPrinter
        Layout.fillWidth: true
        spacing: 12

        WText {
            kind: "heading"
            text: i18n("About ink")
        }
        WText {
            Layout.fillWidth: true
            kind: "dim"
            wrapMode: Text.WordWrap
            text: i18n("Each tank reports how much ink is left, in steps of 10 %. Replace a tank when it shows low or empty. Use the model number below when you buy a new one.")
        }

        InfoBar {
            Layout.fillWidth: true
            visible: tab.source === "ipp"
            kind: "info"
            text: i18n("The printer is asleep, so these levels were read over the network (IPP). Tank model numbers are remembered from the last time it was awake.")
        }
        InfoBar {
            Layout.fillWidth: true
            visible: tab.source === "cache"
            kind: "warning"
            text: i18n("The printer could not be reached. These are the levels from the last reading.")
        }
        InfoBar {
            Layout.fillWidth: true
            visible: tab.source === "none" && Store.snapLoaded
            kind: "warning"
            text: i18n("No ink information is available yet.")
        }

        WCard {
            Layout.fillWidth: true
            implicitHeight: inkCol.implicitHeight
            visible: Store.inks.length > 0

            Column {
                id: inkCol
                width: parent.width
                Repeater {
                    model: Store.inks
                    delegate: Item {
                        id: row
                        required property var modelData
                        required property int index
                        width: inkCol.width
                        height: 52

                        Rectangle {
                            visible: row.index > 0
                            width: parent.width - 32
                            x: 16
                            height: 1
                            color: Theme.line
                        }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 16
                            anchors.rightMargin: 16
                            spacing: 14

                            Rectangle {
                                Layout.preferredWidth: 44
                                Layout.preferredHeight: 24
                                radius: 4
                                color: Fmt.inkColor(row.modelData)
                                readonly property color c: color
                                WText {
                                    anchors.centerIn: parent
                                    text: row.modelData.color
                                    font.pixelSize: 12
                                    font.weight: Font.Bold
                                    color: (parent.c.r * 0.299 + parent.c.g * 0.587 + parent.c.b * 0.114) < 0.62 ? "#FFFFFF" : "#3A3200"
                                }
                            }
                            WText {
                                Layout.preferredWidth: 110
                                text: Fmt.inkName(row.modelData.color)
                                elide: Text.ElideRight
                            }
                            // Level bar
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 12
                                radius: 6
                                color: "#E6E6E6"
                                Rectangle {
                                    width: parent.width * Math.min(100, row.modelData.level || 0) / 100
                                    height: parent.height
                                    radius: 6
                                    color: Fmt.inkColor(row.modelData)
                                    border.width: Fmt.inkColor(row.modelData) === Theme.inkColors["Y"] ? 1 : 0
                                    border.color: "#C9B43A"
                                }
                            }
                            WText {
                                Layout.preferredWidth: 48
                                horizontalAlignment: Text.AlignRight
                                text: Fmt.percent(row.modelData.level)
                                font.weight: Font.DemiBold
                            }
                            Item {
                                Layout.preferredWidth: 22
                                Layout.preferredHeight: 22
                                Glyph {
                                    visible: row.modelData.status === "low" || row.modelData.status === "empty"
                                    anchors.centerIn: parent
                                    name: row.modelData.status === "empty" ? "error" : "warning"
                                    size: 22
                                    weight: 2
                                    color: row.modelData.status === "empty" ? Theme.danger : Theme.warn
                                }
                            }
                            WText {
                                Layout.preferredWidth: 150
                                kind: "small"
                                text: Fmt.tankModel(row.modelData.model)
                                elide: Text.ElideRight
                            }
                        }
                    }
                }
            }
        }

        RowLayout {
            spacing: 10
            WButton {
                text: i18n("Ink Model Number")
                enabled: Store.inks.length > 0
                onClicked: Nav.dialog("ink-models")
            }
            WButton {
                kind: "primary"
                glyph: "cart"
                text: i18n("Order Ink")
                onClicked: Nav.dialog("order-ink")
            }
        }
    }
}
