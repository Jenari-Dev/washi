import QtQuick
import QtQuick.Layouts

PageScroll {
    id: page

    Component.onCompleted: Store.loadNotifications()
    Connections {
        target: Store
        function onConnectedChanged() {
            if (Store.connected) {
                Store.loadNotifications();
            }
        }
    }

    WText {
        kind: "title"
        text: i18n("App Settings")
    }

    // Printers
    WCard {
        Layout.fillWidth: true
        implicitHeight: printersCol.implicitHeight + 36
        ColumnLayout {
            id: printersCol
            anchors.fill: parent
            anchors.margins: 18
            spacing: 10
            WText {
                kind: "heading"
                text: i18n("Printers")
            }
            WSwitch {
                objectName: "autoDetect"
                text: i18n("Look for printers automatically")
                checked: Prefs.autoDetect
                onToggled: Prefs.autoDetect = checked
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("When no printer is set up, Washi searches your network for Canon printers as soon as it starts.")
            }
        }
    }

    // Notifications
    WCard {
        Layout.fillWidth: true
        implicitHeight: notifCol.implicitHeight + 36
        ColumnLayout {
            id: notifCol
            anchors.fill: parent
            anchors.margins: 18
            spacing: 10
            WText {
                kind: "heading"
                text: i18n("Notifications")
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("The Washi service sends desktop notifications for low ink, printer errors and a printer coming back online. The switches are saved in ~/.config/washi/config.toml.")
            }
            InfoBar {
                objectName: "notifyUnavailable"
                Layout.fillWidth: true
                visible: !Store.connected || Store.readOnly || Store.notifications === null
                kind: "info"
                text: !Store.connected ? i18n("The service is not running, so the notification switches are not available.")
                    : (Store.readOnly ? i18n("The service runs in read-only mode and keeps no settings. Start it without --read-only to change these.") : i18n("Reading the notification settings…"))
            }
            Repeater {
                model: [
                    { key: "enabled", text: i18n("Send notifications") },
                    { key: "ink", text: i18n("Ink is low or empty") },
                    { key: "errors", text: i18n("The printer reports an error") },
                    { key: "online", text: i18n("The printer is back online") }
                ]
                delegate: WSwitch {
                    required property var modelData
                    objectName: "notify-" + modelData.key
                    Layout.leftMargin: modelData.key === "enabled" ? 0 : 24
                    text: modelData.text
                    enabled: Store.connected && !Store.readOnly && Store.notifications !== null && (modelData.key === "enabled" || Store.notifications.enabled)
                    checked: Store.notifications ? !!Store.notifications[modelData.key] : false
                    onToggled: {
                        const want = checked;
                        checked = Qt.binding(() => Store.notifications ? !!Store.notifications[modelData.key] : false);
                        Store.setNotifications({ [modelData.key]: want }, null);
                    }
                }
            }
        }
    }

    // About
    WCard {
        Layout.fillWidth: true
        implicitHeight: aboutCol.implicitHeight + 36
        ColumnLayout {
            id: aboutCol
            anchors.fill: parent
            anchors.margins: 18
            spacing: 10
            WText {
                kind: "heading"
                text: i18n("About Washi")
            }
            WText {
                objectName: "versionText"
                text: i18n("Version %1", Qt.application.version)
            }
            WText {
                kind: "dim"
                text: Store.connected ? (Store.readOnly ? i18n("Service (washid): version %1, read-only", Store.serviceVersion) : i18n("Service (washid): version %1", Store.serviceVersion)) : i18n("Service (washid): not running")
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Washi talks to your printer only over your local network and sends nothing to anyone else. Links such as Manual first ask, then open in your web browser.")
            }
            WButton {
                text: i18n("Show License Information")
                onClicked: Nav.dialog("license")
            }
        }
    }
}
