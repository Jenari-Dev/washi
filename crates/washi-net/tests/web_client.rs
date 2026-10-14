// SPDX-License-Identifier: GPL-2.0-or-later
//! The printer's web script client against a scripted in-process HTTP server.

mod common_server;

use std::time::Duration;

use common_server::{http, serve};
use washi_net::{NetError, WebOptions, fetch_icon, fetch_model_links, fetch_strings};

const SAMPLE: &str = include_str!("fixtures/model-sample.js");

fn opts(lang: &str) -> WebOptions {
    WebOptions {
        connect_timeout: Duration::from_secs(2),
        io_timeout: Duration::from_secs(2),
        accept_language: lang.to_string(),
    }
}

#[test]
fn the_script_is_asked_for_in_a_language_and_its_links_are_read() {
    let (addr, seen) = serve(vec![http("200 OK", &[], SAMPLE.as_bytes())]);
    let links = fetch_model_links("127.0.0.1", addr.port(), &opts("ja")).unwrap();
    assert!(links.ink.as_deref().unwrap().starts_with("https://links.example.org/"));
    assert!(links.error.as_deref().unwrap().ends_with("&ERR=NO"));
    let s = &seen.lock().unwrap()[0];
    assert_eq!((s.method.as_str(), s.path.as_str()), ("GET", "/JS_MDL/model.js"));
    // The printer answers 404 to a request without a language: this one carries it.
    assert_eq!(s.header("accept-language"), Some("ja"));
    assert!(s.body.is_empty());
}

#[test]
fn a_missing_script_is_an_error_and_a_script_without_links_is_empty() {
    let (addr, _) = serve(vec![http("404 Not Found", &[], b"A specification file is not found")]);
    let e = fetch_model_links("127.0.0.1", addr.port(), &opts("en")).unwrap_err();
    assert!(matches!(e, NetError::Status { status: 404 }), "{e}");
    let (addr, _) = serve(vec![http("200 OK", &[], b"var g_cur_tank_num=5;")]);
    assert!(fetch_model_links("127.0.0.1", addr.port(), &opts("en")).unwrap().is_empty());
}

// ---- the printer's own words and picture ---------------------------------------------------------

/// Strings written for these tests (not a printer's).
const STRINGS_EN: &str = "/* test */\n\"media-type.test-one\" = \"Test Paper One\";\n\"media-type.test-two\" = \"Test \\\"Two\\\"\";\n";

/// The first bytes of a PNG of this size, then padding (the client only reads the header).
fn png(w: u32, h: u32, padding: usize) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    b.extend(b"IHDR");
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b.extend(vec![0; padding]);
    b
}

#[test]
fn a_strings_file_is_read_from_the_printers_own_server() {
    let (addr, seen) = serve(vec![http("200 OK", &[], STRINGS_EN.as_bytes())]);
    let m = fetch_strings("127.0.0.1", addr.port(), "/strings/en.txt", &opts("en")).unwrap();
    assert_eq!(m["media-type.test-one"], "Test Paper One");
    assert_eq!(m["media-type.test-two"], "Test \"Two\"");
    let s = &seen.lock().unwrap()[0];
    assert_eq!((s.method.as_str(), s.path.as_str()), ("GET", "/strings/en.txt"));
    assert_eq!(s.header("accept-language"), Some("en"));
    assert!(s.body.is_empty());
}

#[test]
fn a_utf16_file_and_a_japanese_one_are_read() {
    let text = "\"media-type.test-one\" = \"\u{30c6}\u{30b9}\u{30c8}\u{7528}\u{7d19}\";\n";
    let mut utf16 = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        utf16.extend(u.to_le_bytes());
    }
    let (addr, _) = serve(vec![http("200 OK", &[], &utf16)]);
    let m = fetch_strings("127.0.0.1", addr.port(), "/strings/ja.txt", &opts("ja")).unwrap();
    assert_eq!(m["media-type.test-one"], "\u{30c6}\u{30b9}\u{30c8}\u{7528}\u{7d19}");
}

#[test]
fn a_missing_or_broken_strings_file_is_an_error() {
    let (addr, _) = serve(vec![http("404 Not Found", &[], b"nope")]);
    let e = fetch_strings("127.0.0.1", addr.port(), "/strings/xx.txt", &opts("en")).unwrap_err();
    assert!(matches!(e, NetError::Status { status: 404 }), "{e}");
    let (addr, _) = serve(vec![http("200 OK", &[], b"this is not a strings file")]);
    let e = fetch_strings("127.0.0.1", addr.port(), "/strings/en.txt", &opts("en")).unwrap_err();
    assert!(matches!(e, NetError::Strings(_)), "{e}");
    // A file that is too large is not read to the end.
    let big = vec![b' '; washi_net::strings::MAX_STRINGS_BYTES + 10];
    let (addr, _) = serve(vec![http("200 OK", &[], &big)]);
    assert!(fetch_strings("127.0.0.1", addr.port(), "/strings/en.txt", &opts("en")).is_err());
}

#[test]
fn only_plain_paths_are_requested() {
    // No server is needed: the request is refused before a connection is made.
    for bad in ["http://elsewhere.example/strings/en.txt", "/a/../b", "strings/en.txt", "/a b"] {
        let e = fetch_strings("127.0.0.1", 9, bad, &opts("en")).unwrap_err();
        assert!(matches!(e, NetError::Http(_)), "{bad:?}: {e}");
    }
}

#[test]
fn the_largest_picture_wins_and_a_bad_one_is_skipped() {
    // The large one is asked for first and is big enough to end the search.
    let (addr, seen) = serve(vec![http("200 OK", &[], &png(512, 512, 100))]);
    let paths =
        vec!["/icon/printer_icon.png".to_string(), "/icon/printer_icon_large.png".to_string()];
    let icon = fetch_icon("127.0.0.1", addr.port(), &paths, &opts("en")).unwrap();
    assert_eq!((icon.width, icon.height), (512, 512));
    assert_eq!(icon.png.len(), 33 + 100);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, "/icon/printer_icon_large.png");
    drop(seen);

    // Two small ones: both are looked at and the bigger one is kept; a GIF is skipped.
    let (addr, seen) = serve(vec![
        http("200 OK", &[], &png(64, 64, 0)),
        http("200 OK", &[], b"GIF89a-not-a-png-at-all-and-longer-than-33-bytes!!"),
        http("200 OK", &[], &png(128, 96, 0)),
    ]);
    let paths: Vec<String> = ["/i/a.png", "/i/b.png", "/i/c.png"].map(String::from).into();
    let icon = fetch_icon("127.0.0.1", addr.port(), &paths, &opts("en")).unwrap();
    assert_eq!((icon.width, icon.height), (128, 96));
    assert_eq!(seen.lock().unwrap().len(), 3);
}

#[test]
fn no_usable_picture_is_an_error() {
    let (addr, _) = serve(vec![http("404 Not Found", &[], b"")]);
    let paths = vec!["/icon/a.png".to_string()];
    let e = fetch_icon("127.0.0.1", addr.port(), &paths, &opts("en")).unwrap_err();
    assert!(matches!(e, NetError::Status { status: 404 }), "{e}");
    let (addr, _) = serve(vec![http("200 OK", &[], &png(5000, 5000, 0))]);
    let e = fetch_icon("127.0.0.1", addr.port(), &paths, &opts("en")).unwrap_err();
    assert!(matches!(e, NetError::Icon(_)), "{e}");
    let e = fetch_icon("127.0.0.1", 9, &[], &opts("en")).unwrap_err();
    assert!(matches!(e, NetError::Http(_)), "{e}");
}
