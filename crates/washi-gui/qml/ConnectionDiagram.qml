import QtQuick
import QtQuick.Layouts

// This computer -- network link -- printer, drawn with the app's own icons.
RowLayout {
    id: diagram

    property string connection: ""      // "wifi", "lan" or "" when the snapshot does not say
    property string printerName: ""
    property bool online: true

    spacing: 0

    ColumnLayout {
        Layout.preferredWidth: 150
        spacing: 8
        Glyph {
            Layout.alignment: Qt.AlignHCenter
            name: "laptop"
            size: 56
            weight: 1.3
        }
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            kind: "small"
            text: i18n("This computer")
        }
    }

    // The link: a line with the connection icon on it
    ColumnLayout {
        Layout.preferredWidth: 150
        Layout.alignment: Qt.AlignTop
        spacing: 8
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 56
            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width
                height: 2
                color: diagram.online ? Theme.accent : Theme.offline
                opacity: diagram.online ? 0.8 : 0.5
            }
            Rectangle {
                anchors.centerIn: parent
                width: 44
                height: 44
                radius: 22
                color: Theme.card
                border.width: 2
                border.color: diagram.online ? Theme.accent : Theme.offline
                Glyph {
                    anchors.centerIn: parent
                    name: diagram.connection === "wifi" ? "wifi" : "lan"
                    size: 26
                    color: diagram.online ? Theme.accent : Theme.offline
                }
            }
        }
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            kind: "small"
            text: diagram.connection === "wifi" ? i18n("Wi-Fi") : (diagram.connection === "lan" ? i18n("Wired LAN") : i18n("Network"))
        }
    }

    ColumnLayout {
        Layout.preferredWidth: 150
        spacing: 8
        Glyph {
            Layout.alignment: Qt.AlignHCenter
            name: "printer"
            size: 56
            weight: 1.3
        }
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            kind: "small"
            text: diagram.printerName
            elide: Text.ElideRight
        }
    }
}
