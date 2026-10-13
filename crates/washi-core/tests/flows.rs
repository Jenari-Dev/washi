// SPDX-License-Identifier: GPL-2.0-or-later
//! Maintenance and settings flows against a scripted in-process printer.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use washi_core::ivec::{AutoPowerOff, CleaningType, DeviceSettings, InkGroup, OnOff};
use washi_core::net::ClientOptions;
use washi_core::{Confirm, Error, MaintOp, MaintOptions, Printer, Progress};

const CAP: &str = include_str!("../../washi-ivec/tests/fixtures/GetCapability-device.xml");
const CFG: &str = include_str!("../../washi-ivec/tests/fixtures/GetConfiguration-device.xml");
const PRINT: &str = include_str!("../../washi-ivec/tests/fixtures/GetStatus-print.xml");
const MAINT: &str = include_str!("../../washi-ivec/tests/fixtures/GetStatus-maintenance.xml");

type Log = Arc<Mutex<Vec<String>>>;
type Responder = dyn Fn(&str, &str, usize) -> String + Send + Sync;

fn ok_reply(op: &str, service: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><cmd xmlns:ivec=\"http://www.canon.com/ns/cmd/2008/07/common/\"><ivec:contents>\
         <ivec:operation>{op}Response</ivec:operation><ivec:param_set servicetype=\"{service}\">\
         <ivec:response>OK</ivec:response><ivec:response_detail/></ivec:param_set></ivec:contents></cmd>"
    )
}

fn ng_reply(op: &str, service: &str, detail: &str) -> String {
    ok_reply(op, service).replace(
        "<ivec:response>OK</ivec:response><ivec:response_detail/>",
        &format!(
            "<ivec:response>NG</ivec:response><ivec:response_detail>{detail}</ivec:response_detail>"
        ),
    )
}

fn print_status(status: &str) -> String {
    PRINT
        .replace("<ivec:status>idle</ivec:status>", &format!("<ivec:status>{status}</ivec:status>"))
}

fn maint_running() -> String {
    MAINT.replace(
        "<ivec:jobinfo/>",
        "<ivec:jobinfo><ivec:jobID>00000002</ivec:jobID></ivec:jobinfo>",
    )
}

fn chunked(body: &str) -> Vec<u8> {
    let mut out = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    if !body.is_empty() {
        out.extend_from_slice(format!("{:X}\r\n{body}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"0\r\n\r\n");
    out
}

/// Starts a fake printer; `responder(operation, service, nth_call_of_that_pair)` yields the reply.
fn spawn(responder: Arc<Responder>) -> (SocketAddr, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let log: Log = Arc::default();
    let counts: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
    let log2 = log.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let (r, l, c) = (responder.clone(), log2.clone(), counts.clone());
            thread::spawn(move || serve(stream, r, l, c));
        }
    });
    (addr, log)
}

fn serve(
    stream: TcpStream,
    responder: Arc<Responder>,
    log: Log,
    counts: Arc<Mutex<HashMap<String, usize>>>,
) {
    // A reply belongs to the connection that carried the command (as on the real printer).
    let mut pending: Option<String> = None;
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut out = stream;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let method = line.split(' ').next().unwrap().to_string();
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).unwrap();
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                if k.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap();
                }
            }
        }
        let mut body = vec![0; len];
        reader.read_exact(&mut body).unwrap();
        let body = String::from_utf8(body).unwrap();
        let reply = if method == "POST" {
            if !body.is_empty() {
                let op = body
                    .split("<ivec:operation>")
                    .nth(1)
                    .unwrap()
                    .split("</ivec:operation>")
                    .next()
                    .unwrap();
                let svc = body.split("servicetype=\"").nth(1).unwrap().split('"').next().unwrap();
                let key = format!("{op}/{svc}");
                let n = {
                    let mut c = counts.lock().unwrap();
                    let e = c.entry(key.clone()).or_insert(0);
                    let n = *e;
                    *e += 1;
                    n
                };
                log.lock().unwrap().push(key);
                pending = Some(responder(op, svc, n));
            }
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()
        } else if let Some(reply) = &pending {
            chunked(reply)
        } else {
            b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\n\r\n".to_vec()
        };
        if out.write_all(&reply).is_err() {
            return;
        }
    }
}

