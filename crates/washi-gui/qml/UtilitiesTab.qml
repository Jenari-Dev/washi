import QtQuick
import QtQuick.Layouts

// Capability-driven maintenance and settings cards: only what the printer reports is shown.
PageScroll {
    id: tab

    readonly property var cap: Store.maintCap
    readonly property var allowed: Store.settings ? Store.settings.allowed : null
    readonly property var cur: Store.settings ? Store.settings.current : null
    readonly property bool canMaintain: Store.canMaintain
    readonly property bool canChange: Store.canChange
    readonly property bool known: cap !== null || allowed !== null

    // The manual alignment values the printer accepts ({id: {sign: {min, max}}}), or null.
    readonly property var manualRanges: allowed && allowed.setregistration && Object.keys(allowed.setregistration).length > 0 ? allowed.setregistration : null

    function hasTest(kind) {
        return !!cap && !!cap.testprint && cap.testprint.some(t => t.kind === kind);
    }

    function hasCleaningType(kind) {
        return !!cap && !!cap.cleaning && cap.cleaning.some(c => c.kind === kind);
    }

    function powerText(p) {
        if (!p || !p.mode) {
            return "";
        }
        return p.mode === "ON" ? i18np("After %1 minute", "After %1 minutes", p.time || 0) : i18n("Off");
    }

    function quietText(q) {
        if (!q || !q.mode) {
            return "";
        }
        if (q.mode === "ON") {
            return i18n("On");
        }
        if (q.mode === "time") {
            return i18n("Scheduled %1 – %2", Fmt.hhmm(q.start), Fmt.hhmm(q.end));
        }
        return i18n("Off");
    }

    function buildCards() {
        const out = [];
        const c = cur || ({});
        const a = allowed || ({});
        if (cap && cap.cleaning && cap.cleaning.length > 0) {
            out.push({ id: "cleaning", glyph: "cleaning", title: i18n("Cleaning"), desc: i18n("Clears clogged nozzles to get rid of streaks and faded colours."), kind: "action", op: "clean" });
        }
        if (hasTest("nozzle_check")) {
            out.push({ id: "nozzle", glyph: "nozzle", title: i18n("Nozzle Check"), desc: i18n("Prints a test pattern so you can see whether any nozzle is blocked."), kind: "action", op: "nozzle_check" });
        }
        if (hasTest("auto_registration")) {
            out.push({ id: "align", glyph: "align", title: i18n("Print Head Alignment"), desc: i18n("Corrects shifted colours and wavy lines by realigning the print head."), kind: "action", op: "align_auto", secondary: hasTest("regi_check") ? { text: i18n("Alignment check"), op: "align_check" } : null, tertiary: hasTest("manual_registration") && manualRanges ? { text: i18n("Manual…") } : null });
        }
        // Models differ: some list roller / bottom plate cleaning as `Cleaning` types, some a
        // `RollerCleaning` operation. Types Washi does not know (e.g. "choke") get no card.
        if (hasCleaningType("roller")) {
            out.push({ id: "roller", glyph: "roller", title: i18n("Roller Cleaning"), desc: i18n("Cleans the paper feed rollers when sheets slip or smudge."), kind: "action", op: "clean", cleanKind: "roller" });
        } else if (cap && cap.roller_cleaning === true) {
            out.push({ id: "roller", glyph: "roller", title: i18n("Roller Cleaning"), desc: i18n("Cleans the paper feed rollers when sheets slip or smudge."), kind: "action", op: "roller_clean" });
        }
        if (hasCleaningType("platen")) {
            out.push({ id: "platen", glyph: "platen", title: i18n("Bottom Plate Cleaning"), desc: i18n("Cleans the inside of the printer so that ink does not smear the back of the paper."), kind: "action", op: "clean", cleanKind: "platen" });
        }
        if (a.autopoweron && a.autopoweron.length > 0) {
            out.push({ id: "autopoweron", glyph: "power", title: i18n("Auto Power On"), desc: i18n("Switches the printer on by itself when a print job arrives."), kind: "toggle", field: "autopoweron", checked: c.autopoweron === "ON" });
        }
        if ((a.autopoweroff && a.autopoweroff.modes) || (a.autopoweroff_standalone && a.autopoweroff_standalone.modes)) {
            let cur2 = powerText(c.autopoweroff);
            if (c.autopoweroff_standalone && a.autopoweroff_standalone) {
                cur2 += (cur2 !== "" ? "  ·  " : "") + i18n("Not connected: %1", powerText(c.autopoweroff_standalone));
            }
            out.push({ id: "autopoweroff", glyph: "timer", title: i18n("Auto Power Off"), desc: i18n("Turns the printer off after it has been idle for a while."), kind: "settings", field: "autopoweroff", current: cur2 });
        }
        if (a.silentmode && a.silentmode.modes && a.silentmode.modes.length > 0) {
            out.push({ id: "quiet", glyph: "quiet", title: i18n("Quiet Mode"), desc: i18n("Makes printing quieter, all the time or during set hours. Printing gets slower."), kind: "settings", field: "quiet", current: quietText(c.silentmode) });
        }
        if (a.drylevel && a.drylevel.length > 0) {
            out.push({ id: "dry", glyph: "hourglass", title: i18n("Ink Drying Time"), desc: i18n("Adds a pause between pages so the ink can dry before the next one lands on it."), kind: "settings", field: "dry_level", current: c.drylevel ? i18n("Level %1", c.drylevel) : "" });
        }
        if (a.papergap && a.papergap.length > 0) {
            out.push({ id: "gap", glyph: "gap", title: i18n("Paper Gap"), desc: i18n("Widens the space between print head and paper to avoid scuffs on thick paper."), kind: "settings", field: "paper_gap", current: c.papergap === "wide" ? i18n("Wide") : (c.papergap ? i18n("Automatic") : "") });
        }
        if (a.auto_printer_rom_update && a.auto_printer_rom_update.length > 0) {
            out.push({ id: "firmware", glyph: "firmware", title: i18n("Automatic Firmware Update"), desc: i18n("Lets the printer install new firmware by itself when it is available."), kind: "toggle", field: "auto_firmware", checked: c.auto_printer_rom_update === "ON" });
        }
        // Always there: the printer's own web page has more (it asks for its admin password).
        out.push({ id: "web-maintenance", glyph: "globe", title: i18n("More maintenance on the printer's web page"), desc: i18n("Some maintenance is only on the printer's own web page. The printer asks for its administrator password there."), kind: "link", link: "maintenance", buttonText: i18n("Open…") });
        return out;
    }

    readonly property var cards: buildCards()

    Component.onCompleted: Store.loadCaps(false)
    Connections {
        target: Store
        function onActiveHostChanged() {
            Store.loadCaps(false);
        }
        function onPowerChanged() {
            if (Store.power === "on" || Store.power === "sleeping") {
                Store.loadCaps(true);
            }
        }
    }

    NoPrinter {
        visible: !Store.hasPrinter
        Layout.fillWidth: true
        Layout.preferredHeight: 320
    }

    ColumnLayout {
        visible: Store.hasPrinter
        Layout.fillWidth: true
        spacing: 14

        InfoBar {
            Layout.fillWidth: true
            visible: Store.power === "sleeping"
            kind: "info"
            text: i18n("The printer is asleep. Cleaning and test prints wake it up by themselves, and settings can still be changed. You can also wake it first.")
            WakeButton {
                objectName: "wakeButton"
            }
        }
        InfoBar {
            Layout.fillWidth: true
            visible: Store.power === "offline"
            kind: "warning"
            text: i18n("The printer cannot be reached, so nothing can be started right now.")
        }
        InfoBar {
            Layout.fillWidth: true
            visible: Store.capsFromCache && Store.power !== "offline"
            kind: "info"
            text: i18n("Showing what the printer reported the last time it answered.")
        }
        InfoBar {
            Layout.fillWidth: true
            visible: !tab.known && !Store.capsLoading && Store.capsError !== ""
            kind: "warning"
            text: i18n("Could not read what this printer offers: %1", Store.capsError)
            WButton {
                text: i18n("Try again")
                onClicked: Store.loadCaps(true)
            }
        }
        WText {
            visible: !tab.known && Store.capsLoading
            kind: "dim"
            text: i18n("Reading what the printer offers…")
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            rowSpacing: 14
            columnSpacing: 14
            uniformCellWidths: true

            Repeater {
                model: tab.cards
                delegate: UtilityCard {
                    id: uc
                    required property var modelData
                    objectName: "card-" + modelData.id
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    glyph: modelData.glyph
                    title: modelData.title
                    description: modelData.desc
                    current: modelData.current || ""
                    kind: modelData.kind
                    checked: !!modelData.checked
                    buttonText: modelData.buttonText !== undefined ? modelData.buttonText : (modelData.kind === "action" ? i18n("Start") : i18n("Settings"))
                    secondaryText: modelData.secondary ? modelData.secondary.text : ""
                    tertiaryText: modelData.tertiary ? modelData.tertiary.text : ""
                    actionEnabled: modelData.kind === "link" ? Store.hasPrinter : (modelData.kind === "action" ? tab.canMaintain : tab.canChange)
                    hint: !actionEnabled && Store.readOnly && modelData.kind !== "link" ? i18n("Read-only mode") : ""

                    onActivated: {
                        if (modelData.kind === "link") {
                            Nav.openLink(modelData.link);
                        } else if (modelData.kind === "action") {
                            Nav.dialog("maintenance", { op: modelData.op, kind: modelData.cleanKind });
                        } else {
                            Nav.dialog("setting", { field: modelData.field });
                        }
                    }
                    onSecondary: Nav.dialog("maintenance", { op: modelData.secondary.op })
                    onTertiary: Nav.dialog("manual-align", {})
                    onToggled: on => Nav.dialog("setting", { field: modelData.field, value: on })
                }
            }
        }
    }
}
