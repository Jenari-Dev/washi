import QtQuick
import QtQuick.Layouts

// The strip across the top of every page: the read-only notice on the left (when the service is
// read-only), the language switch on the right.
Rectangle {
    id: topBar
    objectName: "topBar"

    readonly property bool notice: Store.connected && Store.readOnly

    implicitHeight: Math.max(36, row.implicitHeight + 12)
    color: notice ? Theme.warnBg : Theme.bg

    Rectangle {
        anchors.bottom: parent.bottom
        width: parent.width
        height: 1
        color: topBar.notice ? "#EBD08A" : Theme.line
    }
    RowLayout {
        id: row
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: 16
        anchors.rightMargin: 12
        spacing: 10
        RowLayout {
            objectName: "readOnlyBanner"
            visible: topBar.notice
            Layout.fillWidth: true
            spacing: 10
            Glyph {
                name: "info"
                size: 18
                color: Theme.warn
            }
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                maximumLineCount: 2
                elide: Text.ElideRight
                text: i18n("Read-only mode: Washi can look at the printer but will not change anything. Start washid without --read-only to clean, test or change settings.")
            }
        }
        Item {
            visible: !topBar.notice
            Layout.fillWidth: true
        }
        LangSwitch {
            objectName: "langSwitch"
            Layout.alignment: Qt.AlignVCenter
        }
    }
}