fn printer(addr: SocketAddr) -> Printer {
    let mut p = Printer::with_options(
        "127.0.0.1",
        ClientOptions {
            port: addr.port(),
            read_timeout: Duration::from_millis(400),
            poll_initial: Duration::from_millis(2),
            poll_max: Duration::from_millis(5),
            ..ClientOptions::default()
        },
    );
    p.set_maint_options(MaintOptions {
        timeout: Duration::from_secs(10),
        poll_interval: Duration::from_millis(5),
        start_grace: Duration::from_millis(20),
        stuck_job_grace: Duration::from_millis(60),
    });
    p
}

fn ops(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

fn strip_polls(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in v {
        if s.starts_with("GetStatus/") && out.last().is_some_and(|l| l.starts_with("GetStatus/")) {
            continue;
        }
        out.push(s);
    }
    out
}

#[test]
fn nozzle_check_runs_start_op_poll_end() {
    let (addr, log) = spawn(Arc::new(|op, svc, n| match (op, svc) {
        ("GetStatus", "print") => print_status(if n == 0 { "printing" } else { "idle" }),
        ("GetStatus", "maintenance") => MAINT.to_string(),
        _ => ok_reply(op, svc),
    }));
    let mut events = Vec::new();
    let rep = printer(addr)
        .run_maintenance(&MaintOp::NozzleCheck, Confirm::explicit(), &mut |p| events.push(p))
        .unwrap();
    assert_eq!(rep.operation, "nozzle check");

    let seq = strip_polls(ops(&log));
    assert_eq!(
        seq,
        ["StartJob/maintenance", "TestPrint/maintenance", "GetStatus/print", "EndJob/maintenance"]
    );
    assert!(matches!(events.first(), Some(Progress::JobStarted)));
    assert!(matches!(events[1], Progress::CommandAccepted { .. }));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Progress::Waiting { status, .. } if status == "printing"))
    );
    assert!(matches!(events[events.len() - 2], Progress::JobEnded));
    assert!(matches!(events.last(), Some(Progress::Ending)));
}

/// Real XK140 behaviour (first nozzle check, 2026-10-13): while the maintenance job runs,
/// `GetStatus print` is answered with `NG Suspended`. That must count as busy, not end the job.
#[test]
fn print_service_suspended_during_the_job_means_busy() {
    let (addr, log) = spawn(Arc::new(|op, svc, n| match (op, svc) {
        ("GetStatus", "print") if n < 3 => ng_reply(op, svc, "Suspended"),
        ("GetStatus", "print") => print_status("idle"),
        ("GetStatus", "maintenance") => MAINT.to_string(),
        _ => ok_reply(op, svc),
    }));
    let mut events = Vec::new();
    printer(addr)
        .run_maintenance(&MaintOp::NozzleCheck, Confirm::explicit(), &mut |p| events.push(p))
        .unwrap();
    let print_polls = ops(&log).iter().filter(|s| *s == "GetStatus/print").count();
    assert!(print_polls >= 5, "stopped polling after {print_polls} polls");
    assert_eq!(ops(&log).last().unwrap(), "EndJob/maintenance");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Progress::Waiting { status, .. } if status == "suspended"))
    );
}

