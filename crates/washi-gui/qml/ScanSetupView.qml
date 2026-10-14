import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Scan, step 1: how to put the original on the glass (left) and what to scan (right).
RowLayout {
    id: view

    required property var flow
    spacing: 0

    function refreshCaps() {
        Store.loadScanCaps(false);
    }
    Component.onCompleted: refreshCaps()
    Connections {
        target: Store
        function onActiveHostChanged() {
            view.refreshCaps();
        }
    }
    // The scanner capabilities are optional (there are defaults), but try again if they failed.
    Timer {
        interval: 5000
        repeat: true
        running: Store.connected && Store.hasPrinter && !Store.scanCaps && !Store.scanCapsLoading
        onTriggered: Store.loadScanCaps(true)
    }

    // A fresh scanner offers other resolutions than our defaults: keep the choice valid.
    readonly property var resolutions: flow.resolutions
    onResolutionsChanged: {
        if (resolutions.indexOf(flow.resolution) < 0) {
            flow.resolution = ScanModel.defaultResolution(flow.kind, resolutions);
        }
    }

    PageScroll {
        Layout.fillWidth: true
        Layout.fillHeight: true

        WText {
            kind: "title"
            text: view.flow.mode === "normal" ? i18n("Scan") : ScanModel.scanModes(Img.tools()).find(m => m.value === view.flow.mode).text
        }
        WText {
            Layout.fillWidth: true
            kind: "dim"
            wrapMode: Text.WordWrap
            text: view.flow.mode === "multi-photo" ? i18n("Put several photos on the glass with space between them. Each one becomes its own page.")
                : (view.flow.mode === "stitch" ? i18n("Scan the first part, then the second with “%1”. Washi joins them where they overlap.", i18nc("short button label under the page thumbnails", "+ Next scan"))
                : i18n("Put the original on the glass, then choose the settings on the right."))
        }
        Repeater {
            model: [
                { n: 1, text: i18n("Open the cover and lay the original face down on the glass.") },
                { n: 2, text: i18n("Line up its corner with the arrow mark on the glass edge.") },
                { n: 3, text: i18n("Close the cover gently so the original does not move.") }
            ]
            delegate: WCard {
                required property var modelData
                Layout.fillWidth: true
                implicitHeight: 112
                RowLayout {
                    anchors.fill: parent
                    anchors.margins: 16
                    spacing: 16
                    Rectangle {
                        Layout.preferredWidth: 32
                        Layout.preferredHeight: 32
                        radius: 16
                        color: Theme.accent
                        WText {
                            anchors.centerIn: parent
                            text: modelData.n
                            color: "#FFFFFF"
                            font.weight: Font.Bold
                        }
                    }
                    WText {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: modelData.text
                    }
                    ScanDiagram {
                        Layout.preferredWidth: 150
                        Layout.preferredHeight: 92
                        step: modelData.n
                        photo: view.flow.kind === "photo"
                    }
                }
            }
        }
        InfoBar {
            objectName: "scanError"
            Layout.fillWidth: true
            visible: view.flow.error !== ""
            kind: "error"
            text: view.flow.error
        }
    }

    SidePanel {
        footer: [
            QQC2.Button {
                id: moreButton
                objectName: "moreModes"
                visible: true
                flat: true
                implicitHeight: 34
                implicitWidth: modesLabel.implicitWidth + 6 + 16 + moreButton.leftPadding + moreButton.rightPadding + 4
                // Gives way to the Scan button when the label is long.
                Layout.fillWidth: true
                Layout.minimumWidth: 64
                Layout.maximumWidth: implicitWidth
                hoverEnabled: true
                background: Rectangle {
                    radius: 4
                    color: moreButton.down ? "#DCDCDC" : (moreButton.hovered ? "#E6E6E6" : "transparent")
                }
                contentItem: Row {
                    id: moreRow
                    spacing: 6
                    anchors.centerIn: parent
                    Text {
                        id: modesLabel
                        anchors.verticalCenter: parent.verticalCenter
                        width: Math.min(implicitWidth, moreButton.availableWidth - 6 - 16)
                        elide: Text.ElideRight
                        text: i18n("More scan modes")
                        font.pixelSize: Theme.fontSize
                        color: Theme.accent
                    }
                    Glyph {
                        anchors.verticalCenter: parent.verticalCenter
                        name: "chevdown"
                        size: 16
                        color: Theme.accent
                    }
                }
                onClicked: modesMenu.open()

                QQC2.Menu {
                    id: modesMenu
                    y: -height
                    Repeater {
                        model: ScanModel.scanModes(Img.tools())
                        delegate: QQC2.MenuItem {
                            required property var modelData
                            objectName: "mode-" + modelData.value
                            text: modelData.text
                            checkable: true
                            checked: view.flow.mode === modelData.value
                            onTriggered: {
                                view.flow.mode = modelData.value;
                                if (modelData.value === "ocr") {
                                    view.flow.kind = "document";
                                    view.flow.format = "pdf-ocr";
                                }
                            }
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: modelData.tip
                            QQC2.ToolTip.delay: 400
                        }
                    }
                }
            },
            Item {
                Layout.fillWidth: true
            },
            WButton {
                objectName: "scanButton"
                kind: "primary"
                implicitWidth: 96
                enabled: view.flow.canScan
                text: view.flow.scanning ? i18n("Scanning…") : i18n("Scan")
                onClicked: view.flow.startScan()
            }
        ]

        FormRow {
            label: i18n("Original Type")
            RowLayout {
                spacing: 16
                WRadio {
                    objectName: "typeDocument"
                    text: i18n("Document")
                    checked: view.flow.kind === "document"
                    onClicked: {
                        view.flow.kind = "document";
                        view.flow.resolution = ScanModel.defaultResolution("document", view.resolutions);
                    }
                }
                WRadio {
                    objectName: "typePhoto"
                    text: i18n("Photo")
                    checked: view.flow.kind === "photo"
                    onClicked: {
                        view.flow.kind = "photo";
                        view.flow.resolution = ScanModel.defaultResolution("photo", view.resolutions);
                    }
                }
                Rectangle {
                    id: tipDot
                    Layout.preferredWidth: 20
                    Layout.preferredHeight: 20
                    radius: 10
                    color: "transparent"
                    Glyph {
                        anchors.centerIn: parent
                        name: "info"
                        size: 20
                        color: Theme.textMuted
                    }
                    HoverHandler {
                        id: tipHover
                    }
                    QQC2.ToolTip.visible: tipHover.hovered
                    QQC2.ToolTip.delay: 200
                    QQC2.ToolTip.text: i18n("Document asks the scanner to favour text; Photo favours colour and tone. The image is not changed otherwise.")
                }
            }
        }
        FormRow {
            objectName: "stitchDirectionRow"
            visible: view.flow.mode === "stitch"
            label: i18n("Join the scans")
            Repeater {
                model: ScanModel.stitchDirections()
                delegate: WRadio {
                    required property var modelData
                    objectName: "stitch-" + modelData.value
                    text: modelData.text
                    checked: view.flow.stitchDirection === modelData.value
                    onClicked: view.flow.stitchDirection = modelData.value
                }
            }
        }
        FormRow {
            visible: view.flow.mode !== "auto" && view.flow.mode !== "multi-photo"
            label: i18n("Original Size")
            WCombo {
                objectName: "sizeCombo"
                Layout.fillWidth: true
                textRole: "text"
                valueRole: "value"
                model: ScanModel.sizes()
                selectedValue: view.flow.sizeKey
                onActivated: view.flow.sizeKey = currentValue
            }
        }
        FormRow {
            visible: view.flow.mode === "auto" || view.flow.mode === "multi-photo"
            label: i18n("Original Size")
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("The whole glass is scanned and Washi finds what is on it.")
            }
        }
        FormRow {
            label: i18n("Resolution")
            WCombo {
                objectName: "resolutionCombo"
                Layout.fillWidth: true
                textRole: "text"
                valueRole: "value"
                model: view.resolutions.map(r => ({ text: i18n("%1 dpi", r), value: r }))
                selectedValue: view.flow.resolution
                onActivated: view.flow.resolution = currentValue
            }
            WText {
                Layout.fillWidth: true
                kind: "muted"
                wrapMode: Text.WordWrap
                text: ScanModel.resolutionHint(view.flow.resolution)
            }
        }
        FormRow {
            label: i18n("Color Mode")
            WCombo {
                objectName: "colorCombo"
                Layout.fillWidth: true
                textRole: "text"
                valueRole: "value"
                model: ScanModel.colorModes()
                selectedValue: view.flow.colorMode
                onActivated: view.flow.colorMode = currentValue
            }
        }
        InfoBar {
            Layout.fillWidth: true
            visible: !view.flow.canScan && !view.flow.scanning
            kind: Store.readOnly || !Store.connected ? "warning" : "info"
            text: !Store.connected ? i18n("The Washi service is not running.")
                : (!Store.hasPrinter ? i18n("Add a printer first.")
                : (Store.readOnly ? i18n("Read-only mode: the service will not scan.")
                : (Store.power === "offline" ? i18n("The printer cannot be reached.")
                : (Store.busy ? i18n("The printer is busy: %1", Store.statusTip) : i18n("Reading the printer…")))))
        }
        RowLayout {
            visible: view.flow.scanning
            spacing: 10
            Spinner {
                size: 22
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Scanning. This can take a minute at a high resolution.")
            }
            WButton {
                text: i18n("Cancel")
                onClicked: view.flow.cancelScan()
            }
        }
    }
}
