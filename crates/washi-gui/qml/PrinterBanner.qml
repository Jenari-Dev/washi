import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// The strip across the top of Home and Printer Management: printer name and state, picture,
// ink markers. `management` swaps the carousel arrows for "Change" / "Update Information".
Rectangle {
    id: banner

    property bool management: false
    // Room reserved along the bottom edge (Printer Management puts its tabs there).
    property int footer: 0
    // Drop the ink markers when the window is narrow (the Consumables tab shows them anyway).
    readonly property bool showInk: Store.hasPrinter && Store.inks.length > 0 && width >= (management ? 860 : 790)

    implicitHeight: 144 + footer
    gradient: Gradient {
        orientation: Gradient.Horizontal
        GradientStop { position: 0.0; color: Theme.bannerLeft }
        GradientStop { position: 1.0; color: Theme.bannerRight }
    }

    RowLayout {
        anchors.fill: parent
        anchors.bottomMargin: banner.footer
        anchors.leftMargin: banner.management ? 28 : 14
        anchors.rightMargin: banner.management ? 28 : 14
        spacing: 18

        RoundButton {
            visible: !banner.management
            Layout.alignment: Qt.AlignVCenter
            glyph: "chevleft"
            enabled: Store.printers.length > 1
            onClicked: Store.cycle(-1)
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: i18n("Previous printer")
        }

        // Name and state
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 120
            Layout.preferredWidth: 280
            Layout.alignment: Qt.AlignVCenter
            spacing: 6

            WText {
                Layout.fillWidth: true
                text: Store.hasPrinter ? Store.displayName : i18n("No printer")
                font.pixelSize: 22
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }
            RowLayout {
                visible: Store.hasPrinter
                spacing: 6
                StatusDot {
                    kind: Store.statusKind
                    size: 18
                }
                WText {
                    kind: "dim"
                    text: Store.statusText
                    Layout.fillWidth: !bannerWake.visible
                    elide: Text.ElideRight
                }
                WakeButton {
                    id: bannerWake
                    objectName: "bannerWake"
                    compact: true
                }
                Item {
                    Layout.fillWidth: bannerWake.visible
                }
                HoverHandler { id: statusHover }
                QQC2.ToolTip.visible: statusHover.hovered && Store.statusTip !== ""
                QQC2.ToolTip.delay: 250
                QQC2.ToolTip.text: Store.statusTip
            }
            WText {
                visible: !Store.hasPrinter
                kind: "dim"
                text: i18n("Add a printer to get started.")
            }
        }

        // Picture and "Add Printer"
        ColumnLayout {
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: 176
            spacing: 6

            PrinterImage {
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: 170
                Layout.preferredHeight: 86
                model: Store.modelName
                opacity: Store.statusKind === "offline" ? 0.55 : 1
                MouseArea {
                    anchors.fill: parent
                    enabled: Store.hasPrinter
                    cursorShape: Qt.PointingHandCursor
                    onClicked: Nav.go("device-info")
                }
            }
            Pill {
                Layout.alignment: Qt.AlignHCenter
                glyph: "plus"
                text: i18n("Add Printer")
                onClicked: Nav.dialog("discover")
            }
        }

        // Ink markers and "Order Ink"
        ColumnLayout {
            visible: banner.showInk
            Layout.alignment: Qt.AlignVCenter
            spacing: 10
            Row {
                Layout.alignment: Qt.AlignHCenter
                spacing: 8
                Repeater {
                    model: Store.inks
                    InkChip {
                        required property var modelData
                        ink: modelData
                    }
                }
            }
            Pill {
                Layout.alignment: Qt.AlignHCenter
                glyph: "cart"
                text: i18n("Order Ink")
                onClicked: Nav.dialog("order-ink")
            }
        }

        Item {
            visible: !banner.showInk
            Layout.fillWidth: true
        }

        // Management variant: stacked buttons
        ColumnLayout {
            visible: banner.management
            Layout.alignment: Qt.AlignVCenter
            spacing: 8
            WButton {
                Layout.fillWidth: true
                Layout.minimumWidth: implicitWidth
                text: i18n("Change")
                onClicked: Nav.go("printer-list")
            }
            WButton {
                Layout.fillWidth: true
                Layout.minimumWidth: implicitWidth
                glyph: "refresh"
                text: Store.refreshing ? i18n("Updating…") : i18n("Update Information")
                enabled: Store.hasPrinter && !Store.refreshing
                onClicked: Store.refresh()
            }
        }

        RoundButton {
            visible: !banner.management
            Layout.alignment: Qt.AlignVCenter
            glyph: "chevright"
            enabled: Store.printers.length > 1
            onClicked: Store.cycle(1)
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: i18n("Next printer")
        }
    }
}
