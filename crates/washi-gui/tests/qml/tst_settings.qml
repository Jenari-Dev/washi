import QtQuick
import QtTest 1.2

// Setting dialogs: Apply sends a validated change with "confirm", shows the read-back, and
// read-only mode never reaches the bus.
Item {
    width: 900
    height: 700

    FakeTransport {
        id: fake
    }

    SettingDialog {
        id: dlg
    }

    TestCase {
        name: "Settings"
        when: windowShown

        function init() {
            Store.reset();
            Store.transport = null;
            fake.connected = false;
            fake.readonly = false;
            fake.overrides = ({});
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
            Store.loadCaps(true);
            tryVerify(() => Store.settings !== null);
            fake.clearCalls();
        }

        function applyCalls() {
            return fake.called("ApplySettings");
        }

        function test_quiet_schedule() {
            dlg.open({ field: "quiet" });
            compare(dlg.stage, "edit");
            compare(dlg.quietMode, "OFF");
            dlg.quietMode = "time";
            dlg.quietStart = "22:30";
            dlg.quietEnd = "06:00";
            verify(dlg.valid);
            dlg.apply();
            compare(dlg.stage, "working");
            tryVerify(() => applyCalls().length === 1);
            const body = JSON.parse(applyCalls()[0].args[1]);
            compare(body.confirm, true);
            compare(body.quiet, "2230-0600");
            tryVerify(() => dlg.stage === "done");
            compare(dlg.ok, true);
            compare(dlg.resultChanges.length, 1);
            compare(dlg.resultChanges[0].field, "silentmode");
            dlg.cancel();
        }

        function test_bad_time_cannot_be_applied() {
            dlg.open({ field: "quiet" });
            dlg.quietMode = "time";
            dlg.quietStart = "25:00";
            verify(!dlg.valid);
            dlg.apply();
            wait(100);
            compare(applyCalls().length, 0);
            compare(dlg.stage, "edit");
            dlg.cancel();
        }

        function test_auto_power_off_both_timers() {
            dlg.open({ field: "autopoweroff" });
            verify(dlg.hasStandalone);
            dlg.pwrOn = true;
            dlg.pwrTime = 60;
            dlg.saOn = false;
            dlg.apply();
            tryVerify(() => applyCalls().length === 1);
            const body = JSON.parse(applyCalls()[0].args[1]);
            compare(body.auto_power_off, 60);
            compare(body.auto_power_off_standalone, "off");
            tryVerify(() => dlg.stage === "done");
            dlg.cancel();
        }

        function test_switch_asks_before_changing() {
            dlg.open({ field: "autopoweron", value: false });
            compare(dlg.stage, "edit");
            wait(100);
            compare(applyCalls().length, 0);
            dlg.apply();
            tryVerify(() => applyCalls().length === 1);
            compare(JSON.parse(applyCalls()[0].args[1]).auto_power_on, false);
            tryVerify(() => dlg.stage === "done");
            dlg.cancel();
        }

        function test_dry_level_and_gap() {
            dlg.open({ field: "dry_level" });
            compare(dlg.dryLevel, 3);
            dlg.dryLevel = 5;
            dlg.apply();
            tryVerify(() => applyCalls().length === 1);
            compare(JSON.parse(applyCalls()[0].args[1]).dry_level, 5);
            tryVerify(() => dlg.stage === "done");
            dlg.cancel();
            fake.clearCalls();
            dlg.open({ field: "paper_gap" });
            dlg.gap = "wide";
            dlg.apply();
            tryVerify(() => applyCalls().length === 1);
            compare(JSON.parse(applyCalls()[0].args[1]).paper_gap, "wide");
            tryVerify(() => dlg.stage === "done");
            dlg.cancel();
        }

        function test_error_is_shown() {
            fake.overrides = ({ ApplySettings: [false, JSON.stringify({ name: "InvalidArgs", message: "drylevel 9 is not allowed" })] });
            dlg.open({ field: "dry_level" });
            dlg.apply();
            tryVerify(() => dlg.stage === "done");
            compare(dlg.ok, false);
            verify(dlg.resultError.indexOf("not allowed") >= 0);
            dlg.cancel();
        }

        function test_read_only_never_reaches_the_bus() {
            fake.readonly = true;
            dlg.open({ field: "dry_level" });
            dlg.apply();
            tryVerify(() => dlg.stage === "done");
            compare(dlg.ok, false);
            compare(applyCalls().length, 0);
            dlg.cancel();
        }
    }
}