/// Real XK140 trace (2026-10-13): `processing` + jobinfo while the pattern prints, `idle` + empty
/// jobinfo afterwards, and the printer has already closed the job, so `EndJob` gets `NG NotStart`.
#[test]
fn printer_closed_job_itself_end_job_not_start_is_success() {
    let (addr, log) = spawn(Arc::new(|op, svc, n| match (op, svc) {
        ("GetStatus", "print") => print_status(if n < 4 { "processing" } else { "idle" }),
        ("GetStatus", "maintenance") if n < 4 => maint_running(),
        ("GetStatus", "maintenance") => MAINT.to_string(),
        ("EndJob", _) => ng_reply(op, svc, "NotStart"),
        _ => ok_reply(op, svc),
    }));
    let rep = printer(addr)
        .run_maintenance(&MaintOp::NozzleCheck, Confirm::explicit(), &mut |_| {})
        .unwrap();
    assert_eq!(rep.operation, "nozzle check");
    assert_eq!(ops(&log).last().unwrap(), "EndJob/maintenance");
}

#[test]
fn refused_operation_still_ends_the_job() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| {
        if op == "Cleaning" { ng_reply(op, svc, "DeviceBusy") } else { ok_reply(op, svc) }
    }));
    let err = printer(addr)
        .run_maintenance(
            &MaintOp::Clean { kind: CleaningType::Deep, group: InkGroup::All },
            Confirm::explicit(),
            &mut |_| {},
        )
        .unwrap_err();
    assert!(err.to_string().contains("DeviceBusy"), "{err}");
    assert_eq!(ops(&log), ["StartJob/maintenance", "Cleaning/maintenance", "EndJob/maintenance"]);
}

#[test]
fn start_job_refusal_sends_nothing_else() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| ng_reply(op, svc, "DeviceBusy")));
    let err = printer(addr)
        .run_maintenance(&MaintOp::NozzleCheck, Confirm::explicit(), &mut |_| {})
        .unwrap_err();
    assert!(err.to_string().contains("StartJob"), "{err}");
    assert_eq!(ops(&log), ["StartJob/maintenance"]);
}

#[test]
fn timeout_still_ends_the_job() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| match (op, svc) {
        ("GetStatus", "print") => print_status("printing"),
        ("GetStatus", "maintenance") => MAINT.to_string(),
        _ => ok_reply(op, svc),
    }));
    let mut p = printer(addr);
    p.set_maint_options(MaintOptions {
        timeout: Duration::from_millis(120),
        ..MaintOptions {
            timeout: Duration::ZERO,
            poll_interval: Duration::from_millis(5),
            start_grace: Duration::from_millis(20),
            stuck_job_grace: Duration::from_millis(60),
        }
    });
    let err =
        p.run_maintenance(&MaintOp::RollerClean, Confirm::explicit(), &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Timeout(_)), "{err}");
    assert_eq!(ops(&log).last().unwrap(), "EndJob/maintenance");
}

#[test]
fn idle_printer_with_a_lingering_job_listing_completes_after_the_grace() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| match (op, svc) {
        ("GetStatus", "print") => print_status("idle"),
        ("GetStatus", "maintenance") => maint_running(),
        _ => ok_reply(op, svc),
    }));
    printer(addr).run_maintenance(&MaintOp::AlignCheck, Confirm::explicit(), &mut |_| {}).unwrap();
    assert_eq!(ops(&log).last().unwrap(), "EndJob/maintenance");
}

fn settings_cfg_with_dry(level: u8) -> String {
    CFG.replace(
        "<ivec:drylevel>3</ivec:drylevel>",
        &format!("<ivec:drylevel>{level}</ivec:drylevel>"),
    )
}

#[test]
fn settings_flow_validates_writes_and_reads_back() {
    let (addr, log) = spawn(Arc::new(|op, svc, n| match (op, svc) {
        ("GetCapability", "device") => CAP.to_string(),
        ("GetConfiguration", "device") => settings_cfg_with_dry(if n == 0 { 3 } else { 4 }),
        _ => ok_reply(op, svc),
    }));
    let changes = DeviceSettings { drylevel: Some(4), ..Default::default() };
    let rep = printer(addr).apply_device_settings(&changes, Confirm::explicit()).unwrap();
    assert!(rep.changed);
    assert_eq!(rep.before.drylevel, Some(3));
    assert_eq!(rep.after.drylevel, Some(4));
    assert_eq!(
        ops(&log),
        [
            "GetCapability/device",
            "GetConfiguration/device",
            "StartJob/device",
            "SetConfiguration/device",
            "EndJob/device",
            "GetConfiguration/device"
        ]
    );
}

