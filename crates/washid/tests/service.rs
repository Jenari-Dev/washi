// SPDX-License-Identifier: GPL-2.0-or-later
//! The daemon's service layer against the in-process fake printer: snapshots, read-only mode,
//! maintenance jobs, settings, serialisation of device access, polling and notifications.
//! Nothing here talks to a real printer.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;
use tokio::sync::broadcast::Receiver;
use washi_core::net::{ClientOptions, IppOptions};
use washi_core::{
    FirmwareOptions, Language, LanguageSetting, LocalizationCache, MaintOptions, Power, Printer,
    PrinterSnapshot, split_host_port,
};
use washi_fake::FakePrinter;
use washid::notify::{Notification, Notifier};
use washid::service::{Event, Options, PrinterFactory, Service, ServiceError, Source};

struct Recorder(Mutex<Vec<Notification>>);

impl Notifier for Recorder {
    fn send(&self, n: Notification) {
        self.0.lock().unwrap().push(n);
    }
}

impl Recorder {
    fn keys(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().map(|n| n.key.clone()).collect()
    }
}

struct Rig {
    svc: Arc<Service>,
    host: String,
    dir: TempDir,
}

fn factory(fake_ipp_port: u16) -> PrinterFactory {
    Arc::new(move |host: &str| {
        let (h, port) = split_host_port(host);
        let mut p = Printer::with_options(
            h,
            ClientOptions {
                port: port.unwrap_or(80),
                connect_timeout: Duration::from_millis(500),
                io_timeout: Duration::from_secs(2),
                read_timeout: Duration::from_secs(2),
                poll_initial: Duration::from_millis(2),
                poll_max: Duration::from_millis(5),
                ..ClientOptions::default()
            },
        );
        p.set_ipp_options(IppOptions {
            port: fake_ipp_port,
            connect_timeout: Duration::from_millis(500),
            io_timeout: Duration::from_secs(2),
            ..IppOptions::default()
        });
        p.set_maint_options(MaintOptions {
            timeout: Duration::from_secs(20),
            poll_interval: Duration::from_millis(10),
            start_grace: Duration::from_millis(20),
            stuck_job_grace: Duration::from_millis(100),
        });
        p.set_firmware_options(FirmwareOptions {
            poll_interval: Duration::from_millis(30),
            gone_timeout: Duration::from_secs(5),
            back_timeout: Duration::from_secs(20),
            stable_polls: 2,
        });
        p
    })
}

fn rig(fake: &FakePrinter, read_only: bool, poll: Duration) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let host = fake.chmp_target();
    let svc = Service::new(Options {
        read_only,
        poll_interval: Some(poll),
        // Never the user's real files.
        config_path: Some(dir.path().join("no-config.toml")),
        cache_dir: Some(dir.path().join("cache")),
        state_path: Some(dir.path().join("state").join("washid.json")),
        printers: vec![host.clone()],
        // The assertions below read English whatever the machine's locale is.
        language: Some(LanguageSetting::En),
        printer_factory: Some(factory(fake.ipp.port())),
        discover_timeout: Duration::from_millis(50),
    })
    .unwrap();
    Rig { svc, host, dir }
}

async fn next(rx: &mut Receiver<Event>, pred: impl Fn(&Event) -> bool) -> Event {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let ev = rx.recv().await.expect("event channel closed");
            if pred(&ev) {
                return ev;
            }
        }
    })
    .await
    .expect("timed out waiting for an event")
}

fn parse(json: &str) -> serde_json::Value {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("bad JSON ({e}): {json}"))
}

