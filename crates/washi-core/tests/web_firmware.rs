// SPDX-License-Identifier: GPL-2.0-or-later
//! The printer's web links and the firmware update, against the in-process fake printer.

use std::time::Duration;

use washi_core::ivec::{CleaningType, InkGroup};
use washi_core::net::ClientOptions;
use washi_core::{
    Confirm, Error, FirmwareOptions, FirmwareProgress, FirmwareUpdate, Language, LinkSource,
    LocalizationCache, MaintOp, MaintOptions, Power, Printer,
};
use washi_fake::FakePrinter;

fn printer(fake: &FakePrinter) -> Printer {
    let net = ClientOptions {
        port: fake.chmp.port(),
        connect_timeout: Duration::from_millis(500),
        io_timeout: Duration::from_secs(5),
        ..ClientOptions::default()
    };
    Printer::with_options("127.0.0.1", net)
}

fn quick() -> FirmwareOptions {
    FirmwareOptions {
        poll_interval: Duration::from_millis(40),
        gone_timeout: Duration::from_secs(5),
        back_timeout: Duration::from_secs(20),
        stable_polls: 2,
    }
}

#[test]
fn links_are_read_from_the_printer_and_fall_back_to_its_web_page() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let own = format!("http://127.0.0.1:{}/", fake.chmp.port());

    let l = p.web_links(Some("1300"), Language::En);
    assert!(l.from_printer);
    assert_eq!(l.ink.source, LinkSource::Maker);
    assert!(l.ink.url.starts_with("https://links.example.org/"), "{}", l.ink.url);
    assert!(l.error.as_ref().unwrap().url.ends_with("&ERR=1300"));
    assert_eq!(
        l.maintenance_page.url,
        format!("http://127.0.0.1:{}/index.html?page=PAGE_UTIL", fake.chmp.port())
    );
    assert!(fake.log().iter().any(|l| l == "Web/model.js"));
    assert!(fake.writes().is_empty(), "reading links writes nothing: {:?}", fake.writes());

    // The script is gone: every link is the printer's own page, and nothing was invented.
    fake.set_model_js(None);
    let l = p.web_links(Some("1300"), Language::Ja);
    assert!(!l.from_printer);
    for link in [&l.ink, &l.help, &l.support, l.error.as_ref().unwrap()] {
        assert_eq!((link.url.as_str(), link.source), (own.as_str(), LinkSource::Printer));
    }
}

#[test]
fn links_of_a_printer_that_does_not_answer_are_fallbacks_not_errors() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    fake.set_chmp_down(true);
    let l = p.web_links(None, Language::En);
    assert!(!l.from_printer);
    assert!(l.error.is_none());
    assert_eq!(l.ink.source, LinkSource::Printer);
}

#[test]
fn the_snapshot_says_whether_an_update_is_offered() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    assert_eq!(p.snapshot(None).firmware_update, FirmwareUpdate::Unavailable);
    fake.offer_firmware_update("1.061", 100);
    let s = p.snapshot(None);
    assert_eq!(s.firmware_update, FirmwareUpdate::Available);
    assert_eq!(s.device.unwrap().firmware, "1.060");
}

#[test]
fn firmware_status_reads_version_and_availability() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let s = p.firmware_status().unwrap();
    assert_eq!((s.version.as_str(), s.update), ("1.060", FirmwareUpdate::Unavailable));
    fake.offer_firmware_update("1.061", 100);
    assert_eq!(p.firmware_status().unwrap().update, FirmwareUpdate::Available);
    assert!(fake.writes().is_empty());
}

#[test]
fn no_update_is_sent_when_none_is_available() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let e = p.update_firmware(Confirm::explicit(), &quick(), &mut |_| {}).unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "{e}");
    assert!(fake.writes().is_empty(), "nothing was sent: {:?}", fake.writes());
}

#[test]
fn an_update_waits_for_the_restart_and_reads_the_new_version() {
    let fake = FakePrinter::start();
    fake.offer_firmware_update("1.061", 700);
    let p = printer(&fake);
    let mut events = Vec::new();
    let report = p.update_firmware(Confirm::explicit(), &quick(), &mut |e| events.push(e)).unwrap();
    assert_eq!(fake.writes(), ["VendorCmd:UpdateFRom/device"]);
    assert!(report.restarted);
    assert_eq!((report.before.as_str(), report.after.as_str()), ("1.060", "1.061"));
    assert_eq!(events.first(), Some(&FirmwareProgress::Sent));
    let at = |e: &FirmwareProgress| events.iter().position(|x| x == e);
    assert!(at(&FirmwareProgress::Restarting).unwrap() < at(&FirmwareProgress::Back).unwrap());
    assert_eq!(fake.firmware_version(), "1.061");
    // Nothing is offered any more.
    assert_eq!(p.firmware_status().unwrap().update, FirmwareUpdate::Unavailable);
}

