import QtQuick
import QtTest 1.2

// The optional Wake button, manual head alignment, the follow-ups after a nozzle check, the
// roller cleaning card (hidden unless the printer offers it), notification switches and
// removing a printer from the list.
Item {
    width: 1040
    height: 760

    FakeTransport {
        id: fake
    }

    WakeButton {
        id: wake
        objectName: "wake"
    }
    ManualAlignHost {
        id: align
    }
    MaintenanceHost {
        id: maint
    }
    Loader {
        id: loader
        x: 0
        y: 100
        width: 1000
        height: 600
    }
    Component {
        id: utilitiesComp
        UtilitiesTab {}
    }
    Component {
        id: settingsComp
        AppSettingsPage {}
    }
    Component {
        id: listComp
        PrinterListPage {}
    }

    TestCase {
        name: "WakeAndAlign"
        when: windowShown

        function boot(patch, readOnly) {
            loader.sourceComponent = null;
            align.cancel();
            maint.cancel();
            findChild(wake, "wakeConfirm").close();
            findChild(wake, "wakeFailure").close();
            wake.phase = "idle";
            wake.failureSource = "";
            Store.reset();
            Store.transport = null;
            fake.connected = false;
            fake.readonly = !!readOnly;
            fake.env = ({});
            const s = JSON.parse(JSON.stringify(fake.snapshot));
            for (const k in patch) {
                s[k] = patch[k];
            }
            fake.overrides = ({ GetSnapshot: [true, JSON.stringify(s)] });
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
            Store.loadCaps(true);
            tryVerify(() => Store.maintCap !== null && Store.settings !== null);
            fake.clearCalls();
        }

        // ---- wake ----------------------------------------------------------------------------
        function test_wake_is_shown_only_while_the_printer_sleeps() {
            boot({});
            verify(!wake.visible);
            boot({ power: "sleeping", ink_source: "ipp" });
            verify(wake.visible);
        }

        function test_wake_asks_first_and_sends_confirm() {
            boot({ power: "sleeping" });
            const inner = findChild(wake, "wakeInnerButton");
            verify(inner.enabled);
            inner.clicked();
            const dlg = findChild(wake, "wakeConfirm");
            verify(dlg.visible);
            compare(fake.called("Wake").length, 0, "nothing is sent before the answer");
            findChild(dlg, "acceptButton").clicked();
            tryVerify(() => fake.called("Wake").length === 1);
            compare(fake.called("Wake")[0].args[0], "printer.test");
            compare(JSON.parse(fake.called("Wake")[0].args[1]).confirm, true);
            tryVerify(() => wake.phase === "sent");
        }

        function test_a_wake_that_the_status_confirms_leaves_nothing_to_wait_for() {
            boot({ power: "sleeping" });
            fake.overrides = Object.assign({}, fake.overrides, { Wake: [true, JSON.stringify({ sent: true, verified: true, note: "the printer woke up" })] });
            findChild(wake, "wakeInnerButton").clicked();
            findChild(findChild(wake, "wakeConfirm"), "acceptButton").clicked();
            tryVerify(() => fake.called("Wake").length === 1);
            wait(100);
            compare(wake.phase, "idle", "a confirmed wake does not leave the button waiting");
            compare(wake.failure, "");
        }

        function test_wake_refused_in_read_only_mode() {
            boot({ power: "sleeping" }, true);
            const inner = findChild(wake, "wakeInnerButton");
            verify(!inner.enabled);
            let result = "";
            Store.wake((ok, body) => { result = body; });
            wait(100);
            compare(fake.called("Wake").length, 0);
            verify(result.indexOf("ReadOnly") >= 0);
        }

        function test_a_failed_wake_says_so() {
            boot({ power: "sleeping" });
            fake.overrides = Object.assign({}, fake.overrides, { Wake: [false, Store.errorBody("Printer", "the printer answered NG Suspended")] });
            findChild(wake, "wakeInnerButton").clicked();
            findChild(findChild(wake, "wakeConfirm"), "acceptButton").clicked();
            tryVerify(() => wake.failure !== "");
            verify(wake.failure.indexOf("Suspended") >= 0);
            compare(wake.phase, "idle");
        }

        // ---- maintenance and sleep -----------------------------------------------------------
        function test_maintenance_can_start_while_the_printer_sleeps() {
            boot({ power: "sleeping" });
            verify(Store.canMaintain);
            loader.sourceComponent = utilitiesComp;
            tryVerify(() => loader.item !== null);
            const card = findChild(loader.item, "card-nozzle");
            verify(card !== null);
            verify(findChild(card, "primaryButton").enabled, "Start is not disabled while asleep");
            maint.open({ op: "nozzle_check" });
            findChild(maint, "startButton").clicked();
            tryVerify(() => fake.called("RunMaintenance").length === 1);
            maint.cancel();
            fake.busSignal("MaintenanceFinished", "job-1", "true", "done");
            loader.sourceComponent = null;
        }

        function test_roller_cleaning_is_hidden_unless_the_printer_offers_it() {
            boot({});
            loader.sourceComponent = utilitiesComp;
            tryVerify(() => loader.item !== null);
            verify(findChild(loader.item, "card-roller") === null);
            loader.sourceComponent = null;
            const caps = JSON.parse(JSON.stringify(fake.caps));
            caps.roller_cleaning = true;
            fake.caps = caps;
            boot({});
            loader.sourceComponent = utilitiesComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => findChild(loader.item, "card-roller") !== null);
            caps.roller_cleaning = false;
            fake.caps = caps;
            loader.sourceComponent = null;
        }

        function test_a_crooked_pattern_leads_to_alignment() {
            boot({});
            maint.open({ op: "nozzle_check" });
            findChild(maint, "startButton").clicked();
            tryVerify(() => maint.jobId === "job-1");
            fake.busSignal("MaintenanceFinished", "job-1", "true", "nozzle check finished in 12s.");
            compare(maint.followup, "after-nozzle");
            verify(findChild(maint, "patternBad").visible, "missing or faint lines: cleaning");
            const crooked = findChild(maint, "patternCrooked");
            verify(crooked.visible, "crooked grids: print head alignment");
            crooked.clicked();
            compare(maint.stage, "confirm");
            compare(maint.spec.op, "align_auto");
            maint.cancel();
        }

        // ---- manual alignment ----------------------------------------------------------------
        function test_manual_alignment_goes_through_the_sets() {
            boot({});
            align.open();
            compare(align.stage, "intro");
            compare(align.allIds.join(","), "1,2,3");
            verify(align.paperLine().indexOf("3 sheets") >= 0, align.paperLine());
            // Skip set 2.
            const c = Object.assign({}, align.chosen);
            c["2"] = false;
            align.chosen = c;
            align.begin();
            compare(align.stage, "confirm");
            compare(align.ids.join(","), "1,3");
            compare(fake.called("RunMaintenance").length, 0, "no pattern before the user presses Print pattern");

            align.printPattern();
            compare(align.stage, "running");
            tryVerify(() => fake.called("RunMaintenance").length === 1);
            const spec = JSON.parse(fake.called("RunMaintenance")[0].args[1]);
            compare(spec.confirm, true);
            compare(spec.op, "align_manual");
            compare(spec.id, 1);
            tryVerify(() => align.jobId === "job-1");
            fake.busSignal("MaintenanceFinished", "job-1", "true", "manual alignment pattern 1 finished in 20s.");
            compare(align.stage, "enter");
            compare(align.signs.join(","), "A,B,C,D,E,F,G,H");
            compare(align.values.D, 1, "the pickers start from what the printer holds");

            // Values are clamped to the printer's ranges (G and H only reach 3).
            align.setValue("A", 9);
            compare(align.values.A, 5);
            align.setValue("G", -9);
            compare(align.values.G, -3);
            align.setValue("B", -2);
            align.apply();
            tryVerify(() => fake.called("ApplySettings").length === 1);
            const body = JSON.parse(fake.called("ApplySettings")[0].args[1]);
            compare(body.confirm, true);
            compare(JSON.stringify(Object.keys(body.setregistration)), '["1"]');
            compare(body.setregistration["1"].A, 5);
            compare(body.setregistration["1"].B, -2);
            compare(body.setregistration["1"].G, -3);
            compare(Object.keys(body.setregistration["1"]).length, 8, "the whole set is sent");
            tryVerify(() => align.stage === "applied");
            verify(!align.lastSet);
            align.nextSet();
            compare(align.setId, 3);
            compare(align.stage, "confirm");
            align.cancel();
        }

        function test_a_failed_pattern_or_apply_is_explained() {
            boot({});
            align.open();
            align.begin();
            align.printPattern();
            tryVerify(() => align.jobId === "job-1");
            fake.busSignal("MaintenanceFinished", "job-1", "false", "the printer reported support code 1300");
            compare(align.stage, "failed");
            verify(align.failure.indexOf("1300") >= 0);
            align.cancel();

            boot({});
            align.open();
            align.begin();
            align.patternDone();
            fake.overrides = Object.assign({}, fake.overrides, { ApplySettings: [false, Store.errorBody("InvalidArgs", "setregistration 1/A: 9 is outside -5..5")] });
            align.apply();
            tryVerify(() => align.stage === "failed");
            verify(align.failure.indexOf("outside") >= 0);
            align.cancel();
        }

        function test_manual_alignment_is_refused_in_read_only_mode() {
            boot({}, true);
            align.open();
            align.begin();
            align.printPattern();
            wait(100);
            compare(fake.called("RunMaintenance").length, 0);
            compare(align.stage, "failed");
            align.cancel();
        }

        // ---- notifications and the printer list ----------------------------------------------
        function test_notification_switches_use_the_service() {
            boot({});
            loader.sourceComponent = settingsComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.notifications !== null);
            const ink = findChild(loader.item, "notify-ink");
            verify(ink.checked);
            verify(ink.enabled);
            mouseClick(ink);               // the switch only asks; the state changes after the service says so
            tryVerify(() => fake.called("SetNotificationSettings").length > 0);
            compare(JSON.parse(fake.called("SetNotificationSettings")[0].args[0]).ink, false);
            tryVerify(() => !Store.notifications.ink);
            mouseClick(findChild(loader.item, "notify-enabled"));
            tryVerify(() => !findChild(loader.item, "notify-errors").enabled, 5000);
            compare(JSON.parse(fake.called("SetNotificationSettings")[1].args[0]).enabled, false);
            loader.sourceComponent = null;
            fake.notifications = { enabled: true, ink: true, errors: true, online: true };
        }

        function test_notification_switches_are_off_in_read_only_mode() {
            boot({}, true);
            loader.sourceComponent = settingsComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.notifications !== null);
            verify(!findChild(loader.item, "notify-ink").enabled);
            let result = "";
            Store.setNotifications({ ink: false }, (ok, body) => { result = body; });
            wait(100);
            compare(fake.called("SetNotificationSettings").length, 0);
            verify(result.indexOf("ReadOnly") >= 0);
            loader.sourceComponent = null;
        }

        function test_a_printer_can_be_removed_from_the_list() {
            boot({});
            loader.sourceComponent = listComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => findChild(loader.item, "removePrinter-0") !== null);
            findChild(loader.item, "removePrinter-0").clicked();
            const dlg = findChild(loader.item, "removeDialog");
            verify(dlg.visible);
            compare(fake.called("ForgetPrinter").length, 0, "asks first");
            findChild(dlg, "acceptButton").clicked();
            tryVerify(() => fake.called("ForgetPrinter").length === 1);
            compare(fake.called("ForgetPrinter")[0].args[0], "printer.test");
            loader.sourceComponent = null;
        }
    }
}
