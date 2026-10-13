import QtQuick
import QtTest 1.2

// How a snapshot turns into the status shown in the banner and what may be started.
Item {
    width: 400
    height: 300

    FakeTransport {
        id: fake
    }

    TestCase {
        name: "StoreStatus"
        when: windowShown

        property var defaultPrinters: null

        function load(patch) {
            if (defaultPrinters === null) {
                defaultPrinters = fake.printers;
            }
            Store.reset();
            Store.transport = null;
            fake.connected = false;
            fake.readonly = false;
            const s = JSON.parse(JSON.stringify(fake.snapshot));
            for (const k in patch) {
                s[k] = patch[k];
            }
            fake.overrides = ({ GetSnapshot: [true, JSON.stringify(s)] });
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
        }

        function test_online() {
            load({});
            compare(Store.statusKind, "online");
            compare(Store.statusText, "Online");
            verify(Store.canMaintain);
            verify(Store.canChange);
            compare(Store.inks.map(i => i.color).join(","), "BK,C,M,Y,PGBK");
        }

        function test_sleeping_printer_can_still_be_given_maintenance_and_settings() {
            // Observed on the real XK140: a maintenance job wakes a sleeping printer by itself.
            load({ power: "sleeping", ink_source: "ipp" });
            compare(Store.statusKind, "sleeping");
            verify(Store.statusTip.indexOf("auto power-off") >= 0);
            verify(Store.canMaintain);
            verify(Store.canChange);
        }

        function test_offline() {
            load({ power: "offline" });
            compare(Store.statusKind, "offline");
            verify(!Store.canMaintain);
            verify(!Store.canChange);
        }

        function test_an_error_code_is_an_error_and_names_itself() {
            load({ support: { code: "1300", message_key: "support.paper-jam", message: "Paper is jammed.", severity: "error" } });
            compare(Store.statusKind, "error");
            compare(Store.statusText, "Support code 1300");
            verify(Store.supportShown);
            verify(Store.statusTip.indexOf("Paper is jammed") === 0, Store.statusTip);
        }

        function test_a_warning_code_is_a_warning() {
            load({ support: { code: "1000", message_key: "support.no-paper-rear", message: "x", severity: "warning" } });
            compare(Store.statusKind, "warning");
            compare(Store.statusText, "Support code 1000");
            verify(Store.supportShown);
            verify(Store.statusTip.indexOf("rear tray") >= 0, Store.statusTip);
        }

        function test_a_notice_code_does_not_change_the_state() {
            load({ support: { code: "D109", message_key: "support.notice", message: "x", severity: "info" } });
            compare(Store.statusKind, "online");
            compare(Store.statusText, "Online");
            verify(Store.supportShown, "it is the headline of the Status tab, in a printer that is fine");
            compare(Store.supportSeverity, "info");
            verify(Store.statusTip.indexOf("notice") >= 0, Store.statusTip);
        }

        function test_an_empty_tank_is_the_headline_when_the_code_is_only_a_warning() {
            const inks = JSON.parse(JSON.stringify(fake.snapshot.inks));
            inks[0].status = "empty";
            load({ inks: inks, support: { code: "1000", message_key: "support.no-paper-rear", message: "x", severity: "warning" } });
            compare(Store.statusKind, "error");
            compare(Store.statusText, "Ink empty");
            verify(!Store.supportShown, "the code is a notice below the headline");
        }

        function test_a_code_without_a_severity_counts_as_an_error() {
            // A service older than this app sends no severity.
            load({ support: { code: "1300", message_key: "support.1300", message: "x" } });
            compare(Store.statusKind, "error");
            compare(Store.statusText, "Support code 1300");
        }

        function test_the_firmware_update_state_comes_from_the_snapshot() {
            load({});
            compare(Store.firmwareUpdate, "unknown");
            verify(!Store.canUpdateFirmware);
            load({ firmware_update: "unavailable" });
            compare(Store.firmwareUpdate, "unavailable");
            verify(!Store.canUpdateFirmware);
            load({ firmware_update: "available" });
            verify(Store.canUpdateFirmware);
            fake.readonly = true;
            verify(!Store.canUpdateFirmware, "read-only");
            load({ firmware_update: "available", activity: "maintenance: nozzle check" });
            verify(!Store.canUpdateFirmware, "busy");
            load({ firmware_update: "available", power: "sleeping" });
            verify(!Store.canUpdateFirmware, "asleep");
        }

        function test_low_ink_is_a_warning() {
            const inks = JSON.parse(JSON.stringify(fake.snapshot.inks));
            inks[0].status = "low";
            load({ inks: inks });
            compare(Store.statusKind, "warning");
        }

        function test_busy_while_a_job_runs() {
            load({ activity: "maintenance: nozzle check" });
            compare(Store.statusKind, "busy");
            verify(!Store.canMaintain);
        }

        function test_read_only_blocks_writes() {
            load({});
            fake.readonly = true;
            verify(!Store.canMaintain);
            verify(!Store.canChange);
            let result = "";
            Store.runMaintenance({ op: "nozzle_check" }, (ok, body) => { result = body; });
            Store.applySettings({ dry_level: 4 }, (ok, body) => { result += body; });
            compare(fake.called("RunMaintenance").length, 0);
            compare(fake.called("ApplySettings").length, 0);
            verify(result.indexOf("ReadOnly") >= 0);
        }

        function test_service_down_means_not_connected() {
            load({});
            fake.connected = false;
            verify(!Store.connected);
            verify(!Store.canChange);
        }

        function test_cycle_printers() {
            load({});
            fake.printers = [
                { host: "a.test", name: "A", source: "flag", active: true, power: "on", last_updated: 1 },
                { host: "b.test", name: "B", source: "saved", active: false, power: "sleeping", last_updated: 1 }
            ];
            load({});
            compare(Store.activeHost, "a.test");
            Store.cycle(1);
            tryVerify(() => fake.called("SetActivePrinter").length === 1);
            compare(fake.called("SetActivePrinter")[0].args[0], "b.test");
            fake.printers = defaultPrinters;
        }
    }
}
