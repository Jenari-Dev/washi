import QtQuick
import QtQuick.Layouts

PageScroll {
    id: tab

    readonly property var counters: Store.snap.counters || ({})
    readonly property var trays: Store.snap.trays || []
    readonly property bool fromCache: (Store.snap.from_cache || []).length > 0
    // wifi / lan from the printer's network interface; "" while unknown.
    readonly property string connection: Store.connection === "unknown" ? "" : Store.connection

    readonly property string connectionText: connection === "wifi" ? i18n("Wi-Fi") : (connection === "lan" ? i18n("Wired LAN") : i18n("Network (over IP)"))
    readonly property string hostText: {
        if (!Store.hasPrinter) {
            return "";
        }
        return Store.address !== "" && Store.address !== Store.activeHost ? Store.address + "  (" + Store.activeHost + ")" : Store.activeHost;
    }

    function perTray(map) {
        const keys = Object.keys(map || {}).sort();
        return keys.map(k => Fmt.trayName(k) + " " + Fmt.number(map[k])).join("  ·  ");
    }

    function perInk(map) {
        const keys = Fmt.inkOrder.filter(k => k in (map || {})).concat(Object.keys(map || {}).filter(k => Fmt.inkOrder.indexOf(k) < 0));
        return keys.map(k => k + " " + Fmt.number(map[k])).join("  ·  ");
    }

    readonly property var rows: [
        { label: i18n("Printer name"), value: Store.activePrinter ? Store.printerLabel(Store.activePrinter) : "" },
        { label: i18n("Model name"), value: Store.modelName },
        { label: i18n("Firmware version"), value: Store.firmware, detail: Store.firmwareUpdate === "available" ? i18n("An update is available.") : (Store.firmwareUpdate === "unavailable" ? i18n("Up to date.") : ""), accent: Store.firmwareUpdate === "available" },
        { label: i18n("IP address"), value: tab.hostText },
        { label: i18n("Connection"), value: tab.connectionText },
        { label: i18n("Total pages fed"), value: counters.total_paper_num ? Fmt.number(Fmt.sum(counters.total_paper_num)) : "", detail: counters.total_paper_num ? perTray(counters.total_paper_num) : "" },
        { label: i18n("Ink tank replacements"), value: counters.ink_replacement_count ? Fmt.number(Fmt.sum(counters.ink_replacement_count)) : "", detail: counters.ink_replacement_count ? perInk(counters.ink_replacement_count) : "" },
        { label: i18n("Print head replacements"), value: counters.head_replacement_count !== undefined && counters.head_replacement_count !== null ? Fmt.number(counters.head_replacement_count) : "" }
    ]

    NoPrinter {
        visible: !Store.hasPrinter
        Layout.fillWidth: true
        Layout.preferredHeight: 320
    }

    WCard {
        visible: Store.hasPrinter
        Layout.fillWidth: true
        implicitHeight: 150

        ConnectionDiagram {
            anchors.centerIn: parent
            connection: tab.connection
            printerName: Store.hasPrinter ? Store.displayName : ""
            online: Store.statusKind !== "offline"
        }
    }

    WCard {
        visible: Store.hasPrinter
        Layout.fillWidth: true
        implicitHeight: table.implicitHeight

        Column {
            id: table
            width: parent.width
            Repeater {
                model: tab.rows
                delegate: Item {
                    id: tr
                    required property var modelData
                    required property int index
                    width: table.width
                    height: Math.max(46, rowText.implicitHeight + 22)

                    Rectangle {
                        visible: tr.index > 0
                        x: 16
                        width: parent.width - 32
                        height: 1
                        color: Theme.line
                    }
                    WText {
                        x: 20
                        anchors.verticalCenter: parent.verticalCenter
                        width: 220
                        kind: "dim"
                        text: tr.modelData.label
                    }
                    Column {
                        id: rowText
                        x: 250
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width - 270
                        spacing: 2
                        WText {
                            width: parent.width
                            text: tr.modelData.value !== "" ? tr.modelData.value : "–"
                            font.weight: Font.DemiBold
                            wrapMode: Text.WrapAnywhere
                        }
                        WText {
                            visible: !!tr.modelData.detail
                            width: parent.width
                            kind: "small"
                            text: tr.modelData.detail || ""
                            color: tr.modelData.accent ? Theme.accent : Theme.textDim
                            wrapMode: Text.WordWrap
                        }
                    }
                }
            }
        }
    }

    WCard {
        id: fwCard
        objectName: "firmwareCard"
        visible: Store.hasPrinter
        Layout.fillWidth: true
        implicitHeight: fwCol.implicitHeight + 36

        readonly property string update: Store.firmwareUpdate
        readonly property string hint: {
            if (Store.readOnly) {
                return i18n("Read-only mode");
            }
            if (Store.busy) {
                return i18n("The printer is working on something else.");
            }
            if (Store.power !== "on") {
                return i18n("The printer has to be awake and reachable to update.");
            }
            return "";
        }

        ColumnLayout {
            id: fwCol
            anchors.fill: parent
            anchors.margins: 18
            spacing: 8
            RowLayout {
                spacing: 12
                Glyph {
                    name: "firmware"
                    size: 30
                    weight: 1.5
                    color: fwCard.update === "available" ? Theme.accent : Theme.navy
                }
                WText {
                    Layout.fillWidth: true
                    text: i18n("Firmware")
                    font.weight: Font.DemiBold
                    font.pixelSize: 16
                }
            }
            WText {
                objectName: "firmwareStatus"
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                color: fwCard.update === "available" ? Theme.accent : Theme.text
                font.weight: fwCard.update === "available" ? Font.DemiBold : Font.Normal
                text: {
                    switch (fwCard.update) {
                    case "available": return i18n("A firmware update is available (installed: %1).", Store.firmware);
                    case "unavailable": return i18n("The firmware is up to date (%1). The printer reports no update.", Store.firmware);
                    default: return i18n("The printer does not say whether a firmware update is available (installed: %1).", Store.firmware);
                    }
                }
            }
            RowLayout {
                Layout.alignment: Qt.AlignHCenter
                Layout.topMargin: 4
                spacing: 10
                WButton {
                    objectName: "checkUpdatesButton"
                    glyph: "refresh"
                    text: Store.refreshing ? i18n("Checking…") : i18n("Check for Updates")
                    enabled: Store.hasPrinter && !Store.refreshing
                    onClicked: Store.refresh()
                }
                WButton {
                    objectName: "updateFirmwareButton"
                    kind: "primary"
                    text: i18n("Update Firmware…")
                    enabled: Store.canUpdateFirmware
                    onClicked: Nav.dialog("firmware")
                }
            }
            WText {
                Layout.fillWidth: true
                visible: fwCard.hint !== "" && fwCard.update === "available"
                horizontalAlignment: Text.AlignHCenter
                kind: "muted"
                wrapMode: Text.WordWrap
                text: fwCard.hint
            }
        }
    }

    WText {
        visible: Store.hasPrinter && tab.fromCache
        Layout.fillWidth: true
        kind: "muted"
        wrapMode: Text.WordWrap
        text: i18n("Some values are from the last time the printer was awake.")
    }

    RowLayout {
        visible: Store.hasPrinter
        spacing: 10
        WButton {
            kind: "primary"
            glyph: "external"
            text: i18n("Open Remote UI")
            onClicked: Nav.openLink("printer")
        }
        WButton {
            glyph: "external"
            text: i18n("Manual")
            onClicked: Nav.openLink("help")
        }
    }
}