#[test]
fn a_printer_that_never_restarts_reports_no_update_installed() {
    let fake = FakePrinter::start();
    fake.offer_firmware_update("1.061", 0);
    // The command is accepted but the fake does not restart when it is told 0 ms... make it
    // never restart by taking the update away right after reading the status: use a fake whose
    // update flag is set but whose restart never begins.
    let p = printer(&fake);
    let opts = FirmwareOptions { gone_timeout: Duration::from_millis(300), ..quick() };
    let report = p.update_firmware(Confirm::explicit(), &opts, &mut |_| {}).unwrap();
    // A 0 ms restart is over before the first poll: the printer never looked gone.
    assert!(!report.restarted);
    assert_eq!(report.before, "1.060");
}

#[test]
fn a_printer_that_does_not_return_is_a_timeout() {
    let fake = FakePrinter::start();
    fake.offer_firmware_update("1.061", 60_000);
    let p = printer(&fake);
    let opts = FirmwareOptions { back_timeout: Duration::from_millis(400), ..quick() };
    let e = p.update_firmware(Confirm::explicit(), &opts, &mut |_| {}).unwrap_err();
    assert!(matches!(e, Error::Timeout(_)), "{e}");
}

#[test]
fn extra_cleaning_cards_come_from_the_capability_and_send_cleaning_types() {
    let fake = FakePrinter::start();
    fake.set_cleaning_types(&["roller", "choke", "platen"]);
    let p = printer(&fake);
    let cap = p.maintenance_capability().unwrap();
    assert_eq!(cap.extra_cleanings(), [CleaningType::Roller, CleaningType::Platen]);

    fake.set_job_polls(1);
    let opts = MaintOptions {
        poll_interval: Duration::from_millis(20),
        start_grace: Duration::from_millis(20),
        ..MaintOptions::default()
    };
    let mut p = p;
    p.set_maint_options(opts);
    for kind in [CleaningType::Roller, CleaningType::Platen] {
        fake.clear_log();
        let op = MaintOp::Clean { kind, group: InkGroup::All };
        p.run_maintenance(&op, Confirm::explicit(), &mut |_| {}).unwrap();
        let w = fake.writes();
        assert_eq!(w.first().map(String::as_str), Some("StartJob/maintenance"), "{w:?}");
        assert!(w.iter().any(|l| l == "Cleaning/maintenance"), "{w:?}");
    }
}

// ---- the printer serves model.js only while it is awake -----------------------------------------

#[test]
fn links_are_kept_while_the_printer_is_awake_and_used_while_it_sleeps() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let cache = LocalizationCache::in_dir(dir.path(), "127.0.0.1", fake.chmp.port());
    assert!(cache.links().is_none());

    // Awake: read from the printer and kept.
    let awake = p.web_links_kept(Some("1300"), Language::En, Some(&cache));
    assert!(awake.from_printer && awake.saved_at.is_none(), "{awake:?}");
    let kept = cache.links().expect("a copy was kept");
    assert_eq!(kept.firmware.as_deref(), Some("1.060"));

    // Asleep: the printer answers 404, the kept copy fills in and says when it was saved.
    fake.set_asleep(true);
    fake.clear_log();
    let asleep = p.web_links_kept(Some("1300"), Language::En, Some(&cache));
    assert!(fake.log().iter().any(|l| l == "Web/model.js"), "it was asked: {:?}", fake.log());
    assert!(asleep.from_printer, "{asleep:?}");
    assert_eq!(asleep.saved_at, Some(kept.fetched_at));
    assert_eq!(asleep.ink, awake.ink);
    assert_eq!(asleep.error, awake.error, "the support code still goes in");
    assert_eq!(asleep.ink.source, LinkSource::Maker);
    // Worded for the language asked for even though the copy was read in another.
    let ja = p.web_links_kept(None, Language::Ja, Some(&cache));
    assert!(ja.ink.url.contains("LNG=JP"), "{}", ja.ink.url);
    // Without the kept copy it is the printer's own page, as before.
    let plain = p.web_links(Some("1300"), Language::En);
    assert!(!plain.from_printer && plain.saved_at.is_none());

    // Awake again: read again, and no longer marked as saved. A changed script is kept.
    fake.set_asleep(false);
    fake.set_model_js(Some(
        "var g_ink_url = 'https://links.example.org/new?LNG=EN';\nvar g_sup_url = 'http://example.org/s/';",
    ));
    let woke = p.web_links_kept(None, Language::En, Some(&cache));
    assert!(woke.saved_at.is_none() && woke.ink.url.contains("/new?"), "{}", woke.ink.url);
    assert!(cache.links().unwrap().links.ink.unwrap().contains("/new?"));
    assert!(fake.writes().is_empty());
}

