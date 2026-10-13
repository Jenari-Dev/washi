// SPDX-License-Identifier: GPL-2.0-or-later
//! `washi status` / `washi watch` against the in-process fake printer: awake, asleep (auto
//! power-off), offline. The fake records everything it receives; no test may cause a write.

use std::process::{Command, Output};

use washi_fake::FakePrinter;

struct Run {
    out: Output,
    stdout: String,
}

fn run(fake: &FakePrinter, cache: &std::path::Path, args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_washi"))
        .args(["--host", &fake.chmp_target(), "--ipp-port", &fake.ipp.port().to_string()])
        .args(args)
        .env("XDG_CONFIG_HOME", "/nonexistent/washi-test")
        .env("XDG_CACHE_HOME", cache)
        // Hermetic: never talk to the user's session bus (a running washid).
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    Run { out, stdout }
}

#[test]
fn status_of_an_awake_printer() {
    let fake = FakePrinter::start();
    let cache = tempfile::tempdir().unwrap();
    let r = run(&fake, cache.path(), &["status"]);
    assert!(r.out.status.success(), "{}", String::from_utf8_lossy(&r.out.stderr));
    assert!(r.stdout.contains("Power     on"), "{}", r.stdout);
    assert!(r.stdout.contains("State     idle"), "{}", r.stdout);
    assert!(r.stdout.contains("Magenta") && r.stdout.contains("XKI-N21<M>SETUP"), "{}", r.stdout);
    assert!(r.stdout.contains("Cassette"), "{}", r.stdout);
    assert!(!r.stdout.contains("auto power-off"));
    assert!(fake.writes().is_empty());
    // The cache file is named by a hash, not by the host.
    let files: Vec<_> = std::fs::read_dir(cache.path().join("washi")).unwrap().collect();
    assert_eq!(files.len(), 1);
    let name = files[0].as_ref().unwrap().file_name().to_string_lossy().into_owned();
    assert!(name.len() == 21 && name.ends_with(".json") && !name.contains("127"), "{name}");
}

#[test]
fn status_of_a_sleeping_printer_explains_auto_power_off_and_exits_zero() {
    let fake = FakePrinter::start();
    let cache = tempfile::tempdir().unwrap();
    // One awake read first, so the tank models and trays are cached.
    assert!(run(&fake, cache.path(), &["status"]).out.status.success());
    fake.set_asleep(true);
    fake.set_ink("C", 30);
    fake.clear_log();

    let r = run(&fake, cache.path(), &["status"]);
    assert!(
        r.out.status.success(),
        "sleeping is not an error: {}",
        String::from_utf8_lossy(&r.out.stderr)
    );
    assert!(r.stdout.contains("Power     sleeping"), "{}", r.stdout);
    assert!(
        r.stdout.contains("Printer is in auto power-off; it wakes up when a job arrives."),
        "{}",
        r.stdout
    );
    assert!(r.stdout.contains("(IPP marker levels)"), "{}", r.stdout);
    assert!(r.stdout.contains("Cyan") && r.stdout.contains(" 30%"), "{}", r.stdout);
    assert!(r.stdout.contains("XKI-N21<C>SETUP"), "tank model from the cache: {}", r.stdout);
    assert!(r.stdout.contains("Paper (last known)"), "{}", r.stdout);
    assert!(fake.log().contains(&"IPP/Get-Printer-Attributes".to_string()), "{:?}", fake.log());
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
}

#[test]
fn status_json_is_the_snapshot() {
    let fake = FakePrinter::start();
    fake.set_asleep(true);
    let cache = tempfile::tempdir().unwrap();
    let r = run(&fake, cache.path(), &["status", "--json"]);
    assert!(r.out.status.success());
    let j: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(j["power"], "sleeping");
    assert_eq!(j["ink_source"], "ipp");
    assert_eq!(j["inks"].as_array().unwrap().len(), 5);
    assert_eq!(j["inks"][0]["color"], "M");
    assert!(j["last_updated"].as_u64().unwrap() > 1_700_000_000);
    assert_eq!(j["connection"], "wifi");
    // The resolved address is the one place the printer's network address appears.
    assert_eq!(j["address"], "127.0.0.1");
    assert_eq!(r.stdout.matches("127.0.0.1").count(), 1, "{}", r.stdout);
}

#[test]
fn an_unreachable_printer_is_offline_with_exit_code_one_and_last_known_data() {
    let fake = FakePrinter::start();
    let cache = tempfile::tempdir().unwrap();
    assert!(run(&fake, cache.path(), &["status"]).out.status.success());
    fake.set_chmp_down(true);
    fake.set_ipp_down(true);
    let r = run(&fake, cache.path(), &["status"]);
    assert_eq!(r.out.status.code(), Some(1));
    assert!(r.stdout.contains("Power     offline"), "{}", r.stdout);
    assert!(r.stdout.contains("last known"), "{}", r.stdout);
    assert!(r.stdout.contains("Magenta"), "cached ink is still shown: {}", r.stdout);
    assert!(String::from_utf8_lossy(&r.out.stderr).contains("offline"));
    assert!(fake.writes().is_empty());
}

#[test]
fn watch_prints_the_first_reading_and_then_only_changes() {
    let fake = FakePrinter::start();
    let cache = tempfile::tempdir().unwrap();
    let r = run(&fake, cache.path(), &["watch", "--no-daemon", "--count", "1"]);
    assert!(r.out.status.success(), "{}", String::from_utf8_lossy(&r.out.stderr));
    let lines: Vec<_> = r.stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{}", r.stdout);
    assert!(lines[0].starts_with("[+00:00:00] on") && lines[0].contains("M=60%"), "{}", lines[0]);

    // With --json, one compact snapshot per line.
    let r = run(&fake, cache.path(), &["watch", "--no-daemon", "--count", "1", "--json"]);
    let j: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!(j["power"], "on");

    // Unchanged readings print nothing more: two polls, one line.
    let r = run(&fake, cache.path(), &["watch", "--no-daemon", "--count", "2", "--interval", "2"]);
    assert_eq!(r.stdout.lines().count(), 1, "{}", r.stdout);
    assert!(fake.writes().is_empty());
}
