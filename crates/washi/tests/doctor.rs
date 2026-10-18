// SPDX-License-Identifier: GPL-2.0-or-later
//! `washi doctor` and `washi notify-test` against the in-process fake printer and without any
//! session bus. Nothing here may write to the printer or print an address.

use std::process::{Command, Output};

use washi_fake::FakePrinter;

fn run(fake: Option<&FakePrinter>, home: &std::path::Path, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_washi"));
    if let Some(f) = fake {
        c.args(["--host", &f.chmp_target(), "--ipp-port", &f.ipp.port().to_string()]);
    }
    c.args(args)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        // Hermetic: never talk to the user's session bus (a running washid). Removing the
        // variable is not enough, the library then looks for the bus in the runtime directory.
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/washi-test-bus")
        .output()
        .unwrap()
}

fn checks(report: &serde_json::Value) -> Vec<&serde_json::Value> {
    report["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["checks"].as_array().unwrap())
        .collect()
}

fn status<'a>(report: &'a serde_json::Value, id: &str) -> &'a str {
    checks(report)
        .into_iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no check {id}"))["status"]
        .as_str()
        .unwrap()
}

#[test]
fn doctor_json_checks_the_printer_and_the_computer() {
    let fake = FakePrinter::start();
    let home = tempfile::tempdir().unwrap();
    let out = run(Some(&fake), home.path(), &["doctor", "--json"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let report: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(report["source"], "washi");
    assert_eq!(report["language"], "en");
    assert_eq!(status(&report, "washi.cli"), "ok");
    for id in ["printer.chmp", "printer.ipp", "printer.escl", "printer.links", "printer.host"] {
        assert_eq!(status(&report, id), "ok", "{id}");
    }
    // No session bus here: the service and the notification service cannot be asked.
    assert_eq!(status(&report, "washi.service"), "skip");
    assert_eq!(status(&report, "desktop.notifications"), "fail");
    for id in ["tools.poppler", "tools.tesseract", "tools.soffice", "locale.en", "locale.ja"] {
        assert!(["ok", "skip"].contains(&status(&report, id)), "{id} is optional");
    }
    assert!(report["summary"]["ok"].as_u64().unwrap() >= 6, "{report}");
    assert!(!text.contains("127.0.0.1"), "no printer address in the output: {text}");
    assert!(fake.writes().is_empty(), "doctor writes nothing: {:?}", fake.writes());
}

#[test]
fn doctor_text_has_a_mark_and_a_hint_per_line() {
    let fake = FakePrinter::start();
    let home = tempfile::tempdir().unwrap();
    let out = run(Some(&fake), home.path(), &["doctor"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("\u{2713} washi (this command): "), "{text}");
    assert!(
        text.contains("\u{2713} Management channel (CHMP): XK140 series, firmware 1.060"),
        "{text}"
    );
    assert!(text.contains("\u{2717} Desktop notification service"), "{text}");
    assert!(text.contains("\u{2013} Service (washid) running"), "{text}");
    assert!(text.contains("org.freedesktop.Notifications"), "the hint names the service: {text}");
    assert!(text.contains("problem"), "{text}");
    assert!(!text.contains("127.0.0.1"), "{text}");
}

#[test]
fn doctor_reports_a_printer_that_does_not_answer() {
    let fake = FakePrinter::start();
    fake.set_chmp_down(true);
    fake.set_ipp_down(true);
    let home = tempfile::tempdir().unwrap();
    let out = run(Some(&fake), home.path(), &["doctor", "--json"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    for id in ["printer.chmp", "printer.ipp", "printer.escl"] {
        assert_eq!(status(&report, id), "fail", "{id}");
    }
    assert!(!text.contains("127.0.0.1"), "{text}");
}

#[test]
fn notify_test_without_a_session_bus_explains() {
    let home = tempfile::tempdir().unwrap();
    let out = run(None, home.path(), &["notify-test"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("session bus") || err.contains("D-Bus"), "{err}");
}
