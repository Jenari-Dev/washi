import QtQuick
import QtQuick.Layouts

// Scan, step 4: Image Correction, with the result shown live.
RowLayout {
    id: view

    required property var flow
    spacing: 0

    readonly property var corr: flow.currentEdits ? flow.currentEdits.correction : null

    Rectangle {
        Layout.fillWidth: true
        Layout.fillHeight: true
        color: "#E6E6E6"

        Item {
            anchors.fill: parent
            anchors.margins: 20
            Rectangle {
                id: sheet
                readonly property real iw: view.flow.editWidth > 0 ? view.flow.editWidth : 1
                readonly property real ih: view.flow.editHeight > 0 ? view.flow.editHeight : 1.4
                readonly property real f: Math.min((parent.width - 8) / iw, (parent.height - 8) / ih)
                anchors.centerIn: parent
                width: iw * f
                height: ih * f
                color: "#FFFFFF"
                border.width: 1
                border.color: "#C8C8C8"
                Image {
                    objectName: "correctImage"
                    anchors.fill: parent
                    source: view.flow.editPath !== "" ? "file://" + view.flow.editPath : ""
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    smooth: true
                }
            }
            Spinner {
                visible: view.flow.editBusy
                anchors.centerIn: parent
                size: 30
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
                objectName: "correctCancel"
                text: i18n("Cancel")
                onClicked: view.flow.correctCancel()
            },
            WButton {
                objectName: "correctOk"
                kind: "primary"
                implicitWidth: 92
                text: i18n("OK")
                onClicked: view.flow.correctOk()
            }
        ]

        WText {
            kind: "heading"
            text: i18n("Image Correction")
        }
        FormRow {
            label: i18n("Color Correction")
            Repeater {
                model: ScanModel.correctionPresets()
                delegate: WRadio {
                    required property var modelData
                    objectName: "corr-" + modelData.value
                    text: modelData.text
                    checked: view.corr && view.corr.color_correction === modelData.value
                    onClicked: view.flow.setCorrection({ color_correction: modelData.value })
                }
            }
        }
        FormRow {
            label: i18n("Reduce Show-through")
            RowLayout {
                spacing: 16
                WRadio {
                    objectName: "showThroughOff"
                    text: i18n("Off")
                    checked: view.corr && !view.corr.reduce_show_through
                    onClicked: view.flow.setCorrection({ reduce_show_through: false })
                }
                WRadio {
                    objectName: "showThroughOn"
                    text: i18n("On")
                    checked: view.corr && view.corr.reduce_show_through
                    onClicked: view.flow.setCorrection({ reduce_show_through: true })
                }
            }
        }
        FormRow {
            label: i18n("Color Mode")
            Repeater {
                model: ScanModel.outputModes()
                delegate: WRadio {
                    required property var modelData
                    objectName: "mode-" + modelData.value
                    text: modelData.text
                    checked: view.corr && view.corr.color_mode === modelData.value
                    onClicked: view.flow.setCorrection({ color_mode: modelData.value })
                }
            }
        }
        FormRow {
            label: i18n("Unsharp Mask")
            RowLayout {
                Layout.fillWidth: true
                WSlider {
                    objectName: "unsharpSlider"
                    Layout.fillWidth: true
                    from: 0
                    to: 100
                    stepSize: 5
                    value: view.corr ? view.corr.unsharp_amount * 100 : 0
                    onMoved: view.flow.setCorrection({ unsharp_amount: value / 100 })
                }
                WText {
                    Layout.preferredWidth: 38
                    horizontalAlignment: Text.AlignRight
                    kind: "dim"
                    text: Math.round(view.corr ? view.corr.unsharp_amount * 100 : 0) + "%"
                }
            }
        }
    }
}
