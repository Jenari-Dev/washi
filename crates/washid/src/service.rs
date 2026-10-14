// SPDX-License-Identifier: GPL-2.0-or-later
//! The daemon's logic without any D-Bus: printer registry, polling, snapshots, settings,
//! maintenance jobs and notifications. [`crate::dbus`] is a thin wrapper around this.
//!
//! * All device access for one printer is serialised by a per-printer lock, so only one CHMP
//!   operation is in flight at a time. A maintenance job holds that lock for its whole run;
//!   meanwhile snapshot requests are answered from memory (with `activity` set) and never touch
//!   the network.
//! * `--read-only`: every write method fails with [`ServiceError::ReadOnly`] before it parses
//!   its argument or resolves the printer, so nothing can reach the network.
//! * Logs name printers by a short hash (`printer 3fa2c1`), never by host name, serial or MAC.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use thiserror::Error;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use washi_core::disc::{self, DiscSpec};
use washi_core::ivec::IvecError;
use washi_core::net::ipp::normalize_language;
use washi_core::print::{self, PrintProgress, PrintSpec, RunOptions};
use washi_core::scan::flow::{ScanProgress, ScanSpec, run_scan_streaming};
use washi_core::{
    Config, Confirm, FirmwareProgress, Language, LanguageSetting, LocalizationCache, MaintOp,
    MaintSpec, NotificationConfig, Power, Printer, PrinterSnapshot, Progress, SettingsSpec,
    SnapshotCache, Tools, WebSources, config_changes, printer_key, unix_now,
};

use crate::messages::{self, JobKind, Text};
use crate::notify::{Notifier, NotifyState};
use crate::state::{StateFile, StoredPrinter};

/// How long a request waits for the per-printer lock before giving up.
const LOCK_WAIT: Duration = Duration::from_secs(20);
/// Pauses before each look at a printer that was just asked to wake up (about 8 s in all).
const WAKE_LOOKS_MS: [u64; 5] = [500, 1000, 1500, 2000, 3000];

/// How long the media summary of a printer is reused.
const PRINT_CAPS_MAX_AGE: Duration = Duration::from_secs(30 * 60);

/// How long what the printer says about its web pages is reused.
const WEB_SOURCES_MAX_AGE: Duration = Duration::from_secs(30 * 60);

/// After a failed read of the printer's strings or picture, the next try waits this long.
const LOCALIZATION_RETRY_AFTER: Duration = Duration::from_secs(60);
/// How long a read of the printer's strings or picture waits for the printer's other work.
const LOCALIZATION_LOCK_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("washid is running with --read-only: {0} is disabled")]
    ReadOnly(&'static str),
    #[error("unknown printer {0:?}: call Discover() or SetActivePrinter() first")]
    UnknownPrinter(String),
    #[error("no printer is configured: call Discover() or SetActivePrinter()")]
    NoPrinter,
    #[error("{0}")]
    InvalidArgs(Text),
    #[error("{0} writes to the printer: pass \"confirm\": true after the user has agreed")]
    NotConfirmed(&'static str),
    #[error("{0}")]
    Busy(Text),
    #[error("{0}")]
    Printer(Text),
}

impl ServiceError {
    /// The message in `lang`. `Display` is the English one.
    pub fn message(&self, lang: Language) -> String {
        match self {
            ServiceError::ReadOnly(what) => messages::read_only(lang, what),
            ServiceError::UnknownPrinter(key) => messages::unknown_printer(lang, key),
            ServiceError::NoPrinter => messages::no_printer(lang),
            ServiceError::NotConfirmed(what) => messages::not_confirmed(lang, what),
            ServiceError::InvalidArgs(t) | ServiceError::Busy(t) | ServiceError::Printer(t) => {
                t.get(lang).to_string()
            }
        }
    }

    /// Something went wrong inside the daemon (a worker died, a value cannot be encoded).
    fn internal(detail: impl std::fmt::Display) -> ServiceError {
        ServiceError::Printer(Text::new(
            detail.to_string(),
            format!("内部エラーが発生しました（{detail}）"),
        ))
    }

    pub fn invalid(detail: impl AsRef<str>) -> ServiceError {
        ServiceError::InvalidArgs(messages::invalid_args(detail.as_ref()))
    }
}

pub type Result<T> = std::result::Result<T, ServiceError>;

fn from_core(e: washi_core::Error) -> ServiceError {
    match &e {
        washi_core::Error::Invalid(_) | washi_core::Error::Ivec(IvecError::Invalid { .. }) => {
            ServiceError::InvalidArgs(messages::core_error(&e))
        }
        _ => ServiceError::Printer(messages::core_error(&e)),
    }
}

/// Host names compare case-insensitively and without a trailing dot.
fn norm_host(h: &str) -> String {
    h.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where a printer in the list came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// `--printer HOST`.
    Flag,
    /// `[printer] host` in `config.toml`.
    Config,
    /// Remembered from an earlier run (state file).
    Saved,
    /// Found by mDNS in this run.
    Discovered,
    /// Added with `SetActivePrinter` in this run.
    Manual,
}

/// One row of `ListPrinters`.
#[derive(Debug, Clone, Serialize)]
pub struct PrinterEntry {
    pub host: String,
    pub name: Option<String>,
    pub source: Source,
    pub active: bool,
    /// Last known power state, if the printer has been read in this run.
    pub power: Option<Power>,
    pub last_updated: Option<u64>,
}

/// Signals for the D-Bus layer (and tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Snapshot {
        host: String,
        json: String,
    },
    PrintersChanged,
    MaintProgress {
        job: String,
        stage: String,
        message: String,
    },
    MaintFinished {
        job: String,
        ok: bool,
        message: String,
    },
    /// Page `index` (0-based) of scan `session` was written to `path`.
    ScanPage {
        session: String,
        index: u32,
        path: String,
    },
    ScanFinished {
        session: String,
        ok: bool,
        message: String,
    },
    PrintProgress {
        job: String,
        stage: String,
        message: String,
    },
    PrintFinished {
        job: String,
        ok: bool,
        message: String,
    },
}

pub type PrinterFactory = Arc<dyn Fn(&str) -> Printer + Send + Sync>;

pub struct Options {
    /// Refuse every write (maintenance, settings). Reads, discovery and polling still work.
    pub read_only: bool,
    /// Polling interval of the active printer; `None` = `[daemon] poll_interval_secs`.
    pub poll_interval: Option<Duration>,
    /// Config file (`None` = `~/.config/washi/config.toml`).
    pub config_path: Option<PathBuf>,
    /// Snapshot cache directory (`None` = `~/.cache/washi`).
    pub cache_dir: Option<PathBuf>,
    /// State file (`None` = `~/.local/state/washi/washid.json`). Never written when read-only.
    pub state_path: Option<PathBuf>,
    /// Printers given on the command line.
    pub printers: Vec<String>,
    /// The interface language (`None` = `[ui] language`, which follows the session's locale
    /// unless it says otherwise).
    pub language: Option<LanguageSetting>,
    /// How to build a [`Printer`] for a host string (tests inject fakes here).
    pub printer_factory: Option<PrinterFactory>,
    pub discover_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            read_only: false,
            poll_interval: None,
            config_path: None,
            cache_dir: None,
            state_path: None,
            printers: Vec::new(),
            language: None,
            printer_factory: None,
            discover_timeout: Duration::from_secs(3),
        }
    }
}

struct ActiveJob {
    id: String,
    kind: JobKind,
    label: Text,
    /// Set by `CancelJob`; workers look at it between steps.
    cancel: Arc<AtomicBool>,
}

impl ActiveJob {
    fn activity(&self, lang: Language) -> String {
        messages::activity(lang, self.kind, &self.label)
    }

    /// `kind: label` in both languages (for "busy" messages).
    fn text(&self) -> Text {
        Text::new(self.activity(Language::En), self.activity(Language::Ja))
    }
}

#[derive(Default)]
struct SlotInner {
    snapshot: Option<PrinterSnapshot>,
    fetched: Option<Instant>,
    job: Option<ActiveJob>,
    notify: NotifyState,
}

/// One known printer.
pub struct Slot {
    host: String,
    log_id: String,
    source: Mutex<Source>,
    name: Mutex<Option<String>>,
    printer: Printer,
    cache: Option<SnapshotCache>,
    /// Held for the duration of every operation on the printer (CHMP and IPP alike).
    device: Mutex<()>,
    inner: Mutex<SlotInner>,
    /// The printer's media summary (a 366 KB database boiled down), read at most once per
    /// [`PRINT_CAPS_MAX_AGE`].
    print_caps: Mutex<Option<(Instant, String)>>,
    /// The printer's web links (`model.js`, the page links), read at most once per
    /// [`WEB_SOURCES_MAX_AGE`].
    web: Mutex<Option<(Instant, WebSources)>>,
    /// The printer's own strings and picture, kept between runs.
    words_cache: Option<LocalizationCache>,
    words: Mutex<WordsState>,
}

