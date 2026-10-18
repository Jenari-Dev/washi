// SPDX-License-Identifier: GPL-2.0-or-later
//! The printer checks of `washi doctor` against the in-process fake printer: what they say for a
//! healthy printer, a sleeping one and one that does not answer, and that nothing is written and
//! no address is printed.

use std::time::Duration;

use washi_core::doctor::{Report, Status, printer_checks};
use washi_core::net::ClientOptions;
use washi_core::{Language, LocalizationCache, Printer};
use washi_fake::FakePrinter;

fn printer(fake: &FakePrinter) -> Printer {
    let net = ClientOptions {
        port: fake.chmp.port(),
        connect_timeout: Duration::from_millis(300),
        io_timeout: Duration::from_secs(2),
        ..ClientOptions::default()
    };
    let mut p = Printer::with_options("127.0.0.1", net);
    p.set_ipp_port(fake.ipp.port());
    p
}

fn status(v: &[washi_core::doctor::Check], id: &str) -> Status {
    v.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no check {id}")).status
}

#[test]
fn a_healthy_printer_passes_every_channel_and_keeps_its_links() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let cache = LocalizationCache::in_dir(dir.path(), "127.0.0.1", fake.chmp.port());
    let v = printer_checks(Language::En, &p, Some(&cache));
    for id in ["printer.chmp", "printer.ipp", "printer.escl", "printer.links"] {
        assert_eq!(status(&v, id), Status::Ok, "{id}: {v:?}");
    }
    let chmp = v.iter().find(|c| c.id == "printer.chmp").unwrap();
    assert!(chmp.detail.contains("XK140") && chmp.detail.contains("1.060"), "{}", chmp.detail);
    // Words and picture are only counted as kept once something has read them.
    assert_eq!(status(&v, "printer.strings"), Status::Skip);
    assert_eq!(status(&v, "printer.icon"), Status::Skip);
    assert!(cache.links().is_some(), "the web links were kept on the way");
    assert!(fake.writes().is_empty(), "a check writes nothing: {:?}", fake.writes());

    // Once kept they are reported as kept.
    cache
        .store_strings(
            &washi_core::FetchedStrings {
                language: "ja".into(),
                strings: Default::default(),
                firmware: None,
            },
            1,
        )
        .unwrap();
    let v = printer_checks(Language::En, &p, Some(&cache));
    assert_eq!(status(&v, "printer.strings"), Status::Ok);

    // Japanese texts, same checks.
    let ja = printer_checks(Language::Ja, &p, Some(&cache));
    assert_eq!(ja.len(), v.len());
    assert!(ja.iter().all(|c| !c.label.is_ascii()), "{ja:?}");
}

#[test]
fn a_sleeping_printer_still_answers_but_its_links_come_from_the_kept_copy() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let cache = LocalizationCache::in_dir(dir.path(), "127.0.0.1", fake.chmp.port());
    printer_checks(Language::En, &p, Some(&cache));
    fake.set_asleep(true);
    let v = printer_checks(Language::En, &p, Some(&cache));
    assert_eq!(status(&v, "printer.chmp"), Status::Ok);
    let links = v.iter().find(|c| c.id == "printer.links").unwrap();
    assert_eq!(links.status, Status::Ok);
    assert!(links.detail.contains("earlier read"), "{}", links.detail);
    // Without a kept copy the links are simply not available: optional, not a problem.
    let empty = tempfile::tempdir().unwrap();
    let none = LocalizationCache::in_dir(empty.path(), "127.0.0.1", fake.chmp.port());
    let v = printer_checks(Language::En, &p, Some(&none));
    assert_eq!(status(&v, "printer.links"), Status::Skip);
}

#[test]
fn a_printer_that_does_not_answer_fails_the_channels_without_naming_its_address() {
    let fake = FakePrinter::start();
    let p = printer(&fake);
    fake.set_chmp_down(true);
    fake.set_ipp_down(true);
    let v = printer_checks(Language::En, &p, None);
    for id in ["printer.chmp", "printer.ipp", "printer.escl"] {
        let c = v.iter().find(|c| c.id == id).unwrap();
        assert_eq!(c.status, Status::Fail, "{id}: {c:?}");
        assert!(!c.hint.is_empty());
        assert!(!c.detail.contains("127.0.0.1"), "no address in {:?}", c.detail);
    }
    let report = Report::new(
        "washi",
        Language::En,
        vec![washi_core::doctor::Group::new("printer", "Printer", v)],
    );
    assert!(report.summary.fail >= 3);
    let text = report.render_text();
    assert!(!text.contains("127.0.0.1"), "{text}");
}