/// Drop consecutive duplicate polls (`GetStatus/...`) from a request log.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshots_follow_the_printer_through_sleep_and_wake() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();

    let s = r.svc.get_snapshot("").await.unwrap(); // "" = the active printer
    assert_eq!(s.power, Power::On);
    assert_eq!(s.inks.len(), 5);
    let ev = next(&mut rx, |e| matches!(e, Event::Snapshot { .. })).await;
    let Event::Snapshot { host, json } = ev else { unreachable!() };
    assert_eq!(host, r.host);
    assert_eq!(parse(&json)["power"], "on");

    // Within the polling interval GetSnapshot is answered from memory.
    fake.clear_log();
    let again = r.svc.get_snapshot(&r.host).await.unwrap();
    assert!(again.content_eq(&s));
    assert!(fake.log().is_empty(), "{:?}", fake.log());

    // Refresh reads the printer, which fell asleep in the meantime.
    fake.set_asleep(true);
    fake.set_ink("C", 40);
    let asleep = r.svc.refresh(&r.host).await.unwrap();
    assert_eq!(asleep.power, Power::Sleeping);
    assert_eq!(asleep.ink("C").unwrap().level, Some(40));
    let ev = next(&mut rx, |e| matches!(e, Event::Snapshot { .. })).await;
    let Event::Snapshot { json, .. } = ev else { unreachable!() };
    let j = parse(&json);
    assert_eq!(j["power"], "sleeping");
    assert_eq!(j["ink_source"], "ipp");

    // An unchanged reading is not announced again.
    r.svc.refresh(&r.host).await.unwrap();
    assert!(rx.try_recv().is_err(), "no event for an unchanged snapshot");

    let list = r.svc.list_printers();
    assert_eq!(list.len(), 1);
    assert!(
        list[0].active && list[0].power == Some(Power::Sleeping) && list[0].source == Source::Flag
    );
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_only_mode_refuses_every_write_before_any_network_access() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    assert!(r.svc.read_only());

    let confirmed_op = r#"{"confirm": true, "op": "clean", "kind": "deep"}"#;
    let confirmed_set = r#"{"confirm": true, "dry_level": 4}"#;
    for host in [r.host.as_str(), "", "nowhere.invalid"] {
        for bad in ["not json", "{}", confirmed_op] {
            let e = r.svc.run_maintenance(host, bad).await.unwrap_err();
            assert!(matches!(e, ServiceError::ReadOnly(_)), "{host:?} {bad:?}: {e}");
        }
        for bad in ["not json", "{}", confirmed_set] {
            let e = r.svc.apply_settings(host, bad).await.unwrap_err();
            assert!(matches!(e, ServiceError::ReadOnly(_)), "{host:?} {bad:?}: {e}");
        }
    }
    assert!(fake.log().is_empty(), "read-only writes touched the printer: {:?}", fake.log());
    assert_eq!(fake.chmp_connections() + fake.ipp_connections(), 0);
    assert!(!r.dir.path().join("state").exists(), "read-only mode writes no state");

    // Reads still work, and still never write.
    r.svc.get_snapshot(&r.host).await.unwrap();
    r.svc.get_settings(&r.host).await.unwrap();
    r.svc.get_maintenance_capability(&r.host).await.unwrap();
    assert!(fake.writes().is_empty());
    // SetActivePrinter is local state: allowed, but not persisted.
    r.svc.set_active_printer(&r.host).await.unwrap();
    assert!(!r.dir.path().join("state").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_requests_never_reach_the_printer() {
    let fake = FakePrinter::start();
    let r = rig(&fake, false, Duration::from_secs(60));

    let e = r.svc.run_maintenance(&r.host, r#"{"op": "nozzle_check"}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed(_)), "{e}");
    let e = r
        .svc
        .run_maintenance(&r.host, r#"{"confirm": false, "op": "nozzle_check"}"#)
        .await
        .unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed(_)), "{e}");
    let e = r
        .svc
        .run_maintenance(&r.host, r#"{"confirm": true, "op": "power_off"}"#)
        .await
        .unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e = r
        .svc
        .run_maintenance(&r.host, r#"{"confirm": true, "op": "align_manual", "id": 7}"#)
        .await
        .unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e = r
        .svc
        .run_maintenance("elsewhere.local", r#"{"confirm": true, "op": "nozzle_check"}"#)
        .await
        .unwrap_err();
    assert!(matches!(e, ServiceError::UnknownPrinter(_)), "{e}");
    let e = r.svc.apply_settings(&r.host, r#"{"dry_level": 4}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed(_)), "{e}");
    let e = r.svc.apply_settings(&r.host, r#"{"confirm": true}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e = r.svc.apply_settings(&r.host, r#"{"confirm": true, "dry_levl": 4}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e = r.svc.get_snapshot("elsewhere.local").await.unwrap_err();
    assert!(matches!(e, ServiceError::UnknownPrinter(_)), "{e}");

    // Out-of-range values are caught against the printer's capability, before any write.
    let e =
        r.svc.apply_settings(&r.host, r#"{"confirm": true, "dry_level": 9}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn maintenance_job_reports_progress_and_blocks_other_access() {
    let fake = FakePrinter::start();
    fake.set_job_polls(40);
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    let before = r.svc.get_snapshot(&r.host).await.unwrap();
    assert_eq!(before.activity, None);
    fake.clear_log();

    let job =
        r.svc.run_maintenance(&r.host, r#"{"confirm": true, "op": "nozzle_check"}"#).await.unwrap();
    assert_eq!(job, "job-1");

    // While the job runs: a second job is refused, and snapshots come from memory with the
    // activity set, without any network traffic of their own.
    let e = r
        .svc
        .run_maintenance(&r.host, r#"{"confirm": true, "op": "roller_clean"}"#)
        .await
        .unwrap_err();
    assert!(matches!(e, ServiceError::Busy(_)), "{e}");
    let during = r.svc.get_snapshot(&r.host).await.unwrap();
    assert_eq!(during.activity.as_deref(), Some("maintenance: nozzle check"));
    let during = r.svc.refresh(&r.host).await.unwrap();
    assert_eq!(during.activity.as_deref(), Some("maintenance: nozzle check"));
    let e = r.svc.get_settings(&r.host).await.unwrap_err();
    assert!(matches!(e, ServiceError::Busy(_)), "{e}");
    let e =
        r.svc.apply_settings(&r.host, r#"{"confirm": true, "dry_level": 4}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::Busy(_)), "{e}");

    // Progress, then the outcome.
    let mut stages = Vec::new();
    let fin = loop {
        match next(&mut rx, |e| {
            matches!(e, Event::MaintProgress { .. } | Event::MaintFinished { .. })
        })
        .await
        {
            Event::MaintProgress { job: j, stage, message } => {
                assert_eq!(j, job);
                assert!(!message.is_empty());
                if stages.last() != Some(&stage) {
                    stages.push(stage);
                }
            }
            Event::MaintFinished { job: j, ok, message } => {
                assert_eq!(j, job);
                break (ok, message);
            }
            _ => unreachable!(),
        }
    };
    assert!(fin.0, "{}", fin.1);
    assert!(fin.1.contains("nozzle check"), "{}", fin.1);
    assert_eq!(stages.first().map(String::as_str), Some("job-started"));
    assert_eq!(stages.get(1).map(String::as_str), Some("accepted"));
    assert!(stages.contains(&"waiting".to_string()), "{stages:?}");
    assert_eq!(&stages[stages.len() - 2..], ["job-ended", "closing"]);

    // The exact request sequence: only the job bracket and the operation were written.
    assert_eq!(
        fake.writes(),
        ["StartJob/maintenance", "TestPrint/maintenance", "EndJob/maintenance"]
    );
    let seq = strip_polls(fake.log());
    assert_eq!(seq[0], "StartJob/maintenance");
    assert_eq!(seq[1], "TestPrint/maintenance");
    assert_eq!(seq.last().unwrap(), "EndJob/maintenance");
    assert!(fake.max_concurrent_commands() <= 1);

    // The job is over: a fresh snapshot follows and the printer accepts the next job.
    let after =
        next(&mut rx, |e| matches!(e, Event::Snapshot { json, .. } if !json.contains("activity")))
            .await;
    assert!(matches!(after, Event::Snapshot { .. }));
    let snap = r.svc.get_snapshot(&r.host).await.unwrap();
    assert_eq!(snap.activity, None);
    let job2 =
        r.svc.run_maintenance(&r.host, r#"{"confirm": true, "op": "align_check"}"#).await.unwrap();
    assert_eq!(job2, "job-2");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failing_job_reports_the_reason() {
    let fake = FakePrinter::start();
    fake.set_asleep(true); // maintenance service says Suspended
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    r.svc.run_maintenance(&r.host, r#"{"confirm": true, "op": "nozzle_check"}"#).await.unwrap();
    let Event::MaintFinished { ok, message, .. } =
        next(&mut rx, |e| matches!(e, Event::MaintFinished { .. })).await
    else {
        unreachable!()
    };
    assert!(!ok);
    assert!(message.contains("Suspended") && message.contains("asleep"), "{message}");
    // Only the refused StartJob went out.
    assert_eq!(fake.writes(), ["StartJob/maintenance"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_are_validated_written_read_back_and_diffed() {
    let fake = FakePrinter::start();
    let r = rig(&fake, false, Duration::from_secs(60));

    let got = parse(&r.svc.get_settings(&r.host).await.unwrap());
    assert_eq!(got["current"]["drylevel"], 3);
    assert_eq!(got["allowed"]["drylevel"], serde_json::json!([1, 2, 3, 4, 5]));
    assert_eq!(got["allowed"]["papergap"], serde_json::json!(["auto", "wide"]));
    let mcap = parse(&r.svc.get_maintenance_capability(&r.host).await.unwrap());
    assert_eq!(mcap["cleaning"][1]["kind"], "deep");
    assert_eq!(mcap["testprint"].as_array().unwrap().len(), 4);

    let out = parse(
        &r.svc
            .apply_settings(&r.host, r#"{"confirm": true, "dry_level": 4, "paper_gap": "wide"}"#)
            .await
            .unwrap(),
    );
    assert_eq!(out["changed"], true);
    let changes = out["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2, "{out}");
    let dry = changes.iter().find(|c| c["field"] == "drylevel").unwrap();
    assert_eq!((dry["before"].as_str(), dry["after"].as_str()), (Some("3"), Some("4")));
    assert_eq!(out["current"]["papergap"], "wide");
    assert_eq!(fake.writes(), ["StartJob/device", "SetConfiguration/device", "EndJob/device"]);

    // The same request again changes nothing and writes nothing.
    fake.clear_log();
    let out = parse(
        &r.svc
            .apply_settings(&r.host, r#"{"confirm": true, "dry_level": 4, "paper_gap": "wide"}"#)
            .await
            .unwrap(),
    );
    assert_eq!(out["changed"], false);
    assert_eq!(out["changes"].as_array().unwrap().len(), 0);
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_and_capabilities_can_be_read_while_the_printer_sleeps() {
    let fake = FakePrinter::start();
    fake.set_asleep(true);
    let r = rig(&fake, true, Duration::from_secs(60));
    // Verified on the real printer: only GetStatus print/maintenance say Suspended.
    r.svc.get_settings(&r.host).await.unwrap();
    r.svc.get_maintenance_capability(&r.host).await.unwrap();
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn device_access_is_serialised_per_printer() {
    let fake = FakePrinter::start();
    fake.set_latency_ms(15);
    let r = rig(&fake, true, Duration::from_millis(1));
    let mut tasks = Vec::new();
    for i in 0..8 {
        let svc = r.svc.clone();
        let host = r.host.clone();
        tasks.push(tokio::spawn(async move {
            match i % 3 {
                0 => svc.refresh(&host).await.map(|_| ()),
                1 => svc.get_snapshot(&host).await.map(|_| ()),
                _ => svc.get_settings(&host).await.map(|_| ()),
            }
        }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(fake.max_concurrent_commands(), 1, "two CHMP operations overlapped");
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn poller_publishes_changes_and_notifies_once_per_condition() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_millis(40));
    let rec = Arc::new(Recorder(Mutex::new(Vec::new())));
    r.svc.set_notifier(rec.clone());
    let mut rx = r.svc.subscribe();
    let poller = tokio::spawn(r.svc.clone().run_poller());

    // First poll publishes the initial snapshot.
    next(&mut rx, |e| matches!(e, Event::Snapshot { .. })).await;
    assert!(rec.keys().is_empty());

    // Yellow runs low: one notification however many polls follow.
    fake.set_ink("Y", 10);
    next(&mut rx, |e| matches!(e, Event::Snapshot { json, .. } if json.contains("\"low\""))).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(rec.keys(), ["ink:Y:low"]);
    let n = rec.0.lock().unwrap()[0].clone();
    assert!(n.summary.contains("Yellow") && n.body.contains("10%") && !n.critical, "{n:?}");

    // A new error code.
    fake.set_support_code("1300");
    next(&mut rx, |e| matches!(e, Event::Snapshot { json, .. } if json.contains("1300"))).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(rec.keys(), ["ink:Y:low", "error"]);

    // The printer drops off the network, then comes back (asleep).
    fake.set_support_code("");
    fake.set_chmp_down(true);
    fake.set_ipp_down(true);
    next(&mut rx, |e| matches!(e, Event::Snapshot { json, .. } if json.contains("\"offline\"")))
        .await;
    assert_eq!(rec.keys().len(), 2, "going offline is not announced");
    fake.set_chmp_down(false);
    fake.set_ipp_down(false);
    fake.set_asleep(true);
    next(&mut rx, |e| matches!(e, Event::Snapshot { json, .. } if json.contains("\"sleeping\"")))
        .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(rec.keys(), ["ink:Y:low", "error", "online"]);

    poller.abort();
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn active_printer_selection_is_validated_and_persisted() {
    let fake = FakePrinter::start();
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state.json");
    let mk = |printers: Vec<String>| {
        Service::new(Options {
            poll_interval: Some(Duration::from_secs(60)),
            config_path: Some(dir.path().join("none.toml")),
            cache_dir: Some(dir.path().join("cache")),
            state_path: Some(state.clone()),
            printers,
            language: Some(LanguageSetting::En),
            printer_factory: Some(factory(fake.ipp.port())),
            ..Options::default()
        })
        .unwrap()
    };

    let svc = mk(vec![]);
    assert!(svc.list_printers().is_empty());
    assert!(matches!(svc.get_snapshot("").await.unwrap_err(), ServiceError::NoPrinter));
    let mut rx = svc.subscribe();

    for bad in ["", "a b", "http://x/y", "x\ny"] {
        let e = svc.set_active_printer(bad).await.unwrap_err();
        assert!(matches!(e, ServiceError::InvalidArgs(_)), "{bad:?}: {e}");
    }
    let host = fake.chmp_target();
    svc.set_active_printer(&host).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Event::PrintersChanged);
    assert_eq!(svc.active_host().as_deref(), Some(host.as_str()));
    let list = svc.list_printers();
    assert_eq!((list.len(), list[0].active, list[0].source), (1, true, Source::Manual));
    // A first reading is started in the background.
    next(&mut rx, |e| matches!(e, Event::Snapshot { .. })).await;
    assert!(svc.get_snapshot("").await.is_ok());

    // A later run knows the printer and which one was active.
    let again = mk(vec![]);
    let list = again.list_printers();
    assert_eq!(
        (list.len(), list[0].host.as_str(), list[0].source),
        (1, host.as_str(), Source::Saved)
    );
    assert_eq!(again.active_host().as_deref(), Some(host.as_str()));
    // Selecting the already active printer again is quiet.
    let mut rx2 = again.subscribe();
    again.set_active_printer(&host).await.unwrap();
    assert!(rx2.try_recv().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_is_valid_json_without_identifiers() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    let snap: PrinterSnapshot = r.svc.get_snapshot(&r.host).await.unwrap();
    // The resolved IPv4 address is the one place a network address appears, on purpose.
    assert_eq!(snap.address.as_deref(), Some("127.0.0.1"));
    let json = serde_json::to_string(&PrinterSnapshot { address: None, ..snap }).unwrap();
    assert!(!json.contains("127.0.0.1"), "{json}");
    for needle in ["REDACTED", "gci", "serial"] {
        assert!(!json.contains(needle), "{needle} in {json}");
    }
}

// ---------------------------------------------------------------------------------------------
// Scanning and printing
// ---------------------------------------------------------------------------------------------

/// Wait until the printer is free again (the job record is cleared right after the final event).
async fn until_idle(svc: &Arc<Service>, host: &str) {
    for _ in 0..200 {
        if svc.get_snapshot(host).await.map(|s| s.activity.is_none()).unwrap_or(false) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the printer never became idle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scanner_capabilities_are_a_read_even_when_read_only() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    let v = parse(&r.svc.get_scanner_capabilities(&r.host).await.unwrap());
    assert_eq!(v["capabilities"]["version"], "2.9");
    assert_eq!(v["capabilities"]["sources"][0]["source"], "Platen");
    assert_eq!(v["capabilities"]["sources"][0]["max_height"], 3508);
    assert_eq!(v["status"]["state"], "Idle");
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_only_refuses_scanning_and_printing_before_looking_at_anything() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    let e = r.svc.start_scan("nonexistent", "not json").await.unwrap_err();
    assert!(matches!(e, ServiceError::ReadOnly("StartScan")), "{e}");
    let e = r.svc.print_file("nonexistent", "relative", "not json").await.unwrap_err();
    assert!(matches!(e, ServiceError::ReadOnly("PrintFile")), "{e}");
    let e = r.svc.print_files("nonexistent", &["relative".into()], "not json").await.unwrap_err();
    assert!(matches!(e, ServiceError::ReadOnly("PrintFiles")), "{e}");
    let e =
        r.svc.print_disc(&r.host, r#"{"confirm":true,"texts":[{"text":"x"}]}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::ReadOnly("PrintDisc")), "{e}");
    assert_eq!((fake.chmp_connections(), fake.ipp_connections()), (0, 0));
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scanning_and_printing_need_an_explicit_confirm() {
    let fake = FakePrinter::start();
    let r = rig(&fake, false, Duration::from_secs(60));
    fake.clear_log();
    let e = r.svc.start_scan(&r.host, r#"{"size":"a4"}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed("StartScan")), "{e}");
    let e = r.svc.start_scan(&r.host, r#"{"confirm":false}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed(_)));
    let e = r.svc.print_file(&r.host, "/tmp/x.pdf", r#"{}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed("PrintFile")), "{e}");
    let e = r.svc.print_disc(&r.host, r#"{"texts":[{"text":"x"}]}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed("PrintDisc")), "{e}");
    // Bad input is rejected with an argument error, still without touching the printer.
    for json in [r#"{"confirm":true,"size":"a3"}"#, r#"{"confirm":true,"bogus":1}"#, "nope"] {
        let e = r.svc.start_scan(&r.host, json).await.unwrap_err();
        assert!(matches!(e, ServiceError::InvalidArgs(_)), "{json}: {e}");
    }
    let e =
        r.svc.print_file(&r.host, "relative/path.pdf", r#"{"confirm":true}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e =
        r.svc.print_file(&r.host, "/nonexistent/x.pdf", r#"{"confirm":true}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::Printer(_) | ServiceError::InvalidArgs(_)), "{e}");
    assert!(fake.log().is_empty(), "{:?}", fake.log());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scan_session_delivers_its_page_as_a_file_and_a_signal() {
    let fake = FakePrinter::start();
    fake.set_scan_max_edge(300);
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    let id = r
        .svc
        .start_scan(
            &r.host,
            r#"{"confirm":true,"size":"a5","resolution":150,"mode":"gray","type":"photo"}"#,
        )
        .await
        .unwrap();
    assert_eq!(id, "scan-1");
    // The printer is occupied while the scan runs: other work is refused.
    let e = r.svc.start_scan(&r.host, r#"{"confirm":true}"#).await;
    assert!(
        matches!(e, Err(ServiceError::Busy(_))) || e.is_ok(),
        "either busy or already done: {e:?}"
    );

    let ev = next(&mut rx, |e| matches!(e, Event::ScanPage { .. })).await;
    let Event::ScanPage { session, index, path } = ev else { unreachable!() };
    assert_eq!((session.as_str(), index), ("scan-1", 0));
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "a JPEG");
    assert!(path.contains("scan-scan-1-") && path.ends_with("page-1.jpg"), "{path}");
    use std::os::unix::fs::PermissionsExt;
    let dir_mode = std::fs::metadata(std::path::Path::new(&path).parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(dir_mode & 0o077, 0, "the scan directory is private: {dir_mode:o}");

    let Event::ScanFinished { session, ok, message } =
        next(&mut rx, |e| matches!(e, Event::ScanFinished { .. })).await
    else {
        unreachable!()
    };
    assert!(ok && session == "scan-1", "{message}");
    assert!(message.contains("1 page"), "{message}");
    let xml = &fake.scan_requests()[0];
    assert!(
        xml.contains("Grayscale8")
            && xml.contains("<scan:Intent>Photo</scan:Intent>")
            && xml.contains("<scan:XResolution>150</scan:XResolution>"),
        "{xml}"
    );
    until_idle(&r.svc, &r.host).await;
    let _ = std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scan_failure_is_reported_in_the_finish_signal() {
    let fake = FakePrinter::start();
    fake.set_scan_reject(Some(409));
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    r.svc.start_scan(&r.host, r#"{"confirm":true}"#).await.unwrap();
    let Event::ScanFinished { ok, message, .. } =
        next(&mut rx, |e| matches!(e, Event::ScanFinished { .. })).await
    else {
        unreachable!()
    };
    assert!(!ok && message.contains("409"), "{message}");
    until_idle(&r.svc, &r.host).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn printing_a_photo_reports_progress_and_the_end() {
    let fake = FakePrinter::start();
    fake.set_job_completion_polls(0);
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    let file = r.dir.path().join("pic.png");
    image::RgbImage::from_fn(120, 80, |x, y| image::Rgb([x as u8, y as u8, 7]))
        .save(&file)
        .unwrap();
    let job = r
        .svc
        .print_file(
            &r.host,
            &file.to_string_lossy(),
            r#"{"confirm":true,"size":"l","media_type":"photo","copies":2}"#,
        )
        .await
        .unwrap();
    assert_eq!(job, "print-1");
    let mut stages: Vec<String> = Vec::new();
    let fin = loop {
        match next(&mut rx, |e| {
            matches!(e, Event::PrintProgress { .. } | Event::PrintFinished { .. })
        })
        .await
        {
            Event::PrintProgress { job: j, stage, message } => {
                assert_eq!(j, job);
                assert!(!message.is_empty());
                if stages.last() != Some(&stage) {
                    stages.push(stage);
                }
            }
            Event::PrintFinished { job: j, ok, message } => {
                assert_eq!(j, job);
                break (ok, message);
            }
            _ => unreachable!(),
        }
    };
    assert!(fin.0, "{}", fin.1);
    assert!(fin.1.contains("pic.png: completed"), "{}", fin.1);
    assert_eq!(&stages[..4], ["preflight", "validated", "sending", "submitted"], "{stages:?}");
    assert_eq!(stages.last().map(String::as_str), Some("finished"));
    let sent = fake.printed_jobs();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].format, "image/jpeg");
    until_idle(&r.svc, &r.host).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_disc_label_prints_through_the_disc_tray() {
    let fake = FakePrinter::start();
    fake.set_job_completion_polls(0);
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    let job = r
        .svc
        .print_disc(
            &r.host,
            r##"{"confirm":true,"background_color":"#3060c0","media":"a","inner_mm":24}"##,
        )
        .await
        .unwrap();
    let Event::PrintFinished { job: j, ok, message } =
        next(&mut rx, |e| matches!(e, Event::PrintFinished { .. })).await
    else {
        unreachable!()
    };
    assert!(ok && j == job, "{message}");
    let sent = fake.printed_jobs();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].data.starts_with(b"RaS2"));
    until_idle(&r.svc, &r.host).await;
    // Bad label values are argument errors that never reach the printer.
    fake.clear_log();
    let e = r.svc.print_disc(&r.host, r#"{"confirm":true,"inner_mm":5}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    assert!(fake.log().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_print_job_can_be_cancelled() {
    let fake = FakePrinter::start();
    fake.set_job_completion_polls(100_000);
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    let file = r.dir.path().join("pic.png");
    image::RgbImage::from_pixel(60, 40, image::Rgb([1, 2, 3])).save(&file).unwrap();
    let job =
        r.svc.print_file(&r.host, &file.to_string_lossy(), r#"{"confirm":true}"#).await.unwrap();
    next(&mut rx, |e| matches!(e, Event::PrintProgress { stage, .. } if stage == "submitted"))
        .await;
    assert!(r.svc.cancel_job(&job));
    assert!(!r.svc.cancel_job("print-99"));
    let Event::PrintFinished { ok, message, .. } =
        next(&mut rx, |e| matches!(e, Event::PrintFinished { .. })).await
    else {
        unreachable!()
    };
    assert!(!ok && message.contains("cancelled"), "{message}");
    assert!(fake.log().iter().any(|l| l == "IPP/Cancel-Job"));
    until_idle(&r.svc, &r.host).await;
}

// ---------------------------------------------------------------------------------------------
// Wake, notifications, forgetting printers, print capabilities, manual alignment
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshots_say_how_the_printer_is_connected() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    let v = parse(&snapshot_json_of(&r).await);
    assert_eq!(v["connection"], "wifi", "infrastructure_mode means Wi-Fi");
    assert_eq!(v["address"], "127.0.0.1");
}

async fn snapshot_json_of(r: &Rig) -> String {
    washid::service::snapshot_json(&r.svc.get_snapshot(&r.host).await.unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn waking_a_sleeping_printer_needs_write_access_and_a_confirm() {
    // Read-only: refused before the arguments are even looked at, nothing sent.
    let fake = FakePrinter::start();
    fake.set_asleep(true);
    let ro = rig(&fake, true, Duration::from_secs(60));
    for json in ["not json", "{}", r#"{"confirm": true}"#] {
        let e = ro.svc.wake(&ro.host, json).await.unwrap_err();
        assert!(matches!(e, ServiceError::ReadOnly("Wake")), "{json}: {e}");
    }
    assert!(fake.log().is_empty());

    let r = rig(&fake, false, Duration::from_secs(60));
    fake.clear_log();
    let e = r.svc.wake(&r.host, "{}").await.unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed("Wake")), "{e}");
    let e = r.svc.wake(&r.host, r#"{"confirm": true, "extra": 1}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e = r.svc.wake("nowhere.invalid", r#"{"confirm": true}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::UnknownPrinter(_)), "{e}");
    assert!(fake.log().is_empty(), "{:?}", fake.log());

    assert_eq!(r.svc.get_snapshot(&r.host).await.unwrap().power, Power::Sleeping);
    let mut rx = r.svc.subscribe();
    let out = parse(&r.svc.wake(&r.host, r#"{"confirm": true}"#).await.unwrap());
    assert_eq!(
        (out["sent"].as_bool(), out["verified"].as_bool()),
        (Some(true), Some(true)),
        "the follow-up status shows the printer awake"
    );
    assert!(out["note"].as_str().unwrap().contains("woke up"), "{out}");
    assert_eq!(fake.writes(), ["PowerOn/device"], "one request, no job bracket");
    // The daemon looked again and announced the awake printer.
    let ev = next(
        &mut rx,
        |e| matches!(e, Event::Snapshot { json, .. } if json.contains("\"power\":\"on\"")),
    )
    .await;
    assert!(matches!(ev, Event::Snapshot { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notification_switches_are_read_changed_and_saved() {
    let fake = FakePrinter::start();
    let r = rig(&fake, false, Duration::from_secs(60));
    let all = parse(&r.svc.get_notification_settings());
    assert_eq!(
        all,
        serde_json::json!({"enabled": true, "ink": true, "errors": true, "online": true})
    );
    let out =
        parse(&r.svc.set_notification_settings(r#"{"ink": false, "online": false}"#).unwrap());
    assert_eq!(
        out,
        serde_json::json!({"enabled": true, "ink": false, "errors": true, "online": false})
    );
    assert_eq!(parse(&r.svc.get_notification_settings()), out);
    let saved = std::fs::read_to_string(r.dir.path().join("no-config.toml")).unwrap();
    assert!(saved.contains("[notifications]") && saved.contains("ink = false"), "{saved}");
    for bad in ["nope", r#"{"inks": false}"#, r#"{"ink": "yes"}"#] {
        let e = r.svc.set_notification_settings(bad).unwrap_err();
        assert!(matches!(e, ServiceError::InvalidArgs(_)), "{bad}: {e}");
    }

    // A new daemon reads them back from the file; a read-only one reads but never writes.
    let ro = Service::new(Options {
        read_only: true,
        config_path: Some(r.dir.path().join("no-config.toml")),
        cache_dir: Some(r.dir.path().join("cache")),
        state_path: Some(r.dir.path().join("state2").join("w.json")),
        language: Some(LanguageSetting::En),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(parse(&ro.get_notification_settings()), out);
    let e = ro.set_notification_settings(r#"{"ink": true}"#).unwrap_err();
    assert!(matches!(e, ServiceError::ReadOnly("SetNotificationSettings")), "{e}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_printer_can_be_forgotten() {
    let fake = FakePrinter::start();
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state.json");
    let mk = || {
        Service::new(Options {
            poll_interval: Some(Duration::from_secs(60)),
            config_path: Some(dir.path().join("none.toml")),
            cache_dir: Some(dir.path().join("cache")),
            state_path: Some(state.clone()),
            printer_factory: Some(factory(fake.ipp.port())),
            ..Options::default()
        })
        .unwrap()
    };
    let svc = mk();
    let host = fake.chmp_target();
    svc.set_active_printer(&host).await.unwrap();
    svc.set_active_printer("second.invalid:1").await.unwrap();
    assert_eq!(svc.list_printers().len(), 2);
    let mut rx = svc.subscribe();

    let e = svc.forget_printer("").await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e = svc.forget_printer("never-heard-of.local").await.unwrap_err();
    assert!(matches!(e, ServiceError::UnknownPrinter(_)), "{e}");

    // Forgetting the active printer moves the selection to the one that is left.
    let out = parse(&svc.forget_printer("second.invalid:1").await.unwrap());
    assert_eq!(out["forgotten"], true);
    assert_eq!(out["persistent"], true);
    assert_eq!(out["active"], serde_json::json!(host));
    assert_eq!(rx.recv().await.unwrap(), Event::PrintersChanged);
    assert_eq!(svc.list_printers().len(), 1);
    let again = mk();
    assert_eq!(again.list_printers().len(), 1, "the removal was saved");
    let out = parse(&svc.forget_printer(&host).await.unwrap());
    assert_eq!(out["active"], serde_json::Value::Null);
    assert!(svc.list_printers().is_empty());
    assert!(mk().list_printers().is_empty());
    assert!(fake.writes().is_empty(), "forgetting never touches a printer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_printer_from_the_command_line_is_only_forgotten_for_this_run() {
    let fake = FakePrinter::start();
    let r = rig(&fake, false, Duration::from_secs(60));
    let out = parse(&r.svc.forget_printer(&r.host).await.unwrap());
    assert_eq!(out["persistent"], false);
    assert!(r.svc.list_printers().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn print_capabilities_are_a_cached_read_even_when_read_only() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    let v = parse(&r.svc.get_print_capabilities(&r.host).await.unwrap());
    assert!(v["sources"].as_array().unwrap().iter().any(|s| s == "rear"), "{v}");
    assert!(!v["sources"].as_array().unwrap().iter().any(|s| s == "disc"));
    assert!(v["media_types"].as_array().unwrap().iter().any(|t| t == "stationery"));
    assert!(v["sizes"].as_array().unwrap().iter().any(|z| z["key"] == "a4"));
    assert!(v["media"]["main"]["stationery"]["a4"].as_u64().unwrap() & 1 == 1, "{v}");
    assert_eq!(v["qualities"], serde_json::json!(["draft", "normal", "high"]));
    assert!(fake.writes().is_empty());
    fake.clear_log();
    let again = r.svc.get_print_capabilities(&r.host).await.unwrap();
    assert_eq!(parse(&again), v);
    assert!(fake.log().is_empty(), "the second answer comes from memory: {:?}", fake.log());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_roller_cleaning_flag_follows_the_capability() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    let off = parse(&r.svc.get_maintenance_capability(&r.host).await.unwrap());
    assert_eq!(off["roller_cleaning"], false);
    fake.set_roller_cleaning(true);
    let on = parse(&r.svc.get_maintenance_capability(&r.host).await.unwrap());
    assert_eq!(on["roller_cleaning"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manual_alignment_values_are_validated_written_and_read_back() {
    let fake = FakePrinter::start();
    let r = rig(&fake, false, Duration::from_secs(60));
    let before = parse(&r.svc.get_settings(&r.host).await.unwrap());
    assert_eq!(
        before["allowed"]["setregistration"]["1"]["A"],
        serde_json::json!({"min": -5, "max": 5})
    );
    assert_eq!(before["current"]["setregistration"]["1"]["D"], 1);

    // Out of range: refused before any write.
    for bad in [
        r#"{"confirm":true,"setregistration":{"1":{"A":6}}}"#,
        r#"{"confirm":true,"setregistration":{"1":{"G":-4}}}"#,
        r#"{"confirm":true,"setregistration":{"3":{"e":3}}}"#,
        r#"{"confirm":true,"setregistration":{"4":{"A":0}}}"#,
        r#"{"confirm":true,"setregistration":{"2":{"A":0}}}"#,
    ] {
        let e = r.svc.apply_settings(&r.host, bad).await.unwrap_err();
        assert!(matches!(e, ServiceError::InvalidArgs(_)), "{bad}: {e}");
    }
    assert!(fake.writes().is_empty());

    let out = parse(
        &r.svc
            .apply_settings(
                &r.host,
                r#"{"confirm":true,"setregistration":{"1":{"A":2,"B":-1,"C":0,"D":1,"E":1,"F":0,"G":-3,"H":0}}}"#,
            )
            .await
            .unwrap(),
    );
    assert_eq!(out["changed"], true);
    assert_eq!(out["current"]["setregistration"]["1"]["A"], 2);
    assert_eq!(out["current"]["setregistration"]["1"]["B"], -1);
    assert_eq!(out["current"]["setregistration"]["1"]["G"], -3);
    assert_eq!(out["current"]["setregistration"]["2"]["I"], 0, "other sets are untouched");
    let changes = out["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "{out}");
    assert_eq!(changes[0]["field"], "setregistration");
    assert_eq!(fake.writes(), ["StartJob/device", "SetConfiguration/device", "EndJob/device"]);
    let xml = fake.config_xml();
    assert!(xml.contains("<ivec:item sign=\"A\">2</ivec:item>"), "{xml}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn several_photos_go_onto_shared_sheets_with_an_orientation() {
    let fake = FakePrinter::start();
    fake.set_job_completion_polls(0);
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    let mut files = Vec::new();
    for i in 0..3u8 {
        let f = r.dir.path().join(format!("p{i}.png"));
        image::RgbImage::from_pixel(120, 80, image::Rgb([i * 60, 9, 7])).save(&f).unwrap();
        files.push(f.to_string_lossy().into_owned());
    }
    let job = r
        .svc
        .print_files(
            &r.host,
            &files,
            r#"{"confirm":true,"size":"l","media_type":"photo","layout":2,"orientation":"landscape"}"#,
        )
        .await
        .unwrap();
    let fin = loop {
        if let Event::PrintFinished { job: j, ok, message } =
            next(&mut rx, |e| matches!(e, Event::PrintFinished { .. })).await
        {
            assert_eq!(j, job);
            break (ok, message);
        }
    };
    assert!(fin.0, "{}", fin.1);
    assert_eq!(fake.printed_jobs().len(), 2, "3 photos at 2 per sheet: two sheets");
    let e = r.svc.print_files(&r.host, &[], r#"{"confirm":true}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e =
        r.svc.print_files(&r.host, &["rel.png".into()], r#"{"confirm":true}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    let e = r
        .svc
        .print_files(&r.host, &files, r#"{"confirm":true,"orientation":"tilted"}"#)
        .await
        .unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    until_idle(&r.svc, &r.host).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn messages_follow_the_language_the_app_set() {
    let fake = FakePrinter::start();
    fake.set_job_polls(10);
    let r = rig(&fake, false, Duration::from_secs(60));
    let mut rx = r.svc.subscribe();
    assert_eq!(r.svc.language(), Language::En);
    r.svc.get_snapshot(&r.host).await.unwrap(); // while a job runs, snapshots come from memory

    // Errors and job messages are in English until the app says otherwise.
    let e = r.svc.run_maintenance(&r.host, r#"{"op": "nozzle_check"}"#).await.unwrap_err();
    assert!(e.message(r.svc.language()).contains("writes to the printer"), "{e}");
    r.svc.run_maintenance(&r.host, r#"{"confirm": true, "op": "nozzle_check"}"#).await.unwrap();
    let during = r.svc.get_snapshot(&r.host).await.unwrap();
    assert_eq!(during.activity.as_deref(), Some("maintenance: nozzle check"));
    let Event::MaintFinished { message, .. } =
        next(&mut rx, |e| matches!(e, Event::MaintFinished { .. })).await
    else {
        unreachable!()
    };
    assert!(message.starts_with("nozzle check finished"), "{message}");

    // Switch: the next messages are Japanese, with no restart.
    assert_eq!(r.svc.set_language("ja").unwrap(), Language::Ja);
    assert_eq!(r.svc.language(), Language::Ja);
    let e = r.svc.run_maintenance(&r.host, r#"{"op": "nozzle_check"}"#).await.unwrap_err();
    assert!(e.message(r.svc.language()).contains("同意"), "{}", e.message(r.svc.language()));
    r.svc.run_maintenance(&r.host, r#"{"confirm": true, "op": "nozzle_check"}"#).await.unwrap();
    let during = r.svc.get_snapshot(&r.host).await.unwrap();
    assert_eq!(during.activity.as_deref(), Some("メンテナンス：ノズルチェック"));
    let busy = r.svc.run_maintenance(&r.host, r#"{"confirm": true, "op": "roller_clean"}"#).await;
    let busy = busy.unwrap_err();
    assert!(matches!(busy, ServiceError::Busy(_)));
    assert!(
        busy.message(Language::Ja).contains("ノズルチェック"),
        "{}",
        busy.message(Language::Ja)
    );
    let mut saw_ja_progress = false;
    let message = loop {
        match next(&mut rx, |e| {
            matches!(e, Event::MaintProgress { .. } | Event::MaintFinished { .. })
        })
        .await
        {
            Event::MaintProgress { message, .. } => {
                saw_ja_progress |= message.chars().any(|c| c > '\u{3000}');
            }
            Event::MaintFinished { message, .. } => break message,
            _ => unreachable!(),
        }
    };
    assert!(saw_ja_progress);
    assert_eq!(message, "ノズルチェックが完了しました（0秒）。");

    // Back to English, and "system" follows the environment of the daemon (not asserted here).
    assert_eq!(r.svc.set_language("en").unwrap(), Language::En);
    assert!(r.svc.set_language("system").is_ok());
    for bad in ["", "fr", "ja_JP", "JA!"] {
        let e = r.svc.set_language(bad).unwrap_err();
        assert!(matches!(e, ServiceError::InvalidArgs(_)), "{bad:?}: {e}");
    }
    assert!(fake.writes().iter().all(|w| w.ends_with("/maintenance")), "{:?}", fake.writes());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_language_is_read_from_the_config_and_works_in_read_only_mode() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.toml");
    let mk = |language: Option<LanguageSetting>, read_only: bool| {
        Service::new(Options {
            read_only,
            language,
            config_path: Some(cfg.clone()),
            cache_dir: Some(dir.path().join("cache")),
            state_path: Some(dir.path().join("state").join("w.json")),
            ..Options::default()
        })
        .unwrap()
    };
    std::fs::write(&cfg, "[ui]\nlanguage = \"ja\"\n").unwrap();
    assert_eq!(mk(None, false).language(), Language::Ja);
    // An explicit option beats the file.
    assert_eq!(mk(Some(LanguageSetting::En), false).language(), Language::En);
    std::fs::write(&cfg, "[ui]\nlanguage = \"en\"\n").unwrap();
    assert_eq!(mk(None, false).language(), Language::En);
    // An unknown value does not stop the daemon from starting.
    std::fs::write(&cfg, "[ui]\nlanguage = \"klingon\"\n").unwrap();
    let svc = mk(Some(LanguageSetting::Ja), true);
    assert_eq!(svc.language(), Language::Ja);
    // Switching the language is not a write to the printer.
    assert_eq!(svc.set_language("en").unwrap(), Language::En);
    // The daemon never writes the language itself: the app owns [ui] language.
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "[ui]\nlanguage = \"klingon\"\n");
    let e = svc.set_active_printer("").await.unwrap_err();
    assert!(e.message(Language::Ja).contains("ホスト") || !e.message(Language::Ja).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn web_links_are_a_cached_read_even_when_read_only() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    let j = parse(&r.svc.get_web_links(&r.host, "1300").await.unwrap());
    assert_eq!(j["from_printer"], true);
    assert_eq!(j["ink"]["source"], "maker");
    assert!(j["ink"]["url"].as_str().unwrap().starts_with("https://links.example.org/"));
    assert!(j["error"]["url"].as_str().unwrap().ends_with("&ERR=1300"), "{j}");
    assert_eq!(j["maintenance_page"]["source"], "printer");
    assert!(j["maintenance_page"]["url"].as_str().unwrap().ends_with("/index.html?page=PAGE_UTIL"));
    assert!(fake.writes().is_empty());

    // No code, no error link; the printer is not asked again within half an hour.
    fake.clear_log();
    let again = parse(&r.svc.get_web_links(&r.host, "").await.unwrap());
    assert!(again["error"].is_null());
    assert!(fake.log().is_empty(), "{:?}", fake.log());

    // The language of the app goes into the maker's addresses.
    r.svc.set_language("ja").unwrap();
    let ja = parse(&r.svc.get_web_links(&r.host, "").await.unwrap());
    assert!(ja["ink"]["url"].as_str().unwrap().contains("&LNG=JP"), "{ja}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn web_links_fall_back_to_the_printers_own_page() {
    let fake = FakePrinter::start();
    fake.set_model_js(None);
    let r = rig(&fake, false, Duration::from_secs(60));
    let j = parse(&r.svc.get_web_links(&r.host, "1000").await.unwrap());
    let own = format!("http://{}/", r.host);
    assert_eq!(j["from_printer"], false);
    for k in ["ink", "help", "support", "error", "printer_page"] {
        assert_eq!(j[k]["url"], own.as_str(), "{k}");
        assert_eq!(j[k]["source"], "printer", "{k}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_firmware_update_needs_write_access_a_confirm_and_an_offered_update() {
    let fake = FakePrinter::start();
    let ro = rig(&fake, true, Duration::from_secs(60));
    for bad in ["not json", "{}", r#"{"confirm": true}"#] {
        let e = ro.svc.update_firmware(&ro.host, bad).await.unwrap_err();
        assert!(matches!(e, ServiceError::ReadOnly(_)), "{bad:?}: {e}");
    }
    assert!(fake.log().is_empty(), "{:?}", fake.log());

    let r = rig(&fake, false, Duration::from_secs(60));
    let e = r.svc.update_firmware(&r.host, "{}").await.unwrap_err();
    assert!(matches!(e, ServiceError::NotConfirmed(_)), "{e}");
    let e =
        r.svc.update_firmware(&r.host, r#"{"confirm": true, "force": true}"#).await.unwrap_err();
    assert!(matches!(e, ServiceError::InvalidArgs(_)), "{e}");
    assert!(fake.writes().is_empty());

    // Confirmed, but the printer says its firmware is current: the job ends without sending.
    let mut rx = r.svc.subscribe();
    r.svc.update_firmware(&r.host, r#"{"confirm": true}"#).await.unwrap();
    let ev = next(&mut rx, |e| matches!(e, Event::MaintFinished { .. })).await;
    let Event::MaintFinished { ok, message, .. } = ev else { unreachable!() };
    assert!(!ok && message.contains("up to date"), "{message}");
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_firmware_update_reports_progress_and_the_new_version() {
    let fake = FakePrinter::start();
    fake.offer_firmware_update("1.061", 600);
    let r = rig(&fake, false, Duration::from_secs(60));
    let before = r.svc.get_snapshot(&r.host).await.unwrap();
    assert_eq!(before.firmware_update, washi_core::FirmwareUpdate::Available);
    assert_eq!(before.device.as_ref().unwrap().firmware, "1.060");

    let mut rx = r.svc.subscribe();
    let job = r.svc.update_firmware(&r.host, r#"{"confirm": true}"#).await.unwrap();
    let mut stages = Vec::new();
    let finished = loop {
        match next(&mut rx, |e| {
            matches!(e, Event::MaintProgress { .. } | Event::MaintFinished { .. })
        })
        .await
        {
            Event::MaintProgress { job: j, stage, message } => {
                assert_eq!(j, job);
                assert!(!message.is_empty());
                if stages.last() != Some(&stage) {
                    stages.push(stage);
                }
            }
            Event::MaintFinished { job: j, ok, message } => {
                assert_eq!(j, job);
                break (ok, message);
            }
            _ => unreachable!(),
        }
    };
    assert!(finished.0, "{}", finished.1);
    assert_eq!(finished.1, "The firmware was updated from 1.060 to 1.061.");
    assert_eq!(stages.first().map(String::as_str), Some("sent"));
    assert!(
        stages.iter().any(|s| s == "restarting") && stages.last().unwrap() == "back",
        "{stages:?}"
    );
    assert_eq!(fake.writes(), ["VendorCmd:UpdateFRom/device"]);

    // The printer is read again afterwards: the new version, no update left.
    let mut after = r.svc.get_snapshot(&r.host).await.unwrap();
    for _ in 0..100 {
        if after.device.as_ref().is_some_and(|d| d.firmware == "1.061") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        after = r.svc.get_snapshot(&r.host).await.unwrap();
    }
    assert_eq!(after.device.as_ref().unwrap().firmware, "1.061");
    assert_eq!(after.firmware_update, washi_core::FirmwareUpdate::Unavailable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_maintenance_capability_lists_the_extra_cleaning_types() {
    let fake = FakePrinter::start();
    fake.set_cleaning_types(&["roller", "choke", "platen"]);
    let r = rig(&fake, true, Duration::from_secs(60));
    let cap = parse(&r.svc.get_maintenance_capability(&r.host).await.unwrap());
    let kinds: Vec<&str> =
        cap["cleaning"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["regular", "deep", "roller", "choke", "platen"]);
}

// ---- the printer's own words and picture ---------------------------------------------------------

const WORDS_EN: &str = "\"media-type.test-glossy\" = \"Test Glossy Paper\";\n\
                        \"media-type.test-matte\" = \"Test Matte Paper\";\n";
const WORDS_JA: &str =
    "\"media-type.test-glossy\" = \"\u{30c6}\u{30b9}\u{30c8}\u{5149}\u{6ca2}\u{7d19}\";\n";

fn fake_with_words() -> FakePrinter {
    let fake = FakePrinter::start();
    fake.set_strings("en", Some(WORDS_EN.as_bytes()));
    fake.set_strings("ja", Some(WORDS_JA.as_bytes()));
    fake
}

/// A transparent PNG with a picture in the middle, like the printers' own.
fn picture_png() -> Vec<u8> {
    let mut img = image::RgbaImage::new(256, 256);
    for y in 64..192 {
        for x in 20..236 {
            img.put_pixel(x, y, image::Rgba([230, 230, 230, 255]));
        }
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

fn words_cache(r: &Rig, fake: &FakePrinter) -> LocalizationCache {
    LocalizationCache::in_dir(&r.dir.path().join("cache"), "127.0.0.1", fake.chmp.port())
}

/// Wait (up to 5 s) until `cond` holds.
async fn eventually(cond: impl Fn() -> bool) {
    for _ in 0..250 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the condition did not hold in time");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn printer_strings_are_read_once_per_language_even_when_read_only() {
    let fake = fake_with_words();
    let r = rig(&fake, true, Duration::from_secs(60));
    let ja = parse(&r.svc.get_printer_strings(&r.host, "ja").await.unwrap());
    assert_eq!(ja["media-type.test-glossy"], "\u{30c6}\u{30b9}\u{30c8}\u{5149}\u{6ca2}\u{7d19}");
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
    let en = parse(&r.svc.get_printer_strings("", "EN").await.unwrap());
    assert_eq!(en["media-type.test-matte"], "Test Matte Paper");

    // Kept: the printer is not asked again, in this run or in the next.
    fake.clear_log();
    let again = parse(&r.svc.get_printer_strings(&r.host, "ja").await.unwrap());
    assert_eq!(again, ja);
    assert!(fake.log().is_empty(), "{:?}", fake.log());
    assert_eq!(words_cache(&r, &fake).strings("ja").unwrap().strings.len(), 1);

    // A language it has no file for is an empty map, a bad tag is refused before anything is sent.
    assert_eq!(
        parse(&r.svc.get_printer_strings(&r.host, "fr").await.unwrap()),
        serde_json::json!({})
    );
    for bad in ["", "ja/../x", "not a tag"] {
        let e = r.svc.get_printer_strings(&r.host, bad).await.unwrap_err();
        assert!(matches!(e, ServiceError::InvalidArgs(_)), "{bad:?}: {e}");
    }
    let e = r.svc.get_printer_strings("nowhere.test", "en").await.unwrap_err();
    assert!(matches!(e, ServiceError::UnknownPrinter(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_old_copy_is_answered_at_once_and_read_again_in_the_background() {
    let fake = fake_with_words();
    let r = rig(&fake, false, Duration::from_secs(60));
    let cache = words_cache(&r, &fake);
    let old = washi_core::FetchedStrings {
        language: "en".into(),
        strings: [("media-type.test-glossy".to_string(), "Old Name".to_string())].into(),
        firmware: Some("1.060".into()),
    };
    // 40 days old.
    cache.store_strings(&old, washi_core::unix_now() - 40 * 24 * 3600).unwrap();

    let first = parse(&r.svc.get_printer_strings(&r.host, "en").await.unwrap());
    assert_eq!(first["media-type.test-glossy"], "Old Name", "the kept copy comes first");
    eventually(|| cache.strings("en").is_some_and(|c| c.strings.len() == 2)).await;
    let next = parse(&r.svc.get_printer_strings(&r.host, "en").await.unwrap());
    assert_eq!(next["media-type.test-glossy"], "Test Glossy Paper");
    assert!(cache.strings("en").unwrap().is_current(Some("1.060"), washi_core::unix_now()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_firmware_version_reads_the_strings_again() {
    let fake = fake_with_words();
    let r = rig(&fake, false, Duration::from_secs(60));
    let cache = words_cache(&r, &fake);
    // Read a minute ago, from another firmware than the printer reports now.
    let old = washi_core::FetchedStrings {
        language: "en".into(),
        strings: [("media-type.test-glossy".to_string(), "Old Name".to_string())].into(),
        firmware: Some("0.900".into()),
    };
    cache.store_strings(&old, washi_core::unix_now() - 60).unwrap();

    // Before the printer has been read its firmware is not known: the copy is good.
    fake.clear_log();
    r.svc.get_printer_strings(&r.host, "en").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(fake.log().is_empty(), "{:?}", fake.log());

    let snap = r.svc.get_snapshot("").await.unwrap();
    assert_eq!(snap.device.as_ref().unwrap().firmware, "1.060");
    let first = parse(&r.svc.get_printer_strings(&r.host, "en").await.unwrap());
    assert_eq!(first["media-type.test-glossy"], "Old Name");
    eventually(|| cache.strings("en").is_some_and(|c| c.firmware.as_deref() == Some("1.060")))
        .await;
    let next = parse(&r.svc.get_printer_strings(&r.host, "en").await.unwrap());
    assert_eq!(next["media-type.test-glossy"], "Test Glossy Paper");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unreachable_printer_is_not_asked_again_for_a_minute_but_a_kept_copy_still_answers() {
    let fake = fake_with_words();
    fake.set_ipp_down(true);
    let r = rig(&fake, true, Duration::from_secs(60));
    let e = r.svc.get_printer_strings(&r.host, "en").await.unwrap_err();
    assert!(matches!(e, ServiceError::Printer(_)), "{e}");
    let connections = fake.ipp_connections();
    let e = r.svc.get_printer_strings(&r.host, "en").await.unwrap_err();
    assert!(matches!(e, ServiceError::Printer(_)), "{e}");
    assert_eq!(fake.ipp_connections(), connections, "no second try within a minute");
    assert!(e.message(Language::Ja).contains("プリンター"));
    // The picture has its own account of failures.
    assert!(r.svc.get_printer_icon(&r.host).await.is_err());

    // With a copy kept from an earlier run the printer being away does not matter.
    let kept = washi_core::FetchedStrings {
        language: "ja".into(),
        strings: [("media-type.test-glossy".to_string(), "kept".to_string())].into(),
        firmware: None,
    };
    words_cache(&r, &fake).store_strings(&kept, washi_core::unix_now()).unwrap();
    let ja = parse(&r.svc.get_printer_strings(&r.host, "ja").await.unwrap());
    assert_eq!(ja["media-type.test-glossy"], "kept");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_printers_picture_is_kept_as_a_file_even_when_read_only() {
    let fake = FakePrinter::start();
    fake.set_icons(vec![
        ("printer_icon.png".into(), picture_png()),
        ("printer_icon_large.png".into(), picture_png()),
    ]);
    let r = rig(&fake, true, Duration::from_secs(60));
    let path = r.svc.get_printer_icon(&r.host).await.unwrap();
    assert!(path.starts_with(r.dir.path().join("cache").to_str().unwrap()), "{path}");
    assert!(path.ends_with("-icon.png") && !path.contains("127.0.0.1"), "{path}");
    let bytes = std::fs::read(&path).unwrap();
    let (w, h) = washi_core::net::web::png_size(&bytes).unwrap();
    // 256 x 256 with a 216 x 128 picture in it, cut with a border of 5 pixels.
    assert_eq!((w, h), (226, 138));
    assert!(fake.writes().is_empty());

    fake.clear_log();
    assert_eq!(r.svc.get_printer_icon("").await.unwrap(), path);
    assert!(fake.log().is_empty(), "kept: {:?}", fake.log());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_printer_without_a_picture_answers_with_an_empty_path_and_is_not_asked_again() {
    let fake = FakePrinter::start();
    let r = rig(&fake, true, Duration::from_secs(60));
    assert_eq!(r.svc.get_printer_icon(&r.host).await.unwrap(), "");
    fake.clear_log();
    assert_eq!(r.svc.get_printer_icon(&r.host).await.unwrap(), "");
    assert!(fake.log().is_empty(), "{:?}", fake.log());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reading_the_words_waits_for_nothing_while_a_job_runs() {
    let fake = fake_with_words();
    let r = rig(&fake, false, Duration::from_secs(60));
    fake.set_job_polls(40);
    let job =
        r.svc.run_maintenance(&r.host, r#"{"confirm": true, "op": "nozzle_check"}"#).await.unwrap();
    let t = std::time::Instant::now();
    let e = r.svc.get_printer_strings(&r.host, "en").await.unwrap_err();
    assert!(matches!(e, ServiceError::Busy(_)), "{e}");
    assert!(t.elapsed() < Duration::from_secs(2));
    r.svc.cancel_job(&job);
}
