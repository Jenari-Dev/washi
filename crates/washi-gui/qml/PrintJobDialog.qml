import QtQuick
import QtQuick.Layouts

// Sends a print job (photos, a document or a disc label) and follows it: a spinner with the
// service's progress, a Cancel button while it runs, and the outcome. The only place that starts
// a print job; it does so from start()/startDisc(), which callers invoke after the user
// confirmed in a dialog.
Item {
    id: host

    // closed | running | done
    property string stage: "closed"
    property string jobId: ""
    property bool ok: false
    property string headline: ""
    // Message sources (see Fmt.text()): the progress line, the result and the title follow the
    // language even when the language is switched while the job runs.
    property var progressSource: ""
    property var resultSource: ""
    property var titleSource: () => i18n("Printing")
    readonly property string progress: Fmt.text(progressSource)
    readonly property string result: Fmt.text(resultSource)
    readonly property string jobTitle: Fmt.text(titleSource)
    property bool cancelling: false
    // What was started, for the result text.
    property string what: ""

    signal finished(bool ok)

    width: 0
    height: 0

    function friendly(stage, message) {
        switch (stage) {
        case "preflight": return i18n("Checking what the printer accepts…");
        case "preparing": return message;
        case "validated": return i18n("The printer accepts the job.");
        case "sending": return i18n("Sending the job to the printer…");
        case "submitted": return i18n("The printer took the job. It prints now…");
        case "state": return i18n("Printing…");
        case "finished": return i18n("The printer finished the job.");
        default: return message;
        }
    }

    function begin(title, label) {
        titleSource = title;
        what = label;
        stage = "running";
        ok = false;
        jobId = "";
        cancelling = false;
        resultSource = "";
        headline = "";
        progressSource = () => i18n("Sending the job to the service…");
        dlg.open();
    }

    function _started(success, body) {
        if (stage !== "running") {
            return;     // already over (the signal can beat the reply)
        }
        if (success) {
            jobId = body;
        } else {
            finish(false, Fmt.errorSource(body));
        }
    }

    // Print files; `options` are the PrintFile options (confirm is added by the Store).
    function start(paths, options, label) {
        begin(() => i18n("Printing"), label);
        Store.printFiles(paths, options, _started);
    }

    function startDisc(spec, label) {
        begin(() => i18n("Printing disc label"), label);
        Store.printDisc(spec, _started);
    }

    function finish(success, message) {
        ok = success;
        resultSource = message;
        stage = "done";
        finished(success);
    }

    function cancel() {
        if (stage === "running" && jobId !== "" && !cancelling) {
            cancelling = true;
            progressSource = () => i18n("Stopping…");
            Store.cancelJob(jobId, null);
        }
    }

    function close() {
        stage = "closed";
        dlg.close();
    }

    Connections {
        target: Store
        function onPrintProgress(job, st, message) {
            if (host.stage !== "running" || (host.jobId !== "" && job !== host.jobId)) {
                return;
            }
            if (!host.cancelling) {
                host.progressSource = () => host.friendly(st, message);
            }
        }
        function onPrintFinished(job, success, message) {
            if (host.stage !== "running" || (host.jobId !== "" && job !== host.jobId)) {
                return;
            }
            host.finish(success, success ? message : (host.cancelling ? () => i18n("The job was stopped.") : message));
        }
    }

    WDialog {
        id: dlg
        dialogWidth: 480
        dismissable: host.stage !== "running"
        showClose: host.stage !== "running"
        title: host.jobTitle
        onClosed: if (host.stage !== "running") host.stage = "closed"

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
            WText {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                kind: "muted"
                wrapMode: Text.WordWrap
                text: i18n("Keep the printer switched on until this finishes.")
            }
        }

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
                    objectName: "resultHeadline"
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    font.weight: Font.DemiBold
                    text: host.ok ? i18n("Finished.") : i18n("It did not print.")
                }
            }
            WText {
                objectName: "resultText"
                Layout.fillWidth: true
                visible: host.result !== ""
                kind: "dim"
                wrapMode: Text.WordWrap
                text: host.result
            }
        }

        buttons: [
            WButton {
                objectName: "cancelJobButton"
                visible: host.stage === "running"
                enabled: host.jobId !== "" && !host.cancelling
                text: i18n("Cancel")
                onClicked: host.cancel()
            },
            WButton {
                objectName: "closeJobButton"
                visible: host.stage === "done"
                kind: "primary"
                text: i18n("Close")
                onClicked: host.close()
            }
        ]
    }
}
