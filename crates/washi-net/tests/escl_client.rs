// SPDX-License-Identifier: GPL-2.0-or-later
//! The eSCL client against a scripted in-process HTTP server (no scanner involved).

mod common_server;

use std::time::Duration;

use common_server::{http, serve};
use washi_net::escl::{ColorMode, EsclError, EsclOptions, ScanRegion, ScanSettings};
use washi_net::{Confirm, EsclClient, NetError};

fn client(addr: std::net::SocketAddr) -> EsclClient {
    EsclClient::with_options(
        "127.0.0.1",
        EsclOptions {
            port: addr.port(),
            connect_timeout: Duration::from_secs(2),
            io_timeout: Duration::from_secs(2),
            page_timeout: Duration::from_secs(5),
            busy_timeout: Duration::from_millis(300),
            busy_retry: Duration::from_millis(20),
            ..EsclOptions::default()
        },
    )
}

const CAPS: &str = include_str!("fixtures/escl-capabilities-xk140.xml");
const STATUS: &str = include_str!("fixtures/escl-status-idle.xml");

fn settings() -> ScanSettings {
    ScanSettings {
        region: Some(ScanRegion { x_offset: 0, y_offset: 0, width: 2480, height: 3508 }),
        color_mode: ColorMode::Grayscale8,
        ..ScanSettings::default()
    }
}

#[test]
fn reads_capabilities_and_status() {
    let (addr, seen) = serve(vec![
        http("200 OK", &[("Content-Type", "text/xml")], CAPS.as_bytes()),
        http("200 OK", &[("Content-Type", "text/xml")], STATUS.as_bytes()),
    ]);
    let c = client(addr);
    let caps = c.capabilities().unwrap();
    assert_eq!(caps.platen().unwrap().max_height, 3508);
    assert!(c.status().unwrap().is_idle());
    let s = seen.lock().unwrap();
    assert_eq!((s[0].method.as_str(), s[0].path.as_str()), ("GET", "/eSCL/ScannerCapabilities"));
    assert_eq!((s[1].method.as_str(), s[1].path.as_str()), ("GET", "/eSCL/ScannerStatus"));
    assert_eq!(s[0].header("host"), Some(format!("127.0.0.1:{}", addr.port()).as_str()));
}

#[test]
fn a_platen_scan_is_post_get_get_404() {
    let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 0xFF, 0xD9];
    let (addr, seen) = serve(vec![
        http("201 Created", &[("Location", "/eSCL/ScanJobs/42")], b""),
        http("200 OK", &[("Content-Type", "image/jpeg")], &jpeg),
        http("404 Not Found", &[], b""),
    ]);
    let mut pages = Vec::new();
    let n = client(addr)
        .scan(&settings(), Confirm::explicit(), |d| {
            pages.push(d);
            Ok(())
        })
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(pages[0].data, jpeg);
    assert_eq!(pages[0].content_type, "image/jpeg");
    let s = seen.lock().unwrap();
    assert_eq!((s[0].method.as_str(), s[0].path.as_str()), ("POST", "/eSCL/ScanJobs"));
    assert_eq!(s[0].header("content-type"), Some("text/xml"));
    let body = String::from_utf8_lossy(&s[0].body);
    assert!(body.contains("<pwg:Width>2480</pwg:Width>") && body.contains("Grayscale8"), "{body}");
    assert_eq!(s[1].path, "/eSCL/ScanJobs/42/NextDocument");
    assert_eq!(s[2].path, "/eSCL/ScanJobs/42/NextDocument");
    assert_eq!(s.len(), 3);
}

#[test]
fn absolute_location_urls_and_multi_page_jobs() {
    let (addr, seen) = serve(vec![
        http("201 Created", &[("Location", "http://scanner.example/eSCL/ScanJobs/a-b")], b""),
        http("200 OK", &[("Content-Type", "image/jpeg")], b"one"),
        http("200 OK", &[("Content-Type", "image/jpeg")], b"two"),
        http("404 Not Found", &[], b""),
    ]);
    let n = client(addr).scan(&settings(), Confirm::explicit(), |_| Ok(())).unwrap();
    assert_eq!(n, 2);
    assert_eq!(seen.lock().unwrap()[1].path, "/eSCL/ScanJobs/a-b/NextDocument");
}

#[test]
fn rejected_jobs_explain_themselves() {
    let (addr, _) = serve(vec![http("409 Conflict", &[], b"")]);
    let e = client(addr).create_job(&settings(), Confirm::explicit()).unwrap_err();
    assert!(matches!(e, NetError::Escl(EsclError::JobRejected { status: 409, .. })));
    assert!(e.to_string().contains("occupied"), "{e}");
    let (addr, _) = serve(vec![http("201 Created", &[], b"")]);
    let e = client(addr).create_job(&settings(), Confirm::explicit()).unwrap_err();
    assert!(matches!(e, NetError::Escl(EsclError::NoLocation)));
}

#[test]
fn busy_scanner_is_retried_then_gives_up() {
    let (addr, seen) = serve(vec![
        http("503 Service Unavailable", &[], b""),
        http("201 Created", &[("Location", "/eSCL/ScanJobs/1")], b""),
    ]);
    assert!(client(addr).create_job(&settings(), Confirm::explicit()).is_ok());
    assert_eq!(seen.lock().unwrap().len(), 2);

    let (addr, _) = serve(vec![http("503 Service Unavailable", &[], b""); 40]);
    let e = client(addr).create_job(&settings(), Confirm::explicit()).unwrap_err();
    assert!(matches!(e, NetError::Escl(EsclError::Busy)), "{e}");
}

#[test]
fn a_failing_page_cancels_the_job() {
    let (addr, seen) = serve(vec![
        http("201 Created", &[("Location", "/eSCL/ScanJobs/9")], b""),
        http("200 OK", &[("Content-Type", "image/jpeg")], b"page"),
        http("200 OK", &[], b""),
    ]);
    let e = client(addr)
        .scan(&settings(), Confirm::explicit(), |_| Err(NetError::Http("disk full".into())))
        .unwrap_err();
    assert!(e.to_string().contains("disk full"));
    let s = seen.lock().unwrap();
    assert_eq!((s[2].method.as_str(), s[2].path.as_str()), ("DELETE", "/eSCL/ScanJobs/9"));
}

#[test]
fn cancel_treats_a_vanished_job_as_done() {
    let (addr, seen) = serve(vec![http("404 Not Found", &[], b""), http("200 OK", &[], b"")]);
    let c = client(addr);
    let job = washi_net::escl::ScanJob { path: "/eSCL/ScanJobs/5".into() };
    assert!(c.cancel(&job, Confirm::explicit()).is_ok());
    assert!(c.cancel(&job, Confirm::explicit()).is_ok());
    assert_eq!(seen.lock().unwrap()[0].method, "DELETE");
}
