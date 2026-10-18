import QtQuick
import QtQuick.Layouts

PageScroll {
    id: page

    // The test notification: "" | "sending" | "ok" | "error".
    property string testState: ""
    property var testErrorSource: ""
    // The self-check: null until it has run; running while it is asked.
    property var setupReport: null
    property bool setupRunning: false
    property var setupErrorSource: ""

    function sendTest() {
        testState = "sending";
        Store.sendTestNotification((ok, body) => {
            testErrorSource = ok ? "" : Fmt.errorSource(body);
            testState = ok ? "ok" : "error";
        });
    }

    function checkSetup() {
        setupRunning = true;
        setupErrorSource = "";
        Store.checkSetup((ok, result) => {
            setupRunning = false;
            setupReport = ok ? result : null;
            setupErrorSource = ok ? "" : Fmt.errorSource(result);
        });
    }

    // The screenshot checks do what a click does: WASHI_APPSETTINGS_RUN="test-notify check-setup".
    property bool ranFromEnv: false
    function runFromEnv() {
        if (ranFromEnv || !Store.connected) {
            return;
        }
        ranFromEnv = true;
        const todo = Store.env("WASHI_APPSETTINGS_RUN").split(" ");
        if (todo.indexOf("test-notify") >= 0) {
            sendTest();
        }
        if (todo.indexOf("check-setup") >= 0) {
            checkSetup();
        }
    }

    Component.onCompleted: {
        Store.loadNotifications();
        runFromEnv();
    }
    // The report is worded by the service in the language of the moment: ask again after a switch.
    Connections {
        target: Lang
        function onCurrentChanged() {
            page.setupReport = null;
            page.testState = "";
        }
    }
    Connections {
        target: Store
        function onConnectedChanged() {
            if (Store.connected) {
                Store.loadNotifications();
                page.runFromEnv();
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
            RowLayout {
                Layout.topMargin: 6
                spacing: 12
                WButton {
                    objectName: "testNotifyButton"
                    glyph: "bell"
                    text: i18n("Send a test notification")
                    enabled: Store.connected && page.testState !== "sending"
                    onClicked: page.sendTest()
                }
                Spinner {
                    visible: page.testState === "sending"
                    size: 22
                    running: page.testState === "sending"
                }
            }
            InfoBar {
                objectName: "testNotifyResult"
                Layout.fillWidth: true
                visible: page.testState === "ok" || page.testState === "error"
                kind: page.testState === "ok" ? "info" : "error"
                text: page.testState === "ok"
                    ? i18n("Sent. If a notification appeared on your desktop, notifications work.")
                    : Fmt.text(page.testErrorSource)
            }
        }
    }

    // Check setup
    WCard {
        Layout.fillWidth: true
        implicitHeight: setupCol.implicitHeight + 36
        ColumnLayout {
            id: setupCol
            anchors.fill: parent
            anchors.margins: 18
            spacing: 10
            WText {
                kind: "heading"
                text: i18n("Check setup")
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Looks at the Washi service, your printer, the desktop and the optional helper programs. The printer is only asked read-only questions, and nothing is sent to anyone else.")
            }
            RowLayout {
                spacing: 12
                WButton {
                    objectName: "checkSetupButton"
                    glyph: "refresh"
                    text: page.setupReport ? i18n("Check again") : i18n("Check setup")
                    enabled: Store.connected && !page.setupRunning
                    onClicked: page.checkSetup()
                }
                Spinner {
                    visible: page.setupRunning
                    size: 22
                    running: page.setupRunning
                }
                WText {
                    visible: page.setupRunning
                    kind: "dim"
                    text: i18n("Checking…")
                }
            }
            InfoBar {
                objectName: "setupUnavailable"
                Layout.fillWidth: true
                visible: !Store.connected
                kind: "info"
                text: i18n("The service is not running, so the setup cannot be checked. Run “washi doctor” in a terminal to check without it.")
            }
            InfoBar {
                objectName: "setupError"
                Layout.fillWidth: true
                visible: Fmt.text(page.setupErrorSource) !== ""
                kind: "error"
                text: Fmt.text(page.setupErrorSource)
            }
            ColumnLayout {
                id: reportCol
                objectName: "setupReport"
                visible: page.setupReport !== null
                Layout.fillWidth: true
                spacing: 12
                WText {
                    objectName: "setupSummary"
                    font.weight: Font.DemiBold
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                    text: page.setupReport
                        ? (page.setupReport.summary.fail === 0
                            ? i18n("No problems found.")
                            : i18np("%1 problem found.", "%1 problems found.", page.setupReport.summary.fail))
                        : ""
                }
                Repeater {
                    model: page.setupReport ? page.setupReport.groups : []
                    delegate: ColumnLayout {
                        id: grp
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: 6
                        WText {
                            kind: "heading"
                            text: grp.modelData.title
                        }
                        Repeater {
                            model: grp.modelData.checks
                            delegate: RowLayout {
                                id: row
                                required property var modelData
                                objectName: "check-" + modelData.id
                                Layout.fillWidth: true
                                Layout.leftMargin: 4
                                spacing: 10
                                Item {
                                    Layout.preferredWidth: 22
                                    Layout.preferredHeight: 22
                                    Layout.alignment: Qt.AlignTop
                                    Glyph {
                                        anchors.centerIn: parent
                                        visible: row.modelData.status !== "skip"
                                        name: row.modelData.status === "ok" ? "success" : "error"
                                        size: 20
                                        weight: 1.8
                                        color: row.modelData.status === "ok" ? Theme.ok : Theme.danger
                                    }
                                    WText {
                                        anchors.centerIn: parent
                                        visible: row.modelData.status === "skip"
                                        text: "–"
                                        kind: "dim"
                                        font.pixelSize: 18
                                    }
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2
                                    WText {
                                        Layout.fillWidth: true
                                        wrapMode: Text.WordWrap
                                        font.weight: Font.DemiBold
                                        text: row.modelData.label
                                    }
                                    WText {
                                        visible: row.modelData.detail !== ""
                                        Layout.fillWidth: true
                                        wrapMode: Text.WrapAnywhere
                                        kind: "small"
                                        text: row.modelData.detail
                                    }
                                    WText {
                                        visible: row.modelData.hint !== ""
                                        Layout.fillWidth: true
                                        wrapMode: Text.WordWrap
                                        kind: "small"
                                        color: row.modelData.status === "fail" ? Theme.danger : Theme.textDim
                                        text: row.modelData.hint
                                    }
                                }
                            }
                        }
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
