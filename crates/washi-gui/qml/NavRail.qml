import QtQuick
import QtQuick.Controls as QQC2

// The left navigation pane: a compact icon rail that expands to show labels.
Rectangle {
    id: rail

    property bool expanded: false
    property bool managementOpen: true
    property bool moreOpen: false
    signal toggleExpanded()

    // 10 top items + 3 footer items + margins always fit (the window is at least 600 high);
    // the sub-items only show while there is room for them below.
    readonly property int baseHeight: 549
    readonly property bool showManagement: expanded && managementOpen && height >= baseHeight + 160 + 10
    readonly property bool showMore: expanded && moreOpen && height >= baseHeight + (showManagement ? 160 : 0) + 80 + 10

    width: expanded ? Theme.railExpanded : Theme.railWidth
    color: Theme.navBg
    clip: true

    Behavior on width {
        NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
    }

    Rectangle {
        anchors.right: parent.right
        width: 1
        height: parent.height
        color: "#D6D6D6"
    }

    component RailItem: Item {
        id: it

        property string glyph: ""
        property string label: ""
        property bool selected: false
        property bool child: false
        property string chevron: ""          // "", "chevdown", "chevup"
        property bool dim: false
        signal clicked()

        width: rail.width - 1
        height: 40
        opacity: enabled ? 1 : 0.35

        Rectangle {
            x: 4
            y: 2
            width: parent.width - 7
            height: parent.height - 4
            radius: 6
            color: it.selected ? Theme.accentTint : (area.pressed ? "#D4D4D4" : (area.containsMouse && it.enabled ? "#DEDEDE" : "transparent"))
            border.width: area.activeFocus ? 2 : 0
            border.color: Theme.accent
        }
        Glyph {
            x: it.child ? 24 : 14
            anchors.verticalCenter: parent.verticalCenter
            name: it.glyph
            size: it.child ? 18 : 20
            color: it.selected ? Theme.accent : Theme.navy
        }
        WText {
            visible: rail.expanded
            x: it.child ? 52 : 48
            width: parent.width - x - (it.chevron !== "" ? 30 : 8)
            anchors.verticalCenter: parent.verticalCenter
            text: it.label
            elide: Text.ElideRight
            font.pixelSize: 13
            font.weight: Font.Normal
        }
        Glyph {
            visible: rail.expanded && it.chevron !== ""
            x: parent.width - 30
            anchors.verticalCenter: parent.verticalCenter
            name: it.chevron
            size: 16
            color: Theme.textDim
        }
        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            activeFocusOnTab: true
            onClicked: it.clicked()
            Keys.onReturnPressed: it.clicked()
            Keys.onSpacePressed: it.clicked()
        }
        QQC2.ToolTip.visible: !rail.expanded && area.containsMouse && it.enabled && it.label !== ""
        QQC2.ToolTip.delay: 450
        QQC2.ToolTip.text: it.label
    }

    Column {
        id: top
        y: 6
        spacing: 0

        RailItem {
            objectName: "navBack"
            glyph: "back"
            label: i18n("Back")
            enabled: Nav.canGoBack
            onClicked: Nav.back()
        }
        RailItem {
            objectName: "navMenu"
            glyph: "menu"
            label: i18n("Menu")
            onClicked: rail.toggleExpanded()
        }
        Item {
            width: 1
            height: 8
        }
        RailItem {
            objectName: "nav-home"
            glyph: "home"
            label: i18n("Home")
            selected: Nav.page === "home"
            onClicked: Nav.go("home")
        }
        RailItem {
            objectName: "nav-functions"
            glyph: "functions"
            label: i18n("Functions")
            selected: Nav.page === "functions"
            onClicked: Nav.go("functions")
        }
        RailItem {
            objectName: "nav-photo-print"
            glyph: "photo"
            label: i18n("Photo Print")
            selected: Nav.page === "photo-print"
            onClicked: Nav.go("photo-print")
        }
        RailItem {
            objectName: "nav-document-print"
            glyph: "document"
            label: i18n("Document Print")
            selected: Nav.page === "document-print"
            onClicked: Nav.go("document-print")
        }
        RailItem {
            objectName: "nav-scan"
            glyph: "scan"
            label: i18n("Scan")
            selected: Nav.page === "scan"
            onClicked: Nav.go("scan")
        }
        RailItem {
            objectName: "nav-management"
            glyph: "printer"
            label: i18n("Printer Management")
            selected: Nav.inManagement && !rail.showManagement
            chevron: rail.managementOpen ? "chevup" : "chevdown"
            onClicked: {
                if (!Nav.inManagement) {
                    Nav.go("consumables");
                    rail.managementOpen = true;
                } else if (rail.expanded) {
                    rail.managementOpen = !rail.managementOpen;
                }
            }
        }
        Repeater {
            model: rail.showManagement ? [
                { id: "consumables", label: i18n("Consumables"), glyph: "drop" },
                { id: "status", label: i18n("Status"), glyph: "bell" },
                { id: "device-info", label: i18n("Device Information"), glyph: "info" },
                { id: "utilities", label: i18n("Utilities"), glyph: "wrench" }
            ] : []
            delegate: RailItem {
                required property var modelData
                objectName: "nav-" + modelData.id
                child: true
                glyph: modelData.glyph
                label: modelData.label
                selected: Nav.page === modelData.id
                onClicked: Nav.go(modelData.id)
            }
        }
        RailItem {
            objectName: "nav-recommended-apps"
            glyph: "star"
            label: i18n("Recommended Apps")
            selected: Nav.page === "recommended-apps"
            onClicked: Nav.go("recommended-apps")
        }
        RailItem {
            objectName: "nav-more"
            glyph: "more"
            label: i18n("More")
            selected: Nav.page === "more" && !rail.showMore
            chevron: rail.moreOpen ? "chevup" : "chevdown"
            onClicked: {
                if (Nav.page !== "more") {
                    Nav.go("more");
                    rail.moreOpen = true;
                } else if (rail.expanded) {
                    rail.moreOpen = !rail.moreOpen;
                }
            }
        }
        RailItem {
            visible: rail.showMore
            height: visible ? 40 : 0
            child: true
            glyph: "book"
            label: i18n("Manual")
            onClicked: Nav.openLink("help")
        }
        RailItem {
            visible: rail.showMore
            height: visible ? 40 : 0
            child: true
            glyph: "globe"
            label: i18n("Remote UI")
            enabled: Store.hasPrinter
            onClicked: Nav.openLink("printer")
        }
    }

    Column {
        id: lower
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 6
        spacing: 0

        Rectangle {
            x: 8
            width: rail.width - 17
            height: 1
            color: "#D6D6D6"
        }
        RailItem {
            objectName: "nav-add-printer"
            glyph: "plus"
            label: i18n("Add Printer")
            onClicked: Nav.dialog("discover")
        }
        RailItem {
            objectName: "nav-printer-list"
            glyph: "printerlist"
            label: i18n("Printer List")
            selected: Nav.page === "printer-list"
            onClicked: Nav.go("printer-list")
        }
        RailItem {
            objectName: "nav-app-settings"
            glyph: "gear"
            label: i18n("App Settings")
            selected: Nav.page === "app-settings"
            onClicked: Nav.go("app-settings")
        }
    }
}
