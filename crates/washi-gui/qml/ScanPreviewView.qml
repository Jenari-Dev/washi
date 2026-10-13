import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import QtCore

// Scan, step 2: the thumbnails with their tools (left), the page (centre) and the edit and save
// settings (right).
RowLayout {
    id: view

    required property var flow
    spacing: 0

    // ---- thumbnails ----------------------------------------------------------------------------
    Rectangle {
        Layout.preferredWidth: 124
        Layout.fillHeight: true
        color: "#F7F7F7"

        Rectangle {
            anchors.right: parent.right
            width: 1
            height: parent.height
            color: Theme.cardBorder
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.rightMargin: 1
            spacing: 0

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 6
                spacing: 2
                WButton {
                    objectName: "rotateLeftButton"
                    kind: "quiet"
                    glyph: "rotatel"
                    implicitWidth: 34
                    enabled: view.flow.hasPages
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: i18n("Rotate left")
                    onClicked: view.flow.rotate("left")
                }
                WButton {
                    objectName: "rotateRightButton"
                    kind: "quiet"
                    glyph: "rotater"
                    implicitWidth: 34
                    enabled: view.flow.hasPages
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: i18n("Rotate right")
                    onClicked: view.flow.rotate("right")
                }
                WButton {
                    objectName: "deleteButton"
                    kind: "quiet"
                    glyph: "trash"
                    implicitWidth: 34
                    enabled: view.flow.hasPages
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: i18n("Delete this page")
                    onClicked: view.flow.deleteCurrent()
                }
            }
            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: Theme.line
            }

            ListView {
                id: thumbs
                objectName: "thumbs"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: 10
                topMargin: 10
                bottomMargin: 10
                model: view.flow.pages
                currentIndex: view.flow.current
                delegate: Item {
                    id: cell
                    required property int index
                    required property string thumb
                    width: thumbs.width
                    height: 126
                    Rectangle {
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 90
                        height: 108
                        color: "#FFFFFF"
                        border.width: cell.index === view.flow.current ? 3 : 1
                        border.color: cell.index === view.flow.current ? Theme.accent : "#C8C8C8"
                        Image {
                            anchors.fill: parent
                            anchors.margins: 3
                            source: cell.thumb !== "" ? "file://" + cell.thumb : ""
                            fillMode: Image.PreserveAspectFit
                            asynchronous: true
                            cache: false
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: view.flow.select(cell.index)
                        }
                    }
                    WText {
                        anchors.horizontalCenter: parent.horizontalCenter
                        anchors.bottom: parent.bottom
                        kind: "small"
                        text: cell.index + 1
                    }
                }
                QQC2.ScrollBar.vertical: QQC2.ScrollBar {
                    policy: QQC2.ScrollBar.AsNeeded
                }
            }

            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: Theme.line
            }
            ColumnLayout {
                Layout.fillWidth: true
                Layout.margins: 8
                spacing: 6
                WButton {
                    objectName: "joinButton"
                    Layout.fillWidth: true
                    visible: view.flow.mode === "stitch" && view.flow.count >= 2
                    text: i18n("Join")
                    glyph: "check"
                    onClicked: view.flow.stitchLastTwo("left-right")
                }
                WButton {
                    objectName: "nextScanButton"
                    Layout.fillWidth: true
                    enabled: view.flow.canScan
                    text: i18nc("short button label under the page thumbnails", "+ Next scan")
                    onClicked: view.flow.nextScan()
                }
            }
        }
    }

    // ---- the page ----------------------------------------------------------------------------------
    Rectangle {
        Layout.fillWidth: true
        Layout.fillHeight: true
        color: "#E6E6E6"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 8

            RowLayout {
                Layout.fillWidth: true
                WButton {
                    objectName: "backToSettings"
                    kind: "quiet"
                    glyph: "chevleft"
                    text: i18n("Scan settings")
                    onClicked: view.flow.back()
                }
                Item {
                    Layout.fillWidth: true
                }
                WText {
                    kind: "dim"
                    text: i18n("Page %1 of %2", view.flow.current + 1, Math.max(1, view.flow.count))
                }
            }
            InfoBar {
                objectName: "scanNotice"
                Layout.fillWidth: true
                visible: view.flow.notice !== "" || view.flow.error !== ""
                kind: view.flow.error !== "" ? "error" : "info"
                text: view.flow.error !== "" ? view.flow.error : view.flow.notice
            }
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Rectangle {
                    id: sheet
                    readonly property real iw: view.flow.mainWidth > 0 ? view.flow.mainWidth : 1
                    readonly property real ih: view.flow.mainHeight > 0 ? view.flow.mainHeight : 1.4
                    readonly property real f: Math.min((parent.width - 8) / iw, (parent.height - 8) / ih)
                    anchors.centerIn: parent
                    width: iw * f
                    height: ih * f
                    color: "#FFFFFF"
                    border.width: 1
                    border.color: "#C8C8C8"
                    Image {
                        objectName: "mainImage"
                        anchors.fill: parent
                        source: view.flow.mainPath !== "" ? "file://" + view.flow.mainPath : ""
                        fillMode: Image.PreserveAspectFit
                        asynchronous: true
                        cache: false
                        smooth: true
                    }
                }
                Spinner {
                    visible: view.flow.scanning
                    anchors.centerIn: parent
                    size: 36
                }
            }
        }
    }

    // ---- edit and save ----------------------------------------------------------------------------------
    SidePanel {
        showPrinter: false
        panelWidth: 290
        footer: [
            WButton {
                objectName: "saveButton"
                Layout.fillWidth: true
                kind: "primary"
                enabled: view.flow.hasPages && !view.flow.saving
                text: view.flow.saving ? i18n("Saving…") : i18n("Save")
                onClicked: saveDialog.openNow()
            }
        ]

        WButton {
            objectName: "rotateCropButton"
            Layout.fillWidth: true
            glyph: "crop"
            text: i18n("Rotate/Crop")
            enabled: view.flow.hasPages
            onClicked: view.flow.openCrop()
        }
        WButton {
            objectName: "correctionButton"
            Layout.fillWidth: true
            glyph: "wand"
            text: i18n("Image Correction")
            enabled: view.flow.hasPages
            onClicked: view.flow.openCorrect()
        }
        WButton {
            objectName: "revertButton"
            Layout.fillWidth: true
            kind: "quiet"
            glyph: "undo"
            text: i18n("Revert to original")
            enabled: view.flow.hasPages
            onClicked: view.flow.revert()
        }

        FormRow {
            Layout.topMargin: 6
            label: i18n("Format")
            Flow {
                Layout.fillWidth: true
                spacing: 8
                Repeater {
                    model: ScanModel.formats(Img.tools())
                    delegate: Rectangle {
                        id: tile
                        required property var modelData
                        objectName: "format-" + modelData.value
                        readonly property bool on: view.flow.format === modelData.value
                        width: modelData.value === "pdf" || modelData.value === "jpeg" ? 76 : 136
                        height: 62
                        radius: 6
                        color: on ? Theme.accentSoft : "#FFFFFF"
                        border.width: on ? 2 : 1
                        border.color: on ? Theme.accent : Theme.fieldBorder
                        Column {
                            anchors.centerIn: parent
                            spacing: 4
                            Glyph {
                                anchors.horizontalCenter: parent.horizontalCenter
                                name: tile.modelData.glyph
                                size: 24
                                color: tile.on ? Theme.accent : Theme.navy
                            }
                            WText {
                                anchors.horizontalCenter: parent.horizontalCenter
                                text: tile.modelData.text
                                kind: "small"
                                color: Theme.text
                            }
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: view.flow.format = tile.modelData.value
                        }
                    }
                }
            }
        }
        FormRow {
            label: i18n("Range")
            WRadio {
                objectName: "rangeAll"
                text: i18n("Save all pages")
                checked: view.flow.range === "all"
                onClicked: view.flow.range = "all"
            }
            WRadio {
                objectName: "rangeCurrent"
                text: i18n("Save current page")
                checked: view.flow.range === "current"
                onClicked: view.flow.range = "current"
            }
        }
        WCheck {
            objectName: "singleFile"
            Layout.fillWidth: true
            visible: view.flow.format === "pdf"
            text: i18n("Combine all pages into one file")
            checked: view.flow.singleFile
            onToggled: view.flow.singleFile = checked
        }
        InfoBar {
            objectName: "savedBar"
            Layout.fillWidth: true
            visible: view.flow.savedFiles.length > 0
            kind: "info"
            text: view.flow.savedFiles.length === 1 ? i18n("Saved %1.", Img.baseName(view.flow.savedFiles[0])) : i18np("Saved %1 file.", "Saved %1 files.", view.flow.savedFiles.length)
            WButton {
                text: i18n("Open folder")
                onClicked: Nav.openUrl("file://" + Img.dirName(view.flow.savedFiles[0]))
            }
        }
        InfoBar {
            objectName: "saveError"
            Layout.fillWidth: true
            visible: view.flow.saveError !== ""
            kind: "error"
            text: view.flow.saveError
        }
        WButton {
            Layout.fillWidth: true
            kind: "quiet"
            text: i18n("New scan")
            onClicked: view.flow.newScan()
        }
    }

    // ---- save dialog ---------------------------------------------------------------------------------
    FileDialog {
        id: saveDialog
        objectName: "saveDialog"
        title: i18n("Save")
        fileMode: FileDialog.SaveFile
        nameFilters: view.flow.format === "jpeg" ? ["JPEG (*.jpg)"] : (view.flow.format === "txt" ? [i18n("Text (*.txt)")] : ["PDF (*.pdf)"])
        defaultSuffix: ScanModel.extensionFor(view.flow.format).substring(1)
        function openNow() {
            const folder = Prefs.scanFolder !== "" ? Prefs.scanFolder : StandardPaths.writableLocation(StandardPaths.DocumentsLocation);
            currentFolder = folder;
            currentFile = folder + "/" + ScanModel.suggestedName() + ScanModel.extensionFor(view.flow.format);
            open();
        }
        onAccepted: {
            Prefs.scanFolder = String(currentFolder);
            view.flow.save(Img.localPath(selectedFile), null);
        }
    }

    // ---- "+ Next scan" -------------------------------------------------------------------------------
    WDialog {
        id: nextDlg
        objectName: "nextScanDialog"
        title: i18n("Next scan")
        visible: view.flow.awaitingNext
        onVisibleChanged: if (!visible && view.flow.awaitingNext) view.flow.awaitingNext = false
        WText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: i18n("Put the next original on the glass, face down, then press Scan. It is added as page %1.", view.flow.count + 1)
        }
        buttons: [
            WButton {
                text: i18n("Cancel")
                onClicked: view.flow.awaitingNext = false
            },
            WButton {
                objectName: "scanNextConfirm"
                kind: "primary"
                text: i18n("Scan")
                enabled: view.flow.canScan
                onClicked: view.flow.scanNext()
            }
        ]
    }
}
