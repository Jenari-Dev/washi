// SPDX-License-Identifier: GPL-2.0-or-later
//! The printer's web script client against a scripted in-process HTTP server.

mod common_server;

use std::time::Duration;

use common_server::{http, serve};
use washi_net::{NetError, WebOptions, fetch_model_links};

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
