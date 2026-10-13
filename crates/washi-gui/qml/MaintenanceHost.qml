import QtQuick
import QtQuick.Layouts

// The whole maintenance flow in one dialog: (cleaning options) -> confirmation that says what
// it will use up -> progress -> result with follow-up hints. The only place that starts a
// maintenance job, and it does so only from accept(), i.e. after the user pressed Start.
Item {
    id: host

    // closed | options | confirm | running | done
    property string stage: "closed"
    property var spec: ({ op: "nozzle_check" })
    property string jobId: ""
    property bool ok: false
    // What the dialog says while it works and when it is over; message sources (see Fmt.text()).
    property var progressSource: ""
    property var resultSource: ""
    readonly property string progress: Fmt.text(progressSource)
    readonly property string result: Fmt.text(resultSource)
    // Cleaning kind used last in this session of the dialog (to suggest a deep clean next).
    property string lastCleanKind: ""
    property string followup: ""         // after-clean | after-nozzle | after-align | ""
    property var finishedSpec: ({})
    // Choices in the cleaning options stage.
    property string cleanKind: "regular"
    property string cleanGroup: "all"

    readonly property bool visibleNow: dlg.visible

    width: 0
    height: 0

    // ---- capability lookups ----------------------------------------------------------------
    // The print head cleanings the options page offers; the roller and the bottom plate are cards
    // of their own (the printer may list other types too, which Washi does not know).
    readonly property var cleaningKinds: (Store.maintCap && Store.maintCap.cleaning ? Store.maintCap.cleaning : []).filter(c => c.kind === "regular" || c.kind === "deep")
    readonly property var cleaningInfo: cleaningKinds.find(c => c.kind === cleanKind) || null
    readonly property var groups: cleaningInfo && cleaningInfo.inkgroups ? cleaningInfo.inkgroups : []

    function testKind(op) {
        switch (op) {
        case "nozzle_check": return "nozzle_check";
        case "align_auto": return "auto_registration";
        case "align_check": return "regi_check";
        default: return "";
        }
    }

    function testInfo(op) {
        const k = testKind(op);
        const list = Store.maintCap && Store.maintCap.testprint ? Store.maintCap.testprint : [];
        return list.find(t => t.kind === k) || null;
    }

    function titleFor(s) {
        switch (s.op) {
        case "clean":
            if (s.kind === "roller") {
                return i18n("Roller Cleaning");
            }
            return s.kind === "platen" ? i18n("Bottom Plate Cleaning") : (s.kind === "deep" ? i18n("Deep Cleaning") : i18n("Cleaning"));
        case "nozzle_check": return i18n("Nozzle Check");
        case "align_auto": return i18n("Print Head Alignment");
        case "align_check": return i18n("Alignment Check");
        case "roller_clean": return i18n("Roller Cleaning");
        default: return i18n("Maintenance");
        }
    }

    // What the user is told before anything starts: paper and ink used.
    function paperLine(op) {
        const t = testInfo(op);
        if (!t || !t.papernum) {
            return "";
        }
        if (t.inputbin) {
            return i18n("It prints on %1 of %2 %3 from the %4. Load that paper first.", Fmt.sheets(t.papernum), Fmt.paperSize(t.papersize), Fmt.paperType(t.papertype), Fmt.trayName(t.inputbin));
        }
        return i18n("It prints on %1 of %2 %3. Load that paper first.", Fmt.sheets(t.papernum), Fmt.paperSize(t.papersize), Fmt.paperType(t.papertype));
    }

    function confirmText(s) {
        switch (s.op) {
        case "clean":
            if (s.kind === "roller") {
                return i18n("The printer runs its paper feed rollers to clean them. It may ask you to take out or load paper: follow its display.");
            }
            if (s.kind === "platen") {
                return i18n("The printer cleans the inside of its bottom plate, where ink can collect and smear the back of the paper. It may ask for a sheet of paper: follow its display.");
            }
            return s.kind === "deep"
                ? i18n("Deep cleaning uses a lot more ink than normal cleaning and takes longer. Use it only when normal cleaning did not help.")
                : i18n("Cleaning uses a little ink.");
        case "nozzle_check":
            return Fmt.sentences(i18n("This prints a test pattern with a small amount of ink."), paperLine(s.op));
        case "align_auto":
            return Fmt.sentences(i18n("The printer prints alignment patterns, reads them and adjusts the print head by itself. This uses ink and paper."), paperLine(s.op));
        case "align_check":
            return Fmt.sentences(i18n("This prints a sheet that shows how well the print head is aligned, using a little ink."), paperLine(s.op));
        case "roller_clean":
            return i18n("The printer runs its paper feed rollers to clean them.");
        default:
            return "";
        }
    }

    function groupName(g) {
        switch (g) {
        case "all": return i18n("All colours");
        case "group1": return i18n("Group 1");
        case "group2": return i18n("Group 2");
        default: return g;
        }
    }

    // ---- flow ------------------------------------------------------------------------------
    // Start a flow. For "clean" the options come first; the rest go straight to the confirmation.
    function open(s) {
        resultSource = "";
        progressSource = "";
        jobId = "";
        followup = "";
        if (s.op === "clean" && (s.kind === "roller" || s.kind === "platen")) {
            spec = ({ op: "clean", kind: s.kind });
            stage = "confirm";
        } else if (s.op === "clean") {
            cleanKind = s.kind || (lastCleanKind === "regular" && s.suggestDeep ? "deep" : "regular");
            if (cleaningKinds.length > 0 && !cleaningKinds.some(c => c.kind === cleanKind)) {
                cleanKind = cleaningKinds[0].kind;
            }
            cleanGroup = s.group || "all";
            stage = "options";
        } else {
            spec = ({ op: s.op });
            stage = "confirm";
        }
        dlg.open();
    }

    function nextFromOptions() {
        const s = { op: "clean", kind: cleanKind };
        if (groups.length > 1 || cleanGroup !== "all") {
            s.group = cleanGroup;
        }
        spec = s;
        stage = "confirm";
    }

    // The user pressed Start. This is the only call that starts a job.
    function accept() {
        if (stage !== "confirm") {
            return;
        }
        const s = Object.assign({}, spec);
        finishedSpec = s;
        stage = "running";
        progressSource = () => i18n("Sending the job to the printer…");
        ok = false;
        jobId = "";
        Store.runMaintenance(s, (done, body) => {
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
        const s = finishedSpec;
        followup = "";
        if (success && s.op === "clean" && s.kind !== "roller" && s.kind !== "platen") {
            lastCleanKind = s.kind || "regular";
            followup = hasTest("nozzle_check") ? "after-clean" : "";
        } else if (success && s.op === "nozzle_check") {
            followup = "after-nozzle";
        } else if (success && s.op === "align_auto") {
            followup = hasTest("regi_check") ? "after-align" : "";
        }
    }

    function hasTest(kind) {
        const list = Store.maintCap && Store.maintCap.testprint ? Store.maintCap.testprint : [];
        return list.some(t => t.kind === kind);
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
        dialogWidth: 500
        dismissable: host.stage !== "running"
        showClose: host.stage !== "running"
        title: host.stage === "options" ? i18n("Cleaning") : host.titleFor(host.stage === "done" ? host.finishedSpec : host.spec)
        onClosed: if (host.stage !== "running") host.stage = "closed"

        // ---- options (cleaning only)
        ColumnLayout {
            visible: host.stage === "options"
            Layout.fillWidth: true
            spacing: 6
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Choose how to clean the print head.")
            }
            // Each radio set has its own parent so the sets do not exclude each other.
            ColumnLayout {
                spacing: 0
                Repeater {
                    model: host.cleaningKinds
                    delegate: WRadio {
                        required property var modelData
                        objectName: "kind-" + modelData.kind
                        text: modelData.kind === "deep" ? i18n("Deep Cleaning (uses more ink)") : i18n("Cleaning")
                        checked: host.cleanKind === modelData.kind
                        onClicked: host.cleanKind = modelData.kind
                    }
                }
            }
            WText {
                visible: host.groups.length > 1
                Layout.topMargin: 8
                font.weight: Font.DemiBold
                text: i18n("Ink group")
            }
            ColumnLayout {
                spacing: 0
                Repeater {
                    model: host.groups.length > 1 ? host.groups : []
                    delegate: WRadio {
                        required property var modelData
                        objectName: "group-" + modelData
                        text: host.groupName(modelData)
                        checked: host.cleanGroup === modelData
                        onClicked: host.cleanGroup = modelData
                    }
                }
            }
        }

        // ---- confirm
        ColumnLayout {
            visible: host.stage === "confirm"
            Layout.fillWidth: true
            spacing: 10
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: host.confirmText(host.spec)
            }
            WText {
                visible: host.spec.op === "clean" && !!host.spec.group && host.spec.group !== "all"
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Ink group: %1", host.groupName(host.spec.group || "all"))
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Printer: %1", Store.displayName)
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
            WText {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                kind: "muted"
                wrapMode: Text.WordWrap
                text: i18n("Please keep the printer switched on and its covers closed until this finishes.")
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
                Layout.fillWidth: true
                visible: host.result !== ""
                kind: "dim"
                wrapMode: Text.WordWrap
                text: host.result
            }
            WText {
                objectName: "followupText"
                Layout.fillWidth: true
                Layout.topMargin: 6
                visible: host.followup !== ""
                wrapMode: Text.WordWrap
                text: {
                    switch (host.followup) {
                    case "after-clean": return i18n("Print a nozzle check to see whether the cleaning helped.");
                    case "after-nozzle": return hasTest("auto_registration")
                        ? i18n("Look at the printed pattern. Is every line complete, in every colour, and are the grids straight? Missing or faint lines call for a cleaning; crooked or shifted grids call for a print head alignment.")
                        : i18n("Look at the printed pattern. Is every line complete, in every colour?");
                    case "after-align": return i18n("You can print the alignment check sheet to see the result.");
                    default: return "";
                    }
                }
            }
        }

        buttons: [
            // options
            WButton {
                visible: host.stage === "options"
                text: i18n("Cancel")
                onClicked: host.cancel()
            },
            WButton {
                objectName: "nextButton"
                visible: host.stage === "options"
                kind: "primary"
                text: i18n("Next")
                onClicked: host.nextFromOptions()
            },
            // confirm
            WButton {
                visible: host.stage === "confirm"
                text: i18n("Cancel")
                onClicked: host.cancel()
            },
            WButton {
                objectName: "startButton"
                visible: host.stage === "confirm"
                kind: "primary"
                text: i18n("Start")
                onClicked: host.accept()
            },
            // done: follow-ups
            WButton {
                objectName: "patternBad"
                visible: host.stage === "done" && host.followup === "after-nozzle"
                text: i18n("Lines missing or faint")
                onClicked: host.open({ op: "clean", suggestDeep: true })
            },
            WButton {
                objectName: "patternCrooked"
                visible: host.stage === "done" && host.followup === "after-nozzle" && host.hasTest("auto_registration")
                text: i18n("Lines crooked or shifted")
                onClicked: host.open({ op: "align_auto" })
            },
            WButton {
                objectName: "followNozzle"
                visible: host.stage === "done" && host.followup === "after-clean"
                text: i18n("Print Nozzle Check")
                onClicked: host.open({ op: "nozzle_check" })
            },
            WButton {
                objectName: "followCheck"
                visible: host.stage === "done" && host.followup === "after-align"
                text: i18n("Print Alignment Check")
                onClicked: host.open({ op: "align_check" })
            },
            WButton {
                objectName: "doneButton"
                visible: host.stage === "done"
                kind: host.followup === "after-nozzle" ? "primary" : "secondary"
                text: host.followup === "after-nozzle" ? i18n("Looks good") : i18n("Close")
                onClicked: host.cancel()
            }
        ]
    }
}