/// What the daemon remembers about reading the printer's strings and picture: which reads are
/// under way (so a stale copy is refreshed once, not once per call) and when one last failed.
#[derive(Default)]
struct WordsState {
    reading: std::collections::HashSet<String>,
    failed: std::collections::HashMap<String, Instant>,
}

impl Slot {
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Acquire the device lock, waiting up to `wait`.
    fn lock_device(&self, wait: Duration) -> Option<MutexGuard<'_, ()>> {
        let deadline = Instant::now() + wait;
        loop {
            match self.device.try_lock() {
                Ok(g) => return Some(g),
                Err(std::sync::TryLockError::Poisoned(p)) => return Some(p.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    /// What the running job is (in both languages), if there is one.
    fn job_text(&self) -> Option<Text> {
        lk(&self.inner).job.as_ref().map(ActiveJob::text)
    }

    /// The remembered snapshot, with `activity` set (in `lang`) while a job runs.
    fn cached(&self, lang: Language) -> Option<PrinterSnapshot> {
        let inner = lk(&self.inner);
        let mut snap = inner.snapshot.clone()?;
        snap.activity = inner.job.as_ref().map(|j| j.activity(lang));
        Some(snap)
    }

    /// The firmware version of the last snapshot (`None` before the printer was read).
    fn current_firmware(&self) -> Option<String> {
        let inner = lk(&self.inner);
        let fw = inner.snapshot.as_ref()?.device.as_ref()?.firmware.trim().to_string();
        (!fw.is_empty()).then_some(fw)
    }

    /// Read the strings of a language again and keep them. The caller holds the device lock.
    fn read_strings(&self, lang: &str) {
        let key = format!("strings-{lang}");
        match self.printer.fetch_strings(lang) {
            Ok(fetched) => {
                lk(&self.words).failed.remove(&key);
                if let Some(c) = &self.words_cache {
                    if let Err(e) = c.store_strings(&fetched, unix_now()) {
                        debug!("printer {}: cannot keep the strings: {e}", self.log_id);
                    }
                }
            }
            Err(e) => {
                debug!("printer {}: strings not read: {e}", self.log_id);
                lk(&self.words).failed.insert(key, Instant::now());
            }
        }
    }

    /// Read the picture again and keep it (or the fact that there is none), returning its path
    /// (`""` for none). The caller holds the device lock.
    fn read_icon(&self) -> washi_core::Result<String> {
        let key = "icon";
        match self.printer.fetch_icon() {
            Ok(fetched) => {
                lk(&self.words).failed.remove(key);
                let Some(c) = &self.words_cache else {
                    return Ok(String::new());
                };
                if let Err(e) = c.store_icon(&fetched, unix_now()) {
                    debug!("printer {}: cannot keep the picture: {e}", self.log_id);
                    return Ok(String::new());
                }
                Ok(if fetched.png.is_some() {
                    c.icon_path().display().to_string()
                } else {
                    String::new()
                })
            }
            Err(e) => {
                debug!("printer {}: picture not read: {e}", self.log_id);
                lk(&self.words).failed.insert(key.to_string(), Instant::now());
                Err(e)
            }
        }
    }

    /// The remembered snapshot if it is younger than `max_age` or a job is running.
    fn fresh(&self, max_age: Duration, lang: Language) -> Option<PrinterSnapshot> {
        let inner = lk(&self.inner);
        let job = inner.job.as_ref().map(|j| j.activity(lang));
        let recent = inner.fetched.is_some_and(|t| t.elapsed() < max_age);
        if job.is_none() && !recent {
            return None;
        }
        let mut snap = inner.snapshot.clone()?;
        snap.activity = job;
        Some(snap)
    }
}

struct Registry {
    slots: Vec<Arc<Slot>>,
    active: Option<String>,
}

pub struct Service {
    read_only: bool,
    poll_interval: Duration,
    config: Config,
    /// `[notifications]`, changeable at run time (`SetNotificationSettings`).
    notifications: Mutex<NotificationConfig>,
    /// The language of notifications and messages (`SetLanguage`; not saved by the daemon: the
    /// app owns `[ui] language`).
    language: Mutex<Language>,
    /// Where `[notifications]` is saved.
    config_file: Option<PathBuf>,
    state_path: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    factory: PrinterFactory,
    discover_timeout: Duration,
    registry: Mutex<Registry>,
    events: broadcast::Sender<Event>,
    notifier: OnceLock<Arc<dyn Notifier>>,
    jobs: AtomicU64,
}

/// What a running maintenance job is doing, from the printer's `jobinfo`
/// (`maintenance_operation`, `type`, `jobprogress`); `None` when it says nothing useful.
fn job_phrase(job: &washi_core::ivec::JobInfo, lang: Language) -> Option<String> {
    let get = |k: &str| job.fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let (op, ty) = (get("maintenance_operation")?, get("type").unwrap_or(""));
    Some(messages::job_phrase(lang, op, ty))
}

/// `kind`/`group` strings, progress stages and messages (in `lang`) for the maintenance signals.
pub fn progress_event(p: &Progress, op: &MaintOp, lang: Language) -> (&'static str, String) {
    match p {
        Progress::JobStarted => ("job-started", messages::job_started(lang)),
        Progress::CommandAccepted { .. } => ("accepted", messages::command_accepted(lang, op)),
        Progress::Waiting { elapsed, status, support_code, job } => {
            // While a job runs the real printer answers `NG Suspended` to status reads: that is
            // "working", not a problem. `jobinfo` says what it is doing.
            let status = if status == "suspended" { "working" } else { status.as_str() };
            let phrase = job.as_ref().and_then(|j| job_phrase(j, lang));
            ("waiting", messages::waiting(lang, elapsed.as_secs(), phrase, status, support_code))
        }
        Progress::JobEnded => ("job-ended", messages::job_ended(lang)),
        Progress::Ending => ("closing", messages::closing_job(lang)),
    }
}

/// Stage and message (in `lang`) of a firmware update's progress.
pub fn firmware_event(p: &FirmwareProgress, lang: Language) -> (&'static str, String) {
    match p {
        FirmwareProgress::Sent => ("sent", messages::firmware_sent(lang)),
        FirmwareProgress::Restarting => ("restarting", messages::firmware_restarting(lang)),
        FirmwareProgress::Waiting { elapsed, restarted } => {
            ("waiting", messages::firmware_waiting(lang, elapsed.as_secs(), *restarted))
        }
        FirmwareProgress::Back => ("back", messages::firmware_back(lang)),
    }
}

pub fn snapshot_json(s: &PrinterSnapshot) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "{}".to_string())
}

impl Service {
    pub fn new(opts: Options) -> anyhow::Result<Arc<Service>> {
        let config = match &opts.config_path {
            Some(p) => Config::load_from(p),
            None => Config::load(),
        }
        .unwrap_or_else(|e| {
            warn!("ignoring unreadable config file: {e}");
            Config::default()
        });
        let state_file = opts.state_path.clone().or_else(StateFile::default_path);
        let state = state_file.as_deref().map(StateFile::load).unwrap_or_default();
        // Read-only runs read the state file but never write it.
        let state_path = if opts.read_only { None } else { state_file };
        let (events, _) = broadcast::channel(256);
        let svc = Service {
            read_only: opts.read_only,
            poll_interval: opts.poll_interval.unwrap_or_else(|| config.daemon.poll_interval()),
            notifications: Mutex::new(config.notifications.clone()),
            language: Mutex::new(opts.language.unwrap_or(config.ui.language).resolve()),
            config_file: opts.config_path.clone().or_else(Config::default_path),
            config,
            state_path,
            cache_dir: opts.cache_dir.clone(),
            factory: opts
                .printer_factory
                .clone()
                .unwrap_or_else(|| Arc::new(|host: &str| Printer::from_target(host))),
            discover_timeout: opts.discover_timeout,
            registry: Mutex::new(Registry { slots: Vec::new(), active: None }),
            events,
            notifier: OnceLock::new(),
            jobs: AtomicU64::new(0),
        };
        for h in &opts.printers {
            svc.add_printer(h, None, Source::Flag);
        }
        if let Some(h) = svc.config.printer.host.clone() {
            svc.add_printer(&h, None, Source::Config);
        }
        for p in &state.printers {
            svc.add_printer(&p.host, p.name.clone(), Source::Saved);
        }
        {
            let mut reg = lk(&svc.registry);
            let known = |h: &String| reg.slots.iter().any(|s| &s.host == h);
            let flag =
                reg.slots.iter().find(|s| *lk(&s.source) == Source::Flag).map(|s| s.host.clone());
            let active = state
                .active
                .clone()
                .filter(known)
                .or(flag)
                .or_else(|| svc.config.printer.host.clone().filter(known))
                .or_else(|| reg.slots.first().map(|s| s.host.clone()));
            reg.active = active;
        }
        Ok(Arc::new(svc))
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The language of notifications and of the messages in errors and job signals.
    pub fn language(&self) -> Language {
        *lk(&self.language)
    }

    fn lang(&self) -> Language {
        self.language()
    }

    /// `SetLanguage`: `en`, `ja` or `system` (the session's locale). Not a write to the
    /// printer, so it works with `--read-only`; the app saves the choice in `config.toml`.
    pub fn set_language(&self, value: &str) -> Result<Language> {
        let setting = LanguageSetting::parse_strict(value)
            .ok_or_else(|| ServiceError::InvalidArgs(messages::bad_language(value)))?;
        let lang = setting.resolve();
        *lk(&self.language) = lang;
        info!("language: {lang}");
        Ok(lang)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Where notifications go. Set once, before polling starts.
    pub fn set_notifier(&self, n: Arc<dyn Notifier>) {
        let _ = self.notifier.set(n);
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    fn ensure_writable(&self, what: &'static str) -> Result<()> {
        if self.read_only { Err(ServiceError::ReadOnly(what)) } else { Ok(()) }
    }

    // ---- registry ----------------------------------------------------------------------

    /// Add a printer, or update the name of a known one. Returns true when it is new.
    fn add_printer(&self, host: &str, name: Option<String>, source: Source) -> bool {
        let host = host.trim();
        if host.is_empty() {
            return false;
        }
        let mut reg = lk(&self.registry);
        if let Some(s) = reg.slots.iter().find(|s| s.host == host) {
            if name.is_some() {
                *lk(&s.name) = name;
            }
            return false;
        }
        let printer = (self.factory)(host);
        let cache = match &self.cache_dir {
            Some(d) => Some(SnapshotCache::in_dir(d, printer.host(), printer.chmp_port())),
            None => printer.default_cache(),
        };
        let words_cache = match &self.cache_dir {
            Some(d) => Some(LocalizationCache::in_dir(d, printer.host(), printer.chmp_port())),
            None => LocalizationCache::for_printer(printer.host(), printer.chmp_port()),
        };
        let log_id = printer_key(printer.host(), printer.chmp_port())[..6].to_string();
        reg.slots.push(Arc::new(Slot {
            host: host.to_string(),
            log_id,
            source: Mutex::new(source),
            name: Mutex::new(name),
            printer,
            cache,
            device: Mutex::new(()),
            inner: Mutex::new(SlotInner::default()),
            print_caps: Mutex::new(None),
            web: Mutex::new(None),
            words_cache,
            words: Mutex::new(WordsState::default()),
        }));
        true
    }

    fn resolve(&self, host: &str) -> Result<Arc<Slot>> {
        let reg = lk(&self.registry);
        let host = host.trim();
        let key = if host.is_empty() {
            reg.active.as_deref().ok_or(ServiceError::NoPrinter)?
        } else {
            host
        };
        reg.slots
            .iter()
            .find(|s| s.host == key)
            .cloned()
            .ok_or_else(|| ServiceError::UnknownPrinter(key.to_string()))
    }

    pub fn active_host(&self) -> Option<String> {
        lk(&self.registry).active.clone()
    }

    pub fn list_printers(&self) -> Vec<PrinterEntry> {
        let reg = lk(&self.registry);
        reg.slots
            .iter()
            .map(|s| {
                let inner = lk(&s.inner);
                PrinterEntry {
                    host: s.host.clone(),
                    name: lk(&s.name).clone(),
                    source: *lk(&s.source),
                    active: reg.active.as_deref() == Some(s.host.as_str()),
                    power: inner.snapshot.as_ref().map(|p| p.power),
                    last_updated: inner.snapshot.as_ref().map(|p| p.last_updated),
                }
            })
            .collect()
    }

    /// Remember manually added and discovered printers and the active one (not when read-only).
    fn persist(&self) {
        let Some(path) = &self.state_path else { return };
        let reg = lk(&self.registry);
        let state = StateFile {
            active: reg.active.clone(),
            printers: reg
                .slots
                .iter()
                .filter(|s| !matches!(*lk(&s.source), Source::Flag | Source::Config))
                .map(|s| StoredPrinter { host: s.host.clone(), name: lk(&s.name).clone() })
                .collect(),
        };
        drop(reg);
        if let Err(e) = state.save(path) {
            warn!("cannot save the state file: {e}");
        }
    }

    /// Browse mDNS for Canon printers and add what is found. Read-only network access, so it
    /// also works with `--read-only`.
    pub async fn discover(self: &Arc<Self>) -> Result<Vec<PrinterEntry>> {
        let timeout = self.discover_timeout;
        let found = tokio::task::spawn_blocking(move || washi_core::net::discover(timeout))
            .await
            .map_err(ServiceError::internal)?
            .map_err(|e| {
                ServiceError::Printer(Text::new(
                    format!("mDNS discovery failed: {e}"),
                    format!("ネットワーク上のプリンターを検索できませんでした（{e}）"),
                ))
            })?;
        let mut new = 0;
        for p in &found {
            // The same printer may already be known under its mDNS name or any of its addresses.
            let known = {
                let reg = lk(&self.registry);
                let mut names: Vec<String> = p.addresses.iter().map(|a| norm_host(a)).collect();
                names.push(norm_host(&p.host));
                reg.slots
                    .iter()
                    .find(|s| names.contains(&norm_host(&s.host)))
                    .map(|s| s.host.clone())
            };
            let host = known.unwrap_or_else(|| p.connect_host());
            if self.add_printer(&host, Some(p.name.clone()), Source::Discovered) {
                new += 1;
            }
        }
        if new > 0 {
            info!("discovery found {} printer(s), {new} new", found.len());
            let mut reg = lk(&self.registry);
            if reg.active.is_none() {
                reg.active = reg.slots.first().map(|s| s.host.clone());
            }
            drop(reg);
            self.persist();
            self.emit(Event::PrintersChanged);
        }
        Ok(self.list_printers())
    }

    /// Select the active (polled) printer; an unknown, well-formed host is added.
    pub async fn set_active_printer(self: &Arc<Self>, host: &str) -> Result<()> {
        let host = host.trim();
        washi_core::Config::default().set_host(host).map_err(from_core)?;
        let added = self.add_printer(host, None, Source::Manual);
        let changed = {
            let mut reg = lk(&self.registry);
            let changed = reg.active.as_deref() != Some(host);
            reg.active = Some(host.to_string());
            changed
        };
        if added || changed {
            self.persist();
            self.emit(Event::PrintersChanged);
            // Get a first reading going so the UI is not empty.
            let svc = self.clone();
            let host = host.to_string();
            tokio::spawn(async move {
                let _ = svc.refresh(&host).await;
            });
        }
        Ok(())
    }

    // ---- snapshots ---------------------------------------------------------------------

    /// The latest snapshot: from memory when it is younger than the polling interval (or a job
    /// is running), else freshly read.
    pub async fn get_snapshot(self: &Arc<Self>, host: &str) -> Result<PrinterSnapshot> {
        let slot = self.resolve(host)?;
        if let Some(s) = slot.fresh(self.poll_interval, self.lang()) {
            return Ok(s);
        }
        self.refresh_slot(slot).await
    }

    /// Read the printer now, including its device information (model, firmware version): this
    /// is what "Update Information" and "Check for updates" ask for.
    pub async fn refresh(self: &Arc<Self>, host: &str) -> Result<PrinterSnapshot> {
        let slot = self.resolve(host)?;
        self.refresh_slot_with(slot, true).await
    }

    async fn refresh_slot(self: &Arc<Self>, slot: Arc<Slot>) -> Result<PrinterSnapshot> {
        self.refresh_slot_with(slot, false).await
    }

    async fn refresh_slot_with(
        self: &Arc<Self>,
        slot: Arc<Slot>,
        reread_info: bool,
    ) -> Result<PrinterSnapshot> {
        let svc = self.clone();
        tokio::task::spawn_blocking(move || svc.refresh_blocking(&slot, reread_info))
            .await
            .map_err(|e| ServiceError::internal(format!("refresh task failed: {e}")))?
    }

    fn refresh_blocking(&self, slot: &Arc<Slot>, reread_info: bool) -> Result<PrinterSnapshot> {
        // While a job runs the network belongs to the job.
        if slot.job_text().is_some() {
            return slot
                .cached(self.lang())
                .ok_or_else(|| ServiceError::Busy(messages::busy_maintenance()));
        }
        let Some(_guard) = slot.lock_device(LOCK_WAIT) else {
            return slot
                .cached(self.lang())
                .ok_or_else(|| ServiceError::Busy(messages::busy_printer()));
        };
        if slot.job_text().is_some() {
            return slot
                .cached(self.lang())
                .ok_or_else(|| ServiceError::Busy(messages::busy_maintenance()));
        }
        let snap = if reread_info {
            slot.printer.snapshot_fresh(slot.cache.as_ref())
        } else {
            slot.printer.snapshot(slot.cache.as_ref())
        };
        Ok(self.record(slot, snap))
    }

    /// Store a fresh snapshot, announce it, and evaluate notifications.
    fn record(&self, slot: &Slot, snap: PrinterSnapshot) -> PrinterSnapshot {
        let (prev, notes) = {
            let mut inner = lk(&slot.inner);
            let prev = inner.snapshot.replace(snap.clone());
            inner.fetched = Some(Instant::now());
            let notes = inner.notify.evaluate(&lk(&self.notifications), &snap, self.lang());
            (prev, notes)
        };
        if prev.as_ref().map(|p| p.power) != Some(snap.power) {
            info!(
                "printer {}: {} -> {}",
                slot.log_id,
                prev.as_ref().map_or("unknown", |p| p.power.as_str()),
                snap.power.as_str()
            );
        }
        if let Some(n) = self.notifier.get() {
            for note in notes {
                n.send(note);
            }
        }
        if prev.as_ref().is_none_or(|p| !p.content_eq(&snap)) {
            self.emit(Event::Snapshot { host: slot.host.clone(), json: snapshot_json(&snap) });
        }
        snap
    }

    /// Poll the active printer every interval, for as long as the future is polled.
    pub async fn run_poller(self: Arc<Self>) {
        let mut tick = tokio::time::interval(self.poll_interval.max(Duration::from_millis(1)));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let Some(host) = self.active_host() else { continue };
            let polled = match self.resolve(&host) {
                Ok(slot) => self.refresh_slot(slot).await,
                Err(e) => Err(e),
            };
            if let Err(e) = polled {
                debug!("poll failed: {e}");
            }
        }
    }

    // ---- reads ---------------------------------------------------------------------------

    async fn read_json<F>(self: &Arc<Self>, host: &str, f: F) -> Result<String>
    where
        F: FnOnce(&Printer) -> washi_core::Result<serde_json::Value> + Send + 'static,
    {
        let slot = self.resolve(host)?;
        if let Some(label) = slot.job_text() {
            return Err(ServiceError::Busy(messages::busy_with(&label)));
        }
        tokio::task::spawn_blocking(move || {
            let _guard = slot
                .lock_device(LOCK_WAIT)
                .ok_or_else(|| ServiceError::Busy(messages::busy_printer()))?;
            f(&slot.printer).map(|v| v.to_string()).map_err(from_core)
        })
        .await
        .map_err(ServiceError::internal)?
    }

    /// Current device settings plus the values the printer accepts:
    /// `{"current": DeviceConfig, "allowed": DeviceCapability}`.
    pub async fn get_settings(self: &Arc<Self>, host: &str) -> Result<String> {
        self.read_json(host, |p| {
            let current = p.device_config()?;
            let allowed = p.device_capability()?;
            Ok(serde_json::json!({ "current": current, "allowed": allowed }))
        })
        .await
    }

    /// `GetCapability maintenance` as JSON: cleaning types and ink groups, test prints.
    pub async fn get_maintenance_capability(self: &Arc<Self>, host: &str) -> Result<String> {
        self.read_json(host, |p| {
            let cap = p.maintenance_capability()?;
            Ok(serde_json::to_value(cap).unwrap_or_default())
        })
        .await
    }

    // ---- writes (all guarded by --read-only and an explicit confirm) --------------------------

    /// Start a maintenance job on a worker; returns its id. Progress and the outcome arrive as
    /// [`Event::MaintProgress`] / [`Event::MaintFinished`].
    pub async fn run_maintenance(self: &Arc<Self>, host: &str, op_json: &str) -> Result<String> {
        self.ensure_writable("RunMaintenance")?;
        let spec = MaintSpec::from_json(op_json).map_err(from_core)?;
        if !spec.confirm {
            return Err(ServiceError::NotConfirmed("RunMaintenance"));
        }
        let op = spec.to_op().map_err(from_core)?;
        let slot = self.resolve(host)?;

        let (id, _) =
            self.begin_job(&slot, "job", JobKind::Maintenance, messages::maint_label(&op))?;
        info!("printer {}: {id} started ({})", slot.log_id, op.label());
        // Tell UIs right away that the printer is occupied.
        if let Some(snap) = slot.cached(self.lang()) {
            self.emit(Event::Snapshot { host: slot.host.clone(), json: snapshot_json(&snap) });
        }
        let svc = self.clone();
        let rt = tokio::runtime::Handle::current();
        let job = id.clone();
        tokio::task::spawn_blocking(move || svc.maintenance_worker(&slot, &op, &job, &rt));
        Ok(id)
    }

    fn maintenance_worker(
        self: Arc<Self>,
        slot: &Arc<Slot>,
        op: &MaintOp,
        job: &str,
        rt: &tokio::runtime::Handle,
    ) {
        // Wait for any poll that is mid-read; polls started later see the job and stay away.
        let guard = loop {
            if let Some(g) = slot.lock_device(Duration::from_secs(60)) {
                break g;
            }
        };
        let result = slot.printer.run_maintenance(op, Confirm::explicit(), &mut |p| {
            let (stage, message) = progress_event(&p, op, self.lang());
            self.emit(Event::MaintProgress {
                job: job.to_string(),
                stage: stage.to_string(),
                message,
            });
        });
        drop(guard);
        let (ok, message) = match &result {
            Ok(r) => (true, messages::maintenance_done(self.lang(), op, r.elapsed.as_secs())),
            Err(e) => (false, messages::core_error(e).get(self.lang()).to_string()),
        };
        lk(&slot.inner).job = None;
        info!("printer {}: {job} {}", slot.log_id, if ok { "finished" } else { "failed" });
        self.emit(Event::MaintFinished { job: job.to_string(), ok, message });
        // UIs were told the printer was occupied; tell them it is free again.
        if let Some(snap) = slot.cached(self.lang()) {
            self.emit(Event::Snapshot { host: slot.host.clone(), json: snapshot_json(&snap) });
        }
        // Pick up the printer's state after the job.
        let (svc, slot) = (self.clone(), slot.clone());
        rt.spawn(async move {
            let _ = svc.refresh_slot(slot).await;
        });
    }

    /// Validate, write, read back and return the diff:
    /// `{"changed": bool, "changes": [{field, before, after}], "current": DeviceConfig}`.
    pub async fn apply_settings(
        self: &Arc<Self>,
        host: &str,
        changes_json: &str,
    ) -> Result<String> {
        self.ensure_writable("ApplySettings")?;
        let spec = SettingsSpec::from_json(changes_json).map_err(from_core)?;
        if !spec.confirm {
            return Err(ServiceError::NotConfirmed("ApplySettings"));
        }
        let changes = spec.to_settings().map_err(from_core)?;
        let slot = self.resolve(host)?;
        if let Some(label) = slot.job_text() {
            return Err(ServiceError::Busy(messages::busy_with(&label)));
        }
        let log_id = slot.log_id.clone();
        let out = tokio::task::spawn_blocking(move || {
            let _guard = slot
                .lock_device(LOCK_WAIT)
                .ok_or_else(|| ServiceError::Busy(messages::busy_printer()))?;
            slot.printer.apply_device_settings(&changes, Confirm::explicit()).map_err(from_core)
        })
        .await
        .map_err(ServiceError::internal)??;
        let diff = config_changes(&out.before, &out.after);
        info!("printer {log_id}: settings applied ({} change(s))", diff.len());
        Ok(serde_json::json!({ "changed": out.changed, "changes": diff, "current": out.after })
            .to_string())
    }
}

impl Service {
    // ---- wake, notifications, forgetting printers ----------------------------------------------

    /// Wake a printer in auto power-off with IVEC `PowerOn` (with the power-on mode `ModeA`;
    /// verified on a real XK140, firmware 1.060, 2026-10-14). WRITES: `json` needs
    /// `"confirm": true`. After the request the printer is read again for a few seconds, so
    /// that UIs see it come up (`SnapshotChanged`); the answer says whether the follow-up
    /// status showed the printer awake (`"verified": true`). A printer that is slower keeps
    /// being watched in the background.
    pub async fn wake(self: &Arc<Self>, host: &str, json: &str) -> Result<String> {
        self.ensure_writable("Wake")?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WakeSpec {
            #[serde(default)]
            confirm: bool,
        }
        let spec: WakeSpec = serde_json::from_str(json)
            .map_err(|e| ServiceError::invalid(format!("invalid wake options: {e}")))?;
        if !spec.confirm {
            return Err(ServiceError::NotConfirmed("Wake"));
        }
        let slot = self.resolve(host)?;
        if let Some(label) = slot.job_text() {
            return Err(ServiceError::Busy(messages::busy_with(&label)));
        }
        let worker = slot.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = worker
                .lock_device(LOCK_WAIT)
                .ok_or_else(|| ServiceError::Busy(messages::busy_printer()))?;
            worker.printer.power_on(Confirm::explicit()).map_err(from_core)
        })
        .await
        .map_err(ServiceError::internal)??;
        info!("printer {}: wake request sent (PowerOn)", slot.log_id);
        // The request only says "accepted". The printer needs a few seconds to come up, so look
        // at its status until it does: that is what `verified` reports.
        let mut awake = false;
        for ms in WAKE_LOOKS_MS {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            if let Ok(s) = self.refresh_slot(slot.clone()).await {
                if s.power == Power::On {
                    awake = true;
                    break;
                }
            }
        }
        if awake {
            info!("printer {}: woke up (status confirms it)", slot.log_id);
        } else {
            info!("printer {}: no sign of life yet, still watching", slot.log_id);
            // A slow printer: keep looking so that the UIs see it when it arrives.
            let svc = self.clone();
            tokio::spawn(async move {
                for secs in [5u64, 10] {
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                    if let Ok(s) = svc.refresh_slot(slot.clone()).await {
                        if s.power == Power::On {
                            break;
                        }
                    }
                }
            });
        }
        let note = if awake {
            "the printer woke up (confirmed by its status)"
        } else {
            "the printer accepted the wake command but is not up yet; watch the status or press the power button"
        };
        Ok(serde_json::json!({ "sent": true, "verified": awake, "note": note }).to_string())
    }

