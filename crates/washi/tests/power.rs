// SPDX-License-Identifier: GPL-2.0-or-later
//! `washi power on --confirm` against the in-process fake printer (never a real one): it sends
//! PowerOn with the power-on mode, then looks at the print status until the printer is up.

use std::process::{Command, Output};

use washi_fake::FakePrinter;

fn run(fake: &FakePrinter, args: &[&str]) -> (Output, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_washi"))
        .args(["--host", &fake.chmp_target(), "--ipp-port", &fake.ipp.port().to_string()])
        .args(args)
        .env("XDG_CONFIG_HOME", "/nonexistent/washi-test")
        .env("XDG_CACHE_HOME", "/nonexistent/washi-test")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    (out, stdout)
}

#[test]
fn power_on_wakes_a_sleeping_printer_and_says_so() {
    let fake = FakePrinter::start();
    fake.set_asleep(true);
    let (out, stdout) = run(&fake, &["--confirm", "power", "on"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("Printer woke up"), "{stdout}");
    assert_eq!(fake.writes(), ["PowerOn/device"], "one request, no job bracket");
    // The proof is the print status, not the reply to PowerOn.
    assert!(fake.log().iter().any(|l| l == "GetStatus/print"), "{:?}", fake.log());
    assert_eq!(
        run(&fake, &["status"]).1.lines().find(|l| l.starts_with("Power")),
        Some("Power     on")
    );
}

#[test]
fn power_on_of_an_awake_printer_does_not_claim_it_woke_up() {
    let fake = FakePrinter::start();
    let (out, stdout) = run(&fake, &["--confirm", "power", "on"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("already awake"), "{stdout}");
    assert!(!stdout.contains("woke up"), "{stdout}");
}

#[test]
fn power_on_without_confirm_sends_nothing() {
    let fake = FakePrinter::start();
    fake.set_asleep(true);
    let (out, stdout) = run(&fake, &["power", "on"]);
    assert!(out.status.success());
    assert!(stdout.starts_with("DRY RUN"), "{stdout}");
    assert!(fake.log().is_empty(), "{:?}", fake.log());
}
