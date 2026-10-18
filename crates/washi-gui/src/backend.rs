// SPDX-License-Identifier: GPL-2.0-or-later
//! QML-facing backend: a thin D-Bus client of the `washid` daemon.
//!
//! The UI thread never waits on D-Bus. `call()` queues a request for a worker thread (which owns
//! a small tokio runtime and one session-bus connection) and returns an id; the answer comes back
//! as the `reply(id, ok, body)` signal. Daemon signals (`SnapshotChanged`, `MaintenanceProgress`,
//! ...) are relayed as `bus_signal(name, a, b, c)`. Every structured value is a JSON string, as
//! in the daemon's API, so this file knows nothing about printers.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("shot.h");
        fn washi_grab_window(path: &QString) -> bool;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, connected)]
        #[qproperty(bool, readonly)]
        #[qproperty(QString, version)]
        /// The language the interface is in: "en" or "ja".
        #[qproperty(QString, language)]
        type Backend = super::BackendRust;

        /// Start the worker thread (idempotent). `call()` starts it on demand too.
        #[qinvokable]
        fn start(self: Pin<&mut Backend>);

        /// Queue a call of a daemon method. `args_json` is a JSON array of strings. Returns the
        /// request id; the outcome arrives as `reply(id, ok, body)`. On failure `body` is the
        /// JSON object `{"name": "ReadOnly", "message": "..."}`.
        #[qinvokable]
        fn call(self: Pin<&mut Backend>, method: &QString, args_json: &QString) -> i32;

        /// Switch the interface language: "en", "ja" or "system" (follow the session's locale).
        /// Takes effect at once, saves `[ui] language` in `config.toml`, and returns the language
        /// now shown ("en" or "ja"; "" for a value that is none of the three). Telling the daemon
        /// is up to the caller (`SetLanguage`).
        #[qinvokable]
        #[cxx_name = "chooseLanguage"]
        fn choose_language(self: Pin<&mut Backend>, language: &QString) -> QString;

        /// Environment variable value ("" if unset); used for headless checks.
        #[qinvokable]
        #[cxx_name = "envVar"]
        fn env_var(self: &Backend, name: &QString) -> QString;

        /// Save the window (popups included) to a PNG. Headless screenshot check only.
        #[qinvokable]
        #[cxx_name = "saveScreenshot"]
        fn save_screenshot(self: &Backend, path: &QString) -> bool;

        /// `file://` URL of the local picture-pack image for a printer model
        /// (`~/.local/share/washi/printers/<MODEL>.png`), or "" when the pack has none.
        #[qinvokable]
        #[cxx_name = "packImage"]
        fn pack_image(self: &Backend, model: &QString) -> QString;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        fn reply(self: Pin<&mut Backend>, id: i32, ok: bool, body: QString);

        /// A signal of the daemon: `SnapshotChanged(host, json)`, `PrintersChanged()`,
        /// `MaintenanceProgress(job, stage, message)`, `MaintenanceFinished(job, "true"|"false",
        /// message)`, `PrintProgress(job, stage, message)`, `PrintFinished(job, "true"|"false",
        /// message)`, `ScanPageReady(session, "index", path)`, `ScanFinished(session,
        /// "true"|"false", message)`.
        #[qsignal]
        #[cxx_name = "busSignal"]
        fn bus_signal(self: Pin<&mut Backend>, name: QString, a: QString, b: QString, c: QString);
    }

    impl cxx_qt::Threading for Backend {}
}

use std::net::ToSocketAddrs;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use washi_core::LanguageSetting;
use zbus::proxy::CacheProperties;
use zbus::{Connection, Proxy};

const BUS: &str = "io.github.Jenari_Dev.Washi";
const PATH: &str = "/io/github/Jenari_Dev/Washi";
const IFACE: &str = "io.github.Jenari_Dev.Washi1";
const ERROR_PREFIX: &str = "io.github.Jenari_Dev.Washi.Error.";

/// Methods the UI may call. `x-Resolve` is local (host name -> IPv4 address).
const METHODS: &[&str] = &[
    "ListPrinters",
    "Discover",
    "SetActivePrinter",
    "GetSnapshot",
    "Refresh",
    "GetSettings",
    "GetMaintenanceCapability",
    "RunMaintenance",
    "ApplySettings",
    "GetScannerCapabilities",
    "StartScan",
    "PrintFile",
    "PrintFiles",
    "PrintDisc",
    "CancelJob",
    "GetPrintCapabilities",
    "Wake",
    "GetWebLinks",
    "GetPrinterStrings",
    "GetPrinterIcon",
    "UpdateFirmware",
    "GetNotificationSettings",
    "SetNotificationSettings",
    "SendTestNotification",
    "Doctor",
    "SetLanguage",
    "ForgetPrinter",
    "Version",
    "x-Resolve",
];

