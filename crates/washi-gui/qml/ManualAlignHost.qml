import QtQuick
import QtQuick.Layouts

// Manual print head alignment: for each of the printer's value sets (A-H, I-M, a-e) print the
// pattern sheet, read off the best square per letter, enter the numbers here and apply them.
// Printing goes through RunMaintenance (TestPrint manual_registration with the set's id); the
// values are written with ApplySettings (setregistration), which validates them against the
// printer's ranges and reads them back. Writing the values is unverified on a real printer.
Item {
    id: host

    // closed | intro | confirm | running | enter | applying | applied | failed
    property string stage: "closed"
    property var ids: []                    // the sets to go through, e.g. [1, 2, 3]
    property var chosen: ({ "1": true, "2": true, "3": true })
    property int pos: 0
    property string jobId: ""
    // Message sources (see Fmt.text()): what the dialog says while it works, and why it failed.
    property var progressSource: ""
    property var failureSource: ""
    readonly property string progress: Fmt.text(progressSource)
    readonly property string failure: Fmt.text(failureSource)
    property var values: ({})               // sign -> number, for the set in front of the user
    property var changes: []

    width: 0
    height: 0

    readonly property var ranges: Store.settings && Store.settings.allowed && Store.settings.allowed.setregistration ? Store.settings.allowed.setregistration : ({})
    readonly property var current: Store.settings && Store.settings.current && Store.settings.current.setregistration ? Store.settings.current.setregistration : ({})
    readonly property var allIds: Object.keys(ranges).map(k => parseInt(k)).sort((a, b) => a - b)
    readonly property int setId: pos < ids.length ? ids[pos] : 0
    readonly property var signs: setId > 0 && ranges[setId] ? Object.keys(ranges[setId]).sort() : []
    readonly property var testInfo: {
        const list = Store.maintCap && Store.maintCap.testprint ? Store.maintCap.testprint : [];
        return list.find(t => t.kind === "manual_registration") || null;
    }
    readonly property bool lastSet: pos >= ids.length - 1

    function signsOf(id) {
        return ranges[id] ? Object.keys(ranges[id]).sort() : [];
    }

    function setLabel(id) {
        if (id === undefined) {
            return "";
        }
        const s = signsOf(id);
        return s.length > 0 ? i18n("Set %1 (%2 to %3)", id, s[0], s[s.length - 1]) : i18n("Set %1", id);
    }

    function open() {
        chosen = ({});
        for (const id of allIds) {
            chosen[id] = true;
        }
        chosen = Object.assign({}, chosen);
        ids = [];
        pos = 0;
        failureSource = "";
        changes = [];
        stage = "intro";
        dlg.open();
    }

    function begin() {
        ids = allIds.filter(id => chosen[id]);
        pos = 0;
        if (ids.length === 0) {
            return;
        }
        stage = "confirm";
    }

    function paperLine() {
        const t = testInfo;
        if (!t || !t.papernum) {
            return "";
        }
        if (t.inputbin) {
            return i18n("The whole alignment uses %1 of %2 %3 from the %4. Load that paper first.", Fmt.sheets(t.papernum), Fmt.paperSize(t.papersize), Fmt.paperType(t.papertype), Fmt.trayName(t.inputbin));
        }
        return i18n("The whole alignment uses %1 of %2 %3. Load that paper first.", Fmt.sheets(t.papernum), Fmt.paperSize(t.papersize), Fmt.paperType(t.papertype));
    }

    // The user pressed "Print pattern": the only call that starts a job here.
    function printPattern() {
        if (stage !== "confirm") {
            return;
        }
        stage = "running";
        progressSource = () => i18n("Sending the job to the printer…");
        jobId = "";
        Store.runMaintenance({ op: "align_manual", id: setId }, (ok, body) => {
            if (stage !== "running") {
                return;
            }
            if (ok) {
                jobId = body;
            } else {
                fail(Fmt.errorSource(body));
            }
        });
    }

    function patternDone() {
        // Start the pickers from what the printer holds now.
        const v = {};
        for (const sign of signs) {
            const have = current[setId] ? current[setId][sign] : undefined;
            v[sign] = have !== undefined ? have : 0;
        }
        values = v;
        stage = "enter";
    }

    function setValue(sign, n) {
        const r = ranges[setId][sign];
        const v = Object.assign({}, values);
        v[sign] = Math.max(r.min, Math.min(r.max, n));
        values = v;
    }

    function apply() {
        if (stage !== "enter") {
            return;
        }
        stage = "applying";
        Store.applySettings({ setregistration: { [setId]: values } }, (ok, body) => {
            if (!ok) {
                fail(Fmt.errorSource(body));
                return;
            }
            const r = Store.parse(body);
            changes = r && r.changes ? r.changes : [];
            stage = "applied";
        });
    }

    function nextSet() {
        if (lastSet) {
            stage = "closed";
            dlg.close();
            return;
        }
        pos++;
        stage = "confirm";
    }

    function skipSet() {
        nextSet();
    }

    function fail(message) {
        failureSource = message;
        stage = "failed";
    }

    function cancel() {
        if (stage === "running" || stage === "applying") {
            return;
        }
        stage = "closed";
        dlg.close();
    }

    Connections {
        target: Store
        function onMaintenanceProgress(job, st, message) {
            if (host.stage === "running" && (host.jobId === "" || job === host.jobId)) {
                host.progressSource = message;
            }
        }
        function onMaintenanceFinished(job, success, message) {
            if (host.stage !== "running" || (host.jobId !== "" && job !== host.jobId)) {
                return;
            }
            if (success) {
                host.patternDone();
            } else {
                host.fail(message);
            }
        }
    }

    WDialog {
        id: dlg
        objectName: "manualAlignDialog"
        dialogWidth: 560
        dismissable: host.stage !== "running" && host.stage !== "applying"
        showClose: dismissable
        title: i18n("Manual Print Head Alignment")
        onClosed: if (host.stage !== "running" && host.stage !== "applying") host.stage = "closed"

        // ---- intro
        ColumnLayout {
            visible: host.stage === "intro"
            Layout.fillWidth: true
            spacing: 8
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: i18n("The printer prints a pattern sheet for each set of values. For every letter on a sheet, find the square with the least visible stripes and enter its number here. Washi then saves the numbers to the printer and reads them back.")
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: host.paperLine()
            }
            WText {
                Layout.topMargin: 4
                font.weight: Font.DemiBold
                text: i18n("Which sets?")
            }
            Repeater {
                model: host.allIds
                delegate: WCheck {
                    required property int modelData
                    objectName: "set-" + modelData
                    text: host.setLabel(modelData)
                    checked: !!host.chosen[modelData]
                    onToggled: {
                        const c = Object.assign({}, host.chosen);
                        c[modelData] = checked;
                        host.chosen = c;
                    }
                }
            }
            WText {
                Layout.fillWidth: true
                kind: "muted"
                wrapMode: Text.WordWrap
                text: i18n("Automatic alignment is easier and usually enough. Use this when the automatic one leaves the grids crooked.")
            }
        }

        // ---- confirm
        ColumnLayout {
            visible: host.stage === "confirm"
            Layout.fillWidth: true
            spacing: 8
            WText {
                font.weight: Font.DemiBold
                text: host.setLabel(host.setId)
            }
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: i18n("This prints the pattern sheet for this set with a little ink.")
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: host.paperLine()
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                text: i18n("Printer: %1", Store.displayName)
            }
        }

        // ---- running / applying
        ColumnLayout {
            visible: host.stage === "running" || host.stage === "applying"
            Layout.fillWidth: true
            spacing: 14
            Spinner {
                Layout.alignment: Qt.AlignHCenter
                size: 44
            }
            WText {
                objectName: "progressText"
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                text: host.stage === "applying" ? i18n("Saving the values and reading them back…") : host.progress
            }
        }

        // ---- enter the values
        ColumnLayout {
            visible: host.stage === "enter"
            Layout.fillWidth: true
            spacing: 8
            WText {
                font.weight: Font.DemiBold
                text: host.setLabel(host.setId)
            }
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("For each letter, enter the number of the square with the least visible stripes.")
            }
            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: 28
                rowSpacing: 8
                Repeater {
                    model: host.signs
                    delegate: RowLayout {
                        id: row
                        required property string modelData
                        readonly property var range: host.ranges[host.setId][modelData]
                        spacing: 6
                        WText {
                            Layout.preferredWidth: 18
                            font.weight: Font.Bold
                            text: row.modelData
                        }
                        WSpin {
                            objectName: "value-" + row.modelData
                            implicitWidth: 112
                            from: row.range.min
                            to: row.range.max
                            value: host.values[row.modelData] !== undefined ? host.values[row.modelData] : 0
                            onValueModified: host.setValue(row.modelData, value)
                        }
                        WText {
                            kind: "muted"
                            text: "%1…%2".arg(row.range.min).arg(row.range.max)
                        }
                    }
                }
            }
        }

        // ---- applied
        ColumnLayout {
            visible: host.stage === "applied"
            Layout.fillWidth: true
            spacing: 10
            RowLayout {
                spacing: 10
                Glyph {
                    name: "success"
                    size: 30
                    weight: 1.8
                    color: Theme.ok
                }
                WText {
                    Layout.fillWidth: true
                    objectName: "appliedText"
                    font.weight: Font.DemiBold
                    wrapMode: Text.WordWrap
                    text: host.changes.length > 0 ? i18n("The printer holds the new values.") : i18n("The printer already had these values; nothing was written.")
                }
            }
            WText {
                visible: !host.lastSet
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Next: %1", host.setLabel(host.ids[host.pos + 1]))
            }
            WText {
                visible: host.lastSet
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Print the alignment check sheet to see the result.")
            }
        }

        // ---- failed
        ColumnLayout {
            visible: host.stage === "failed"
            Layout.fillWidth: true
            spacing: 10
            RowLayout {
                spacing: 10
                Glyph {
                    name: "error"
                    size: 30
                    weight: 1.8
                    color: Theme.danger
                }
                WText {
                    Layout.fillWidth: true
                    font.weight: Font.DemiBold
                    text: i18n("It did not work.")
                }
            }
            WText {
                objectName: "failureText"
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: host.failure
            }
        }

        buttons: [
            WButton {
                visible: host.stage === "intro" || host.stage === "confirm" || host.stage === "enter" || host.stage === "failed"
                text: host.stage === "failed" ? i18n("Close") : i18n("Cancel")
                onClicked: host.cancel()
            },
            WButton {
                objectName: "introNext"
                visible: host.stage === "intro"
                kind: "primary"
                enabled: host.allIds.some(id => host.chosen[id]) && Store.canMaintain
                text: i18n("Next")
                onClicked: host.begin()
            },
            WButton {
                objectName: "printPattern"
                visible: host.stage === "confirm"
                kind: "primary"
                enabled: Store.canMaintain
                text: i18n("Print pattern")
                onClicked: host.printPattern()
            },
            WButton {
                objectName: "alreadyHavePattern"
                visible: host.stage === "confirm"
                text: i18n("I have the sheet")
                enabled: Store.canChange
                onClicked: host.patternDone()
            },
            WButton {
                objectName: "applyValues"
                visible: host.stage === "enter"
                kind: "primary"
                enabled: Store.canChange
                text: i18n("Apply")
                onClicked: host.apply()
            },
            WButton {
                objectName: "checkSheet"
                visible: host.stage === "applied" && host.lastSet
                text: i18n("Print Alignment Check")
                onClicked: {
                    host.cancel();
                    Nav.dialog("maintenance", { op: "align_check" });
                }
            },
            WButton {
                objectName: "appliedNext"
                visible: host.stage === "applied"
                kind: "primary"
                text: host.lastSet ? i18n("Done") : i18n("Next set")
                onClicked: host.nextSet()
            }
        ]
    }
}
