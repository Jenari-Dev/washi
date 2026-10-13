import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Modal dialog card. Put the body inside; put buttons in `buttons` (right-aligned row).
QQC2.Popup {
    id: dlg

    property string title: ""
    property bool dismissable: true
    property bool showClose: dismissable
    property int dialogWidth: 460
    default property alias content: contentCol.data
    property alias buttons: footerRow.data
    property alias bodySpacing: contentCol.spacing
    // Set when this dialog needs its footer row.
    property bool hasButtons: footerRow.children.length > 0

    parent: QQC2.Overlay.overlay
    anchors.centerIn: parent
    modal: true
    dim: true
    padding: 0
    width: Math.min(dialogWidth, (parent ? parent.width : dialogWidth) - 48)
    closePolicy: dismissable ? (QQC2.Popup.CloseOnEscape | QQC2.Popup.CloseOnPressOutside) : QQC2.Popup.NoAutoClose

    QQC2.Overlay.modal: Rectangle {
        color: "#66000000"
    }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 110 }
    }
    exit: Transition {
        NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 80 }
    }

    background: Rectangle {
        radius: 8
        color: Theme.card
        border.width: 1
        border.color: "#C9C9C9"
        // A soft drop shadow made of two translucent plates.
        Rectangle {
            z: -1
            anchors.fill: parent
            anchors.margins: -2
            radius: 10
            color: "transparent"
            border.width: 2
            border.color: "#14000000"
        }
    }

    contentItem: ColumnLayout {
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 24
            Layout.rightMargin: 12
            Layout.topMargin: 18
            visible: dlg.title !== ""
            spacing: 8
            WText {
                Layout.fillWidth: true
                kind: "heading"
                text: dlg.title
                wrapMode: Text.WordWrap
            }
            WButton {
                visible: dlg.showClose
                kind: "quiet"
                glyph: "close"
                implicitWidth: 34
                onClicked: dlg.close()
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: i18n("Close")
            }
        }

        ColumnLayout {
            id: contentCol
            Layout.fillWidth: true
            Layout.leftMargin: 24
            Layout.rightMargin: 24
            Layout.topMargin: dlg.title !== "" ? 10 : 22
            Layout.bottomMargin: dlg.hasButtons ? 4 : 22
            spacing: 10
        }

        RowLayout {
            id: footerRow
            Layout.fillWidth: true
            Layout.leftMargin: 24
            Layout.rightMargin: 24
            Layout.topMargin: 14
            Layout.bottomMargin: 20
            Layout.alignment: Qt.AlignRight
            visible: dlg.hasButtons
            spacing: 10
        }
    }
}