pub struct BackendRust {
    connected: bool,
    readonly: bool,
    version: QString,
    language: QString,
    tx: Option<UnboundedSender<Request>>,
    next_id: Arc<AtomicI32>,
}

impl Default for BackendRust {
    fn default() -> Self {
        BackendRust {
            connected: false,
            readonly: false,
            version: QString::default(),
            language: QString::from(crate::i18n::started_in().code()),
            tx: None,
            next_id: Arc::default(),
        }
    }
}

struct Request {
    id: i32,
    method: String,
    args: Vec<String>,
}

impl qobject::Backend {
    pub fn start(mut self: Pin<&mut Self>) {
        if self.tx.is_some() {
            return;
        }
        let (tx, rx) = unbounded_channel();
        self.as_mut().rust_mut().tx = Some(tx);
        let qt = self.qt_thread();
        std::thread::Builder::new()
            .name("washi-dbus".into())
            .spawn(move || worker(qt, rx))
            .expect("spawn the D-Bus worker thread");
    }

    pub fn call(mut self: Pin<&mut Self>, method: &QString, args_json: &QString) -> i32 {
        self.as_mut().start();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let args: Vec<String> =
            serde_json::from_str::<Vec<serde_json::Value>>(&args_json.to_string())
                .unwrap_or_default()
                .into_iter()
                .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
                .collect();
        let req = Request { id, method: method.to_string(), args };
        let sent = self.tx.as_ref().is_some_and(|tx| tx.send(req).is_ok());
        if !sent {
            // Answer later, never inside call(): the caller does not know the id yet.
            let body = error_body("Internal", "the D-Bus worker is not running");
            let _ = self
                .qt_thread()
                .queue(move |mut o| o.as_mut().reply(id, false, QString::from(&body)));
        }
        id
    }

    pub fn choose_language(mut self: Pin<&mut Self>, language: &QString) -> QString {
        let Some(setting) = LanguageSetting::parse_strict(&language.to_string()) else {
            return QString::default();
        };
        let lang = match crate::i18n::save_setting(setting) {
            Ok(l) => l,
            Err(e) => {
                // Not being able to save is no reason to refuse the switch.
                eprintln!("washi-app: cannot save the language to config.toml: {e}");
                setting.resolve()
            }
        };
        crate::i18n::apply(lang);
        self.as_mut().set_language(QString::from(lang.code()));
        QString::from(lang.code())
    }

    pub fn env_var(&self, name: &QString) -> QString {
        QString::from(&std::env::var(name.to_string()).unwrap_or_default())
    }

    pub fn save_screenshot(&self, path: &QString) -> bool {
        qobject::washi_grab_window(path)
    }

    pub fn pack_image(&self, model: &QString) -> QString {
        QString::from(&pack_image_path(&model.to_string()).unwrap_or_default())
    }
}

/// The picture-pack image for `model` ("XK140 series" -> `XK140.png`, `XK140 series.png`). The
/// pack is a local-only set of files the user installs; Washi ships none.
fn pack_image_path(model: &str) -> Option<String> {
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.');
    if model.is_empty() || !model.chars().all(ok) || model.contains("..") {
        return None;
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })?
        .join("washi/printers");
    let short = model.trim_end_matches(" series").trim_end_matches(" Series").trim();
    let names = [
        short.to_string(),
        model.to_string(),
        short.to_ascii_uppercase(),
        short.to_ascii_lowercase(),
    ];
    let found = names.iter().map(|n| base.join(format!("{n}.png"))).find(|p| p.is_file())?;
    let shown = trimmed_pack_copy(&found).unwrap_or(found);
    Some(format!("file://{}", shown.display()))
}

