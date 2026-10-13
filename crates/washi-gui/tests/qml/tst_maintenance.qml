import QtQuick
import QtTest 1.2

// The maintenance flow: nothing is sent before the user presses Start, what is sent says
// "confirm", progress and results follow the daemon's signals, and follow-ups are offered.
Item {
    width: 900
    height: 700

    FakeTransport {
        id: fake
    }

    MaintenanceHost {
        id: host
    }

    TestCase {
        name: "Maintenance"
        when: windowShown

        function init() {
            host.cancel();
            host.lastCleanKind = "";
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
            tryVerify(() => Store.maintCap !== null);
            fake.clearCalls();
        }

        function runCalls() {
            return fake.called("RunMaintenance");
        }

        function test_nothing_is_sent_before_confirmation() {
            host.open({ op: "nozzle_check" });
            compare(host.stage, "confirm");
            wait(100);
            compare(runCalls().length, 0);
            host.cancel();
            compare(host.stage, "closed");
            wait(100);
            compare(runCalls().length, 0);
        }

        function test_confirmation_names_paper_and_ink() {
            host.open({ op: "nozzle_check" });
            const t = host.confirmText(host.spec);
            verify(t.indexOf("1 sheet") >= 0, t);
            verify(t.indexOf("A4") >= 0, t);
            verify(t.indexOf("plain paper") >= 0, t);
            verify(t.indexOf("Cassette") >= 0, t);
            host.cancel();
            host.open({ op: "align_auto" });
            verify(host.confirmText(host.spec).indexOf("2 sheets") >= 0);
            host.cancel();
            host.open({ op: "clean" });
            host.cleanKind = "deep";
            host.nextFromOptions();
            verify(host.confirmText(host.spec).indexOf("more ink") >= 0);
            host.cancel();
        }

        function test_start_sends_confirm_true_and_follows_signals() {
            host.open({ op: "nozzle_check" });
            host.accept();
            compare(host.stage, "running");
            tryVerify(() => runCalls().length === 1);
            const c = runCalls()[0];
            compare(c.args[0], "printer.test");
            const spec = JSON.parse(c.args[1]);
            compare(spec.confirm, true);
            compare(spec.op, "nozzle_check");
            tryVerify(() => host.jobId === "job-1");

            fake.busSignal("MaintenanceProgress", "job-1", "waiting", "4s: the printer is printing");
            compare(host.progress, "4s: the printer is printing");
            // another job's progress is ignored
            fake.busSignal("MaintenanceProgress", "job-9", "waiting", "someone else");
            compare(host.progress, "4s: the printer is printing");

            fake.busSignal("MaintenanceFinished", "job-1", "true", "nozzle check finished in 12s.");
            compare(host.stage, "done");
            compare(host.ok, true);
            // "Is the pattern OK?" -> not OK leads to cleaning
            compare(host.followup, "after-nozzle");
            findChild(host, "patternBad").clicked();
            compare(host.stage, "options");
            host.nextFromOptions();
            compare(host.stage, "confirm");
            compare(host.spec.op, "clean");
            host.cancel();
        }

        function test_clean_options_and_follow_up() {
            host.open({ op: "clean" });
            compare(host.stage, "options");
            verify(host.groups.length === 3);
            host.cleanKind = "deep";
            host.cleanGroup = "group2";
            host.nextFromOptions();
            compare(host.stage, "confirm");
            host.accept();
            tryVerify(() => runCalls().length === 1);
            const spec = JSON.parse(runCalls()[0].args[1]);
            compare(spec.op, "clean");
            compare(spec.kind, "deep");
            compare(spec.group, "group2");
            compare(spec.confirm, true);
            tryVerify(() => host.jobId === "job-1");
            fake.busSignal("MaintenanceFinished", "job-1", "true", "done");
            compare(host.followup, "after-clean");
            findChild(host, "followNozzle").clicked();
            compare(host.stage, "confirm");
            compare(host.spec.op, "nozzle_check");
            host.cancel();
        }

        function test_finished_before_the_reply_is_handled() {
            fake.auto = false;
            host.open({ op: "nozzle_check" });
            host.accept();
            compare(host.stage, "running");
            fake.busSignal("MaintenanceFinished", "job-1", "false", "paper jam");
            compare(host.stage, "done");
            compare(host.ok, false);
            compare(host.result, "paper jam");
            // the late reply must not bring the running state back
            fake.reply(fake.calls[fake.calls.length - 1].id, true, "job-1");
            compare(host.stage, "done");
            fake.auto = true;
            host.cancel();
        }

        function test_service_refusal_is_shown() {
            fake.overrides = ({ RunMaintenance: [false, JSON.stringify({ name: "Busy", message: "job-3 (cleaning) is still running on this printer" })] });
            host.open({ op: "nozzle_check" });
            host.accept();
            tryVerify(() => host.stage === "done");
            compare(host.ok, false);
            verify(host.result.indexOf("still running") >= 0);
            host.cancel();
        }

        function test_read_only_never_reaches_the_bus() {
            fake.readonly = true;
            host.open({ op: "nozzle_check" });
            host.accept();
            tryVerify(() => host.stage === "done");
            compare(host.ok, false);
            compare(runCalls().length, 0);
            host.cancel();
        }

        function test_accept_outside_confirm_does_nothing() {
            host.open({ op: "clean" });
            compare(host.stage, "options");
            host.accept();
            compare(host.stage, "options");
            wait(100);
            compare(runCalls().length, 0);
            host.cancel();
        }
    }
}
