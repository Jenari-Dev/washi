import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Shown while the Washi service (washid) is not running.
Item {
    id: page

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width - 48, 520)
        spacing: 14

        Glyph {
            Layout.alignment: Qt.AlignHCenter
            name: "power"
            size: 60
            weight: 1.2
            color: Theme.sleeping
        }
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            kind: "heading"
            text: i18n("The Washi service is not running")
        }
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            kind: "dim"
            wrapMode: Text.WordWrap
            text: i18n("Washi talks to your printer through a small background service. Start it from a terminal with the command below; this window connects by itself as soon as it is up.")
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 48
            radius: 6
            color: "#1F2937"
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 14
                anchors.rightMargin: 6
                TextEdit {
                    id: cmd
                    objectName: "serviceCommand"
                    Layout.fillWidth: true
                    readOnly: true
                    selectByMouse: true
                    text: "systemctl --user start washid"
                    font.family: "monospace"
                    font.pixelSize: Theme.fontSize
                    color: "#E5EEF9"
                }
                QQC2.AbstractButton {
                    id: copyButton
                    implicitWidth: 38
                    implicitHeight: 38
                    hoverEnabled: true
                    onClicked: {
                        cmd.selectAll();
                        cmd.copy();
                        cmd.deselect();
                    }
                    background: Rectangle {
                        radius: 4
                        color: copyButton.down ? "#3B4A60" : (copyButton.hovered ? "#2D3B50" : "transparent")
                    }
                    contentItem: Item {
                        Glyph {
                            anchors.centerIn: parent
                            name: "copy"
                            size: 18
                            color: "#E5EEF9"
                        }
                    }
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: i18n("Copy")
                }
            }
        }
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            kind: "muted"
            wrapMode: Text.WordWrap
            text: i18n("To start it at every login: systemctl --user enable --now washid")
        }
        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 10
            Spinner {
                size: 18
            }
            WText {
                kind: "dim"
                text: i18n("Waiting for the service…")
            }
        }
    }
}