/// Pack renders often sit in a large transparent canvas, which makes the printer tiny in the
/// banner. Keep a copy with the margin cut in `~/.cache/washi/pack/` (rewritten when the source
/// file changes); `None` when nothing needs cutting or the copy cannot be written.
fn trimmed_pack_copy(src: &std::path::Path) -> Option<std::path::PathBuf> {
    let meta = std::fs::metadata(src).ok()?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?
        .join("washi/pack");
    let name = src.file_stem()?.to_string_lossy().into_owned();
    let out = cache.join(format!("{name}-{stamp}-{}.png", meta.len()));
    if out.is_file() {
        return Some(out);
    }
    let png = std::fs::read(src).ok()?;
    let icon = washi_core::net::web::PrinterIcon { png, width: 0, height: 0 };
    let (trimmed, _, _) = washi_core::localization::trim_margin(&icon)?;
    std::fs::create_dir_all(&cache).ok()?;
    // Older copies of the same picture are no longer needed.
    if let Ok(dir) = std::fs::read_dir(&cache) {
        for e in dir.flatten() {
            let f = e.file_name().to_string_lossy().into_owned();
            if f.starts_with(&format!("{name}-")) && e.path() != out {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, trimmed).ok()?;
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}

fn error_body(name: &str, message: &str) -> String {
    serde_json::json!({ "name": name, "message": message }).to_string()
}

/// Turn a zbus error into `(name, message)`; daemon errors carry the `Washi.Error.` prefix.
fn split_error(e: &zbus::Error) -> (String, String) {
    match e {
        zbus::Error::MethodError(name, msg, _) => {
            let name = name.as_str();
            let short = name.strip_prefix(ERROR_PREFIX).unwrap_or(name).to_string();
            (short, msg.clone().unwrap_or_default())
        }
        e => ("Bus".to_string(), e.to_string()),
    }
}

fn worker(qt: cxx_qt::CxxQtThread<qobject::Backend>, mut rx: UnboundedReceiver<Request>) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("washi-dbus-rt")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("washi-app: cannot start the async runtime: {e}");
            return;
        }
    };
    rt.block_on(async move {
        // The session bus itself may be missing (no desktop session): keep retrying quietly.
        let conn = loop {
            match Connection::session().await {
                Ok(c) => break c,
                Err(_) => {
                    // Fail every request meanwhile.
                    while let Ok(req) = rx.try_recv() {
                        reply_err(&qt, req.id, "Bus", "no D-Bus session bus");
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        };
        let proxy: Arc<tokio::sync::RwLock<Option<Proxy<'static>>>> =
            Arc::new(tokio::sync::RwLock::new(None));
        tokio::spawn(presence(conn.clone(), qt.clone(), proxy.clone()));

        while let Some(req) = rx.recv().await {
            let (qt, proxy) = (qt.clone(), proxy.clone());
            tokio::spawn(async move {
                let result = handle(&proxy, &req).await;
                let id = req.id;
                let _ = qt.queue(move |mut o| match result {
                    Ok(body) => o.as_mut().reply(id, true, QString::from(&body)),
                    Err((name, msg)) => {
                        o.as_mut().reply(id, false, QString::from(&error_body(&name, &msg)))
                    }
                });
            });
        }
    });
}

fn reply_err(qt: &cxx_qt::CxxQtThread<qobject::Backend>, id: i32, name: &str, msg: &str) {
    let body = error_body(name, msg);
    let _ = qt.queue(move |mut o| o.as_mut().reply(id, false, QString::from(&body)));
}

async fn handle(
    proxy: &tokio::sync::RwLock<Option<Proxy<'static>>>,
    req: &Request,
) -> Result<String, (String, String)> {
    if !METHODS.contains(&req.method.as_str()) {
        return Err(("InvalidArgs".into(), format!("unknown method {}", req.method)));
    }
    if req.method == "x-Resolve" {
        let host = req.args.first().cloned().unwrap_or_default();
        return Ok(tokio::task::spawn_blocking(move || resolve(&host)).await.unwrap_or_default());
    }
    let proxy = proxy.read().await.clone();
    let Some(proxy) = proxy else {
        return Err(("NotRunning".into(), "the Washi service (washid) is not running".into()));
    };
    let call = async {
        let msg = match req.args.as_slice() {
            [] => proxy.call_method(req.method.as_str(), &()).await,
            [a] => proxy.call_method(req.method.as_str(), &(a.as_str(),)).await,
            [a, b] => proxy.call_method(req.method.as_str(), &(a.as_str(), b.as_str())).await,
            [a, b, c] => {
                proxy.call_method(req.method.as_str(), &(a.as_str(), b.as_str(), c.as_str())).await
            }
            _ => return Err(("InvalidArgs".to_string(), "too many arguments".to_string())),
        }
        .map_err(|e| split_error(&e))?;
        // Methods that return nothing have an empty body: that is an empty string here;
        // `CancelJob` answers a boolean.
        let body = msg.body();
        Ok(match body.deserialize::<String>() {
            Ok(s) => s,
            Err(_) => body.deserialize::<bool>().map(|b| b.to_string()).unwrap_or_default(),
        })
    };
    match tokio::time::timeout(Duration::from_secs(90), call).await {
        Ok(r) => r,
        Err(_) => Err(("Timeout".into(), "the service did not answer in time".into())),
    }
}

/// `host` or `host:port` to an IPv4 address (empty when it does not resolve). Blocks, so it runs
/// on the blocking pool. Resolution is a normal name lookup (mDNS through nss-mdns if installed).
fn resolve(host: &str) -> String {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() {
        return String::new();
    }
    let target = if host.contains(':') && !host.starts_with('[') && host.matches(':').count() == 1 {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    target
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.find(|a| a.is_ipv4()))
        .map(|a| a.ip().to_string())
        .unwrap_or_default()
}

/// Watch for the daemon appearing and disappearing; on arrival read `Version` and `ReadOnly` and
/// relay its signals.
async fn presence(
    conn: Connection,
    qt: cxx_qt::CxxQtThread<qobject::Backend>,
    shared: Arc<tokio::sync::RwLock<Option<Proxy<'static>>>>,
) {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else { return };
    // If the user installed the D-Bus activation file, this starts washid on demand; otherwise
    // it fails at once and the UI shows how to start the service.
    if let Ok(name) = zbus::names::WellKnownName::try_from(BUS) {
        let dbus = dbus.clone();
        tokio::spawn(async move {
            let _ =
                tokio::time::timeout(Duration::from_secs(30), dbus.start_service_by_name(name, 0))
                    .await;
        });
    }
    let mut present = false;
    let mut relay: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        let now = match zbus::names::BusName::try_from(BUS) {
            Ok(n) => dbus.name_has_owner(n).await.unwrap_or(false),
            Err(_) => false,
        };
        if now && !present {
            // A cache-free proxy: the properties are read once per daemon start.
            if let Ok(proxy) = zbus::proxy::Builder::<Proxy<'static>>::new(&conn)
                .destination(BUS)
                .and_then(|b| b.path(PATH))
                .and_then(|b| b.interface(IFACE))
                .map(|b| b.cache_properties(CacheProperties::No))
            {
                if let Ok(p) = proxy.build().await {
                    let version: String = match p.call_method("Version", &()).await {
                        Ok(m) => m.body().deserialize::<String>().unwrap_or_default(),
                        Err(_) => String::new(),
                    };
                    let ro = p.get_property::<bool>("ReadOnly").await.unwrap_or(false);
                    *shared.write().await = Some(p.clone());
                    let q = qt.clone();
                    let _ = q.queue(move |mut o| {
                        o.as_mut().set_version(QString::from(&version));
                        o.as_mut().set_readonly(ro);
                        o.as_mut().set_connected(true);
                    });
                    if let Some(h) = relay.take() {
                        h.abort();
                    }
                    relay = Some(tokio::spawn(relay_signals(p, qt.clone())));
                    present = true;
                }
            }
        } else if !now && present {
            present = false;
            *shared.write().await = None;
            if let Some(h) = relay.take() {
                h.abort();
            }
            let _ = qt.queue(|mut o| o.as_mut().set_connected(false));
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
}

/// Forward the daemon's signals to QML.
async fn relay_signals(proxy: Proxy<'static>, qt: cxx_qt::CxxQtThread<qobject::Backend>) {
    let Ok(mut stream) = proxy.receive_all_signals().await else { return };
    while let Some(msg) = stream.next().await {
        let header = msg.header();
        let Some(member) = header.member().map(|m| m.to_string()) else { continue };
        let body = msg.body();
        let (name, a, b, c) = match member.as_str() {
            "SnapshotChanged" => match body.deserialize::<(String, String)>() {
                Ok((host, json)) => (member, host, json, String::new()),
                Err(_) => continue,
            },
            "PrintersChanged" => (member, String::new(), String::new(), String::new()),
            "MaintenanceProgress" => match body.deserialize::<(String, String, String)>() {
                Ok((job, stage, message)) => (member, job, stage, message),
                Err(_) => continue,
            },
            "MaintenanceFinished" | "ScanFinished" | "PrintFinished" => {
                match body.deserialize::<(String, bool, String)>() {
                    Ok((job, ok, message)) => (member, job, ok.to_string(), message),
                    Err(_) => continue,
                }
            }
            "PrintProgress" => match body.deserialize::<(String, String, String)>() {
                Ok((job, stage, message)) => (member, job, stage, message),
                Err(_) => continue,
            },
            "ScanPageReady" => match body.deserialize::<(String, u32, String)>() {
                Ok((session, index, path)) => (member, session, index.to_string(), path),
                Err(_) => continue,
            },
            _ => continue,
        };
        let queued = qt.queue(move |mut o| {
            o.as_mut().bus_signal(
                QString::from(&name),
                QString::from(&a),
                QString::from(&b),
                QString::from(&c),
            );
        });
        if queued.is_err() {
            break;
        }
    }
}