    /// The `[notifications]` switches as JSON `{"enabled", "ink", "errors", "online"}`.
    pub fn get_notification_settings(&self) -> String {
        serde_json::to_string(&*lk(&self.notifications)).unwrap_or_else(|_| "{}".to_string())
    }

    /// Change some of the switches (a JSON object with any of the four keys, booleans), save
    /// them to `config.toml` and return all four. Not available with `--read-only`, which
    /// writes no files.
    pub fn set_notification_settings(&self, json: &str) -> Result<String> {
        self.ensure_writable("SetNotificationSettings")?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Patch {
            enabled: Option<bool>,
            ink: Option<bool>,
            errors: Option<bool>,
            online: Option<bool>,
        }
        let p: Patch = serde_json::from_str(json).map_err(|e| {
            ServiceError::invalid(format!(
                "invalid notification settings (booleans enabled, ink, errors, online): {e}"
            ))
        })?;
        let mut n = lk(&self.notifications).clone();
        n.enabled = p.enabled.unwrap_or(n.enabled);
        n.ink = p.ink.unwrap_or(n.ink);
        n.errors = p.errors.unwrap_or(n.errors);
        n.online = p.online.unwrap_or(n.online);
        if let Some(path) = &self.config_file {
            Config::save_notifications(path, &n).map_err(from_core)?;
        }
        *lk(&self.notifications) = n.clone();
        Ok(serde_json::to_string(&n).unwrap_or_else(|_| "{}".to_string()))
    }