#[test]
fn settings_mismatch_after_readback_is_an_error() {
    let (addr, _log) = spawn(Arc::new(|op, svc, _| match (op, svc) {
        ("GetCapability", "device") => CAP.to_string(),
        ("GetConfiguration", "device") => settings_cfg_with_dry(3),
        _ => ok_reply(op, svc),
    }));
    let changes = DeviceSettings { drylevel: Some(5), ..Default::default() };
    let err = printer(addr).apply_device_settings(&changes, Confirm::explicit()).unwrap_err();
    match err {
        Error::Mismatch(m) => {
            assert_eq!(m.len(), 1);
            assert_eq!(
                (m[0].field, m[0].wanted.as_str(), m[0].actual.as_str()),
                ("drylevel", "5", "3")
            );
        }
        other => panic!("expected mismatch, got {other}"),
    }
}

#[test]
fn invalid_settings_never_reach_the_write_path() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| match (op, svc) {
        ("GetCapability", "device") => CAP.to_string(),
        _ => ok_reply(op, svc),
    }));
    let changes = DeviceSettings {
        autopoweroff: Some(AutoPowerOff { mode: OnOff::On, time: Some(45) }),
        ..Default::default()
    };
    let err = printer(addr).apply_device_settings(&changes, Confirm::explicit()).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err}");
    assert_eq!(ops(&log), ["GetCapability/device"]);
}

#[test]
fn settings_already_applied_write_nothing() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| match (op, svc) {
        ("GetCapability", "device") => CAP.to_string(),
        ("GetConfiguration", "device") => CFG.to_string(),
        _ => ok_reply(op, svc),
    }));
    let changes = DeviceSettings { drylevel: Some(3), ..Default::default() };
    let rep = printer(addr).apply_device_settings(&changes, Confirm::explicit()).unwrap();
    assert!(!rep.changed);
    assert_eq!(ops(&log), ["GetCapability/device", "GetConfiguration/device"]);
}

#[test]
fn failed_set_configuration_still_ends_the_job() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| match (op, svc) {
        ("GetCapability", "device") => CAP.to_string(),
        ("GetConfiguration", "device") => CFG.to_string(),
        ("SetConfiguration", _) => ng_reply(op, svc, "DataError"),
        _ => ok_reply(op, svc),
    }));
    let changes = DeviceSettings { drylevel: Some(1), ..Default::default() };
    let err = printer(addr).apply_device_settings(&changes, Confirm::explicit()).unwrap_err();
    assert!(err.to_string().contains("DataError"), "{err}");
    let seq = ops(&log);
    assert_eq!(&seq[seq.len() - 2..], ["SetConfiguration/device", "EndJob/device"]);
}

#[test]
fn power_off_is_a_single_device_request() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| ok_reply(op, svc)));
    printer(addr).power_off(Confirm::explicit()).unwrap();
    assert_eq!(ops(&log), ["PowerOff/device"]);
}

#[test]
fn power_on_is_a_single_device_request_without_a_job_bracket() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| ok_reply(op, svc)));
    printer(addr).power_on(Confirm::explicit()).unwrap();
    assert_eq!(ops(&log), ["PowerOn/device"]);
}

#[test]
fn power_on_refused_by_a_sleeping_printer_is_an_error() {
    let (addr, log) = spawn(Arc::new(|op, svc, _| {
        if op == "PowerOn" { ng_reply(op, svc, "Suspended") } else { ok_reply(op, svc) }
    }));
    let e = printer(addr).power_on(Confirm::explicit()).unwrap_err();
    assert!(
        e.to_string().to_lowercase().contains("asleep") || e.to_string().contains("Suspended"),
        "{e}"
    );
    assert_eq!(ops(&log), ["PowerOn/device"]);
}
