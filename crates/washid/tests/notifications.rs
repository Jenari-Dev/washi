// SPDX-License-Identifier: GPL-2.0-or-later
//! The test notification through the real delivery code (`DbusNotifier`) on a private session
//! bus with a fake notification server. Needs `dbus-daemon`; without it the tests say so and
//! pass (the same delivery is also covered by `service.rs` with a recording notifier).
//! Nothing here touches the user's session bus or a printer.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use washi_core::{Language, LanguageSetting};
use washid::notify::{DbusNotifier, Notification, Notifier};
use washid::service::{Options, Service, ServiceError};
use zbus::zvariant::OwnedValue;

/// A bus with no service files at all: asking for a name nobody owns fails at once with
/// `ServiceUnknown` instead of starting whatever notification daemon the machine has installed.
const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// A private session bus; the daemon process is stopped when this is dropped.
struct PrivateBus {
    child: Child,
    address: String,
    _config: tempfile::TempDir,
}

impl PrivateBus {
    fn start() -> Option<PrivateBus> {
        let dir = tempfile::tempdir().ok()?;
        let config = dir.path().join("bus.conf");
        std::fs::write(&config, CONFIG).ok()?;
        let mut child = Command::new("dbus-daemon")
            .arg("--config-file")
            .arg(&config)
            .args(["--nofork", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        let address = line.trim().to_string();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(PrivateBus { child, address, _config: dir })
    }

    async fn connect(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .await
            .expect("connect to the private bus")
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone)]
struct Shown {
    app_name: String,
    replaces_id: u32,
    summary: String,
    body: String,
    urgency: Option<u8>,
    desktop_entry: Option<String>,
}

struct FakeServer {
    shown: Arc<Mutex<Vec<Shown>>>,
    refuse: Arc<Mutex<bool>>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl FakeServer {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &mut self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> zbus::fdo::Result<u32> {
        if *self.refuse.lock().unwrap() {
            return Err(zbus::fdo::Error::Failed("the server is in do-not-disturb mode".into()));
        }
        let mut shown = self.shown.lock().unwrap();
        shown.push(Shown {
            app_name,
            replaces_id,
            summary,
            body,
            urgency: hints.get("urgency").and_then(|v| u8::try_from(v).ok()),
            desktop_entry: hints
                .get("desktop-entry")
                .and_then(|v| String::try_from(v.clone()).ok()),
        });
        Ok(if replaces_id != 0 { replaces_id } else { 40 + shown.len() as u32 })
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("Fake Notifications".into(), "tests".into(), "1".into(), "1.2".into())
    }
}

async fn serve(bus: &PrivateBus) -> (zbus::Connection, Arc<Mutex<Vec<Shown>>>, Arc<Mutex<bool>>) {
    let shown = Arc::new(Mutex::new(Vec::new()));
    let refuse = Arc::new(Mutex::new(false));
    let conn = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at(
            "/org/freedesktop/Notifications",
            FakeServer { shown: shown.clone(), refuse: refuse.clone() },
        )
        .unwrap()
        .build()
        .await
        .expect("serve the fake notification server");
    (conn, shown, refuse)
}

fn service(dir: &std::path::Path) -> Arc<Service> {
    Service::new(Options {
        read_only: true,
        config_path: Some(dir.join("none.toml")),
        cache_dir: Some(dir.join("cache")),
        state_path: Some(dir.join("state.json")),
        language: Some(LanguageSetting::En),
        keep_web_links: false,
        ..Options::default()
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_test_notification_reaches_the_server_like_a_real_one() {
    let Some(bus) = PrivateBus::start() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let (_server, shown, refuse) = serve(&bus).await;
    let dir = tempfile::tempdir().unwrap();
    let svc = service(dir.path());
    let notifier = Arc::new(DbusNotifier::new(bus.connect().await));
    svc.set_notifier(notifier.clone());

    // The sample, in English and then in Japanese; allowed although the daemon is read-only.
    svc.send_test_notification().await.unwrap();
    svc.set_language("ja").unwrap();
    svc.send_test_notification().await.unwrap();
    // A real notification takes the very same road (fire and forget).
    notifier.send(Notification {
        key: "ink:C:low".into(),
        summary: "Cyan ink is low".into(),
        body: "The tank is at 10%.".into(),
        icon: "dialog-warning",
        critical: false,
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while shown.lock().unwrap().len() < 3 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("three notifications");

    let s = shown.lock().unwrap().clone();
    assert_eq!(
        (s[0].summary.as_str(), s[0].body.as_str()),
        ("Washi test notification", "Notifications from Washi work.")
    );
    assert_eq!(s[1].summary, "Washiのテスト通知");
    for n in &s {
        assert_eq!(n.app_name, "Washi");
        assert_eq!(n.desktop_entry.as_deref(), Some("io.github.Jenari_Dev.Washi"), "{n:?}");
        assert_eq!(n.urgency, Some(1), "{n:?}");
    }
    assert_eq!(s[0].replaces_id, 0);
    assert_eq!(s[1].replaces_id, 41, "a repeated test replaces the earlier one, as real ones do");
    assert_eq!(s[2].replaces_id, 0, "another condition is a notification of its own");

    // The server being asked who it is (the self-check).
    let probe = notifier.probe().await.unwrap();
    assert_eq!(probe, "Fake Notifications");

    // The server refusing is reported with its reason, in the app's language.
    *refuse.lock().unwrap() = true;
    let e = svc.send_test_notification().await.unwrap_err();
    assert!(matches!(e, ServiceError::Notification(_)), "{e}");
    assert!(e.message(Language::En).contains("do-not-disturb"), "{}", e.message(Language::En));
    assert!(e.message(Language::Ja).contains("do-not-disturb"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_notification_service_the_test_says_so() {
    let Some(bus) = PrivateBus::start() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    // Nobody owns org.freedesktop.Notifications on this bus.
    let dir = tempfile::tempdir().unwrap();
    let svc = service(dir.path());
    let notifier = Arc::new(DbusNotifier::new(bus.connect().await));
    svc.set_notifier(notifier.clone());
    let e = svc.send_test_notification().await.unwrap_err();
    assert!(matches!(e, ServiceError::Notification(_)), "{e}");
    let en = e.message(Language::En);
    assert!(en.contains("org.freedesktop.Notifications") && en.contains("notification"), "{en}");
    assert!(e.message(Language::Ja).contains("通知サービス"));
    assert!(notifier.probe().await.is_err());

    // The self-check reports the same.
    let report: serde_json::Value = serde_json::from_str(&svc.doctor().await.unwrap()).unwrap();
    let note = report["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["checks"].as_array().unwrap())
        .find(|c| c["id"] == "desktop.notifications")
        .unwrap();
    assert_eq!(note["status"], "fail", "{note}");
    assert!(!note["hint"].as_str().unwrap().is_empty());
}
