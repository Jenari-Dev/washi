import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// The grid of function tiles (4 per row). On Home the user can remove and re-add tiles with
// "Customize"; on the Functions page `all` shows every tile.
ColumnLayout {
    id: grid

    property bool all: false
    property bool editing: false
    readonly property int tileW: 132
    readonly property int tileH: 98

    readonly property var tiles: [
        { id: "document-print", label: i18n("Document Print"), glyph: "document", color: "#2F6FD6", page: "document-print" },
        { id: "photo-print", label: i18n("Photo Print"), glyph: "photo", color: "#E0702B", page: "photo-print" },
        { id: "scan", label: i18n("Scan"), glyph: "scan", color: "#1E9E8A", page: "scan" },
        { id: "printer-status", label: i18nc("tile on the Home page", "Printer Status"), glyph: "printer", color: "#7B5CC9", page: "status" },
        { id: "utilities", label: i18n("Utilities"), glyph: "wrench", color: "#C98A00", page: "utilities" },
        { id: "disc-label", label: i18n("Disc Label"), glyph: "disc", color: "#C9437E", page: "disc-label" },
        { id: "manual", label: i18n("Manual"), glyph: "book", color: "#4C8F3A", link: "help" },
        { id: "remote-ui", label: i18n("Remote UI"), glyph: "globe", color: "#3A7CA5", link: "printer" },
        { id: "recommended-apps", label: i18n("Recommended Apps"), glyph: "star", color: "#B8860B", page: "recommended-apps", extra: true }
    ]
    readonly property var shown: tiles.filter(t => (all || !t.extra) && (all || editing || !Prefs.isHidden(t.id)))

    spacing: 14

    function open(t) {
        if (t.page) {
            Nav.go(t.page);
        } else if (t.link) {
            Nav.openLink(t.link);
        }
    }

    GridLayout {
        columns: 4
        rowSpacing: 12
        columnSpacing: 12

        Repeater {
            model: grid.shown
            delegate: Rectangle {
                id: tile
                required property var modelData
                readonly property bool hidden: Prefs.isHidden(modelData.id)
                readonly property bool unavailable: !!modelData.link && !Store.hasPrinter

                Layout.preferredWidth: grid.tileW
                Layout.preferredHeight: grid.tileH
                radius: Theme.radius
                color: area.pressed ? "#EEF3FB" : (area.containsMouse ? "#F8FAFE" : Theme.card)
                border.width: area.activeFocus ? 2 : 1
                border.color: area.activeFocus || area.containsMouse ? Theme.accent : Theme.cardBorder
                opacity: tile.unavailable || (grid.editing && tile.hidden) ? 0.5 : 1

                Column {
                    anchors.centerIn: parent
                    spacing: 8
                    Glyph {
                        anchors.horizontalCenter: parent.horizontalCenter
                        name: tile.modelData.glyph
                        size: 36
                        weight: 1.6
                        color: tile.modelData.color
                    }
                    WText {
                        width: grid.tileW - 16
                        horizontalAlignment: Text.AlignHCenter
                        text: tile.modelData.label
                        wrapMode: Text.WordWrap
                        maximumLineCount: 2
                        elide: Text.ElideRight
                    }
                }
                Glyph {
                    visible: !!tile.modelData.link && !grid.editing
                    x: parent.width - 22
                    y: 6
                    name: "external"
                    size: 14
                    color: Theme.textMuted
                }
                // Customize badge: remove / add back
                Rectangle {
                    visible: grid.editing && !grid.all
                    x: parent.width - 24
                    y: 6
                    width: 18
                    height: 18
                    radius: 9
                    color: tile.hidden ? Theme.accent : "#6B6B6B"
                    Glyph {
                        anchors.centerIn: parent
                        name: tile.hidden ? "plus" : "close"
                        size: 12
                        weight: 2.4
                        color: "#FFFFFF"
                    }
                }
                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    activeFocusOnTab: true
                    enabled: !tile.unavailable
                    onClicked: {
                        if (grid.editing && !grid.all) {
                            Prefs.setHidden(tile.modelData.id, !tile.hidden);
                        } else {
                            grid.open(tile.modelData);
                        }
                    }
                    Keys.onReturnPressed: clicked(null)
                    Keys.onSpacePressed: clicked(null)
                }
            }
        }
    }

    WButton {
        visible: !grid.all
        text: grid.editing ? i18n("Done") : i18n("Customize")
        kind: grid.editing ? "primary" : "secondary"
        glyph: grid.editing ? "check" : ""
        onClicked: grid.editing = !grid.editing
    }
    WText {
        visible: grid.editing && !grid.all
        kind: "small"
        text: i18n("Click a tile to remove it from Home, or to bring it back.")
    }
}
