// SPDX-License-Identifier: GPL-2.0-or-later
//! `Printer::snapshot` against the in-process fake printer: awake, asleep (auto power-off),
//! offline, and IPP-only, with the cache in between. Nothing here may ever write to the fake.

use std::time::Duration;

use washi_core::net::{ClientOptions, IppOptions};
use washi_core::{InkSource, InkStatus, Power, Printer, SnapshotCache};
use washi_fake::FakePrinter;

fn printer(fake: &FakePrinter) -> Printer {
    let mut p = Printer::with_options(
        "127.0.0.1",
        ClientOptions {
            port: fake.chmp.port(),
            connect_timeout: Duration::from_millis(500),
            io_timeout: Duration::from_millis(800),
            read_timeout: Duration::from_millis(500),
            poll_initial: Duration::from_millis(2),
            poll_max: Duration::from_millis(5),
            ..ClientOptions::default()
        },
    );
    p.set_ipp_options(IppOptions {
        port: fake.ipp.port(),
        connect_timeout: Duration::from_millis(500),
        io_timeout: Duration::from_millis(800),
        ..IppOptions::default()
    });
    p
}

fn cache(dir: &tempfile::TempDir, p: &Printer) -> SnapshotCache {
    SnapshotCache::in_dir(dir.path(), p.host(), p.chmp_port())
}

#[test]
fn awake_printer_is_read_through_ivec_and_cached() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let c = cache(&dir, &p);

    let s = p.snapshot(Some(&c));
    assert_eq!(s.power, Power::On);
    assert_eq!(s.state.as_deref(), Some("idle"));
    assert_eq!(s.ink_source, InkSource::Ivec);
    assert_eq!(s.inks.len(), 5);
    assert_eq!(s.ink("C").unwrap().model.as_deref(), Some("XKI-N21<C>SETUP"));
    assert_eq!(s.trays.len(), 2);
    assert_eq!(s.device.as_ref().unwrap().model, "XK140 series");
    assert!(s.counters.is_some() && s.from_cache.is_empty());
    assert!(s.last_awake.is_some());

    // IPP is not needed while the print service reports ink.
    assert_eq!(fake.log(), ["GetStatus/print", "GetStatus/device", "GetCapability/device"]);
    assert_eq!(c.load().as_ref(), Some(&s), "snapshot was cached");

    // A second read reuses the cached device information (no GetCapability).
    fake.clear_log();
    let s2 = p.snapshot(Some(&c));
    assert_eq!(fake.log(), ["GetStatus/print", "GetStatus/device"]);
    assert!(s2.content_eq(&s));
    assert!(fake.writes().is_empty());
}

#[test]
fn sleeping_printer_uses_ipp_for_ink_and_the_cache_for_the_rest() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let c = cache(&dir, &p);
    let awake = p.snapshot(Some(&c));

    fake.set_asleep(true);
    fake.set_ink("C", 40);
    fake.clear_log();
    let s = p.snapshot(Some(&c));

    assert_eq!(s.power, Power::Sleeping);
    assert_eq!(s.state.as_deref(), Some("idle"), "IPP printer-state");
    assert_eq!(s.state_reasons, ["none"]);
    assert_eq!(s.ink_source, InkSource::Ipp);
    let cyan = s.ink("C").unwrap();
    assert_eq!((cyan.level, cyan.status), (Some(40), InkStatus::Ok));
    assert_eq!(cyan.model.as_deref(), Some("XKI-N21<C>SETUP"), "tank model from the awake read");
    assert_eq!(s.ink("PGBK").unwrap().model.as_deref(), Some("XKI-N20<PGBK>SETUP"));
    assert_eq!(s.ink("M").unwrap().level, Some(60));
    assert_eq!(s.trays, awake.trays);
    assert_eq!(s.from_cache, ["trays"], "device info was read recently enough");
    assert!(s.counters.is_some(), "GetStatus device still works while asleep");
    assert_eq!(s.last_awake, awake.last_awake);
    assert!(s.support.is_none());

    assert_eq!(
        fake.log(),
        ["GetStatus/print", "GetStatus/device", "IPP/Get-Printer-Attributes"],
        "exactly the reads the sleeping case needs"
    );
    assert!(fake.writes().is_empty());
    assert_eq!(c.load().unwrap().power, Power::Sleeping);

    // Waking up restores IVEC readings, still without a write.
    fake.set_asleep(false);
    let s = p.snapshot(Some(&c));
    assert_eq!((s.power, s.ink_source), (Power::On, InkSource::Ivec));
    assert!(fake.writes().is_empty());
}