    /// Remove a printer from the list. Printers that came from `--printer` or `config.toml`
    /// are only forgotten until the daemon restarts; the rest are also dropped from the state
    /// file. With `--read-only` nothing is saved, so the printer returns on the next start.
    /// Returns `{"forgotten", "persistent", "active"}`. Never touches the printer.
    pub async fn forget_printer(self: &Arc<Self>, host: &str) -> Result<String> {
        let host = host.trim();
        if host.is_empty() {
            return Err(ServiceError::invalid("name the printer to forget"));
        }
        let slot = self.resolve(host)?;
        if let Some(label) = slot.job_text() {
            return Err(ServiceError::Busy(messages::busy_with_here(&label)));
        }
        let source = *lk(&slot.source);
        let (active, was_active) = {
            let mut reg = lk(&self.registry);
            reg.slots.retain(|s| s.host != slot.host);
            let was_active = reg.active.as_deref() == Some(slot.host.as_str());
            if was_active {
                reg.active = reg.slots.first().map(|s| s.host.clone());
            }
            (reg.active.clone(), was_active)
        };
        let persistent = !self.read_only && !matches!(source, Source::Flag | Source::Config);
        self.persist();
        info!("printer {}: forgotten", slot.log_id);
        self.emit(Event::PrintersChanged);
        if was_active {
            if let Some(next) = active.clone() {
                let svc = self.clone();
                tokio::spawn(async move {
                    let _ = svc.refresh(&next).await;
                });
            }
        }
        Ok(serde_json::json!({ "forgotten": true, "persistent": persistent, "active": active })
            .to_string())
    }

