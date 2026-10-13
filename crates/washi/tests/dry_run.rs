// SPDX-License-Identifier: GPL-2.0-or-later
//! Write commands without --confirm must print their XML and never open a connection.

use std::net::TcpListener;
use std::process::{Command, Output};

fn run(args: &[&str]) -> (Output, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let host = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let out = Command::new(env!("CARGO_BIN_EXE_washi"))
        .args(["--host", &host])
        .args(args)
        .env("XDG_CONFIG_HOME", "/nonexistent/washi-test")
        .output()
        .unwrap();
    (out, listener)
}

fn assert_dry(args: &[&str], expected_ops: &[&str]) -> String {
    let (out, listener) = run(args);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.starts_with("DRY RUN"), "{stdout}");
    // The listing is indented for display; compare with the indentation removed.
    let stdout: String = stdout.lines().map(str::trim).collect::<Vec<_>>().join("");
    for op in expected_ops {
        assert!(
            stdout.contains(&format!("<ivec:operation>{op}</ivec:operation>")),
            "{args:?} missing {op}\n{stdout}"
        );
    }
    assert!(
        matches!(listener.accept(), Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "{args:?} opened a connection"
    );
    stdout
}

#[test]
fn maintenance_commands_are_dry_by_default() {
    let s = assert_dry(&["maint", "nozzle-check"], &["StartJob", "TestPrint", "EndJob"]);
    assert!(s.contains("nozzle_check"));
    assert!(s.contains("<ivec:jobID>00000002</ivec:jobID>"));
    assert!(s.contains("Run again with --confirm"), "{s}");
    let s = assert_dry(
        &["maint", "clean", "--deep", "--group", "2"],
        &["StartJob", "Cleaning", "EndJob"],
    );
    assert!(s.contains("<ivec:type>deep</ivec:type><ivec:inkgroup>group2</ivec:inkgroup>"));
    let s = assert_dry(&["maint", "clean"], &["Cleaning"]);
    assert!(s.contains("<ivec:type>regular</ivec:type><ivec:inkgroup>all</ivec:inkgroup>"));
    assert!(assert_dry(&["maint", "align"], &["TestPrint"]).contains("auto_registration"));
    assert!(assert_dry(&["maint", "align", "--check"], &["TestPrint"]).contains("regi_check"));
    assert!(
        assert_dry(&["maint", "align", "--manual", "2"], &["TestPrint"])
            .contains("<ivec:id>2</ivec:id>")
    );
    assert_dry(&["maint", "roller-clean"], &["StartJob", "RollerCleaning", "EndJob"]);
    // The roller and the bottom plate as `Cleaning` types: no ink group.
    let s = assert_dry(&["maint", "clean", "--roller"], &["StartJob", "Cleaning", "EndJob"]);
    assert!(s.contains("<ivec:type>roller</ivec:type>") && !s.contains("inkgroup"), "{s}");
    let s = assert_dry(&["maint", "clean", "--platen"], &["Cleaning"]);
    assert!(s.contains("<ivec:type>platen</ivec:type>") && !s.contains("inkgroup"), "{s}");
}

#[test]
fn a_firmware_update_is_dry_by_default_and_a_single_vendor_command() {
    let s = assert_dry(&["firmware", "update"], &["VendorCmd"]);
    assert!(s.contains("<vcn:ijoperation>UpdateFRom</vcn:ijoperation>"), "{s}");
    assert!(!s.contains("StartJob") && !s.contains("jobID"), "no job bracket: {s}");
}

#[test]
fn settings_commands_are_dry_by_default() {
    let s = assert_dry(&["set", "dry-level", "4"], &["StartJob", "SetConfiguration", "EndJob"]);
    assert!(s.contains("<ivec:drylevel>4</ivec:drylevel>"));
    assert!(s.contains("servicetype=\"device\""));
    assert!(
        assert_dry(&["set", "quiet", "2100-0700"], &["SetConfiguration"])
            .contains("<ivec:starttime>2100</ivec:starttime>")
    );
    assert!(
        assert_dry(&["set", "quiet", "off"], &["SetConfiguration"])
            .contains("<ivec:mode>OFF</ivec:mode>")
    );
    assert!(
        assert_dry(&["set", "auto-power-off", "60"], &["SetConfiguration"])
            .contains("<ivec:autopoweroff><ivec:mode>ON</ivec:mode><ivec:time>60</ivec:time>")
    );
    assert!(
        assert_dry(&["set", "auto-power-on", "off"], &["SetConfiguration"])
            .contains("<ivec:autopoweron>OFF</ivec:autopoweron>")
    );
    assert!(
        assert_dry(&["set", "paper-gap", "wide"], &["SetConfiguration"])
            .contains("<ivec:papergap>wide</ivec:papergap>")
    );
    assert!(
        assert_dry(&["set", "auto-firmware", "off"], &["SetConfiguration"])
            .contains("<vcn:auto_printer_rom_update>OFF</vcn:auto_printer_rom_update>")
    );
}

#[test]
fn power_off_is_dry_by_default() {
    assert_dry(&["power", "off"], &["PowerOff"]);
}

#[test]
fn power_on_is_dry_by_default_and_a_single_request() {
    let s = assert_dry(&["power", "on"], &["PowerOn"]);
    assert!(!s.contains("StartJob"), "no job bracket: {s}");
    assert_dry(
        &["set", "registration", "1", "A=2", "B=-1"],
        &["StartJob", "SetConfiguration", "EndJob"],
    );
}

#[test]
fn registration_values_appear_in_the_planned_request() {
    let s = assert_dry(&["set", "registration", "3", "a=1", "e=-2"], &["SetConfiguration"]);
    assert!(s.contains("<ivec:setregistration id=\"3\">"), "{s}");
    assert!(s.contains("<ivec:item sign=\"a\">1</ivec:item>"), "{s}");
    assert!(s.contains("<ivec:item sign=\"e\">-2</ivec:item>"), "{s}");
}

#[test]
fn bad_arguments_fail_before_anything_else() {
    for args in [
        &["set", "dry-level", "9"][..],
        &["set", "auto-power-off", "45"],
        &["set", "quiet", "25:00"],
        &["maint", "align", "--manual", "9"],
    ] {
        let (out, listener) = run(args);
        assert!(!out.status.success(), "{args:?} should fail");
        assert!(
            matches!(listener.accept(), Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock)
        );
    }
}

#[test]
fn help_lists_every_command() {
    let out = Command::new(env!("CARGO_BIN_EXE_washi")).arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    for c in [
        "discover",
        "status",
        "watch",
        "info",
        "settings",
        "caps",
        "raw",
        "maint",
        "set",
        "power",
        "firmware",
        "links",
        "config",
        "--confirm",
        "--json",
        "--host",
    ] {
        assert!(help.contains(c), "help is missing {c}\n{help}");
    }
}
