import QtQuick
import QtQuick.Layouts

// The firmware update in one dialog: a strong warning that has to be read and ticked, progress
// while the printer downloads, installs and restarts, and the result with the new version. The
// only place that starts an update, and only from accept(), after the box is ticked and "Update
// Firmware" is pressed. Washi's update has not been tried on a real printer yet, and the dialog
// says so.
Item {
    id: host

    // closed | confirm | running | done
    property string stage: "closed"
    property string jobId: ""
    property bool ok: false
    property bool understood: false
    // What the dialog says while it works and when it is over; message sources (see Fmt.text()).
    property var progressSource: ""
    property var resultSource: ""
    readonly property string progress: Fmt.text(progressSource)
    readonly property string result: Fmt.text(resultSource)

    width: 0
    height: 0

    function open() {
        if (stage === "running") {
            dlg.open();
            return;
        }
        understood = false;
        jobId = "";
        progressSource = "";
        resultSource = "";
        stage = "confirm";
        dlg.open();
    }

    // The user pressed "Update Firmware". This is the only call that starts an update.
    function accept() {
        if (stage !== "confirm" || !understood) {
            return;
        }
        stage = "running";
        ok = false;
        jobId = "";
        progressSource = () => i18n("Sending the update request to the printer…");
        Store.updateFirmware((done, body) => {
            if (stage !== "running") {
                return;     // already finished (the signal can beat the reply)
            }
            if (done) {
                jobId = body;
            } else {
                finish(false, Fmt.errorSource(body));
            }
        });
    }

    function cancel() {
        if (stage === "running") {
            return;
        }
        stage = "closed";
        dlg.close();
    }

    function finish(success, message) {
        ok = success;
        resultSource = message;
        stage = "done";
    }

    Connections {
        target: Store
        function onMaintenanceProgress(job, st, message) {
            if (host.stage !== "running" || (host.jobId !== "" && job !== host.jobId)) {
                return;
            }
            host.progressSource = message;
        }
        function onMaintenanceFinished(job, success, message) {
            if (host.stage !== "running" || (host.jobId !== "" && job !== host.jobId)) {
                return;
            }
            host.finish(success, message);
        }
    }

    WDialog {
        id: dlg
        objectName: "firmwareDialog"
        dialogWidth: 520
        dismissable: host.stage !== "running"
        showClose: host.stage !== "running"
        title: i18n("Update Firmware")
        onClosed: if (host.stage !== "running") host.stage = "closed"

        // ---- warning
        ColumnLayout {
            visible: host.stage === "confirm"
            Layout.fillWidth: true
            spacing: 10
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: i18n("The printer downloads and installs new firmware by itself, from Canon's servers.")
            }
            InfoBar {
                objectName: "firmwareWarning"
                Layout.fillWidth: true
                kind: "warning"
                text: i18n("Do not switch the printer off, unplug it or cut its network connection until it has finished. An interrupted update can make the printer unusable.")
            }
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: i18n("The update takes several minutes. The printer restarts and cannot be used until it is back. An update cannot be undone.")
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Printer: %1 (firmware %2)", Store.displayName, Store.firmware)
            }
            WText {
                Layout.fillWidth: true
                kind: "muted"
                wrapMode: Text.WordWrap
                text: i18n("This feature is new in Washi and has not yet been tried on a real printer.")
            }
            WCheck {
                objectName: "understoodCheck"
                Layout.fillWidth: true
                Layout.topMargin: 4
                text: i18n("I have read this and want to update the firmware now.")
                checked: host.understood
                onToggled: host.understood = checked
            }
        }

        // ---- running
        ColumnLayout {
            visible: host.stage === "running"
            Layout.fillWidth: true
            spacing: 14
            Spinner {
                Layout.alignment: Qt.AlignHCenter
                size: 44
                running: host.stage === "running"
            }
            WText {
                objectName: "progressText"
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                text: host.progress
            }
            InfoBar {
                Layout.fillWidth: true
                kind: "warning"
                text: i18n("Do not switch the printer off. This takes several minutes.")
            }
        }

        // ---- done
        ColumnLayout {
            visible: host.stage === "done"
            Layout.fillWidth: true
            spacing: 10
            RowLayout {
                spacing: 10
                Glyph {
                    name: host.ok ? "success" : "error"
                    size: 30
                    weight: 1.8
                    color: host.ok ? Theme.ok : Theme.danger
                }
                WText {
                    Layout.fillWidth: true
                    objectName: "resultText"
                    wrapMode: Text.WordWrap
                    font.weight: Font.DemiBold
                    text: host.ok ? i18n("Finished.") : i18n("It did not work.")
                }
            }
            WText {
                objectName: "resultDetail"
                Layout.fillWidth: true
                visible: host.result !== ""
                kind: "dim"
                wrapMode: Text.WordWrap
                text: host.result
            }
        }

        buttons: [
            WButton {
                visible: host.stage === "confirm"
                text: i18n("Cancel")
                onClicked: host.cancel()
            },
            WButton {
                objectName: "startButton"
                visible: host.stage === "confirm"
                kind: "danger"
                enabled: host.understood
                text: i18n("Update Firmware")
                onClicked: host.accept()
            },
            WButton {
                objectName: "doneButton"
                visible: host.stage === "done"
                kind: "primary"
                text: i18n("Close")
                onClicked: host.cancel()
            }
        ]
    }
}
