import QtQuick
import QtQuick.Layouts

Item {
    id: tab

    readonly property var s: Store.snap
    readonly property string kind: Store.statusKind
    readonly property var lowInks: Store.inks.filter(i => i.status === "low" || i.status === "empty")
    readonly property bool allClear: kind === "online"

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width - 48, 520)
        spacing: 12

        Glyph {
            Layout.alignment: Qt.AlignHCenter
            name: {
                switch (tab.kind) {
                case "online": return "success";
                case "sleeping": return "moon";
                case "error": return "error";
                case "warning": return "warning";
                case "busy": return "timer";
                default: return "info";
                }
            }
            size: 60
            weight: 1.3
            color: {
                switch (tab.kind) {
                case "online": return Theme.ok;
                case "error": return Theme.danger;
                case "warning": return Theme.warn;
                case "sleeping": return Theme.sleeping;
                case "busy": return Theme.accent;
                default: return Theme.offline;
                }
            }
        }

        // Headline
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            kind: "heading"
            text: {
                switch (tab.kind) {
                case "none": return i18n("No printer selected.");
                case "unknown": return i18n("Reading the printer…");
                case "offline": return i18n("The printer cannot be reached.");
                case "sleeping": return i18n("The printer is asleep.");
                case "busy": return i18n("The printer is working on something else.");
                case "error": return Store.supportShown ? i18n("Support code %1", tab.s.support.code) : i18n("An ink tank is empty.");
                case "warning": return Store.supportShown ? i18n("Support code %1", tab.s.support.code) : i18n("Ink is running low.");
                default: return Store.supportShown ? i18n("Support code %1", tab.s.support.code) : i18n("No new notices.");
                }
            }
        }
        // Detail
        WText {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            kind: "dim"
            text: {
                switch (tab.kind) {
                case "none": return i18n("Add a printer to see its status.");
                case "offline": return Store.snapError !== "" ? Store.snapError : i18n("Check that it is switched on and connected to the same network as this computer.");
                case "sleeping": return i18n("It saved power with auto power-off and wakes up by itself when a job arrives.");
                case "busy": return Store.statusTip;
                case "error": return Store.supportShown ? Fmt.supportMessage(tab.s.support) : i18n("Replace the empty tank to keep printing.");
                case "warning": return Store.supportShown ? Fmt.supportMessage(tab.s.support) : i18n("You can keep printing, but have a new tank ready.");
                case "online": return Store.supportShown ? Fmt.supportMessage(tab.s.support) : i18n("The printer is ready.");
                default: return "";
                }
            }
        }
        WText {
            visible: (tab.kind === "warning" || tab.kind === "error") && tab.lowInks.length > 0
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: i18n("Low or empty: %1", Fmt.list(tab.lowInks.map(i => i.color)))
        }
        // A support code that is not the headline: a notice, or a code next to an empty tank.
        WText {
            objectName: "supportNotice"
            visible: !!tab.s.support && !Store.supportShown && Store.power === "on"
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: !!tab.s.support ? i18n("Support code %1: %2", tab.s.support.code, Fmt.supportMessage(tab.s.support)) : ""
        }
        WakeButton {
            objectName: "wakeButton"
            Layout.alignment: Qt.AlignHCenter
        }
        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            visible: tab.kind === "error" || tab.kind === "warning" || (!!tab.s.support && Store.power === "on")
            spacing: 10
            WButton {
                objectName: "supportHelpButton"
                visible: !!tab.s.support
                kind: "primary"
                text: i18n("Help for this Support Code")
                glyph: "external"
                onClicked: Nav.openLink("error")
            }
            WButton {
                objectName: "manualButton"
                text: i18n("Open the Manual")
                glyph: "external"
                onClicked: Nav.openLink("help")
            }
            WButton {
                visible: tab.lowInks.length > 0
                text: i18n("Go to Consumables")
                onClicked: Nav.replace("consumables")
            }
        }
        WText {
            visible: Store.snapLoaded && !!tab.s.last_updated
            Layout.fillWidth: true
            Layout.topMargin: 8
            horizontalAlignment: Text.AlignHCenter
            kind: "muted"
            text: i18n("Last read at %1", Fmt.clock(new Date(tab.s.last_updated * 1000)))
        }
    }
}
