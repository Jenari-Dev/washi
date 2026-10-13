import QtQuick
import QtQuick.Layouts

// The white settings panel on the right of the print and scan screens: a scrolling column of
// controls with the printer's name on top, and an optional footer row for the main buttons.
Rectangle {
    id: panel

    property int panelWidth: 300
    property bool showPrinter: true
    default property alias content: scroll.content
    property alias footer: footerRow.data

    Layout.preferredWidth: panelWidth
    Layout.fillHeight: true
    color: Theme.card

    Rectangle {
        width: 1
        height: parent.height
        color: Theme.cardBorder
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        PageScroll {
            id: scroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            margin: 18
            spacing: 12

            RowLayout {
                visible: panel.showPrinter
                spacing: 10
                Glyph {
                    name: "printer"
                    size: 26
                }
                WText {
                    Layout.fillWidth: true
                    font.weight: Font.DemiBold
                    text: Store.hasPrinter ? Store.displayName : i18n("No printer")
                    elide: Text.ElideRight
                }
            }
        }

        // The footer has its own opaque background: icons of the scrolled content that run out
        // below the viewport are not clipped by every renderer, and must not show through.
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: footerRow.implicitHeight + 28
            visible: footerRow.children.length > 0
            z: 10
            color: Theme.card
            Rectangle {
                width: parent.width
                height: 1
                color: Theme.line
            }
            RowLayout {
                id: footerRow
                anchors.fill: parent
                anchors.margins: 14
                spacing: 10
            }
        }
    }
}
