import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// "Wake printer": asks a printer in auto power-off to wake up (IVEC PowerOn). That command is
// unverified on real printers, so the button asks first and says so when it fails; a print job,
// a maintenance job or the printer's own power button wake it just as well. Shown only while the
// printer sleeps. Read-only mode refuses (the button is off, with a tooltip saying why).
Item {
    id: w

    property bool compact: false
    // idle | sending | sent
    property string phase: "idle"
    // Why it failed: a message source (see Fmt.text()).
    property var failureSource: ""
    readonly property string failure: Fmt.text(failureSource)

    visible: Store.power === "sleeping"
    implicitWidth: btn.implicitWidth
    implicitHeight: btn.implicitHeight

    Connections {
        target: Store
        function onPowerChanged() {
            if (Store.power !== "sleeping") {
                w.phase = "idle";
            }
        }
    }

    WButton {
        id: btn
        objectName: "wakeInnerButton"
        anchors.fill: parent
        glyph: "power"
        text: w.phase === "sending" ? i18n("Waking…") : (w.phase === "sent" ? i18n("Wake request sent") : i18n("Wake printer"))
        implicitHeight: w.compact ? 28 : 34
        enabled: Store.connected && !Store.readOnly && Store.hasPrinter && w.phase === "idle"
        onClicked: confirm.open()
    }
    HoverHandler {
        id: hover
    }
    QQC2.ToolTip.visible: hover.hovered && (Store.readOnly || w.phase === "sent")
    QQC2.ToolTip.delay: 300
    QQC2.ToolTip.text: Store.readOnly ? i18n("Read-only mode: the service sends nothing to the printer.") : i18n("The printer should wake up within a few seconds. If it does not, press its power button or send a print job.")

    function send() {
        phase = "sending";
        Store.wake((ok, body) => {
            if (ok) {
                phase = "sent";
                resetTimer.restart();
            } else {
                phase = "idle";
                failureSource = Fmt.errorSource(body);
                failDlg.open();
            }
        });
    }

    // The headless screenshot checks open the dialog through Nav (WASHI_DIALOG=nav-wake-confirm).
    Connections {
        target: Nav
        function onDialogRequested(name, args) {
            if (name === "wake-confirm" && w.visible && w.objectName === "wakeButton") {
                confirm.open();
            }
        }
    }

    Timer {
        id: resetTimer
        interval: 20000
        onTriggered: w.phase = "idle"
    }

    ConfirmDialog {
        id: confirm
        objectName: "wakeConfirm"
        title: i18n("Wake printer")
        acceptText: i18n("Wake")
        glyph: "power"
        text: i18n("Ask %1 to wake up? This sends the PowerOn command, which has not been tried on a real printer yet. Starting a print or a cleaning wakes the printer too.", Store.displayName)
        onAccepted: w.send()
    }

    WDialog {
        id: failDlg
        objectName: "wakeFailure"
        title: i18n("The printer did not wake up")
        WText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: w.failure
        }
        WText {
            Layout.fillWidth: true
            kind: "dim"
            wrapMode: Text.WordWrap
            text: i18n("A sleeping printer may refuse the wake command. Press its power button, or start a print or a cleaning: those wake it by themselves.")
        }
        buttons: [
            WButton {
                kind: "primary"
                text: i18n("Close")
                onClicked: failDlg.close()
            }
        ]
    }
}