    // ---- what the printer can print ----------------------------------------------------------

    /// The media summary (`washi_core::print::caps::PrintSummary`) as JSON: media types, sizes
    /// and, per source, type and size, whether bordered / borderless printing is offered.
    /// Read-only IPP queries; the answer is reused for half an hour.
    pub async fn get_print_capabilities(self: &Arc<Self>, host: &str) -> Result<String> {
        let slot = self.resolve(host)?;
        if let Some((at, json)) = lk(&slot.print_caps).as_ref() {
            if at.elapsed() < PRINT_CAPS_MAX_AGE {
                return Ok(json.clone());
            }
        }
        if let Some(label) = slot.job_text() {
            return Err(ServiceError::Busy(messages::busy_with(&label)));
        }
        let worker = slot.clone();
        let json = tokio::task::spawn_blocking(move || {
            let _guard = worker
                .lock_device(LOCK_WAIT)
                .ok_or_else(|| ServiceError::Busy(messages::busy_printer()))?;
            let summary = worker.printer.print_summary().map_err(from_core)?;
            serde_json::to_string(&summary)
                .map_err(|e| ServiceError::internal(format!("cannot encode the summary: {e}")))
        })
        .await
        .map_err(ServiceError::internal)??;
        *lk(&slot.print_caps) = Some((Instant::now(), json.clone()));
        Ok(json)
    }
}

impl Service {
    // ---- the printer's own words and picture -----------------------------------------------------

    /// The printer's display names for its keywords (`media-type.<keyword>`, ...) in a language,
    /// as a JSON object `{"media-type.com.example": "Name", ...}`; `{}` when the printer has none
    /// in that language. Reads only (IPP `Get-Printer-Attributes`, `GET` of the strings file), so
    /// it works with `--read-only`.
    ///
    /// A copy kept from an earlier read is answered at once, even an old one: when it is older
    /// than 30 days or was read from another firmware version than the printer reports now, it
    /// is read again in the background and the next call gets the new one. Only the very first
    /// call for a language waits for the printer, and a printer that could not be read is not
    /// asked again for a minute.
    pub async fn get_printer_strings(
        self: &Arc<Self>,
        host: &str,
        language: &str,
    ) -> Result<String> {
        let slot = self.resolve(host)?;
        let lang = normalize_language(language).ok_or_else(|| {
            ServiceError::invalid(format!(
                "{language:?} is not a language tag such as \"en\" or \"ja\""
            ))
        })?;
        let key = format!("strings-{lang}");
        let kept = slot.words_cache.as_ref().and_then(|c| c.strings(&lang));
        if let Some(k) = kept {
            if !k.is_current(slot.current_firmware().as_deref(), unix_now()) {
                self.read_words_in_background(&slot, key, move |s| {
                    s.read_strings(&lang);
                });
            }
            return encode_strings(&k.strings);
        }
        let worker = slot.clone();
        let strings = self
            .read_words_now(&slot, &key, move || {
                let fetched = worker.printer.fetch_strings(&lang)?;
                if let Some(c) = &worker.words_cache {
                    if let Err(e) = c.store_strings(&fetched, unix_now()) {
                        debug!("printer {}: cannot keep the strings: {e}", worker.log_id);
                    }
                }
                Ok(fetched.strings)
            })
            .await?;
        encode_strings(&strings)
    }

    /// The path of a picture of the printer (PNG, the transparent margin cut off) in the cache
    /// directory, read from the printer once and then reused; `""` when the printer offers none.
    /// Same rules as [`Service::get_printer_strings`]: reads only, a kept copy is answered at
    /// once and refreshed in the background when it is old or the firmware changed.
    pub async fn get_printer_icon(self: &Arc<Self>, host: &str) -> Result<String> {
        let slot = self.resolve(host)?;
        let kept = slot.words_cache.as_ref().and_then(|c| c.icon());
        if let Some(k) = kept {
            if !k.is_current(slot.current_firmware().as_deref(), unix_now()) {
                self.read_words_in_background(&slot, "icon".to_string(), |s| {
                    let _ = s.read_icon();
                });
            }
            return Ok(k.path.map(|p| p.display().to_string()).unwrap_or_default());
        }
        let worker = slot.clone();
        self.read_words_now(&slot, "icon", move || worker.read_icon()).await
    }

