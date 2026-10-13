import QtQuick
import QtQuick.Layouts

// Printer Management: banner plus the four-tab pivot (Consumables, Status, Device Information,
// Utilities). The current tab is the current page id, so the left navigation can pick one too.
ColumnLayout {
    id: page

    // Page id of the tab shown; tests set it directly.
    property string tab: Nav.page
    readonly property var tabs: [
        { id: "consumables", label: i18n("Consumables"), glyph: "drop", tint: "#12A4DC" },
        { id: "status", label: i18n("Status"), glyph: "bell", tint: "#E8A01C" },
        { id: "device-info", label: i18n("Device Information"), glyph: "info", tint: "#5B6FD8" },
        { id: "utilities", label: i18n("Utilities"), glyph: "wrench", tint: "#2E9E4F" }
    ]
    readonly property int tabIndex: Math.max(0, tabs.findIndex(t => t.id === page.tab))

    spacing: 0

    // The banner with the tabs in its bottom edge.
    Item {
        Layout.fillWidth: true
        implicitHeight: banner.implicitHeight

        PrinterBanner {
            id: banner
            anchors.fill: parent
            management: true
            footer: tabStrip.implicitHeight
        }
        ManagementTabs {
            id: tabStrip
            anchors.left: parent.left
            anchors.leftMargin: 20
            anchors.bottom: parent.bottom
            height: implicitHeight
            tabs: page.tabs
            current: page.tabIndex
            onSelected: id => Nav.replace(id)
        }
    }

    Loader {
        id: body
        Layout.fillWidth: true
        Layout.fillHeight: true
        sourceComponent: [cConsumables, cStatus, cDevice, cUtilities][page.tabIndex]
    }

    Component { id: cConsumables; ConsumablesTab {} }
    Component { id: cStatus; StatusTab {} }
    Component { id: cDevice; DeviceInfoTab {} }
    Component { id: cUtilities; UtilitiesTab {} }
}
