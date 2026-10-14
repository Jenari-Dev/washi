// SPDX-License-Identifier: GPL-2.0-or-later
//! The printer's own words and picture, read from the in-process fake printer. The fake serves
//! strings and a picture written for these tests.

use std::time::Duration;

use washi_core::net::ClientOptions;
use washi_core::{LocalizationCache, Printer};
use washi_fake::FakePrinter;

const EN: &str = "/* made up for the tests */\n\
    \"media-type.test-glossy\" = \"Test Glossy Paper\";\n\
    \"media-type.test-matte\" = \"Test Matte Paper\";\n\
    \"media-source.test-tray\" = \"Test Tray\";\n";
const JA: &str = "\"media-type.test-glossy\" = \"\u{30c6}\u{30b9}\u{30c8}\u{5149}\u{6ca2}\u{7d19}\";\n\
    \"media-type.test-matte\" = \"\u{30c6}\u{30b9}\u{30c8}\u{30de}\u{30c3}\u{30c8}\u{7d19}\";\n";

fn printer(fake: &FakePrinter) -> Printer {
    let net = ClientOptions {
        port: fake.chmp.port(),
        connect_timeout: Duration::from_millis(500),
        io_timeout: Duration::from_secs(5),
        ..ClientOptions::default()
    };
    let mut p = Printer::with_options("127.0.0.1", net);
    p.set_ipp_port(fake.ipp.port());
    p
}

fn with_strings() -> FakePrinter {
    let fake = FakePrinter::start();
    fake.set_strings("en", Some(EN.as_bytes()));
    fake.set_strings("ja", Some(JA.as_bytes()));
    fake
}

/// A picture on a transparent ground, as the printers' own are.
fn picture(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(w, h);
    for y in h / 4..h * 3 / 4 {
        for x in w / 10..w * 9 / 10 {
            img.put_pixel(x, y, image::Rgba([240, 240, 240, 255]));
        }
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

#[test]
fn strings_are_read_per_language_with_reads_only() {
    let fake = with_strings();
    let p = printer(&fake);
    let en = p.fetch_strings("en").unwrap();
    assert_eq!(en.language, "en");
    assert_eq!(en.strings["media-type.test-glossy"], "Test Glossy Paper");
    assert_eq!(en.strings["media-source.test-tray"], "Test Tray");
    assert_eq!(en.firmware.as_deref(), Some("1.060"));
    let ja = p.fetch_strings("ja").unwrap();
    assert_eq!(
        ja.strings["media-type.test-glossy"],
        "\u{30c6}\u{30b9}\u{30c8}\u{5149}\u{6ca2}\u{7d19}"
    );
    // The log shows what was asked for: the attributes, then one file per language.
    let log = fake.log();
    assert!(log.iter().any(|l| l == "Web/strings/en") && log.iter().any(|l| l == "Web/strings/ja"));
    assert_eq!(log.iter().filter(|l| l.as_str() == "IPP/Get-Printer-Attributes").count(), 2);
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
}

#[test]
fn a_language_the_printer_has_no_file_for_gives_an_empty_result() {
    let fake = with_strings();
    let p = printer(&fake);
    let fr = p.fetch_strings("fr").unwrap();
    assert!(fr.strings.is_empty());
    assert!(!fake.log().iter().any(|l| l.starts_with("Web/strings/")), "no file was asked for");
    // A printer without any strings is the same.
    let bare = FakePrinter::start();
    assert!(printer(&bare).fetch_strings("en").unwrap().strings.is_empty());
    // And a bad language tag is refused before anything is sent.
    bare.clear_log();
    assert!(printer(&bare).fetch_strings("en\r\nX").is_err());
    assert!(bare.log().is_empty(), "{:?}", bare.log());
}

#[test]
fn an_address_that_ignores_the_language_is_followed_through_the_language_list() {
    let fake = with_strings();
    fake.set_strings_one_address(true);
    // Asked in Japanese, the fake names the first listed file (English): Washi derives ja.txt
    // because the printer lists ja.
    let ja = printer(&fake).fetch_strings("ja").unwrap();
    assert_eq!(
        ja.strings["media-type.test-matte"],
        "\u{30c6}\u{30b9}\u{30c8}\u{30de}\u{30c3}\u{30c8}\u{7d19}"
    );
    // A language it does not list gets nothing, not the English file in its place.
    assert!(printer(&fake).fetch_strings("de").unwrap().strings.is_empty());
}

#[test]
fn the_picture_is_the_largest_and_has_its_margin_cut() {
    let fake = FakePrinter::start();
    fake.set_icons(vec![
        ("printer_icon.png".into(), picture(128, 128)),
        ("printer_icon_large.png".into(), picture(512, 512)),
    ]);
    let icon = printer(&fake).fetch_icon().unwrap();
    let png = icon.png.unwrap();
    // 512 x 512 with a 409 x 256 picture in it: with a border of 10 the cut one is 429 x 276.
    assert_eq!((icon.width, icon.height), (429, 276));
    assert_eq!(washi_core::net::web::png_size(&png), Some((429, 276)));
    assert_eq!(icon.firmware.as_deref(), Some("1.060"));
    let log = fake.log();
    assert!(log.iter().any(|l| l == "Web/icon/printer_icon_large.png"), "{log:?}");
    assert!(fake.writes().is_empty());
}

#[test]
fn a_printer_without_a_picture_or_with_a_broken_one() {
    let fake = FakePrinter::start();
    let none = printer(&fake).fetch_icon().unwrap();
    assert_eq!(none.png, None);
    // Listed but not served: an error, so that the caller tries again later.
    fake.set_icons(vec![(
        "printer_icon.png".into(),
        b"not a png at all, just text bytes!!!".to_vec(),
    )]);
    assert!(printer(&fake).fetch_icon().is_err());
}

#[test]
fn what_is_read_can_be_kept_and_found_again() {
    let fake = with_strings();
    fake.set_icons(vec![("printer_icon_large.png".into(), picture(512, 512))]);
    let p = printer(&fake);
    let dir = tempfile::tempdir().unwrap();
    let cache = LocalizationCache::in_dir(dir.path(), p.host(), p.chmp_port());
    cache.store_strings(&p.fetch_strings("ja").unwrap(), 1_000).unwrap();
    cache.store_icon(&p.fetch_icon().unwrap(), 1_000).unwrap();
    let s = cache.strings("ja").unwrap();
    assert!(s.is_current(Some("1.060"), 2_000));
    assert!(!s.is_current(Some("1.061"), 2_000), "a new firmware reads them again");
    assert_eq!(s.strings.len(), 2);
    let i = cache.icon().unwrap();
    assert!(i.path.unwrap().is_file());
}
