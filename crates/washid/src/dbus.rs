// SPDX-License-Identifier: GPL-2.0-or-later
//! D-Bus API on the session bus.
//!
//! Bus name `io.github.Jenari_Dev.Washi`, object `/io/github/Jenari_Dev/Washi`, interface
//! `io.github.Jenari_Dev.Washi1`. Structured data is passed as JSON strings. Errors carry names
//! below `io.github.Jenari_Dev.Washi.Error.` (`ReadOnly`, `UnknownPrinter`, `InvalidArgs`,
//! `NotConfirmed`, `Busy`, `Printer`).

use std::sync::Arc;

use tokio::sync::broadcast::{self, error::RecvError};
use washi_core::Language;
use zbus::{interface, object_server::SignalEmitter};

use crate::service::{Event, Service, ServiceError, snapshot_json, version};

pub const BUS_NAME: &str = "io.github.Jenari_Dev.Washi";
pub const PATH: &str = "/io/github/Jenari_Dev/Washi";

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "io.github.Jenari_Dev.Washi.Error")]
pub enum DbusError {
    #[zbus(error)]
    ZBus(zbus::Error),
    ReadOnly(String),
    UnknownPrinter(String),
    InvalidArgs(String),
    NotConfirmed(String),
    Busy(String),
    Printer(String),
}

impl DbusError {
    /// A service error with its message in the language the app asked for.
    fn from_service(e: ServiceError, lang: Language) -> Self {
        let msg = e.message(lang);
        match e {
            ServiceError::ReadOnly(_) => DbusError::ReadOnly(msg),
            ServiceError::UnknownPrinter(_) | ServiceError::NoPrinter => {
                DbusError::UnknownPrinter(msg)
            }
            ServiceError::InvalidArgs(_) => DbusError::InvalidArgs(msg),
            ServiceError::NotConfirmed(_) => DbusError::NotConfirmed(msg),
            ServiceError::Busy(_) => DbusError::Busy(msg),
            ServiceError::Printer(_) => DbusError::Printer(msg),
        }
    }
}

pub struct Washi {
    svc: Arc<Service>,
}

impl Washi {
    pub fn new(svc: Arc<Service>) -> Self {
        Washi { svc }
    }

    /// The error to answer with: the message follows the app's language.
    fn err(&self, e: ServiceError) -> DbusError {
        DbusError::from_service(e, self.svc.language())
    }
}

fn json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".to_string())
}

#[interface(name = "io.github.Jenari_Dev.Washi1")]
impl Washi {
    /// Known printers as JSON `[{"host", "name", "source", "active", "power", "last_updated"}]`.
    async fn list_printers(&self) -> String {
        json(&self.svc.list_printers())
    }

    /// Browse mDNS (`_canon-chmp._tcp`, ~3 s) and add what is found; returns the new list.
    /// Emits `PrintersChanged` when something was added. Allowed in read-only mode.
    async fn discover(&self) -> Result<String, DbusError> {
        self.svc.discover().await.map(|v| json(&v)).map_err(|e| self.err(e))
    }

    /// Select the printer that is polled every interval. An unknown but well-formed host name
    /// or address (`host` or `host:port`) is added. An empty `host` is not allowed here.
    async fn set_active_printer(&self, host: String) -> Result<(), DbusError> {
        self.svc.set_active_printer(&host).await.map_err(|e| self.err(e))
    }

    /// The latest `PrinterSnapshot` as JSON: from memory when younger than the polling
    /// interval or while a maintenance job runs, else freshly read. Empty `host` = active.
    async fn get_snapshot(&self, host: String) -> Result<String, DbusError> {
        self.svc.get_snapshot(&host).await.map(|s| snapshot_json(&s)).map_err(|e| self.err(e))
    }

    /// Read the printer now, device information (model, firmware version, update available)
    /// included, and return the snapshot (also emitted as `SnapshotChanged` when something
    /// changed).
    async fn refresh(&self, host: String) -> Result<String, DbusError> {
        self.svc.refresh(&host).await.map(|s| snapshot_json(&s)).map_err(|e| self.err(e))
    }

    /// `{"current": DeviceConfig, "allowed": DeviceCapability}` as JSON.
    async fn get_settings(&self, host: String) -> Result<String, DbusError> {
        self.svc.get_settings(&host).await.map_err(|e| self.err(e))
    }

    /// `GetCapability maintenance` as JSON (cleaning types / ink groups, test prints).
    async fn get_maintenance_capability(&self, host: String) -> Result<String, DbusError> {
        self.svc.get_maintenance_capability(&host).await.map_err(|e| self.err(e))
    }