    /// Run a read of the printer's words in the background (once at a time per kind), for a
    /// copy that is only stale.
    fn read_words_in_background(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        key: String,
        read: impl FnOnce(&Slot) + Send + 'static,
    ) {
        {
            let mut w = lk(&slot.words);
            let recently_failed =
                w.failed.get(&key).is_some_and(|t| t.elapsed() < LOCALIZATION_RETRY_AFTER);
            if recently_failed || !w.reading.insert(key.clone()) {
                return;
            }
        }
        let slot = slot.clone();
        tokio::task::spawn_blocking(move || {
            // Only when the printer is free: a refresh is never worth waiting for.
            if slot.job_text().is_none() {
                if let Some(_guard) = slot.lock_device(LOCALIZATION_LOCK_WAIT) {
                    read(&slot);
                }
            }
            lk(&slot.words).reading.remove(&key);
        });
    }

    /// Read now, for a caller that has nothing kept: on the blocking pool, under the printer's
    /// lock, with the failure of a minute ago remembered.
    async fn read_words_now<T: Send + 'static>(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        key: &str,
        read: impl FnOnce() -> washi_core::Result<T> + Send + 'static,
    ) -> Result<T> {
        if lk(&slot.words).failed.get(key).is_some_and(|t| t.elapsed() < LOCALIZATION_RETRY_AFTER) {
            return Err(ServiceError::Printer(messages::words_unavailable()));
        }
        if let Some(label) = slot.job_text() {
            return Err(ServiceError::Busy(messages::busy_with(&label)));
        }
        let worker = slot.clone();
        let result = tokio::task::spawn_blocking(move || {
            let _guard = worker
                .lock_device(LOCALIZATION_LOCK_WAIT)
                .ok_or_else(|| ServiceError::Busy(messages::busy_printer()))?;
            read().map_err(from_core)
        })
        .await
        .map_err(ServiceError::internal)?;
        match &result {
            Ok(_) => {
                lk(&slot.words).failed.remove(key);
            }
            // Busy is not the printer's fault: ask again next time.
            Err(ServiceError::Busy(_)) => {}
            Err(_) => {
                lk(&slot.words).failed.insert(key.to_string(), Instant::now());
            }
        }
        result
    }
}

fn encode_strings(map: &std::collections::BTreeMap<String, String>) -> Result<String> {
    serde_json::to_string(map)
        .map_err(|e| ServiceError::internal(format!("cannot encode the strings: {e}")))
}

impl Service {
    // ---- web links -----------------------------------------------------------------------------

    /// The web pages the app can point to, as JSON (`washi_core::WebLinks`): `ink`, `help`,
    /// `error` (only with a `code`), `support`, `printer_page` and `maintenance_page`, each
    /// `{"url", "source": "maker" | "printer"}`, and `from_printer`. The printer's own script is
    /// read (read-only) and reused for half an hour; whatever it does not offer falls back to the
    /// printer's own web page. Never fails because the printer is busy or silent: the links are
    /// then fallbacks. The maker's addresses get the app's language.
    pub async fn get_web_links(self: &Arc<Self>, host: &str, code: &str) -> Result<String> {
        let slot = self.resolve(host)?;
        let lang = self.lang();
        let code = Some(code.trim()).filter(|c| !c.is_empty()).map(str::to_string);
        let cached = lk(&slot.web)
            .as_ref()
            .filter(|(at, w)| at.elapsed() < WEB_SOURCES_MAX_AGE && w.language == lang)
            .map(|(_, w)| w.clone());
        let sources = match cached {
            Some(w) => w,
            // A running job owns the printer: do not queue behind it for a menu entry.
            None if slot.job_text().is_some() => {
                WebSources { language: lang, ..Default::default() }
            }
            None => {
                let worker = slot.clone();
                let read = tokio::task::spawn_blocking(move || {
                    let _guard = worker.lock_device(Duration::from_secs(5))?;
                    Some(worker.printer.web_sources(lang))
                })
                .await
                .map_err(ServiceError::internal)?;
                match read {
                    Some(w) => {
                        // Only a read that found something is worth keeping.
                        if w.model.is_some() || !w.pages.is_empty() {
                            *lk(&slot.web) = Some((Instant::now(), w.clone()));
                        }
                        w
                    }
                    None => WebSources { language: lang, ..Default::default() },
                }
            }
        };
        let links =
            sources.links(slot.printer.host(), slot.printer.chmp_port(), code.as_deref(), lang);
        serde_json::to_string(&links)
            .map_err(|e| ServiceError::internal(format!("cannot encode the links: {e}")))
    }

    // ---- firmware ------------------------------------------------------------------------------

    /// Ask the printer to update its firmware (`VendorCmd UpdateFRom`), **unverified** on a real
    /// printer, and follow it until the printer has restarted. WRITES: `json` needs
    /// `"confirm": true`, the daemon must not be read-only, and the printer must say an update is
    /// available. Returns the job id; progress arrives as `MaintenanceProgress` (stages `sent`,
    /// `restarting`, `waiting`, `back`) and the end as `MaintenanceFinished`, whose message says
    /// the old and the new version.
    pub async fn update_firmware(self: &Arc<Self>, host: &str, json: &str) -> Result<String> {
        self.ensure_writable("UpdateFirmware")?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Spec {
            #[serde(default)]
            confirm: bool,
        }
        let spec: Spec = serde_json::from_str(json)
            .map_err(|e| ServiceError::invalid(format!("invalid firmware options: {e}")))?;
        if !spec.confirm {
            return Err(ServiceError::NotConfirmed("UpdateFirmware"));
        }
        let slot = self.resolve(host)?;
        let (id, _) =
            self.begin_job(&slot, "job", JobKind::Maintenance, messages::firmware_label())?;
        info!("printer {}: {id} started (firmware update, unverified)", slot.log_id);
        self.announce_activity(&slot);
        let svc = self.clone();
        let rt = tokio::runtime::Handle::current();
        let job = id.clone();
        tokio::task::spawn_blocking(move || {
            let events = svc.clone();
            svc.clone().run_job_with(slot, rt, true, move |slot| {
                let lang = events.lang();
                let opts = slot.printer.firmware_options().clone();
                let result = slot.printer.update_firmware(Confirm::explicit(), &opts, &mut |p| {
                    let (stage, message) = firmware_event(&p, lang);
                    events.emit(Event::MaintProgress {
                        job: job.clone(),
                        stage: stage.to_string(),
                        message,
                    });
                });
                let (ok, message) = match &result {
                    Ok(r) => (true, messages::firmware_done(lang, r)),
                    Err(e) => (false, messages::core_error(e).get(lang).to_string()),
                };
                info!("printer {}: {job} {}", slot.log_id, if ok { "finished" } else { "failed" });
                Event::MaintFinished { job: job.clone(), ok, message }
            });
        });
        Ok(id)
    }
}

impl Service {
    /// Register a running job on the printer, or say what is in the way.
    fn begin_job(
        &self,
        slot: &Arc<Slot>,
        id_prefix: &str,
        kind: JobKind,
        label: Text,
    ) -> Result<(String, Arc<AtomicBool>)> {
        let mut inner = lk(&slot.inner);
        if let Some(j) = &inner.job {
            return Err(ServiceError::Busy(messages::busy_job_still_running(&j.id, &j.text())));
        }
        let id = format!("{id_prefix}-{}", self.jobs.fetch_add(1, Ordering::SeqCst) + 1);
        let cancel = Arc::new(AtomicBool::new(false));
        inner.job = Some(ActiveJob { id: id.clone(), kind, label, cancel: cancel.clone() });
        Ok((id, cancel))
    }

    /// Announce that the printer is occupied (or free again) to UIs.
    fn announce_activity(&self, slot: &Slot) {
        if let Some(snap) = slot.cached(self.lang()) {
            self.emit(Event::Snapshot { host: slot.host.clone(), json: snapshot_json(&snap) });
        }
    }

    /// Wait for the device lock (a poll may be mid-read), run `work`, release the printer, then
    /// announce the outcome (`work` returns the finishing event) and that the printer is free.
    fn run_job<F>(self: Arc<Self>, slot: Arc<Slot>, rt: tokio::runtime::Handle, work: F)
    where
        F: FnOnce(&Slot) -> Event,
    {
        self.run_job_with(slot, rt, false, work);
    }

    /// [`Service::run_job`]; `reread_info` makes the read after the job include the device
    /// information (a firmware update changes the version).
    fn run_job_with<F>(
        self: Arc<Self>,
        slot: Arc<Slot>,
        rt: tokio::runtime::Handle,
        reread_info: bool,
        work: F,
    ) where
        F: FnOnce(&Slot) -> Event,
    {
        let guard = loop {
            if let Some(g) = slot.lock_device(Duration::from_secs(60)) {
                break g;
            }
        };
        let finished = work(&slot);
        drop(guard);
        lk(&slot.inner).job = None;
        self.emit(finished);
        self.announce_activity(&slot);
        // Pick up the printer's state after the job.
        rt.spawn(async move {
            let _ = self.refresh_slot_with(slot, reread_info).await;
        });
    }