#[test]
fn sleeping_printer_without_a_cache() {
    let fake = FakePrinter::start();
    fake.set_asleep(true);
    let p = printer(&fake);
    let s = p.snapshot(None);
    assert_eq!(s.power, Power::Sleeping);
    assert_eq!(s.ink_source, InkSource::Ipp);
    assert_eq!(s.inks.len(), 5);
    assert!(s.inks.iter().all(|i| i.model.is_none()));
    assert_eq!(
        s.device.as_ref().unwrap().model,
        "XK140 series",
        "GetCapability device works asleep"
    );
    assert!(fake.writes().is_empty());
}

#[test]
fn low_and_empty_ink_levels_are_flagged() {
    let fake = FakePrinter::start();
    fake.set_ink("Y", 10);
    fake.set_ink("M", 0);
    let p = printer(&fake);
    let awake = p.snapshot(None);
    assert_eq!(awake.ink("Y").unwrap().status, InkStatus::Low);
    assert_eq!(awake.ink("M").unwrap().status, InkStatus::Empty);
    assert_eq!(awake.ink("C").unwrap().status, InkStatus::Ok);

    fake.set_asleep(true);
    let asleep = p.snapshot(None);
    assert_eq!(asleep.ink("Y").unwrap().status, InkStatus::Low);
    assert_eq!(asleep.ink("M").unwrap().status, InkStatus::Empty);
}

#[test]
fn support_code_is_reported_when_awake() {
    let fake = FakePrinter::start();
    fake.set_support_code("1300");
    let s = printer(&fake).snapshot(None);
    let sup = s.support.unwrap();
    assert_eq!((sup.code.as_str(), sup.message_key.as_str()), ("1300", "support.paper-jam"));
    assert_eq!(sup.severity, washi_core::Severity::Error);
}

#[test]
fn unreachable_printer_is_offline_and_shows_the_last_known_data() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let c = cache(&dir, &p);
    let awake = p.snapshot(Some(&c));

    fake.set_chmp_down(true);
    fake.set_ipp_down(true);
    let s = p.snapshot(Some(&c));
    assert_eq!(s.power, Power::Offline);
    assert_eq!(s.ink_source, InkSource::Cache);
    assert_eq!(s.inks, awake.inks);
    assert_eq!(s.trays, awake.trays);
    assert_eq!(s.counters, awake.counters);
    assert_eq!(s.last_awake, awake.last_awake);
    assert!(s.from_cache.contains(&"inks".to_string()));
    assert!(fake.writes().is_empty());
    assert_eq!(c.load().unwrap().power, Power::Offline);

    // A dead port (nothing listening at all) is the same.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut dead = Printer::with_options(
        "127.0.0.1",
        ClientOptions { port, connect_timeout: Duration::from_millis(300), ..Default::default() },
    );
    dead.set_ipp_port(port);
    let s = dead.snapshot(None);
    assert_eq!(s.power, Power::Offline);
    assert!(s.inks.is_empty() && s.device.is_none());
    // The resolved address is the one place the printer's network address appears.
    assert_eq!(s.address.as_deref(), Some("127.0.0.1"));
    let json = serde_json::to_string(&washi_core::PrinterSnapshot { address: None, ..s }).unwrap();
    assert!(!json.contains("127.0.0.1"), "no host or address in the snapshot: {json}");
}

#[test]
fn chmp_down_but_ipp_answering_is_on_with_a_warning() {
    let fake = FakePrinter::start();
    fake.set_chmp_down(true);
    let s = printer(&fake).snapshot(None);
    assert_eq!(s.power, Power::On);
    assert_eq!(s.ink_source, InkSource::Ipp);
    assert_eq!(s.last_awake, None);
    assert!(s.warnings.iter().any(|w| w.contains("CHMP is not answering")), "{:?}", s.warnings);
}

#[test]
fn ipp_failure_while_asleep_falls_back_to_cached_ink() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let c = cache(&dir, &p);
    p.snapshot(Some(&c));

    fake.set_asleep(true);
    fake.set_ipp_down(true);
    let s = p.snapshot(Some(&c));
    assert_eq!(s.power, Power::Sleeping);
    assert_eq!(s.ink_source, InkSource::Cache);
    assert!(s.warnings.iter().any(|w| w.starts_with("IPP status failed")), "{:?}", s.warnings);
    assert!(fake.writes().is_empty());
}
