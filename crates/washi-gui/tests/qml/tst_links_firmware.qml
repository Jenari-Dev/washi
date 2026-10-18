import QtQuick
import QtTest 1.2

// The printer's web links (asked first, whose page it is, nothing opens before "Open"), the
// support code on the Status tab, the firmware card and its update dialog, the extra cleaning
// cards and the tabs inside the banner.
Item {
    width: 1000
    height: 780

    FakeTransport {
        id: fake
    }

    ManagementPage {
        id: page
        anchors.fill: parent
    }
    LinkHost {
        id: links
    }
    FirmwareHost {
        id: firmware
    }
    MaintenanceHost {
        id: maint
    }

    // What Main.qml does with the requests of the pages.
    Connections {
        target: Nav
        function onDialogRequested(name, args) {
            switch (name) {
            case "open-link": links.open(args); break;
            case "order-ink": links.open({ kind: "ink" }); break;
            case "firmware": firmware.open(); break;
            case "maintenance": maint.open(args); break;
            }
        }
    }

    SignalSpy {
        id: opened
        target: Nav
        signalName: "urlOpened"
    }

    TestCase {
        name: "LinksAndFirmware"
        when: windowShown

        function boot(patch, opts) {
            opts = opts || ({});
            firmware.cancel();
            links.link = null;
            Nav.openBrowser = false;
            Nav.page = opts.tab || "status";
            Nav.history = [];
            Store.reset();
            Store.transport = null;
            fake.connected = false;
            fake.readonly = !!opts.readOnly;
            fake.overrides = ({});
            const s = JSON.parse(JSON.stringify(fake.snapshot));
            for (const k in (patch || {})) {
                s[k] = patch[k];
            }
            fake.overrides.GetSnapshot = [true, JSON.stringify(s)];
            if (opts.caps) {
                fake.overrides.GetMaintenanceCapability = [true, JSON.stringify(opts.caps)];
            }
            if (opts.webLinks) {
                fake.overrides.GetWebLinks = opts.webLinks;
            }
            fake.clearCalls();
            opened.clear();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
            Store.loadCaps(true);
            tryVerify(() => Store.maintCap !== null && Store.settings !== null);
        }

        function capsWith(types, roller) {
            const c = JSON.parse(JSON.stringify(fake.caps));
            for (const t of types) {
                c.cleaning.push({ kind: t, inkgroups: ["all"] });
            }
            c.roller_cleaning = !!roller;
            return c;
        }

        function popupOf(host) {
            for (const c of host.data) {
                if (c.visible !== undefined && c.opened !== undefined) {
                    return c;
                }
            }
            return null;
        }

        // ---- the web links ---------------------------------------------------------------------
        function test_order_ink_asks_first_and_opens_only_after_open() {
            boot({});
            Nav.dialog("order-ink");
            tryVerify(() => links.link !== null);
            compare(fake.called("GetWebLinks").length, 1);
            compare(JSON.stringify(fake.called("GetWebLinks")[0].args), JSON.stringify(["printer.test", ""]));
            const q = findChild(popupOf(links), "linkQuestion").text;
            verify(q.indexOf("Canon web page") >= 0 && q.indexOf("links.example.org") >= 0, q);
            verify(findChild(popupOf(links), "linkInkHint").visible, "the tank models to look for");
            verify(findChild(popupOf(links), "linkInkHint").text.indexOf("XKI-N21") >= 0);
            verify(!findChild(popupOf(links), "linkPasswordHint").visible);
            wait(100);
            compare(opened.count, 0, "nothing is opened before the person says so");
            findChild(popupOf(links), "acceptButton").clicked();
            compare(opened.count, 1);
            compare(opened.signalArguments[0][0], "https://links.example.org/ink?DEV=XK140+series&LNG=EN");
        }

        function test_cancelling_opens_nothing() {
            boot({});
            Nav.openLink("help");
            tryVerify(() => links.link !== null);
            verify(findChild(popupOf(links), "linkQuestion").text.indexOf("Canon web page") >= 0);
            popupOf(links).close();
            wait(100);
            compare(opened.count, 0);
        }

        function test_help_for_the_support_code_asks_with_the_code() {
            boot({ support: { code: "1300", message_key: "support.paper-jam", message: "x", severity: "error" } });
            const help = findChild(page, "supportHelpButton");
            verify(help !== null && help.visible);
            help.clicked();
            tryVerify(() => links.link !== null);
            compare(JSON.stringify(fake.called("GetWebLinks")[0].args), JSON.stringify(["printer.test", "1300"]));
            compare(links.kind, "error");
            compare(links.code, "1300");
            findChild(popupOf(links), "acceptButton").clicked();
            verify(String(opened.signalArguments[0][0]).endsWith("&ERR=1300"), opened.signalArguments[0][0]);
        }

        function test_links_the_printer_does_not_offer_are_its_own_page_and_say_so() {
            const own = { url: "http://printer.test/", source: "printer" };
            boot({}, { webLinks: [true, JSON.stringify({ ink: own, help: own, support: own, printer_page: own, maintenance_page: own, from_printer: false })] });
            Nav.openLink("help");
            tryVerify(() => links.link !== null);
            const q = findChild(popupOf(links), "linkQuestion").text;
            verify(q.indexOf("gave no address of its own") >= 0 && q.indexOf("printer.test") >= 0, q);
            compare(findChild(popupOf(links), "acceptButton").text, "Open Printer Page");
            findChild(popupOf(links), "acceptButton").clicked();
            compare(opened.signalArguments[0][0], "http://printer.test/");
        }

        function test_links_kept_from_the_last_time_the_printer_was_awake_say_so() {
            const kept = JSON.parse(JSON.stringify(fake.webLinks));
            kept.saved_at = 1791936000;
            boot({}, { webLinks: [true, JSON.stringify(kept)] });
            Nav.dialog("order-ink");
            tryVerify(() => links.link !== null);
            const hint = findChild(popupOf(links), "linkSavedHint");
            verify(hint.visible, "the copy is marked");
            verify(hint.text.indexOf("From the printer (saved ") === 0 && hint.text.indexOf("only while it is awake") >= 0, hint.text);
            verify(hint.text.indexOf("2026") >= 0 || hint.text.indexOf("26") >= 0, hint.text);
            popupOf(links).close();
            // A live read is not marked.
            boot({});
            Nav.dialog("order-ink");
            tryVerify(() => links.link !== null);
            verify(!findChild(popupOf(links), "linkSavedHint").visible);
        }

        function test_without_an_answer_from_the_service_the_printers_page_is_the_fallback() {
            boot({}, { webLinks: [false, JSON.stringify({ name: "NotRunning", message: "" })] });
            Nav.openLink("printer");
            tryVerify(() => links.link !== null);
            compare(links.link.url, "http://printer.test/");
            findChild(popupOf(links), "acceptButton").clicked();
            compare(opened.signalArguments[0][0], "http://printer.test/");
        }

        function test_the_status_tab_shows_a_support_code_with_its_words() {
            boot({ support: { code: "1000", message_key: "support.no-paper-rear", message: "x", severity: "warning" } });
            const tab = page;
            tryVerify(() => findChild(tab, "supportHelpButton") !== null);
            compare(Store.statusText, "Support code 1000");
            let found = false;
            for (const t of findChild(tab, "supportHelpButton").parent.parent.children) {
                if (t.text !== undefined && String(t.text).indexOf("Support code 1000") >= 0) {
                    found = true;
                }
            }
            verify(found, "the headline names the code");
            verify(!findChild(tab, "supportNotice").visible, "the code is the headline already");
            // A notice in a printer that is fine: the code is the headline, nothing is an error.
            boot({ support: { code: "D109", message_key: "support.notice", message: "x", severity: "info" } });
            tryVerify(() => findChild(page, "supportHelpButton") !== null && findChild(page, "supportHelpButton").visible);
            verify(!findChild(page, "supportNotice").visible);
            compare(Store.statusKind, "online");
            // A code that is only a warning next to an empty tank: the tank is the headline and the code a note.
            const inks = JSON.parse(JSON.stringify(fake.snapshot.inks));
            inks[0].status = "empty";
            boot({ inks: inks, support: { code: "1000", message_key: "support.no-paper-rear", message: "x", severity: "warning" } });
            tryVerify(() => findChild(page, "supportNotice") !== null && findChild(page, "supportNotice").visible);
            verify(findChild(page, "supportNotice").text.indexOf("1000") >= 0);
        }

        // ---- tabs in the banner ----------------------------------------------------------------
        function test_the_tabs_sit_in_the_bottom_of_the_banner() {
            boot({}, { tab: "consumables" });
            wait(150);
            const banner = page.children[0];       // the item that holds the banner and the tabs
            const tabs = ["consumables", "status", "device-info", "utilities"].map(id => findChild(page, "tab-" + id));
            let prev = -1;
            for (const t of tabs) {
                verify(t !== null);
                const p = t.mapToItem(page, 0, 0);
                verify(p.y >= banner.y + banner.height - 50 && p.y + t.height <= banner.y + banner.height + 1, "tab inside the bottom edge: " + p.y + " in " + banner.height);
                verify(p.x > prev, "side by side");
                prev = p.x;
            }
            compare(page.tabs.map(t => t.tint).filter((v, i, a) => a.indexOf(v) === i).length, 4, "every icon has its own colour");
            // A click still switches the tab.
            mouseClick(tabs[3]);
            compare(Nav.page, "utilities");
        }

        // ---- the firmware card -----------------------------------------------------------------
        function test_the_firmware_card_says_when_an_update_is_available() {
            boot({ firmware_update: "available" }, { tab: "device-info" });
            tryVerify(() => findChild(page, "firmwareCard") !== null);
            verify(findChild(page, "firmwareStatus").text.indexOf("update is available") >= 0);
            verify(findChild(page, "updateFirmwareButton").enabled);
            boot({ firmware_update: "unavailable" }, { tab: "device-info" });
            tryVerify(() => findChild(page, "firmwareCard") !== null);
            verify(findChild(page, "firmwareStatus").text.indexOf("up to date (1.060)") >= 0, findChild(page, "firmwareStatus").text);
            verify(!findChild(page, "updateFirmwareButton").enabled, "only when an update is available");
            boot({}, { tab: "device-info" });
            tryVerify(() => findChild(page, "firmwareCard") !== null);
            verify(findChild(page, "firmwareStatus").text.indexOf("does not say") >= 0);
            verify(!findChild(page, "updateFirmwareButton").enabled);
            boot({ firmware_update: "available" }, { tab: "device-info", readOnly: true });
            tryVerify(() => findChild(page, "firmwareCard") !== null);
            verify(!findChild(page, "updateFirmwareButton").enabled, "read-only");
        }

        function test_a_sleeping_printers_answer_is_marked_as_the_last_known() {
            // Asleep, with an offer remembered from the last time it was on.
            boot({ power: "sleeping", firmware_update: "available", firmware_update_remembered: true }, { tab: "device-info" });
            tryVerify(() => findChild(page, "firmwareCard") !== null);
            verify(findChild(page, "firmwareStatus").text.indexOf("was available the last time the printer was on") >= 0, findChild(page, "firmwareStatus").text);
            verify(!findChild(page, "updateFirmwareButton").enabled, "not on a remembered offer while asleep");
            // Asleep, up to date as of the last time it was on.
            boot({ power: "sleeping", firmware_update: "unavailable", firmware_update_remembered: true }, { tab: "device-info" });
            tryVerify(() => findChild(page, "firmwareCard") !== null);
            const t = findChild(page, "firmwareStatus").text;
            verify(t.indexOf("up to date the last time the printer was on") >= 0 && t.indexOf("only while it is on") >= 0, t);
            // Asleep and nothing known: it is not "up to date".
            boot({ power: "sleeping", firmware_update: "unknown" }, { tab: "device-info" });
            tryVerify(() => findChild(page, "firmwareCard") !== null);
            const u = findChild(page, "firmwareStatus").text;
            verify(u.indexOf("only while it is on") >= 0 && u.indexOf("up to date") < 0, u);
        }

        function test_check_for_updates_reads_the_printer_again() {
            boot({ firmware_update: "unavailable" }, { tab: "device-info" });
            tryVerify(() => findChild(page, "checkUpdatesButton") !== null);
            fake.clearCalls();
            findChild(page, "checkUpdatesButton").clicked();
            tryVerify(() => fake.called("Refresh").length === 1);
            compare(fake.called("UpdateFirmware").length, 0);
        }

        function test_the_firmware_update_needs_a_warning_read_and_ticked() {
            boot({ firmware_update: "available" }, { tab: "device-info" });
            tryVerify(() => findChild(page, "updateFirmwareButton") !== null);
            findChild(page, "updateFirmwareButton").clicked();
            compare(firmware.stage, "confirm");
            const dlg = popupOf(firmware);
            verify(findChild(dlg, "firmwareWarning").visible);
            verify(findChild(dlg, "firmwareWarning").text.indexOf("Do not switch the printer off") >= 0);
            const start = findChild(dlg, "startButton");
            verify(!start.enabled, "the box has to be ticked first");
            start.clicked();
            wait(100);
            compare(fake.called("UpdateFirmware").length, 0, "nothing is sent before the update is confirmed");
            const check = findChild(dlg, "understoodCheck");
            check.checked = true;
            check.toggled();
            verify(start.enabled);
            start.clicked();
            tryVerify(() => fake.called("UpdateFirmware").length === 1);
            compare(JSON.stringify(fake.called("UpdateFirmware")[0].args), JSON.stringify(["printer.test", JSON.stringify({ confirm: true })]));
            compare(firmware.stage, "running");
            tryVerify(() => firmware.jobId === "job-1");
            fake.busSignal("MaintenanceProgress", "job-1", "restarting", "The printer is restarting to finish the update.");
            compare(findChild(dlg, "progressText").text, "The printer is restarting to finish the update.");
            verify(!firmware.cancel() && firmware.stage === "running", "it cannot be closed while it runs");
            fake.busSignal("MaintenanceProgress", "job-9", "sent", "someone else's job");
            compare(findChild(dlg, "progressText").text, "The printer is restarting to finish the update.");
            fake.busSignal("MaintenanceFinished", "job-1", "true", "The firmware was updated from 1.060 to 1.061.");
            compare(firmware.stage, "done");
            verify(firmware.ok);
            compare(findChild(dlg, "resultDetail").text, "The firmware was updated from 1.060 to 1.061.");
            findChild(dlg, "doneButton").clicked();
            compare(firmware.stage, "closed");
        }

        function test_a_failed_firmware_update_is_reported() {
            boot({ firmware_update: "available" }, { tab: "device-info" });
            firmware.open();
            firmware.understood = true;
            fake.overrides.UpdateFirmware = [false, JSON.stringify({ name: "Printer", message: "The printer says its firmware is up to date." })];
            firmware.accept();
            tryVerify(() => firmware.stage === "done");
            verify(!firmware.ok);
            verify(firmware.result.indexOf("up to date") >= 0, firmware.result);
        }

        // ---- the Utilities cards -----------------------------------------------------------------
        function test_roller_and_bottom_plate_cleaning_come_from_the_cleaning_types() {
            boot({}, { tab: "utilities", caps: capsWith(["roller", "choke", "platen"]) });
            tryVerify(() => findChild(page, "card-roller") !== null);
            verify(findChild(page, "card-platen") !== null);
            verify(findChild(page, "card-choke") === null, "types Washi does not know get no card");
            compare(maint.cleaningKinds.map(c => c.kind).join(","), "regular,deep", "the cleaning options keep to the print head cleanings");
            // The roller card asks first, then sends Cleaning type=roller (no ink group).
            findChild(findChild(page, "card-roller"), "primaryButton").clicked();
            compare(maint.stage, "confirm");
            compare(JSON.stringify(maint.spec), JSON.stringify({ op: "clean", kind: "roller" }));
            compare(maint.titleFor(maint.spec), "Roller Cleaning");
            wait(100);
            compare(fake.called("RunMaintenance").length, 0);
            findChild(popupOf(maint), "startButton").clicked();
            tryVerify(() => fake.called("RunMaintenance").length === 1);
            compare(JSON.stringify(JSON.parse(fake.called("RunMaintenance")[0].args[1])), JSON.stringify({ confirm: true, op: "clean", kind: "roller" }));
            fake.busSignal("MaintenanceFinished", "job-1", "true", "done");
            compare(maint.followup, "", "no nozzle check is suggested after a roller cleaning");
            maint.cancel();
            // The bottom plate.
            findChild(findChild(page, "card-platen"), "primaryButton").clicked();
            compare(maint.titleFor(maint.spec), "Bottom Plate Cleaning");
            verify(maint.confirmText(maint.spec).indexOf("bottom plate") >= 0);
            maint.cancel();
        }

        function test_the_roller_cleaning_operation_still_works_and_the_type_wins() {
            boot({}, { tab: "utilities", caps: capsWith([], true) });
            tryVerify(() => findChild(page, "card-roller") !== null);
            verify(findChild(page, "card-platen") === null);
            findChild(findChild(page, "card-roller"), "primaryButton").clicked();
            compare(JSON.stringify(maint.spec), JSON.stringify({ op: "roller_clean" }));
            maint.cancel();
            boot({}, { tab: "utilities", caps: capsWith(["roller"], true) });
            tryVerify(() => findChild(page, "card-roller") !== null);
            findChild(findChild(page, "card-roller"), "primaryButton").clicked();
            compare(JSON.stringify(maint.spec), JSON.stringify({ op: "clean", kind: "roller" }), "one card, the cleaning type");
            maint.cancel();
        }

        function test_the_printers_maintenance_page_is_always_offered() {
            boot({}, { tab: "utilities" });
            tryVerify(() => findChild(page, "card-web-maintenance") !== null);
            verify(findChild(page, "card-roller") === null, "the XK140 has no roller card");
            const card = findChild(page, "card-web-maintenance");
            verify(card.title.indexOf("More maintenance on the printer's web page") === 0);
            verify(card.description.indexOf("administrator password") >= 0);
            const go = findChild(card, "primaryButton");
            verify(go.enabled);
            go.clicked();
            tryVerify(() => links.link !== null);
            compare(links.kind, "maintenance");
            const dlg = popupOf(links);
            verify(findChild(dlg, "linkPasswordHint").visible, "the password is explained in the question");
            compare(opened.count, 0);
            findChild(dlg, "acceptButton").clicked();
            compare(opened.signalArguments[0][0], "http://printer.test/index.html?page=PAGE_UTIL");
            // It only opens a page: also fine while the service is read-only.
            boot({}, { tab: "utilities", readOnly: true });
            tryVerify(() => findChild(page, "card-web-maintenance") !== null);
            verify(findChild(findChild(page, "card-web-maintenance"), "primaryButton").enabled);
            verify(!findChild(findChild(page, "card-cleaning"), "primaryButton").enabled);
        }
    }
}