    /// Start a maintenance job; returns the job id. WRITES to the printer, so `op_json` must
    /// contain `"confirm": true` and the daemon must not be read-only. Example:
    /// `{"confirm": true, "op": "clean", "kind": "deep", "group": "all"}`; ops: `clean`,
    /// `nozzle_check`, `align_auto`, `align_check`, `align_manual` (+`"id"`), `roller_clean`.
    /// Progress arrives as `MaintenanceProgress`, the end as `MaintenanceFinished`.
    async fn run_maintenance(&self, host: String, op_json: String) -> Result<String, DbusError> {
        self.svc.run_maintenance(&host, &op_json).await.map_err(|e| self.err(e))
    }

    /// Validate, write, read back, and return `{"changed", "changes": [{field, before, after}],
    /// "current"}`. WRITES, so `changes_json` must contain `"confirm": true`. Keys:
    /// `auto_power_on`, `auto_power_off`, `auto_power_off_standalone`, `quiet`, `dry_level`,
    /// `paper_gap`, `auto_firmware`, `cloud_link`, and `setregistration` (manual head alignment
    /// values, `{"1": {"A": 1, "B": -2}}`, checked against the printer's ranges).
    async fn apply_settings(
        &self,
        host: String,
        changes_json: String,
    ) -> Result<String, DbusError> {
        self.svc.apply_settings(&host, &changes_json).await.map_err(|e| self.err(e))
    }

    /// Scanner capabilities and state as JSON, `{"capabilities": {...}, "status": {...}}`:
    /// sources, colour modes, resolutions, formats, intents, scan area (1/300 inch). Read-only,
    /// allowed with `--read-only`.
    async fn get_scanner_capabilities(&self, host: String) -> Result<String, DbusError> {
        self.svc.get_scanner_capabilities(&host).await.map_err(|e| self.err(e))
    }

    /// Scan one page; returns the session id. WRITES (moves the scanner), so `settings_json`
    /// must contain `"confirm": true` and the daemon must not be read-only. Example:
    /// `{"confirm": true, "size": "a4", "resolution": 300, "mode": "color", "type": "document"}`
    /// (`size`: a4, letter, a5, b5, l, 2l, postcard, card, auto; `mode`: color, gray, bw;
    /// `type`: document, photo). Each page arrives as `ScanPageReady`, the end as
    /// `ScanFinished`.
    async fn start_scan(&self, host: String, settings_json: String) -> Result<String, DbusError> {
        self.svc.start_scan(&host, &settings_json).await.map_err(|e| self.err(e))
    }

    /// Print a photo, PDF or Office file (absolute `path`); returns the job id. WRITES (prints),
    /// so `options_json` must contain `"confirm": true`. Keys (all optional): `size`,
    /// `media_type`, `source` (auto, rear, cassette), `borderless`, `quality` (draft, normal,
    /// high), `color`, `copies`, `pages` ("1-3,5"), `layout` (1, 2, 4, 6, 9, 16), `duplex`
    /// (long, short), `orientation` (auto, portrait, landscape), `job_name`. Progress arrives as `PrintProgress`, the end as
    /// `PrintFinished`.
    async fn print_file(
        &self,
        host: String,
        path: String,
        options_json: String,
    ) -> Result<String, DbusError> {
        self.svc.print_file(&host, &path, &options_json).await.map_err(|e| self.err(e))
    }

    /// Like `PrintFile` for several files at once (`paths_json` is a JSON array of absolute
    /// paths): photos for the 2/4/6/9/16-in-1 layouts go onto shared sheets in the order given.
    async fn print_files(
        &self,
        host: String,
        paths_json: String,
        options_json: String,
    ) -> Result<String, DbusError> {
        let paths: Vec<String> = serde_json::from_str(&paths_json).map_err(|e| {
            self.err(ServiceError::invalid(format!("paths must be a JSON array of strings: {e}")))
        })?;
        self.svc.print_files(&host, &paths, &options_json).await.map_err(|e| self.err(e))
    }

    /// What the printer prints on: JSON `{sources, media_types, sizes, media, qualities, duplex,
    /// copies_max, ...}` where `media[source][media type][size key]` is a bit set (1 = with
    /// margins, 2 = borderless). Read-only IPP queries, allowed with `--read-only`; cached for
    /// half an hour.
    async fn get_print_capabilities(&self, host: String) -> Result<String, DbusError> {
        self.svc.get_print_capabilities(&host).await.map_err(|e| self.err(e))
    }

