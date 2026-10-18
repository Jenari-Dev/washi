import QtQuick
import QtQuick.Layouts

// Edits one printer setting. Apply sends the change to the service (which validates, writes and
// reads the value back); the dialog then shows what the printer reports now.
Item {
    id: host

    // edit | working | done
    property string stage: "edit"
    // autopoweroff | quiet | dry_level | paper_gap (Prevent paper abrasion) | autopoweron | auto_firmware
    property string field: "quiet"
    property bool ok: false
    // Why the change was refused: a message source (see Fmt.text()).
    property var resultErrorSource: ""
    readonly property string resultError: Fmt.text(resultErrorSource)
    property var resultChanges: []

    // Working values
    property bool pwrOn: true
    property int pwrTime: 30
    property bool saOn: true
    property int saTime: 30
    property string quietMode: "OFF"
    property string quietStart: "21:00"
    property string quietEnd: "07:00"
    property int dryLevel: 3
    property bool toggleValue: false

    readonly property var allowed: Store.settings ? Store.settings.allowed : ({})
    readonly property var current: Store.settings ? Store.settings.current : ({})
    readonly property var pwrTimes: allowed.autopoweroff && allowed.autopoweroff.times ? allowed.autopoweroff.times : [15, 30, 60, 120, 240]
    readonly property var saTimes: allowed.autopoweroff_standalone && allowed.autopoweroff_standalone.times ? allowed.autopoweroff_standalone.times : pwrTimes
    readonly property bool hasStandalone: !!allowed.autopoweroff_standalone && !!allowed.autopoweroff_standalone.modes
    readonly property var quietModes: allowed.silentmode && allowed.silentmode.modes ? allowed.silentmode.modes : ["OFF", "ON", "time"]
    readonly property var dryLevels: allowed.drylevel && allowed.drylevel.length > 0 ? allowed.drylevel : [1, 2, 3, 4, 5]

    width: 0
    height: 0

    function titleFor(f) {
        switch (f) {
        case "autopoweroff": return i18n("Auto Power Off");
        case "quiet": return i18n("Quiet Mode");
        case "dry_level": return i18n("Ink Drying Time");
        case "paper_gap": return i18n("Prevent paper abrasion");
        case "autopoweron": return i18n("Auto Power On");
        case "auto_firmware": return i18n("Automatic Firmware Update");
        default: return i18n("Setting");
        }
    }

    // args: {field, value?}. A "value" (toggles) asks to switch it on or off.
    function open(args) {
        field = args.field;
        stage = "edit";
        resultErrorSource = "";
        resultChanges = [];
        const c = current || ({});
        const p = c.autopoweroff || ({});
        pwrOn = p.mode !== "OFF";
        pwrTime = p.time || pwrTimes[0];
        const s = c.autopoweroff_standalone || ({});
        saOn = s.mode !== "OFF";
        saTime = s.time || saTimes[0];
        const q = c.silentmode || ({});
        quietMode = q.mode || "OFF";
        quietStart = Fmt.hhmm(q.start) || "21:00";
        quietEnd = Fmt.hhmm(q.end) || "07:00";
        dryLevel = c.drylevel || 3;
        toggleValue = args.value !== undefined ? args.value : (field === "autopoweron" ? c.autopoweron !== "ON" : (field === "paper_gap" ? c.papergap !== "wide" : c.auto_printer_rom_update !== "ON"));
        dlg.open();
    }

    // The request body for ApplySettings (without "confirm"; the Store adds it).
    function changes() {
        switch (field) {
        case "autopoweroff": {
            const r = { auto_power_off: pwrOn ? pwrTime : "off" };
            if (hasStandalone) {
                r.auto_power_off_standalone = saOn ? saTime : "off";
            }
            return r;
        }
        case "quiet":
            if (quietMode === "time") {
                return { quiet: Fmt.unhhmm(quietStart) + "-" + Fmt.unhhmm(quietEnd) };
            }
            return { quiet: quietMode === "ON" ? "on" : "off" };
        case "dry_level": return { dry_level: dryLevel };
        case "paper_gap": return { paper_gap: toggleValue ? "wide" : "auto" };
        case "autopoweron": return { auto_power_on: toggleValue };
        case "auto_firmware": return { auto_firmware: toggleValue };
        }
        return ({});
    }

    readonly property bool valid: field !== "quiet" || quietMode !== "time" || (Fmt.clockValid(quietStart) && Fmt.clockValid(quietEnd))

    function apply() {
        if (stage !== "edit" || !valid) {
            return;
        }
        stage = "working";
        Store.applySettings(changes(), (success, body) => {
            ok = success;
            if (success) {
                const r = Store.parse(body) || ({});
                resultChanges = r.changes || [];
                resultErrorSource = "";
            } else {
                resultErrorSource = Fmt.errorSource(body);
            }
            stage = "done";
        });
    }

    function cancel() {
        dlg.close();
    }

    WDialog {
        id: dlg
        dialogWidth: 480
        dismissable: host.stage !== "working"
        showClose: host.stage !== "working"
        title: host.titleFor(host.field)

        // ---- edit: Auto Power Off
        ColumnLayout {
            visible: host.stage === "edit" && host.field === "autopoweroff"
            Layout.fillWidth: true
            spacing: 8
            WText {
                font.weight: Font.DemiBold
                text: host.hasStandalone ? i18n("When the printer is connected to a network") : i18n("Turn off automatically")
            }
            RowLayout {
                spacing: 12
                WSwitch {
                    objectName: "pwrSwitch"
                    checked: host.pwrOn
                    text: checked ? i18n("On") : i18n("Off")
                    onToggled: host.pwrOn = checked
                }
                WCombo {
                    objectName: "pwrTime"
                    textRole: "text"
                    valueRole: "value"
                    enabled: host.pwrOn
                    implicitWidth: 170
                    model: host.pwrTimes.map(t => ({ text: i18np("after %1 minute", "after %1 minutes", t), value: t }))
                    selectedValue: host.pwrTime
                    onActivated: host.pwrTime = currentValue
                }
            }
            WText {
                visible: host.hasStandalone
                Layout.topMargin: 10
                font.weight: Font.DemiBold
                text: i18n("When it is not connected to a network")
            }
            RowLayout {
                visible: host.hasStandalone
                spacing: 12
                WSwitch {
                    objectName: "saSwitch"
                    checked: host.saOn
                    text: checked ? i18n("On") : i18n("Off")
                    onToggled: host.saOn = checked
                }
                WCombo {
                    objectName: "saTime"
                    textRole: "text"
                    valueRole: "value"
                    enabled: host.saOn
                    implicitWidth: 170
                    model: host.saTimes.map(t => ({ text: i18np("after %1 minute", "after %1 minutes", t), value: t }))
                    selectedValue: host.saTime
                    onActivated: host.saTime = currentValue
                }
            }
        }

        // ---- edit: Quiet Mode
        ColumnLayout {
            visible: host.stage === "edit" && host.field === "quiet"
            Layout.fillWidth: true
            spacing: 4
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Quiet mode lowers printing noise by printing more slowly.")
            }
            WRadio {
                objectName: "quietOff"
                visible: host.quietModes.indexOf("OFF") >= 0
                text: i18n("Off")
                checked: host.quietMode === "OFF"
                onClicked: host.quietMode = "OFF"
            }
            WRadio {
                objectName: "quietOn"
                visible: host.quietModes.indexOf("ON") >= 0
                text: i18n("On")
                checked: host.quietMode === "ON"
                onClicked: host.quietMode = "ON"
            }
            WRadio {
                objectName: "quietTime"
                visible: host.quietModes.indexOf("time") >= 0
                text: i18n("Scheduled")
                checked: host.quietMode === "time"
                onClicked: host.quietMode = "time"
            }
            RowLayout {
                visible: host.quietMode === "time"
                Layout.leftMargin: 28
                spacing: 10
                WText {
                    text: i18n("From")
                }
                WField {
                    objectName: "quietStart"
                    implicitWidth: 80
                    text: host.quietStart
                    inputMask: "99:99"
                    horizontalAlignment: TextInput.AlignHCenter
                    onTextEdited: host.quietStart = text
                }
                WText {
                    text: i18n("to")
                }
                WField {
                    objectName: "quietEnd"
                    implicitWidth: 80
                    text: host.quietEnd
                    inputMask: "99:99"
                    horizontalAlignment: TextInput.AlignHCenter
                    onTextEdited: host.quietEnd = text
                }
            }
            WText {
                visible: host.quietMode === "time" && !host.valid
                Layout.leftMargin: 28
                color: Theme.danger
                kind: "small"
                text: i18n("Enter times as 24-hour HH:MM, for example 21:00.")
            }
        }

        // ---- edit: Ink drying time
        ColumnLayout {
            visible: host.stage === "edit" && host.field === "dry_level"
            Layout.fillWidth: true
            spacing: 8
            WText {
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("A longer wait lets the ink dry between pages, which helps if prints smear. Printing takes longer.")
            }
            WSlider {
                objectName: "drySlider"
                Layout.fillWidth: true
                from: host.dryLevels[0]
                to: host.dryLevels[host.dryLevels.length - 1]
                stepSize: 1
                value: host.dryLevel
                onMoved: host.dryLevel = Math.round(value)
            }
            RowLayout {
                Layout.fillWidth: true
                WText {
                    kind: "small"
                    text: i18n("Shorter")
                }
                Item {
                    Layout.fillWidth: true
                }
                WText {
                    objectName: "dryValue"
                    font.weight: Font.DemiBold
                    text: i18n("Level %1", host.dryLevel)
                }
                Item {
                    Layout.fillWidth: true
                }
                WText {
                    kind: "small"
                    text: i18n("Longer")
                }
            }
        }

        // ---- edit: on/off switches
        ColumnLayout {
            visible: host.stage === "edit" && (host.field === "autopoweron" || host.field === "auto_firmware" || host.field === "paper_gap")
            Layout.fillWidth: true
            spacing: 8
            WText {
                visible: host.field === "paper_gap"
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("Raises the print head a little so that thick or curled paper is not scuffed. Printing can be slower.")
            }
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: host.toggleValue
                    ? i18n("Turn on “%1” for %2?", host.titleFor(host.field), Store.displayName)
                    : i18n("Turn off “%1” for %2?", host.titleFor(host.field), Store.displayName)
            }
        }

        // ---- working
        RowLayout {
            visible: host.stage === "working"
            Layout.fillWidth: true
            spacing: 14
            Spinner {
                size: 28
            }
            WText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: i18n("Saving to the printer and reading it back…")
            }
        }

        // ---- done
        ColumnLayout {
            visible: host.stage === "done"
            Layout.fillWidth: true
            spacing: 8
            RowLayout {
                spacing: 10
                Glyph {
                    name: host.ok ? "success" : "error"
                    size: 28
                    weight: 1.8
                    color: host.ok ? Theme.ok : Theme.danger
                }
                WText {
                    objectName: "settingResult"
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    font.weight: Font.DemiBold
                    text: !host.ok ? i18n("The printer did not accept the change.")
                        : (host.resultChanges.length === 0 ? i18n("Nothing changed: the printer already had these values.") : i18n("Saved. The printer now reports:"))
                }
            }
            Repeater {
                model: host.resultChanges
                delegate: WText {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.leftMargin: 38
                    wrapMode: Text.WordWrap
                    text: i18n("%1: %2  →  %3", Fmt.settingLabel(modelData.field), Fmt.settingValueFor(modelData.field, modelData.before), Fmt.settingValueFor(modelData.field, modelData.after))
                }
            }
            WText {
                visible: host.resultError !== ""
                Layout.fillWidth: true
                Layout.leftMargin: 38
                color: Theme.danger
                wrapMode: Text.WordWrap
                text: host.resultError
            }
        }

        buttons: [
            WButton {
                visible: host.stage === "edit"
                text: i18n("Cancel")
                onClicked: host.cancel()
            },
            WButton {
                objectName: "applyButton"
                visible: host.stage === "edit"
                kind: "primary"
                text: i18n("Apply")
                enabled: host.valid
                onClicked: host.apply()
            },
            WButton {
                objectName: "closeButton"
                visible: host.stage === "done"
                kind: "primary"
                text: i18n("Close")
                onClicked: host.cancel()
            }
        ]
    }
}