    // ---- scanning --------------------------------------------------------------------------

    /// Scanner capabilities and status as JSON: `{"capabilities": ..., "status": ...}`. Read-only.
    pub async fn get_scanner_capabilities(self: &Arc<Self>, host: &str) -> Result<String> {
        self.read_json(host, |p| {
            let escl = p.escl();
            let caps = escl.capabilities()?;
            let status = escl.status().ok();
            Ok(serde_json::json!({ "capabilities": caps, "status": status }))
        })
        .await
    }

    /// Scan one page on a worker; returns the session id. Each page arrives as
    /// [`Event::ScanPage`] (a JPEG file in a private temporary directory), the end as
    /// [`Event::ScanFinished`]. WRITES (moves the scanner): `settings_json` needs
    /// `"confirm": true`.
    pub async fn start_scan(self: &Arc<Self>, host: &str, settings_json: &str) -> Result<String> {
        self.ensure_writable("StartScan")?;
        let spec = ScanSpec::from_json(settings_json).map_err(from_core)?;
        if !spec.confirm {
            return Err(ServiceError::NotConfirmed("StartScan"));
        }
        let req = spec.to_request().map_err(from_core)?;
        let slot = self.resolve(host)?;
        let label = messages::scan_label(req.size.label(), req.resolution);
        let (id, _cancel) = self.begin_job(&slot, "scan", JobKind::Scan, label)?;
        info!("printer {}: {id} started", slot.log_id);
        self.announce_activity(&slot);
        let (svc, rt, session) = (self.clone(), tokio::runtime::Handle::current(), id.clone());
        let worker_svc = svc.clone();
        tokio::task::spawn_blocking(move || {
            svc.run_job(slot, rt, move |slot| {
                let mut dir: Option<PathBuf> = None;
                let result = run_scan_streaming(
                    &slot.printer,
                    &req,
                    Confirm::explicit(),
                    &mut |p| {
                        if let ScanProgress::Started = p {
                            debug!("scan {session}: started");
                        }
                    },
                    &mut |index, page| {
                        let d = match &dir {
                            Some(d) => d.clone(),
                            None => {
                                let d = scan_dir(&session).map_err(|e| {
                                    washi_core::Error::io("cannot create the scan directory", e)
                                })?;
                                dir = Some(d.clone());
                                d
                            }
                        };
                        let path = d.join(format!("page-{}.jpg", index + 1));
                        std::fs::write(&path, &page.jpeg).map_err(|e| {
                            washi_core::Error::io(format!("cannot write {}", path.display()), e)
                        })?;
                        worker_svc.emit(Event::ScanPage {
                            session: session.clone(),
                            index: index as u32,
                            path: path.to_string_lossy().into_owned(),
                        });
                        Ok(())
                    },
                );
                let (ok, message) = match &result {
                    Ok(n) => (true, messages::pages_scanned(worker_svc.lang(), *n)),
                    Err(e) => (false, messages::core_error(e).get(worker_svc.lang()).to_string()),
                };
                info!(
                    "printer {}: {session} {}",
                    slot.log_id,
                    if ok { "finished" } else { "failed" }
                );
                Event::ScanFinished { session: session.clone(), ok, message }
            });
        });
        Ok(id)
    }

    // ---- printing --------------------------------------------------------------------------

    /// Print a file (photo, PDF or Office document) on a worker; returns the job id. Progress
    /// arrives as [`Event::PrintProgress`], the end as [`Event::PrintFinished`]. WRITES
    /// (prints): `options_json` needs `"confirm": true`.
    pub async fn print_file(
        self: &Arc<Self>,
        host: &str,
        path: &str,
        options_json: &str,
    ) -> Result<String> {
        self.print_paths(host, &[path.to_string()], options_json, "PrintFile").await
    }

    /// Like [`Service::print_file`] for several files (photos for the n-in-1 layouts). The files
    /// are planned together, so `layout` puts consecutive photos on one sheet.
    pub async fn print_files(
        self: &Arc<Self>,
        host: &str,
        paths: &[String],
        options_json: &str,
    ) -> Result<String> {
        self.print_paths(host, paths, options_json, "PrintFiles").await
    }

    async fn print_paths(
        self: &Arc<Self>,
        host: &str,
        paths: &[String],
        options_json: &str,
        what: &'static str,
    ) -> Result<String> {
        self.ensure_writable(what)?;
        let spec = PrintSpec::from_json(options_json).map_err(from_core)?;
        if !spec.confirm {
            return Err(ServiceError::NotConfirmed(what));
        }
        let opts = spec.to_options().map_err(from_core)?;
        if paths.is_empty() {
            return Err(ServiceError::invalid("no files to print"));
        }
        let files: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        if let Some(bad) = files.iter().find(|f| !f.is_absolute()) {
            return Err(ServiceError::invalid(format!(
                "{:?} is not an absolute path",
                bad.display().to_string()
            )));
        }
        let slot = self.resolve(host)?;
        // Plan on a blocking thread (reads the files, may run pdfinfo) so that bad input is
        // reported to the caller instead of ending in a signal.
        let plan = {
            let slot = slot.clone();
            tokio::task::spawn_blocking(move || {
                print::plan_print(&slot.printer, &files, &opts, &Tools::detect())
            })
            .await
            .map_err(ServiceError::internal)?
            .map_err(from_core)?
        };
        for w in &plan.warnings {
            warn!("print plan: {w}");
        }
        let label = plan.jobs.first().map_or_else(
            || Text::new("document", "文書"),
            |j| Text::new(j.label.clone(), j.label.clone()),
        );
        let (id, cancel) = self.begin_job(&slot, "print", JobKind::Print, label)?;
        info!("printer {}: {id} started", slot.log_id);
        self.announce_activity(&slot);
        let (svc, rt, job) = (self.clone(), tokio::runtime::Handle::current(), id.clone());
        let worker_svc = svc.clone();
        tokio::task::spawn_blocking(move || {
            svc.run_job(slot, rt, move |slot| {
                let run = RunOptions { cancel: Some(cancel), ..RunOptions::default() };
                let result = print::run_print(
                    &slot.printer,
                    &plan,
                    &run,
                    &Tools::detect(),
                    Confirm::explicit(),
                    &mut |ev| {
                        worker_svc.print_event(&job, &ev);
                    },
                );
                let lang = worker_svc.lang();
                finish_print(
                    &slot.log_id,
                    &job,
                    lang,
                    result.map(|r| {
                        let items: Vec<(String, &str)> =
                            r.jobs.iter().map(|j| (j.label.clone(), j.state.as_str())).collect();
                        messages::print_result(lang, &items)
                    }),
                )
            });
        });
        Ok(id)
    }

    /// Print a disc label on a worker (same signals as [`Service::print_file`]). WRITES:
    /// `label_json` needs `"confirm": true`.
    pub async fn print_disc(self: &Arc<Self>, host: &str, label_json: &str) -> Result<String> {
        self.ensure_writable("PrintDisc")?;
        let spec = DiscSpec::from_json(label_json).map_err(from_core)?;
        if !spec.confirm {
            return Err(ServiceError::NotConfirmed("PrintDisc"));
        }
        let (label, quality) = spec.to_label().map_err(from_core)?;
        if let Some(b) = &label.background {
            disc::check_image(&b.path).map_err(from_core)?;
        }
        let slot = self.resolve(host)?;
        let (id, cancel) =
            self.begin_job(&slot, "print", JobKind::DiscLabel, messages::disc_label_name())?;
        info!("printer {}: {id} started (disc label)", slot.log_id);
        self.announce_activity(&slot);
        let (svc, rt, job) = (self.clone(), tokio::runtime::Handle::current(), id.clone());
        let worker_svc = svc.clone();
        tokio::task::spawn_blocking(move || {
            svc.run_job(slot, rt, move |slot| {
                let run = RunOptions { cancel: Some(cancel), ..RunOptions::default() };
                let result = disc::run_disc(
                    &slot.printer,
                    &label,
                    quality,
                    &run,
                    Confirm::explicit(),
                    &mut |ev| {
                        worker_svc.print_event(&job, &ev);
                    },
                );
                let lang = worker_svc.lang();
                finish_print(
                    &slot.log_id,
                    &job,
                    lang,
                    result.map(|r| messages::disc_result(lang, r.state.as_str())),
                )
            });
        });
        Ok(id)
    }