    /// Wake a printer that is in auto power-off (IVEC `PowerOn` with the power-on mode
    /// `ModeA`; verified on a real XK140, firmware 1.060). WRITES, so `options_json` must
    /// contain `"confirm": true` and the daemon must not be read-only. After the request the
    /// printer is read again for up to about 8 seconds; the call then returns
    /// `{"sent": true, "verified", "note"}` with `verified` true when that status showed the
    /// printer awake. A slower printer keeps being watched, so watch `SnapshotChanged`.
    async fn wake(&self, host: String, options_json: String) -> Result<String, DbusError> {
        self.svc.wake(&host, &options_json).await.map_err(|e| self.err(e))
    }

    /// The web pages the app can point to, as JSON: `ink`, `help`, `error` (with a support
    /// `code`, else absent), `support`, `printer_page`, `maintenance_page`, each
    /// `{"url", "source": "maker" | "printer"}`, plus `from_printer`. They come from the
    /// printer's own web script (read-only) and fall back to the printer's own web page. Pass
    /// an empty `code` when there is none. Allowed in read-only mode; nothing is opened.
    async fn get_web_links(&self, host: String, code: String) -> Result<String, DbusError> {
        self.svc.get_web_links(&host, &code).await.map_err(|e| self.err(e))
    }

    /// The printer's own display names for its keywords in a language (`en`, `ja`, ...), as a
    /// JSON object `{"media-type.<keyword>": "Name", ...}` (`{}` when the printer has none in
    /// that language). They come from the printer itself (IPP `printer-strings-uri`, read-only)
    /// and are kept in `~/.cache/washi`; a kept copy is answered at once and read again in the
    /// background after 30 days or a firmware change. Allowed in read-only mode.
    async fn get_printer_strings(
        &self,
        host: String,
        language: String,
    ) -> Result<String, DbusError> {
        self.svc.get_printer_strings(&host, &language).await.map_err(|e| self.err(e))
    }

    /// The path of a picture of the printer (PNG; IPP `printer-icons`, read-only), kept in
    /// `~/.cache/washi`; `""` when the printer offers none. Refreshed like `GetPrinterStrings`.
    /// Allowed in read-only mode.
    async fn get_printer_icon(&self, host: String) -> Result<String, DbusError> {
        self.svc.get_printer_icon(&host).await.map_err(|e| self.err(e))
    }

    /// Update the printer's firmware (IVEC `VendorCmd UpdateFRom`, **unverified** on real
    /// printers) and follow it until the printer has restarted. WRITES: `options_json` must be
    /// `{"confirm": true}`, the daemon must not be read-only, and the printer must say an
    /// update is available. Returns the job id; progress arrives as `MaintenanceProgress`
    /// (stages `sent`, `restarting`, `waiting`, `back`), the end as `MaintenanceFinished`.
    async fn update_firmware(
        &self,
        host: String,
        options_json: String,
    ) -> Result<String, DbusError> {
        self.svc.update_firmware(&host, &options_json).await.map_err(|e| self.err(e))
    }

    /// The desktop-notification switches as JSON `{"enabled", "ink", "errors", "online"}`.
    async fn get_notification_settings(&self) -> String {
        self.svc.get_notification_settings()
    }

    /// Change some of the notification switches (a JSON object with any of the four keys) and
    /// save them to `config.toml`; returns all four. Refused with `--read-only`.
    async fn set_notification_settings(&self, settings_json: String) -> Result<String, DbusError> {
        self.svc.set_notification_settings(&settings_json).map_err(|e| self.err(e))
    }

    /// Remove a printer from the list (never touches the printer). Returns
    /// `{"forgotten": true, "persistent": bool, "active": host-or-null}`: printers from
    /// `--printer` / `config.toml` come back when the daemon restarts, and so does everything
    /// with `--read-only`, which writes no state file.
    async fn forget_printer(&self, host: String) -> Result<String, DbusError> {
        self.svc.forget_printer(&host).await.map_err(|e| self.err(e))
    }

    /// Print a disc label through the disc tray; returns the job id (signals as `PrintFile`).
    /// WRITES, so `label_json` must contain `"confirm": true`. Keys: `image` (path), `fit`,
    /// `scale`, `offset_mm`, `rotation`, `background_color`, `texts` (a list of
    /// `{text, x_mm, y_mm, size_pt, color, rotation, align, font | family, bold, italic, ring}`),
    /// `inner_mm`, `outer_mm`,
    /// `media` (a or b), `quality`.
    async fn print_disc(&self, host: String, label_json: String) -> Result<String, DbusError> {
        self.svc.print_disc(&host, &label_json).await.map_err(|e| self.err(e))
    }