#[test]
fn a_sleeping_printer_with_no_kept_links_falls_back_to_its_page() {
    let fake = FakePrinter::start();
    fake.set_asleep(true);
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let cache = LocalizationCache::in_dir(dir.path(), "127.0.0.1", fake.chmp.port());
    let l = p.web_links_kept(None, Language::En, Some(&cache));
    assert!(!l.from_printer && l.saved_at.is_none());
    assert_eq!(l.ink.source, LinkSource::Printer);
    assert!(cache.links().is_none(), "nothing invented, nothing kept");
    // fetch_links is an error while asleep, and a read while awake.
    assert!(p.fetch_links().is_err());
    fake.set_asleep(false);
    let f = p.fetch_links().unwrap();
    assert!(f.links.ink.is_some() && f.firmware.as_deref() == Some("1.060"));
}

// ---- the firmware update state of a sleeping printer ----------------------------------------------

#[test]
fn a_sleeping_printer_says_unavailable_even_though_an_update_is_offered() {
    let fake = FakePrinter::start();
    fake.offer_firmware_update("1.061", 100);
    let p = printer(&fake);

    // Awake: the update is offered, for real.
    let awake = p.snapshot(None);
    assert_eq!(awake.power, Power::On);
    assert_eq!(
        (awake.firmware_update, awake.firmware_update_remembered),
        (FirmwareUpdate::Available, false)
    );
    let status = p.firmware_status_with(Some(&awake)).unwrap();
    assert_eq!(
        (status.update, status.asleep, status.remembered),
        (FirmwareUpdate::Available, false, false)
    );

    // Asleep: the printer now reports `unavailable`. With the earlier reading the offer is
    // remembered, without one nothing is known; never "up to date".
    fake.set_asleep(true);
    let with_memory = p.snapshot_fresh(None);
    assert_eq!(with_memory.power, Power::Sleeping);
    assert_eq!(with_memory.firmware_update, FirmwareUpdate::Unknown, "nothing cached");
    let mut cached = awake.clone();
    cached.last_updated -= 100;
    let remembered = washi_core::PrinterSnapshot::compose(
        p.observe(Some(&cached), washi_core::unix_now()),
        Some(&cached),
        washi_core::unix_now(),
    );
    assert_eq!(
        (remembered.firmware_update, remembered.firmware_update_remembered),
        (FirmwareUpdate::Available, true)
    );

    let none = p.firmware_status_with(None).unwrap();
    assert_eq!(none.reported.as_deref(), Some("unavailable"), "what the sleeping printer said");
    assert_eq!((none.update, none.asleep, none.remembered), (FirmwareUpdate::Unknown, true, false));
    let some = p.firmware_status_with(Some(&awake)).unwrap();
    assert_eq!(
        (some.update, some.asleep, some.remembered),
        (FirmwareUpdate::Available, true, true)
    );
    // An earlier "up to date" from an awake reading is remembered as such.
    fake.set_asleep(false);
    let fake2 = FakePrinter::start();
    let p2 = printer(&fake2);
    let up_to_date = p2.snapshot(None);
    fake2.set_asleep(true);
    let s = p2.firmware_status_with(Some(&up_to_date)).unwrap();
    assert_eq!((s.update, s.asleep, s.remembered), (FirmwareUpdate::Unavailable, true, true));

    // No update is sent to a sleeping printer on the strength of a remembered offer.
    fake.set_asleep(true);
    let e = p.update_firmware(Confirm::explicit(), &quick(), &mut |_| {}).unwrap_err();
    assert!(e.to_string().contains("auto power-off"), "{e}");
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
}
