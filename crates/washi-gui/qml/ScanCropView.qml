import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Scan, step 3: Rotate/Crop. A dark canvas with the page and a draggable trimming box (left);
// rotate buttons, slant correction and the trimming aspect ratio (right).
RowLayout {
    id: view

    required property var flow
    spacing: 0

    readonly property var edits: flow.currentEdits
    readonly property real slant: edits ? edits.slant_degrees : 0
    readonly property var crop: edits ? edits.crop : null
    property string aspect: "custom"
    // Width / height of the box while an aspect preset is chosen (pixels of the page).
    readonly property real lock: aspect === "custom" || !crop ? 0 : (crop.w * flow.editState.rotated_width) / (crop.h * flow.editState.rotated_height)

    Rectangle {
        id: canvas
        objectName: "cropCanvas"
        Layout.fillWidth: true
        Layout.fillHeight: true
        color: "#2B2B2B"

        Item {
            id: stage
            anchors.fill: parent
            anchors.margins: 24

            Image {
                id: pic
                objectName: "cropImage"
                anchors.fill: parent
                source: view.flow.editPath !== "" ? "file://" + view.flow.editPath : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                cache: false
                smooth: true
            }
            CropOverlay {
                id: overlay
                objectName: "cropOverlay"
                anchors.fill: parent
                visible: pic.status === Image.Ready
                imageRect: Qt.rect((stage.width - pic.paintedWidth) / 2, (stage.height - pic.paintedHeight) / 2, pic.paintedWidth, pic.paintedHeight)
                crop: view.crop
                lockRatio: view.lock
                pictureWidth: view.flow.editState ? view.flow.editState.rotated_width : 1
                pictureHeight: view.flow.editState ? view.flow.editState.rotated_height : 1
                onEdited: (x, y, w, h) => view.flow.setCrop(x, y, w, h)
            }
            Spinner {
                visible: view.flow.editBusy
                anchors.centerIn: parent
                size: 30
                color: "#FFFFFF"
            }
        }
    }

    SidePanel {
        showPrinter: false
        panelWidth: 290
        footer: [
            Item {
                Layout.fillWidth: true
            },
            WButton {
                objectName: "cropCancel"
                text: i18n("Cancel")
                onClicked: view.flow.cropCancel()
            },
            WButton {
                objectName: "cropOk"
                kind: "primary"
                implicitWidth: 92
                text: i18n("OK")
                onClicked: view.flow.cropOk()
            }
        ]

        WText {
            kind: "heading"
            text: i18n("Rotate/Crop")
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            WButton {
                objectName: "cropRotateLeft"
                Layout.fillWidth: true
                glyph: "rotatel"
                text: i18n("Rotate left")
                onClicked: view.flow.cropRotate("left")
            }
            WButton {
                objectName: "cropRotateRight"
                Layout.fillWidth: true
                glyph: "rotater"
                text: i18n("Rotate right")
                onClicked: view.flow.cropRotate("right")
            }
        }
        FormRow {
            Layout.topMargin: 6
            label: i18n("Slant Correction")
            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                WButton {
                    objectName: "slantMinus"
                    kind: "quiet"
                    implicitWidth: 34
                    text: "−"
                    onClicked: view.flow.setSlant(Math.round((view.slant - 0.1) * 10) / 10)
                }
                WSlider {
                    id: slantSlider
                    objectName: "slantSlider"
                    Layout.fillWidth: true
                    from: -10
                    to: 10
                    stepSize: 0.1
                    value: view.slant
                    onMoved: view.flow.setSlant(Math.round(value * 10) / 10)
                }
                WButton {
                    objectName: "slantPlus"
                    kind: "quiet"
                    implicitWidth: 34
                    text: "+"
                    onClicked: view.flow.setSlant(Math.round((view.slant + 0.1) * 10) / 10)
                }
            }
            RowLayout {
                Layout.fillWidth: true
                WText {
                    objectName: "slantValue"
                    kind: "dim"
                    text: i18n("%1°", (view.slant > 0 ? "+" : "") + view.slant.toFixed(1))
                }
                Item {
                    Layout.fillWidth: true
                }
                WButton {
                    objectName: "autoStraighten"
                    kind: "quiet"
                    text: i18n("Straighten automatically")
                    onClicked: view.flow.autoStraighten()
                }
            }
        }
        FormRow {
            label: i18n("Trimming Aspect Ratio")
            WCombo {
                objectName: "aspectCombo"
                Layout.fillWidth: true
                textRole: "text"
                valueRole: "value"
                model: ScanModel.aspectPresets()
                selectedValue: view.aspect
                onActivated: {
                    view.aspect = currentValue;
                    view.flow.setAspect(currentValue);
                }
            }
        }
        WText {
            Layout.fillWidth: true
            kind: "muted"
            wrapMode: Text.WordWrap
            text: i18n("Drag the corners of the frame to choose what is kept. The rest is cut off when you press OK.")
        }
    }
}