    /// Ask a running scan or print job to stop at its next opportunity. Returns false when no
    /// such job runs.
    async fn cancel_job(&self, job: String) -> bool {
        self.svc.cancel_job(&job)
    }

    /// Set the language of notifications and of the messages in errors and job signals: `en`,
    /// `ja` or `system` (the session's locale). The app calls this when the user switches
    /// language and when it connects; it saves the choice in `config.toml` itself. Not a write
    /// to the printer, so it works with `--read-only`. Returns the language now in use.
    async fn set_language(&self, language: String) -> Result<String, DbusError> {
        self.svc.set_language(&language).map(|l| l.code().to_string()).map_err(|e| self.err(e))
    }

    /// Daemon version.
    async fn version(&self) -> String {
        version().to_string()
    }

    /// True when the daemon refuses every write.
    #[zbus(property)]
    async fn read_only(&self) -> bool {
        self.svc.read_only()
    }

    /// The language of notifications and messages: `en` or `ja` (see `SetLanguage`).
    #[zbus(property)]
    async fn language(&self) -> String {
        self.svc.language().code().to_string()
    }

    /// Seconds between polls of the active printer.
    #[zbus(property)]
    async fn poll_interval_secs(&self) -> u32 {
        u32::try_from(self.svc.poll_interval().as_secs()).unwrap_or(u32::MAX)
    }

    /// The snapshot of a printer changed (JSON `PrinterSnapshot`).
    #[zbus(signal)]
    pub async fn snapshot_changed(
        emitter: &SignalEmitter<'_>,
        host: &str,
        json: &str,
    ) -> zbus::Result<()>;

    /// The list of printers or the active printer changed; call `ListPrinters`.
    #[zbus(signal)]
    pub async fn printers_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// `stage` is `job-started`, `accepted`, `waiting`, `job-ended` or `closing`.
    #[zbus(signal)]
    pub async fn maintenance_progress(
        emitter: &SignalEmitter<'_>,
        job: &str,
        stage: &str,
        message: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn maintenance_finished(
        emitter: &SignalEmitter<'_>,
        job: &str,
        ok: bool,
        message: &str,
    ) -> zbus::Result<()>;

    /// Page `page_index` (0-based) of scan `session` is ready: `temp_path` is a JPEG in a
    /// private temporary directory (the GUI loads it into a scan session).
    #[zbus(signal)]
    pub async fn scan_page_ready(
        emitter: &SignalEmitter<'_>,
        session: &str,
        page_index: u32,
        temp_path: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn scan_finished(
        emitter: &SignalEmitter<'_>,
        session: &str,
        ok: bool,
        message: &str,
    ) -> zbus::Result<()>;

    /// `stage` is `preflight`, `preparing`, `validated`, `sending`, `submitted`, `state` or
    /// `finished`.
    #[zbus(signal)]
    pub async fn print_progress(
        emitter: &SignalEmitter<'_>,
        job: &str,
        stage: &str,
        message: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn print_finished(
        emitter: &SignalEmitter<'_>,
        job: &str,
        ok: bool,
        message: &str,
    ) -> zbus::Result<()>;
}

/// Turn service events into D-Bus signals until the channel closes.
pub async fn forward_events(mut rx: broadcast::Receiver<Event>, emitter: SignalEmitter<'static>) {
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(RecvError::Lagged(n)) => {
                tracing::warn!("dropped {n} events");
                continue;
            }
            Err(RecvError::Closed) => return,
        };
        let r = match &ev {
            Event::Snapshot { host, json } => Washi::snapshot_changed(&emitter, host, json).await,
            Event::PrintersChanged => Washi::printers_changed(&emitter).await,
            Event::MaintProgress { job, stage, message } => {
                Washi::maintenance_progress(&emitter, job, stage, message).await
            }
            Event::MaintFinished { job, ok, message } => {
                Washi::maintenance_finished(&emitter, job, *ok, message).await
            }
            Event::ScanPage { session, index, path } => {
                Washi::scan_page_ready(&emitter, session, *index, path).await
            }
            Event::ScanFinished { session, ok, message } => {
                Washi::scan_finished(&emitter, session, *ok, message).await
            }
            Event::PrintProgress { job, stage, message } => {
                Washi::print_progress(&emitter, job, stage, message).await
            }
            Event::PrintFinished { job, ok, message } => {
                Washi::print_finished(&emitter, job, *ok, message).await
            }
        };
        if let Err(e) = r {
            tracing::warn!("signal failed: {e}");
        }
    }
}