    fn print_event(&self, job: &str, ev: &PrintProgress) {
        let (stage, message) = print_event_text(ev, self.lang());
        self.emit(Event::PrintProgress { job: job.to_string(), stage: stage.to_string(), message });
    }

    /// Ask a running scan or print job to stop at its next opportunity (between sheets and
    /// while the printer works; a scan already in progress finishes its page). Returns false
    /// when no such job runs. Not a write: it only ends activity.
    pub fn cancel_job(&self, job: &str) -> bool {
        let reg = lk(&self.registry);
        for slot in &reg.slots {
            if let Some(j) = &lk(&slot.inner).job {
                if j.id == job {
                    j.cancel.store(true, Ordering::SeqCst);
                    return true;
                }
            }
        }
        false
    }
}

fn finish_print(
    log_id: &str,
    job: &str,
    lang: Language,
    result: washi_core::Result<String>,
) -> Event {
    let (ok, message) = match result {
        Ok(m) => (true, m),
        Err(e) => (false, messages::core_error(&e).get(lang).to_string()),
    };
    info!("printer {log_id}: {job} {}", if ok { "finished" } else { "failed" });
    Event::PrintFinished { job: job.to_string(), ok, message }
}

/// `(stage, message)` of a print progress event, for the D-Bus signal.
pub fn print_event_text(ev: &PrintProgress, lang: Language) -> (&'static str, String) {
    match ev {
        PrintProgress::Preflight => ("preflight", messages::print_preflight(lang)),
        PrintProgress::Preparing { job, jobs, side, sides } if side >= sides => {
            ("preparing", messages::print_ready(lang, job + 1, *jobs))
        }
        PrintProgress::Preparing { job, jobs, side, sides } => {
            ("preparing", messages::print_preparing(lang, job + 1, *jobs, side + 1, *sides))
        }
        PrintProgress::Validated { ignored } => {
            ("validated", messages::print_validated(lang, ignored))
        }
        PrintProgress::Sending { bytes, .. } => ("sending", messages::print_sending(lang, *bytes)),
        PrintProgress::Submitted { id, .. } => {
            ("submitted", messages::print_submitted(lang, &id.to_string()))
        }
        PrintProgress::State { id, state, reasons, impressions } => {
            let r: Vec<&str> =
                reasons.iter().map(String::as_str).filter(|r| *r != "none").collect();
            (
                "state",
                messages::print_state(
                    lang,
                    &id.to_string(),
                    state.as_str(),
                    impressions.map(|n| n.max(0) as u32),
                    &r,
                ),
            )
        }
        PrintProgress::Finished { id, state } => {
            ("finished", messages::print_finished(lang, &id.to_string(), state.as_str()))
        }
    }
}

/// Where the daemon puts files it hands to the app: the runtime directory, or the temp directory
/// when the session has none. Inside a Flatpak sandbox the daemon and the app are different
/// instances (D-Bus activation starts the daemon in a sandbox of its own) with separate runtime
/// directories; only `$XDG_RUNTIME_DIR/app/$FLATPAK_ID` is shared between the instances of one
/// application, so that is used when it exists.
fn runtime_base(runtime: Option<PathBuf>, flatpak_id: Option<std::ffi::OsString>) -> PathBuf {
    if let (Some(dir), Some(id)) = (&runtime, flatpak_id) {
        let shared = dir.join("app").join(id);
        if shared.is_dir() {
            return shared;
        }
    }
    runtime.unwrap_or_else(std::env::temp_dir)
}

/// A private directory for the pages of one scan: `<runtime dir or temp>/washi/scan-<id>-<pid>`.
/// Older directories of this daemon are removed.
fn scan_dir(session: &str) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let base = runtime_base(dirs::runtime_dir(), std::env::var_os("FLATPAK_ID")).join("washi");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&base)?;
    clean_old_scans(&base);
    let dir = base.join(format!("scan-{session}-{}", std::process::id()));
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
    Ok(dir)
}

fn clean_old_scans(base: &Path) {
    let Ok(rd) = std::fs::read_dir(base) else { return };
    let day = Duration::from_secs(24 * 3600);
    for e in rd.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().starts_with("scan-") {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > day);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Convenience for tests and `Version()`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn files_for_the_app_go_where_both_flatpak_instances_see_them() {
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().to_path_buf();
        // Not in a sandbox: the runtime directory itself.
        assert_eq!(runtime_base(Some(run.clone()), None), run);
        // In a sandbox that has the shared directory: that one.
        std::fs::create_dir_all(run.join("app/org.example.App")).unwrap();
        assert_eq!(
            runtime_base(Some(run.clone()), Some("org.example.App".into())),
            run.join("app/org.example.App")
        );
        // The id is set but the directory is not there: do not invent one.
        assert_eq!(runtime_base(Some(run.clone()), Some("other.App".into())), run);
        // No runtime directory at all: the temp directory.
        assert_eq!(runtime_base(None, None), std::env::temp_dir());
    }

    #[test]
    fn progress_stages() {
        let op = MaintOp::NozzleCheck;
        let pe = |p: &Progress| progress_event(p, &op, Language::En);
        assert_eq!(pe(&Progress::JobStarted).0, "job-started");
        let (stage, msg) = pe(&Progress::Waiting {
            elapsed: Duration::from_secs(12),
            status: "printing".into(),
            support_code: "1300".into(),
            job: None,
        });
        assert_eq!(stage, "waiting");
        assert!(msg.contains("12s") && msg.contains("printing") && msg.contains("1300"), "{msg}");
        let (_, working) = pe(&Progress::Waiting {
            elapsed: Duration::from_secs(3),
            status: "suspended".into(),
            support_code: String::new(),
            job: None,
        });
        assert_eq!(working, "3s: the printer is working", "suspended is not an error");
        let fields = |pairs: &[(&str, &str)]| washi_core::ivec::JobInfo {
            fields: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        let with_job = |pairs: &[(&str, &str)]| {
            pe(&Progress::Waiting {
                elapsed: Duration::from_secs(8),
                status: "processing".into(),
                support_code: String::new(),
                job: Some(fields(pairs)),
            })
            .1
        };
        assert_eq!(
            with_job(&[
                ("jobID", "00000002"),
                ("maintenance_operation", "TestPrint"),
                ("type", "nozzle_check"),
                ("jobprogress", "processing")
            ]),
            "Printing the nozzle check pattern… (8s)"
        );
        assert_eq!(
            with_job(&[("maintenance_operation", "Cleaning"), ("type", "deep")]),
            "Deep cleaning in progress… (8s)"
        );
        assert_eq!(
            with_job(&[("maintenance_operation", "TestPrint"), ("type", "auto_registration")]),
            "Alignment in progress… (8s)"
        );
        assert_eq!(with_job(&[("maintenance_operation", "Mystery")]), "Working… (8s)");
        assert!(with_job(&[("jobID", "1")]).contains("8s: the printer is processing"));
        assert_eq!(pe(&Progress::JobEnded).0, "job-ended");
        assert_eq!(pe(&Progress::Ending).0, "closing");
        let (s, m) = pe(&Progress::CommandAccepted { operation: "nozzle check".into() });
        assert_eq!(s, "accepted");
        assert!(m.contains("nozzle check"));
        // The same event in Japanese.
        let (_, ja) = progress_event(
            &Progress::Waiting {
                elapsed: Duration::from_secs(8),
                status: "processing".into(),
                support_code: String::new(),
                job: Some(fields(&[("maintenance_operation", "Cleaning"), ("type", "deep")])),
            },
            &op,
            Language::Ja,
        );
        assert_eq!(ja, "強力クリーニング中…（8秒）");
    }

    #[test]
    fn service_errors_speak_both_languages() {
        let all = [
            ServiceError::ReadOnly("RunMaintenance"),
            ServiceError::UnknownPrinter("p.local".into()),
            ServiceError::NoPrinter,
            ServiceError::NotConfirmed("Wake"),
            ServiceError::invalid("bad"),
            ServiceError::Busy(messages::busy_printer()),
            ServiceError::internal("oops"),
        ];
        for e in all {
            // The English message is what Display says.
            assert_eq!(e.message(Language::En), e.to_string());
            let ja = e.message(Language::Ja);
            assert_ne!(ja, e.to_string());
            assert!(ja.chars().any(|c| c > '\u{3000}'), "{ja}");
        }
    }

    #[test]
    fn core_errors_map_to_service_errors() {
        use washi_core::Error;
        assert!(matches!(from_core(Error::Invalid("x".into())), ServiceError::InvalidArgs(_)));
        let ng = Error::Ivec(IvecError::Ng {
            operation: "GetStatus".into(),
            detail: "Suspended".into(),
        });
        let m = from_core(ng);
        assert!(matches!(&m, ServiceError::Printer(t) if t.en().contains("asleep")), "{m}");
        assert!(m.message(Language::Ja).contains("スリープ"), "{}", m.message(Language::Ja));
    }
}
