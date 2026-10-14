// SPDX-License-Identifier: GPL-2.0-or-later
//! A scriptable fake Canon printer for tests: CHMP/IVEC on one local TCP port and IPP on another.
//!
//! It serves the scrubbed fixtures of `washi-ivec`, can "fall asleep" (IVEC print/maintenance
//! services answer `NG Suspended`; device status and IPP keep working), simulates a maintenance
//! job (the print status says `printing` for a few polls) and a few `SetConfiguration` fields,
//! and records every request so tests can assert exactly what was (not) sent.
//!
//! The CHMP port also speaks eSCL like the real printer's web server (`/eSCL/ScannerCapabilities`,
//! `ScannerStatus`, `ScanJobs`: POST, `NextDocument`, DELETE) and delivers a synthetic JPEG per
//! page. The IPP port accepts `Validate-Job`, `Print-Job` (the whole document is kept for the
//! test to inspect), `Get-Job-Attributes`, `Get-Jobs`, `Cancel-Job` and `Identify-Printer`, and
//! serves a `media-col-database` shaped like the XK140's (including the disc tray) when asked
//! for it on its own.
//!
//! Binds to `127.0.0.1` only. It never contacts a real printer.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use washi_net::ipp::{
    IppAttribute, IppGroup, IppMessage, IppValue, OutOfBand, op as ipp_op, tag as ipp_tag,
};

const PRINT: &str = include_str!("../../washi-ivec/tests/fixtures/GetStatus-print.xml");
const DEVICE_STATUS: &str = include_str!("../../washi-ivec/tests/fixtures/GetStatus-device.xml");
const MAINT_STATUS: &str =
    include_str!("../../washi-ivec/tests/fixtures/GetStatus-maintenance.xml");
const DEVICE_CAP: &str = include_str!("../../washi-ivec/tests/fixtures/GetCapability-device.xml");
const MAINT_CAP: &str =
    include_str!("../../washi-ivec/tests/fixtures/GetCapability-maintenance.xml");
const DEVICE_CFG: &str =
    include_str!("../../washi-ivec/tests/fixtures/GetConfiguration-device.xml");
const ESCL_CAPS: &str = include_str!("../../washi-net/tests/fixtures/escl-capabilities-xk140.xml");
const ESCL_STATUS_IDLE: &str = include_str!("../../washi-net/tests/fixtures/escl-status-idle.xml");
const MODEL_JS: &str = include_str!("../../washi-net/tests/fixtures/model-sample.js");

/// Ink colours in the order the XK140 reports them (IVEC `order` and IPP marker order).
pub const INK_ORDER: [&str; 5] = ["M", "BK", "Y", "PGBK", "C"];

#[derive(Debug, Clone)]
struct State {
    asleep: bool,
    /// The maintenance capability lists `RollerCleaning` (the real XK140 does not).
    roller_cleaning: bool,
    /// Extra `Cleaning` types the maintenance capability lists (`roller`, `platen`, `choke`...).
    cleaning_types: Vec<String>,
    /// What `GET /JS_MDL/model.js` serves (`None` = 404).
    model_js: Option<String>,
    /// The printer's own words and pictures (`printer-strings-uri`, `printer-icons`).
    words: Words,
    firmware: FirmwareState,
    levels: [u8; 5],
    support_code: String,
    config_xml: String,
    /// `GetStatus print` still answers `printing` this many more times.
    busy_polls: usize,
    /// Number of `GetStatus print` polls that stay busy after a maintenance command.
    job_polls: usize,
    chmp_down: bool,
    ipp_down: bool,
    scan: ScanState,
    prints: PrintState,
}

/// What the printer serves for `printer-strings-uri` and `printer-icons`. Nothing by default: a
/// printer that has none of them is the usual case in the tests.
#[derive(Debug, Clone, Default)]
struct Words {
    /// `.strings` files by language (`ja` -> `/strings/ja.txt`).
    strings: BTreeMap<String, Vec<u8>>,
    /// The address of the file does not follow the language of the request: the English one (or
    /// the first language) is named whatever is asked for.
    one_address: bool,
    /// Pictures by file name (`/icon/<name>`), in the order they are listed.
    icons: Vec<(String, Vec<u8>)>,
}

/// A print or validate request the fake received.
#[derive(Debug, Clone)]
pub struct ReceivedJob {
    /// `Print-Job` or `Validate-Job`.
    pub operation: String,
    /// The decoded IPP request.
    pub request: IppMessage,
    /// `document-format` of the request.
    pub format: String,
    /// The document that followed the IPP message (empty for `Validate-Job`).
    pub data: Vec<u8>,
    /// Job id given to a `Print-Job` (0 for `Validate-Job`).
    pub job_id: i32,
}

/// The firmware the fake reports and the update it can "install".
#[derive(Debug, Clone)]
struct FirmwareState {
    version: String,
    /// `networkupdate_available` (`None` = `unavailable`).
    update_available: Option<String>,
    /// The version after an update, and how long the printer is gone meanwhile.
    next_version: String,
    restart_ms: u64,
    /// The printer is "restarting" from `down_from` (shortly after it accepted the command, so
    /// the reply gets out) until `down_until`; the new version is applied when that ends.
    down_from: Option<std::time::Instant>,
    down_until: Option<std::time::Instant>,
}

#[derive(Debug, Clone)]
struct ScanState {
    /// Pages each scan job delivers before answering 404.
    pages: usize,
    /// Answer `503` to `POST ScanJobs` this many more times.
    busy_posts: usize,
    /// Answer this HTTP status to `POST ScanJobs` instead of accepting.
    reject_status: Option<u16>,
    /// A JPEG to deliver instead of the generated test pattern.
    image: Option<Vec<u8>>,
    /// Longest edge of the generated test pattern (pixels).
    max_edge: u32,
    /// `ScannerStatus/State` (default `Idle`).
    state: String,
    next_id: u32,
    /// `(job id, pages already delivered)`.
    jobs: Vec<(u32, usize)>,
    requests: Vec<String>,
}

#[derive(Debug, Clone)]
struct PrintState {
    received: Vec<ReceivedJob>,
    next_id: i32,
    /// `(job id, polls so far, canceled)`.
    jobs: Vec<(i32, usize, bool)>,
    /// `Get-Job-Attributes` polls until a job is `completed`.
    polls_to_complete: usize,
    /// Answer `Print-Job` / `Validate-Job` with this IPP status (and `media-col` unsupported).
    reject: Option<u16>,
    /// End the job `aborted` instead of `completed`.
    abort_jobs: bool,
    identify: Vec<String>,
    accepting: bool,
    /// Answer `client-error-not-found` for a job once it would be `completed` (printers
    /// forget finished jobs).
    forget_finished: bool,
}

struct Shared {
    state: Mutex<State>,
    log: Mutex<Vec<String>>,
    chmp_connections: AtomicUsize,
    ipp_connections: AtomicUsize,
    /// IVEC commands being handled right now / the most at once / artificial latency (ms).
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    latency_ms: AtomicUsize,
}

/// A running fake printer. The listener threads end with the process.
pub struct FakePrinter {
    pub chmp: SocketAddr,
    pub ipp: SocketAddr,
    shared: Arc<Shared>,
}

impl FakePrinter {
    /// Start on random free ports.
    pub fn start() -> FakePrinter {
        Self::start_on(0, 0)
    }

    /// Start on fixed ports (0 = random).
    pub fn start_on(chmp_port: u16, ipp_port: u16) -> FakePrinter {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                asleep: false,
                roller_cleaning: false,
                cleaning_types: Vec::new(),
                model_js: Some(MODEL_JS.to_string()),
                words: Words::default(),
                firmware: FirmwareState {
                    version: "1.060".to_string(),
                    update_available: None,
                    next_version: "1.061".to_string(),
                    restart_ms: 1500,
                    down_from: None,
                    down_until: None,
                },
                levels: [60, 80, 70, 60, 60],
                support_code: String::new(),
                config_xml: DEVICE_CFG.to_string(),
                busy_polls: 0,
                job_polls: 2,
                chmp_down: false,
                ipp_down: false,
                scan: ScanState {
                    pages: 1,
                    busy_posts: 0,
                    reject_status: None,
                    image: None,
                    max_edge: 1200,
                    state: "Idle".to_string(),
                    next_id: 1,
                    jobs: Vec::new(),
                    requests: Vec::new(),
                },
                prints: PrintState {
                    received: Vec::new(),
                    next_id: 101,
                    jobs: Vec::new(),
                    polls_to_complete: 2,
                    reject: None,
                    abort_jobs: false,
                    identify: Vec::new(),
                    accepting: true,
                    forget_finished: false,
                },
            }),
            log: Mutex::new(Vec::new()),
            chmp_connections: AtomicUsize::new(0),
            ipp_connections: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
            latency_ms: AtomicUsize::new(0),
        });
        let chmp_l = TcpListener::bind(("127.0.0.1", chmp_port)).expect("bind CHMP port");
        let ipp_l = TcpListener::bind(("127.0.0.1", ipp_port)).expect("bind IPP port");
        let fake = FakePrinter {
            chmp: chmp_l.local_addr().unwrap(),
            ipp: ipp_l.local_addr().unwrap(),
            shared: shared.clone(),
        };
        let s1 = shared.clone();
        thread::spawn(move || {
            for stream in chmp_l.incoming().flatten() {
                s1.chmp_connections.fetch_add(1, Ordering::SeqCst);
                let s = s1.clone();
                thread::spawn(move || serve_chmp(stream, s));
            }
        });
        thread::spawn(move || {
            for stream in ipp_l.incoming().flatten() {
                shared.ipp_connections.fetch_add(1, Ordering::SeqCst);
                let s = shared.clone();
                thread::spawn(move || serve_ipp(stream, s));
            }
        });
        fake
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap()
    }

    /// `host:port` to pass as `--host` (CHMP).
    pub fn chmp_target(&self) -> String {
        format!("127.0.0.1:{}", self.chmp.port())
    }

    /// List `RollerCleaning` in the maintenance capability.
    pub fn set_roller_cleaning(&self, on: bool) {
        self.state().roller_cleaning = on;
    }

    /// List extra `Cleaning` types in the maintenance capability, e.g. `["roller", "platen",
    /// "choke"]`.
    pub fn set_cleaning_types(&self, types: &[&str]) {
        self.state().cleaning_types = types.iter().map(|t| (*t).to_string()).collect();
    }

    /// What `GET /JS_MDL/model.js` serves (`None` = `404`).
    pub fn set_model_js(&self, script: Option<&str>) {
        self.state().model_js = script.map(str::to_string);
    }

    /// The `.strings` file for a language (`None` removes the language). The IPP attributes
    /// `printer-strings-languages-supported` and `printer-strings-uri` follow.
    pub fn set_strings(&self, language: &str, file: Option<&[u8]>) {
        let mut st = self.state();
        match file {
            Some(f) => st.words.strings.insert(language.to_ascii_lowercase(), f.to_vec()),
            None => st.words.strings.remove(&language.to_ascii_lowercase()),
        };
    }

    /// Name the same strings address (the first language's) whatever language is asked for, like
    /// a printer whose `printer-strings-uri` does not change with the request.
    pub fn set_strings_one_address(&self, on: bool) {
        self.state().words.one_address = on;
    }

    /// The pictures (`printer-icons`): file name and PNG bytes, served at `/icon/<name>`.
    pub fn set_icons(&self, icons: Vec<(String, Vec<u8>)>) {
        self.state().words.icons = icons;
    }

    /// Offer a firmware update: `GetStatus device` reports `networkupdate_available`, and an
    /// `UpdateFRom` command makes the printer go away for `restart_ms` and come back reporting
    /// `next_version` with no update left.
    pub fn offer_firmware_update(&self, next_version: &str, restart_ms: u64) {
        let mut st = self.state();
        st.firmware.update_available = Some("available".to_string());
        st.firmware.next_version = next_version.to_string();
        st.firmware.restart_ms = restart_ms;
    }

    /// The firmware version the fake reports now.
    pub fn firmware_version(&self) -> String {
        let mut st = self.state();
        apply_restart(&mut st);
        st.firmware.version.clone()
    }

    /// Auto power-off: IVEC print/maintenance say `NG Suspended`, device status and IPP work.
    pub fn set_asleep(&self, asleep: bool) {
        self.state().asleep = asleep;
    }

    /// Percent remaining of one ink (`M`, `BK`, `Y`, `PGBK`, `C`).
    pub fn set_ink(&self, color: &str, level: u8) {
        let i = INK_ORDER.iter().position(|c| *c == color).expect("unknown ink colour");
        self.state().levels[i] = level;
    }

    /// Error code reported by `GetStatus print` (empty = none).
    pub fn set_support_code(&self, code: &str) {
        self.state().support_code = code.to_string();
    }

    /// Drop every CHMP connection without answering (printer unreachable on port 80).
    pub fn set_chmp_down(&self, down: bool) {
        self.state().chmp_down = down;
    }

    /// Drop every IPP connection without answering.
    pub fn set_ipp_down(&self, down: bool) {
        self.state().ipp_down = down;
    }

    /// How many `GetStatus print` polls report `printing` after a maintenance command.
    pub fn set_job_polls(&self, n: usize) {
        self.state().job_polls = n;
    }

    /// Every request seen so far as `Operation/service` (IVEC) or `IPP/<operation>`.
    pub fn log(&self) -> Vec<String> {
        self.shared.log.lock().unwrap().clone()
    }

    pub fn clear_log(&self) {
        self.shared.log.lock().unwrap().clear();
    }

    /// The logged requests that change the printer or move its hardware: IVEC operations other
    /// than `Get*`, IPP operations other than the three reads, and the eSCL requests that start,
    /// advance or cancel a scan.
    pub fn writes(&self) -> Vec<String> {
        self.log()
            .into_iter()
            .filter(|l| {
                if let Some(op) = l.strip_prefix("IPP/") {
                    !matches!(op, "Get-Printer-Attributes" | "Get-Job-Attributes" | "Get-Jobs")
                } else if let Some(op) = l.strip_prefix("eSCL/") {
                    !matches!(op, "ScannerCapabilities" | "ScannerStatus")
                } else if l.starts_with("Web/") {
                    false
                } else {
                    !l.starts_with("Get")
                }
            })
            .collect()
    }

    // ---- scanning ------------------------------------------------------------------------

    /// How many pages each scan job delivers (default 1, like a platen).
    pub fn set_scan_pages(&self, n: usize) {
        self.state().scan.pages = n;
    }

    /// Deliver this JPEG for every page instead of the generated test pattern.
    pub fn set_scan_image(&self, jpeg: Vec<u8>) {
        self.state().scan.image = Some(jpeg);
    }

    /// Longest edge of the generated test pattern (default 1200 px).
    pub fn set_scan_max_edge(&self, px: u32) {
        self.state().scan.max_edge = px.max(16);
    }

    /// Answer `503` to the next `n` `POST ScanJobs`.
    pub fn set_scanner_busy_for(&self, n: usize) {
        self.state().scan.busy_posts = n;
    }

    /// The state `ScannerStatus` reports (`Idle`, `Processing`, `Stopped`, ...).
    pub fn set_scanner_state(&self, state: &str) {
        self.state().scan.state = state.to_string();
    }

    /// Answer `POST ScanJobs` with this HTTP status (e.g. 409) instead of accepting.
    pub fn set_scan_reject(&self, status: Option<u16>) {
        self.state().scan.reject_status = status;
    }

    /// The `ScanSettings` documents received so far.
    pub fn scan_requests(&self) -> Vec<String> {
        self.state().scan.requests.clone()
    }

    // ---- printing ------------------------------------------------------------------------

    /// `Print-Job` and `Validate-Job` requests received so far.
    pub fn received_jobs(&self) -> Vec<ReceivedJob> {
        self.state().prints.received.clone()
    }

    /// Only the `Print-Job`s.
    pub fn printed_jobs(&self) -> Vec<ReceivedJob> {
        self.received_jobs().into_iter().filter(|j| j.operation == "Print-Job").collect()
    }

    /// `Get-Job-Attributes` polls before a job reports `completed` (default 2).
    pub fn set_job_completion_polls(&self, n: usize) {
        self.state().prints.polls_to_complete = n;
    }

    /// Answer `Print-Job` and `Validate-Job` with this IPP status code.
    pub fn set_print_reject(&self, status: Option<u16>) {
        self.state().prints.reject = status;
    }

    /// Finish jobs as `aborted` instead of `completed`.
    pub fn set_abort_jobs(&self, abort: bool) {
        self.state().prints.abort_jobs = abort;
    }

    /// Forget jobs as soon as they are done: `Get-Job-Attributes` then says "not found".
    pub fn set_forget_finished_jobs(&self, forget: bool) {
        self.state().prints.forget_finished = forget;
    }

    /// `printer-is-accepting-jobs`.
    pub fn set_accepting_jobs(&self, accepting: bool) {
        self.state().prints.accepting = accepting;
    }

    /// The `identify-actions` received so far.
    pub fn identify_requests(&self) -> Vec<String> {
        self.state().prints.identify.clone()
    }

    pub fn chmp_connections(&self) -> usize {
        self.shared.chmp_connections.load(Ordering::SeqCst)
    }

    pub fn ipp_connections(&self) -> usize {
        self.shared.ipp_connections.load(Ordering::SeqCst)
    }

    /// Delay every IVEC command by this long (to make overlapping access visible).
    pub fn set_latency_ms(&self, ms: usize) {
        self.shared.latency_ms.store(ms, Ordering::SeqCst);
    }

    /// The most IVEC commands that were ever being handled at the same time. A client that
    /// serialises its device access keeps this at 1.
    pub fn max_concurrent_commands(&self) -> usize {
        self.shared.max_in_flight.load(Ordering::SeqCst)
    }

    /// The configuration XML the fake currently serves (after `SetConfiguration`).
    pub fn config_xml(&self) -> String {
        self.state().config_xml.clone()
    }

    /// Run a line-oriented control command (used by the `washi-fake-printer` binary):
    /// `sleep`, `wake`, `roller on|off`, `cleaning-types roller platen ...`,
    /// `firmware-update <version> [<restart ms>]`, `model-js on|off`, `strings-address one|each`, `ink <C> <pct>`, `code <n>`, `chmp-down`, `chmp-up`, `ipp-down`,
    /// `ipp-up`, `scan-pages <n>`, `scan-busy <n>`, `scan-reject [<http status>]`,
    /// `print-reject [<ipp status>]`, `job-polls <n>`, `abort-jobs on|off`, `accepting on|off`,
    /// `log`.
    pub fn command(&self, line: &str) -> String {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["sleep"] => self.set_asleep(true),
            ["wake"] => self.set_asleep(false),
            ["roller", v @ ("on" | "off")] => self.set_roller_cleaning(*v == "on"),
            ["cleaning-types", types @ ..] => self.set_cleaning_types(types),
            ["firmware-update", version] => self.offer_firmware_update(version, 1500),
            ["firmware-update", version, ms] if ms.parse::<u64>().is_ok() => {
                self.offer_firmware_update(version, ms.parse().unwrap())
            }
            ["model-js", "off"] => self.set_model_js(None),
            ["model-js", "on"] => self.set_model_js(Some(MODEL_JS)),
            ["strings-address", v @ ("one" | "each")] => self.set_strings_one_address(*v == "one"),
            ["ink", color, pct] if INK_ORDER.contains(color) && pct.parse::<u8>().is_ok() => {
                self.set_ink(color, pct.parse().unwrap());
            }
            ["code"] => self.set_support_code(""),
            ["code", n] => self.set_support_code(n),
            ["chmp-down"] => self.set_chmp_down(true),
            ["chmp-up"] => self.set_chmp_down(false),
            ["ipp-down"] => self.set_ipp_down(true),
            ["ipp-up"] => self.set_ipp_down(false),
            ["scan-pages", n] if n.parse::<usize>().is_ok() => {
                self.set_scan_pages(n.parse().unwrap())
            }
            ["scan-busy", n] if n.parse::<usize>().is_ok() => {
                self.set_scanner_busy_for(n.parse().unwrap())
            }
            ["scan-reject"] => self.set_scan_reject(None),
            ["scan-reject", code] if code.parse::<u16>().is_ok() => {
                self.set_scan_reject(code.parse().ok())
            }
            ["print-reject"] => self.set_print_reject(None),
            ["print-reject", code] if code.parse::<u16>().is_ok() => {
                self.set_print_reject(code.parse().ok())
            }
            ["job-polls", n] if n.parse::<usize>().is_ok() => {
                self.set_job_completion_polls(n.parse().unwrap())
            }
            ["abort-jobs", v @ ("on" | "off")] => self.set_abort_jobs(*v == "on"),
            ["accepting", v @ ("on" | "off")] => self.set_accepting_jobs(*v == "on"),
            ["log"] => return self.log().join("\n"),
            _ => return format!("unknown command {line:?}"),
        }
        "ok".to_string()
    }
}

// ---------------------------------------------------------------------------------------------
// IVEC
// ---------------------------------------------------------------------------------------------

fn reply(op: &str, service: &str, ok: bool, detail: &str) -> String {
    let (resp, det) = if ok { ("OK", String::new()) } else { ("NG", detail.to_string()) };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><cmd xmlns:ivec=\"http://www.canon.com/ns/cmd/2008/07/common/\" \
         xmlns:vcn=\"http://www.canon.com/ns/cmd/2008/07/canon/\"><ivec:contents>\
         <ivec:operation>{op}Response</ivec:operation><ivec:param_set servicetype=\"{service}\">\
         <ivec:response>{resp}</ivec:response><ivec:response_detail>{det}</ivec:response_detail>\
         </ivec:param_set></ivec:contents></cmd>"
    )
}

/// Replace the first `<tag>..</tag>` after `anchor` with `new` (or leave `xml` unchanged).
fn replace_after(xml: &str, anchor: &str, open: &str, close: &str, new: &str) -> String {
    let Some(a) = xml.find(anchor) else { return xml.to_string() };
    let Some(o) = xml[a..].find(open).map(|i| a + i + open.len()) else { return xml.to_string() };
    let Some(c) = xml[o..].find(close).map(|i| o + i) else { return xml.to_string() };
    format!("{}{}{}", &xml[..o], new, &xml[c..])
}

fn print_status_xml(st: &State) -> String {
    let mut xml = PRINT.to_string();
    for (i, color) in INK_ORDER.iter().enumerate() {
        let anchor = format!("<ivec:color>{color}</ivec:color>");
        let level = st.levels[i];
        xml = replace_after(&xml, &anchor, "<ivec:level>", "</ivec:level>", &level.to_string());
        let icon = match level {
            0 => "error",
            1..=15 => "warning",
            _ => "none",
        };
        xml = replace_after(&xml, &anchor, "<ivec:icon>", "</ivec:icon>", icon);
    }
    // `icon` precedes `level` in the fixture; the anchor search above starts at the colour, which
    // is before both, so both replacements hit the right ink.
    let status = if st.busy_polls > 0 { "printing" } else { "idle" };
    xml = xml.replace(
        "<ivec:status>idle</ivec:status>",
        &format!("<ivec:status>{status}</ivec:status>"),
    );
    if !st.support_code.is_empty() {
        xml = xml.replace(
            "<ivec:current_support_code/>",
            &format!("<ivec:current_support_code>{}</ivec:current_support_code>", st.support_code),
        );
    }
    xml
}

fn set_simple_tag(xml: &str, tag: &str, value: &str) -> String {
    let open = format!("<ivec:{tag}>");
    let close = format!("</ivec:{tag}>");
    replace_after(xml, &open, &open, &close, value)
}

/// Apply the `<ivec:setregistration id="N"><ivec:item sign="X">v</ivec:item>..` sets of a
/// `SetConfiguration` body to the configuration XML.
fn apply_registration(cfg: &str, body: &str) -> String {
    let mut cfg = cfg.to_string();
    let mut rest = body;
    while let Some(a) = rest.find("<ivec:setregistration id=\"") {
        let after = &rest[a + "<ivec:setregistration id=\"".len()..];
        let Some(id_end) = after.find('"') else { break };
        let id = after[..id_end].to_string();
        let Some(block_end) = after.find("</ivec:setregistration>") else { break };
        let block = &after[..block_end];
        let anchor = format!("<ivec:setregistration id=\"{id}\">");
        let mut items = block;
        while let Some(i) = items.find("<ivec:item sign=\"") {
            let item = &items[i + "<ivec:item sign=\"".len()..];
            let Some(sign_end) = item.find('"') else { break };
            let sign = &item[..sign_end];
            let Some(v_start) = item.find('>') else { break };
            let Some(v_end) = item.find("</ivec:item>") else { break };
            let value = &item[v_start + 1..v_end];
            // Replace that sign's value inside this set's block of the stored XML.
            if let Some(set_at) = cfg.find(&anchor) {
                let key = format!("<ivec:item sign=\"{sign}\">");
                if let Some(k) = cfg[set_at..].find(&key).map(|k| set_at + k + key.len()) {
                    if let Some(end) = cfg[k..].find("</ivec:item>").map(|e| k + e) {
                        cfg = format!("{}{}{}", &cfg[..k], value, &cfg[end..]);
                    }
                }
            }
            items = &item[v_end..];
        }
        rest = &after[block_end..];
    }
    cfg
}

/// Apply the simple (single-element) settings of a `SetConfiguration` body.
fn apply_set_configuration(cfg: &str, body: &str) -> String {
    let mut cfg = apply_registration(cfg, body);
    for tag in ["drylevel", "papergap", "autopoweron"] {
        let open = format!("<ivec:{tag}>");
        let close = format!("</ivec:{tag}>");
        if let Some(a) = body.find(&open) {
            let v = &body[a + open.len()..];
            if let Some(end) = v.find(&close) {
                cfg = set_simple_tag(&cfg, tag, &v[..end]);
            }
        }
    }
    cfg
}

fn answer(shared: &Shared, op: &str, service: &str, body: &str) -> String {
    // A vendor command is logged with its name: `VendorCmd:UpdateFRom/device`.
    let vendor = body
        .split("<vcn:ijoperation>")
        .nth(1)
        .and_then(|s| s.split("</vcn:ijoperation>").next())
        .filter(|_| op == "VendorCmd");
    match vendor {
        Some(v) => shared.log.lock().unwrap().push(format!("{op}:{v}/{service}")),
        None => shared.log.lock().unwrap().push(format!("{op}/{service}")),
    }
    let mut st = shared.state.lock().unwrap();
    // Verified on the XK140: while asleep only `GetStatus` of the print and maintenance services
    // answers `Suspended`; `GetCapability` still works. Writes are assumed to be refused too.
    let suspended = st.asleep
        && matches!(service, "print" | "maintenance")
        && (op == "GetStatus" || !op.starts_with("Get"));
    if suspended {
        return reply(op, service, false, "Suspended");
    }
    match (op, service) {
        ("GetStatus", "print") => {
            let xml = print_status_xml(&st);
            st.busy_polls = st.busy_polls.saturating_sub(1);
            xml
        }
        ("GetStatus", "device") => match &st.firmware.update_available {
            Some(v) => DEVICE_STATUS.replace(
                "<ivec:networkupdate_available>unavailable</ivec:networkupdate_available>",
                &format!("<ivec:networkupdate_available>{v}</ivec:networkupdate_available>"),
            ),
            None => DEVICE_STATUS.to_string(),
        },
        ("GetStatus", "maintenance") => MAINT_STATUS.to_string(),
        ("GetCapability", "device") => DEVICE_CAP.replace(
            "<ivec:firmver>1.060</ivec:firmver>",
            &format!("<ivec:firmver>{}</ivec:firmver>", st.firmware.version),
        ),
        ("GetCapability", "maintenance") => {
            let mut xml = MAINT_CAP.to_string();
            if !st.cleaning_types.is_empty() {
                let extra: String = st
                    .cleaning_types
                    .iter()
                    .map(|t| format!("<ivec:type value=\"{t}\"><ivec:inkgroup>all</ivec:inkgroup></ivec:type>"))
                    .collect();
                xml = xml.replace("</ivec:Cleaning>", &format!("{extra}</ivec:Cleaning>"));
            }
            if st.roller_cleaning {
                xml = xml.replace(
                    "</ivec:TestPrint>",
                    "</ivec:TestPrint><ivec:RollerCleaning><ivec:type value=\"regular\"/></ivec:RollerCleaning>",
                );
            }
            xml
        }
        ("GetConfiguration", "device") => st.config_xml.clone(),
        ("PowerOn", _) => {
            // Verified on the XK140 (firmware 1.060, 2026-10-14): without the power-on mode the
            // printer answers `NG ParameterError` and stays as it is; with it, it wakes up.
            if has_power_on_mode(body) {
                st.asleep = false;
                reply(op, service, true, "")
            } else {
                reply(op, service, false, "ParameterError")
            }
        }
        ("VendorCmd", "device") if body.contains(">UpdateFRom<") => {
            if st.firmware.update_available.is_some() {
                let from = std::time::Instant::now() + std::time::Duration::from_millis(300);
                st.firmware.down_from = Some(from);
                st.firmware.down_until =
                    Some(from + std::time::Duration::from_millis(st.firmware.restart_ms));
            }
            reply(op, service, true, "")
        }
        ("Cleaning" | "TestPrint" | "RollerCleaning", _) => {
            st.busy_polls = st.job_polls;
            reply(op, service, true, "")
        }
        ("SetConfiguration", "device") => {
            st.config_xml = apply_set_configuration(&st.config_xml, body);
            reply(op, service, true, "")
        }
        (op, _) if op.starts_with("Get") => reply(op, service, false, "NotSupportService"),
        _ => reply(op, service, true, ""),
    }
}

/// Does a `PowerOn` body carry a non-empty `<ivec:poweronmode>` element?
fn has_power_on_mode(body: &str) -> bool {
    body.split("<ivec:poweronmode>")
        .nth(1)
        .and_then(|rest| rest.split("</ivec:poweronmode>").next())
        .is_some_and(|mode| !mode.trim().is_empty())
}

/// Finish a firmware restart that has run its course: the new version is in place.
fn apply_restart(st: &mut State) {
    if st.firmware.down_until.is_some_and(|t| std::time::Instant::now() >= t) {
        st.firmware.down_from = None;
        st.firmware.down_until = None;
        st.firmware.version = st.firmware.next_version.clone();
        st.firmware.update_available = None;
    }
}

/// True while CHMP does not answer: switched off by a test, or restarting after a firmware update.
fn chmp_unreachable(shared: &Shared) -> bool {
    let mut st = shared.state.lock().unwrap();
    apply_restart(&mut st);
    st.chmp_down || st.firmware.down_from.is_some_and(|t| std::time::Instant::now() >= t)
}

fn chunked(body: &str) -> Vec<u8> {
    let mut out = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/octet-stream\r\n\r\n"
        .to_vec();
    if !body.is_empty() {
        out.extend_from_slice(format!("{:X}\r\n{body}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"0\r\n\r\n");
    out
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        headers.push((k.to_string(), v.trim().to_string()));
    }
    let len: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).ok()?;
    Some(Request { method, path, headers, body })
}

fn serve_chmp(stream: TcpStream, shared: Arc<Shared>) {
    // As on the real printer, a reply belongs to the connection that carried the command.
    let mut pending: Option<String> = None;
    let Ok(clone) = stream.try_clone() else { return };
    let mut reader = BufReader::new(clone);
    let mut out = stream;
    while let Some(req) = read_request(&mut reader) {
        if chmp_unreachable(&shared) {
            return;
        }
        let closing = req.header("connection").is_some_and(|v| v.eq_ignore_ascii_case("close"));
        if req.method == "GET"
            && (matches!(req.path.as_str(), "/" | "/index.html" | "/JS_MDL/model.js")
                || req.path.starts_with("/strings/")
                || req.path.starts_with("/icon/"))
        {
            let resp = serve_web(&shared, &req);
            if out.write_all(&resp).is_err() || closing {
                return;
            }
            continue;
        }
        if req.path.starts_with("/eSCL/") || req.path == "/eSCL" {
            let resp = serve_escl(&shared, &req);
            if out.write_all(&resp).is_err() || closing {
                return;
            }
            continue;
        }
        let resp = if req.method == "POST" {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            if let (Some(op), Some(svc)) = (
                body.split("<ivec:operation>")
                    .nth(1)
                    .and_then(|s| s.split("</ivec:operation>").next()),
                body.split("servicetype=\"").nth(1).and_then(|s| s.split('"').next()),
            ) {
                let now = shared.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                shared.max_in_flight.fetch_max(now, Ordering::SeqCst);
                let ms = shared.latency_ms.load(Ordering::SeqCst);
                if ms > 0 {
                    thread::sleep(std::time::Duration::from_millis(ms as u64));
                }
                pending = Some(answer(&shared, op, svc, &body));
                shared.in_flight.fetch_sub(1, Ordering::SeqCst);
            }
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()
        } else if let Some(r) = &pending {
            chunked(r)
        } else {
            b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\n\r\n".to_vec()
        };
        if out.write_all(&resp).is_err() || closing {
            return;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The printer's own web page
// ---------------------------------------------------------------------------------------------

fn serve_web(shared: &Shared, req: &Request) -> Vec<u8> {
    match req.path.as_str() {
        "/JS_MDL/model.js" => {
            shared.log.lock().unwrap().push("Web/model.js".to_string());
            // Like the real printer: the file is served for a language only (`Accept-Language`
            // here; `lang=` and a cookie work on the real one) and worded for it.
            let lang = req.header("accept-language").map(str::to_ascii_lowercase);
            let script = shared.state.lock().unwrap().model_js.clone();
            match (script, lang) {
                (Some(js), Some(lang)) => {
                    let js =
                        if lang.starts_with("ja") { js.replace("LNG=EN", "LNG=JP") } else { js };
                    http_reply(
                        "200 OK",
                        &[("Content-Type", "application/javascript")],
                        js.as_bytes(),
                    )
                }
                _ => http_reply("404 Not Found", &[], b"A specification file is not found"),
            }
        }
        path if path.starts_with("/strings/") => {
            let file = path.trim_start_matches("/strings/");
            let lang = file.split('.').next().unwrap_or("").to_ascii_lowercase();
            shared.log.lock().unwrap().push(format!("Web/strings/{lang}"));
            let body = shared.state.lock().unwrap().words.strings.get(&lang).cloned();
            match body {
                Some(b) => http_reply("200 OK", &[], &b),
                None => http_reply("404 Not Found", &[], b"A specification file is not found"),
            }
        }
        path if path.starts_with("/icon/") => {
            let name = path.trim_start_matches("/icon/").to_string();
            shared.log.lock().unwrap().push(format!("Web/icon/{name}"));
            let body = shared
                .state
                .lock()
                .unwrap()
                .words
                .icons
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, b)| b.clone());
            match body {
                Some(b) => http_reply("200 OK", &[("Content-Type", "image/png")], &b),
                None => http_reply("404 Not Found", &[], b"not found"),
            }
        }
        _ => {
            shared.log.lock().unwrap().push("Web/page".to_string());
            http_reply(
                "200 OK",
                &[("Content-Type", "text/html")],
                b"<html><body>The fake printer's web page</body></html>",
            )
        }
    }
}

// ---------------------------------------------------------------------------------------------
// eSCL
// ---------------------------------------------------------------------------------------------

fn http_reply(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut out = out.into_bytes();
    out.extend_from_slice(body);
    out
}

/// Text of the first `<prefix:local>..</prefix:local>` element.
fn xml_value<'a>(xml: &'a str, local: &str) -> Option<&'a str> {
    let key = format!(":{local}>");
    let start = xml.find(&key)? + key.len();
    let end = xml[start..].find('<')? + start;
    Some(xml[start..end].trim())
}

/// A page-like test pattern: white, a dark block, a gradient bar. Grey scans are single-channel.
fn test_pattern_jpeg(w: u32, h: u32, gray: bool) -> Vec<u8> {
    use image::codecs::jpeg::JpegEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    let mut rgb = vec![250u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 3) as usize;
            let in_block = x > w / 5 && x < w * 3 / 5 && y > h / 5 && y < h * 2 / 5;
            let in_bar = y > h * 3 / 5 && y < h * 7 / 10 && x > w / 10 && x < w * 9 / 10;
            let px = if in_block {
                [30, 30, 30]
            } else if in_bar {
                let v = ((x * 255) / w) as u8;
                [v, 128, 255 - v]
            } else {
                [250, 250, 250]
            };
            rgb[i..i + 3].copy_from_slice(&px);
        }
    }
    let mut out = Vec::new();
    let enc = JpegEncoder::new_with_quality(&mut out, 85);
    if gray {
        let g: Vec<u8> = rgb
            .chunks_exact(3)
            .map(|p| {
                ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000)
                    as u8
            })
            .collect();
        let _ = enc.write_image(&g, w, h, ExtendedColorType::L8);
    } else {
        let _ = enc.write_image(&rgb, w, h, ExtendedColorType::Rgb8);
    }
    out
}

fn serve_escl(shared: &Shared, req: &Request) -> Vec<u8> {
    let path = req.path.trim_end_matches('/');
    let xml = |body: &str| http_reply("200 OK", &[("Content-Type", "text/xml")], body.as_bytes());
    let not_found = || http_reply("404 Not Found", &[], b"");
    let log = |what: &str| shared.log.lock().unwrap().push(format!("eSCL/{what}"));
    match (req.method.as_str(), path) {
        ("GET", "/eSCL/ScannerCapabilities") => {
            log("ScannerCapabilities");
            xml(ESCL_CAPS)
        }
        ("GET", "/eSCL/ScannerStatus") => {
            log("ScannerStatus");
            let state = shared.state.lock().unwrap().scan.state.clone();
            xml(&ESCL_STATUS_IDLE
                .replace("<pwg:State>Idle</pwg:State>", &format!("<pwg:State>{state}</pwg:State>")))
        }
        ("POST", "/eSCL/ScanJobs") => {
            log("POST-ScanJobs");
            let body = String::from_utf8_lossy(&req.body).into_owned();
            let mut st = shared.state.lock().unwrap();
            if st.scan.busy_posts > 0 {
                st.scan.busy_posts -= 1;
                return http_reply("503 Service Unavailable", &[], b"");
            }
            if let Some(code) = st.scan.reject_status {
                return http_reply(&format!("{code} Rejected"), &[], b"");
            }
            if !body.contains("ScanSettings") || xml_value(&body, "XResolution").is_none() {
                return http_reply("400 Bad Request", &[], b"");
            }
            st.scan.requests.push(body);
            let id = st.scan.next_id;
            st.scan.next_id += 1;
            st.scan.jobs.push((id, 0));
            let loc = format!("/eSCL/ScanJobs/{id}");
            http_reply("201 Created", &[("Location", &loc)], b"")
        }
        ("GET", p) if p.starts_with("/eSCL/ScanJobs/") && p.ends_with("/NextDocument") => {
            log("NextDocument");
            let id: Option<u32> = p
                .trim_start_matches("/eSCL/ScanJobs/")
                .trim_end_matches("/NextDocument")
                .parse()
                .ok();
            let mut st = shared.state.lock().unwrap();
            let pages = st.scan.pages;
            let Some(job) = st.scan.jobs.iter_mut().find(|j| Some(j.0) == id) else {
                return not_found();
            };
            if job.1 >= pages {
                return not_found();
            }
            job.1 += 1;
            let settings = st.scan.requests.last().cloned().unwrap_or_default();
            let num = |name: &str, default: u32| {
                xml_value(&settings, name).and_then(|v| v.parse().ok()).unwrap_or(default)
            };
            let dpi = num("XResolution", 300).max(1);
            let (rw, rh) = (num("Width", 2480), num("Height", 3508));
            let (w, h) = (rw * dpi / 300, rh * dpi / 300);
            let cap = st.scan.max_edge;
            let f = (f64::from(cap) / f64::from(w.max(h).max(1))).min(1.0);
            let (w, h) = (((f64::from(w) * f) as u32).max(16), ((f64::from(h) * f) as u32).max(16));
            let gray = xml_value(&settings, "ColorMode") == Some("Grayscale8");
            let jpeg = st.scan.image.clone().unwrap_or_else(|| test_pattern_jpeg(w, h, gray));
            drop(st);
            http_reply("200 OK", &[("Content-Type", "image/jpeg")], &jpeg)
        }
        ("DELETE", p) if p.starts_with("/eSCL/ScanJobs/") => {
            log("DELETE-ScanJob");
            let id: Option<u32> = p.trim_start_matches("/eSCL/ScanJobs/").parse().ok();
            let mut st = shared.state.lock().unwrap();
            let before = st.scan.jobs.len();
            st.scan.jobs.retain(|j| Some(j.0) != id);
            if st.scan.jobs.len() == before { not_found() } else { http_reply("200 OK", &[], b"") }
        }
        _ => {
            log("unknown");
            not_found()
        }
    }
}

// ---------------------------------------------------------------------------------------------
// IPP
// ---------------------------------------------------------------------------------------------

fn name(s: &str) -> IppValue {
    IppValue::Name(s.to_string())
}

fn media_entry(w: i32, h: i32, m: [i32; 4], source: &str, ty: &str) -> IppValue {
    let int = |v: i32| vec![IppValue::Integer(v)];
    let kw = |s: &str| vec![IppValue::Keyword(s.into())];
    IppValue::Collection(vec![
        (
            "media-size".into(),
            vec![IppValue::Collection(vec![
                ("x-dimension".into(), int(w)),
                ("y-dimension".into(), int(h)),
            ])],
        ),
        ("media-bottom-margin".into(), int(m[1])),
        ("media-left-margin".into(), int(m[2])),
        ("media-right-margin".into(), int(m[3])),
        ("media-top-margin".into(), int(m[0])),
        ("media-source".into(), kw(source)),
        ("media-type".into(), kw(ty)),
    ])
}

/// A small `media-col-database` shaped like the XK140's: A4, L and card plain / photo, the borderless
/// photo-paper entries, the cassette's A4 / A5 / B5, and the two disc entries.
pub fn media_col_database() -> Vec<IppValue> {
    let bordered = [500, 500, 340, 340];
    let zero = [0, 0, 0, 0];
    let mut v = Vec::new();
    for src in ["auto", "rear"] {
        for ty in
            ["stationery", "photographic", "com.canon-mtglossy", "com.canon-mtluster", "envelope"]
        {
            v.push(media_entry(21000, 29700, bordered, src, ty));
            v.push(media_entry(8890, 12700, bordered, src, ty));
        }
        for ty in ["stationery", "photographic", "com.canon-mtglossy"] {
            v.push(media_entry(5500, 9100, bordered, src, ty));
        }
        for ty in ["photographic", "com.canon-mtglossy", "com.canon-mtluster"] {
            v.push(media_entry(21000, 29700, zero, src, ty));
            v.push(media_entry(8890, 12700, zero, src, ty));
        }
    }
    for (w, h) in [(14800, 21000), (18200, 25700), (21000, 29700)] {
        v.push(media_entry(w, h, bordered, "main", "stationery"));
    }
    v.push(media_entry(21000, 29700, zero, "main", "stationery"));
    for ty in ["com.canon-mtdisca", "com.canon-mtdiscb"] {
        v.push(media_entry(12000, 12000, [10, 10, 10, 10], "disc", ty));
    }
    v
}

/// The printer-attributes the fake serves (shaped like the XK140's).
pub fn ipp_printer_response(levels: [u8; 5]) -> IppMessage {
    let names = ["Magenta", "Black(BK)", "Yellow", "Black(PGBK)", "Cyan"];
    let colors = ["#F200FF", "#000000", "#FFDA00", "#000000", "#00CFFF"];
    let kws = |n: &str, v: &[&str]| IppAttribute::keywords(n, v);
    let attrs = vec![
        IppAttribute::single("printer-state", IppValue::Enum(3)),
        IppAttribute::single("printer-state-reasons", IppValue::Keyword("none".into())),
        IppAttribute::single("printer-state-message", IppValue::Text(String::new())),
        IppAttribute::single("printer-is-accepting-jobs", IppValue::Boolean(true)),
        IppAttribute::new("marker-names", names.map(name).into()),
        IppAttribute::new("marker-colors", colors.map(name).into()),
        IppAttribute::new("marker-types", vec![IppValue::Keyword("ink-cartridge".into()); 5]),
        IppAttribute::new("marker-levels", levels.map(|l| IppValue::Integer(i32::from(l))).into()),
        IppAttribute::new("marker-low-levels", vec![IppValue::Integer(15); 5]),
        IppAttribute::new("marker-high-levels", vec![IppValue::Integer(100); 5]),
        IppAttribute::single("printer-firmware-string-version", IppValue::Text("1.060".into())),
        IppAttribute::single("printer-make-and-model", IppValue::Text("Canon XK140 series".into())),
        kws("media-source-supported", &["auto", "main", "rear", "disc"]),
        kws(
            "media-type-supported",
            &[
                "stationery",
                "photographic",
                "envelope",
                "com.canon-mtglossy",
                "com.canon-mtluster",
                "com.canon-mtdisca",
                "com.canon-mtdiscb",
            ],
        ),
        kws("media-supported", &["iso_a4_210x297mm", "na_letter_8.5x11in", "om_square_120x120mm"]),
        IppAttribute::new(
            "document-format-supported",
            ["application/octet-stream", "image/jpeg", "image/urf", "image/pwg-raster"]
                .map(|s| IppValue::MimeMediaType(s.into()))
                .into(),
        ),
        IppAttribute::new("print-quality-supported", [3, 4, 5].map(IppValue::Enum).into()),
        kws("pwg-raster-document-type-supported", &["srgb_8", "sgray_8"]),
        IppAttribute::single(
            "pwg-raster-document-resolution-supported",
            IppValue::Resolution { x: 600, y: 600, units: 3 },
        ),
        kws("pwg-raster-document-sheet-back", &["rotated"]),
        kws("sides-supported", &["one-sided", "two-sided-long-edge", "two-sided-short-edge"]),
        IppAttribute::single("copies-supported", IppValue::Range { lower: 1, upper: 99 }),
        IppAttribute::single(
            "jpeg-x-dimension-supported",
            IppValue::Range { lower: 16, upper: 19200 },
        ),
        IppAttribute::single("jpeg-k-octets-supported", IppValue::Range { lower: 0, upper: 16384 }),
        kws(
            "job-creation-attributes-supported",
            &[
                "copies",
                "sides",
                "media",
                "print-quality",
                "printer-resolution",
                "media-col",
                "print-color-mode",
                "job-name",
            ],
        ),
        IppAttribute::single(
            "printer-more-info",
            IppValue::Uri("http://printer.example./index.html?page=PAGE_AAP".into()),
        ),
        kws("identify-actions-supported", &["flash", "sound"]),
        // Like the real printer: empty unless asked for on its own.
        IppAttribute::single("media-col-database", IppValue::OutOfBand(OutOfBand::NoValue)),
    ];
    ipp_response(0x0000, vec![IppGroup::new(ipp_tag::PRINTER_ATTRIBUTES, attrs)])
}

fn ipp_response(status: u16, extra: Vec<IppGroup>) -> IppMessage {
    let mut groups = vec![IppGroup::new(
        ipp_tag::OPERATION_ATTRIBUTES,
        vec![
            IppAttribute::single("attributes-charset", IppValue::Charset("utf-8".into())),
            IppAttribute::single(
                "attributes-natural-language",
                IppValue::NaturalLanguage("en".into()),
            ),
        ],
    )];
    groups.extend(extra);
    IppMessage { version: (1, 1), code: status, request_id: 1, groups }
}

fn op_attr<'a>(m: &'a IppMessage, name: &str) -> Option<&'a IppAttribute> {
    m.group(ipp_tag::OPERATION_ATTRIBUTES)?.get(name)
}

fn job_group(id: i32, state: i32, impressions: i32) -> IppGroup {
    IppGroup::new(
        ipp_tag::JOB_ATTRIBUTES,
        vec![
            IppAttribute::single("job-id", IppValue::Integer(id)),
            IppAttribute::single(
                "job-uri",
                IppValue::Uri(format!("ipp://printer.example/ipp/print/{id}")),
            ),
            IppAttribute::single("job-state", IppValue::Enum(state)),
            IppAttribute::keywords(
                "job-state-reasons",
                &[match state {
                    9 => "job-completed-successfully",
                    7 => "job-canceled-by-user",
                    8 => "aborted-by-system",
                    5 => "job-printing",
                    _ => "none",
                }],
            ),
            IppAttribute::single("job-impressions-completed", IppValue::Integer(impressions)),
        ],
    )
}

/// The reply to one IPP request, plus the operation's log name.
/// `attributes-natural-language` of a request.
fn asked_language(m: &IppMessage) -> String {
    op_attr(m, "attributes-natural-language")
        .and_then(|a| a.values.first())
        .and_then(IppValue::as_str)
        .unwrap_or("en")
        .to_ascii_lowercase()
}

/// `printer-strings-*` and `printer-icons` for a request in `lang`: what a printer with an
/// IPP Everywhere strings file says. The addresses name a host that is not the fake's own, so a
/// client that follows the host instead of the path fails its test.
fn words_attributes(w: &Words, lang: &str) -> Vec<IppAttribute> {
    let mut v = Vec::new();
    if !w.strings.is_empty() {
        let langs: Vec<IppValue> = w.strings.keys().map(|l| IppValue::Keyword(l.clone())).collect();
        v.push(IppAttribute::new("printer-strings-languages-supported", langs));
        let file = if w.one_address {
            w.strings.keys().next().cloned()
        } else {
            w.strings.contains_key(lang).then(|| lang.to_string())
        };
        // Like the real printer: a language it has no file for gets an empty address.
        let uri =
            file.map(|l| format!("http://printer.example./strings/{l}.txt")).unwrap_or_default();
        v.push(IppAttribute::single("printer-strings-uri", IppValue::Uri(uri)));
    }
    if !w.icons.is_empty() {
        let icons: Vec<IppValue> = w
            .icons
            .iter()
            .map(|(n, _)| IppValue::Uri(format!("http://printer.example./icon/{n}")))
            .collect();
        v.push(IppAttribute::new("printer-icons", icons));
    }
    v
}

fn ipp_answer(shared: &Shared, m: &IppMessage, document: &[u8]) -> IppMessage {
    let opname = ipp_op::name(m.code);
    shared.log.lock().unwrap().push(format!("IPP/{opname}"));
    let mut st = shared.state.lock().unwrap();
    let format = op_attr(m, "document-format")
        .and_then(|a| a.values.first())
        .and_then(IppValue::as_str)
        .unwrap_or("")
        .to_string();
    let reject = |code: u16| {
        let unsupported = IppGroup::new(
            ipp_tag::UNSUPPORTED_ATTRIBUTES,
            vec![IppAttribute::single("media-col", IppValue::OutOfBand(OutOfBand::Unsupported))],
        );
        ipp_response(code, vec![unsupported])
    };
    match m.code {
        ipp_op::GET_PRINTER_ATTRIBUTES => {
            let mut resp = ipp_printer_response(st.levels);
            let wanted: Vec<String> = op_attr(m, "requested-attributes")
                .map(|a| a.values.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let all = wanted.is_empty() || wanted.iter().any(|w| w == "all");
            if let Some(g) = resp.groups.iter_mut().find(|g| g.tag == ipp_tag::PRINTER_ATTRIBUTES) {
                if wanted.iter().any(|w| w == "media-col-database") {
                    for a in &mut g.attributes {
                        if a.name == "media-col-database" {
                            a.values = media_col_database();
                        }
                    }
                }
                g.attributes.extend(words_attributes(&st.words, &asked_language(m)));
                if !all {
                    g.attributes.retain(|a| wanted.contains(&a.name));
                }
                for a in &mut g.attributes {
                    if a.name == "printer-is-accepting-jobs" {
                        a.values = vec![IppValue::Boolean(st.prints.accepting)];
                    }
                    if a.name == "printer-firmware-string-version" {
                        a.values = vec![IppValue::Text(st.firmware.version.clone())];
                    }
                }
            }
            resp
        }
        ipp_op::VALIDATE_JOB | ipp_op::PRINT_JOB => {
            let print = m.code == ipp_op::PRINT_JOB;
            let supported =
                ["application/octet-stream", "image/jpeg", "image/urf", "image/pwg-raster"];
            let status = if let Some(code) = st.prints.reject {
                Some(code)
            } else if !supported.contains(&format.as_str()) {
                Some(0x040A)
            } else {
                None
            };
            let job_id = if print && status.is_none() {
                let id = st.prints.next_id;
                st.prints.next_id += 1;
                st.prints.jobs.push((id, 0, false));
                id
            } else {
                0
            };
            st.prints.received.push(ReceivedJob {
                operation: opname.clone(),
                request: m.clone(),
                format,
                data: if print { document.to_vec() } else { Vec::new() },
                job_id,
            });
            match status {
                Some(code) => reject(code),
                None if print => ipp_response(0, vec![job_group(job_id, 3, 0)]),
                None => ipp_response(0, vec![]),
            }
        }
        ipp_op::GET_JOB_ATTRIBUTES => {
            let id = op_attr(m, "job-id")
                .and_then(|a| a.values.first())
                .and_then(IppValue::as_i32)
                .unwrap_or(0);
            let polls = st.prints.polls_to_complete;
            let abort = st.prints.abort_jobs;
            let forget = st.prints.forget_finished;
            match st.prints.jobs.iter_mut().find(|j| j.0 == id) {
                None => ipp_response(0x0406, vec![]),
                Some(j) if forget && j.1 >= polls && !j.2 => ipp_response(0x0406, vec![]),
                Some(j) => {
                    j.1 += 1;
                    let state = if j.2 {
                        7
                    } else if j.1 > polls {
                        if abort { 8 } else { 9 }
                    } else if j.1 == 1 && polls > 0 {
                        3
                    } else {
                        5
                    };
                    ipp_response(0, vec![job_group(id, state, if state == 9 { 1 } else { 0 })])
                }
            }
        }
        ipp_op::GET_JOBS => {
            let groups: Vec<IppGroup> = st
                .prints
                .jobs
                .iter()
                .filter(|j| !j.2 && j.1 <= st.prints.polls_to_complete)
                .map(|j| job_group(j.0, 5, 0))
                .collect();
            ipp_response(0, groups)
        }
        ipp_op::CANCEL_JOB => {
            let id = op_attr(m, "job-id")
                .and_then(|a| a.values.first())
                .and_then(IppValue::as_i32)
                .unwrap_or(0);
            match st.prints.jobs.iter_mut().find(|j| j.0 == id) {
                Some(j) => {
                    j.2 = true;
                    ipp_response(0, vec![])
                }
                None => ipp_response(0x0406, vec![]),
            }
        }
        ipp_op::IDENTIFY_PRINTER => {
            let action = op_attr(m, "identify-actions")
                .and_then(|a| a.values.first())
                .and_then(IppValue::as_str)
                .unwrap_or("")
                .to_string();
            st.prints.identify.push(action);
            ipp_response(0, vec![])
        }
        _ => ipp_response(0x0501, vec![]), // server-error-operation-not-supported
    }
}

fn serve_ipp(stream: TcpStream, shared: Arc<Shared>) {
    let Ok(clone) = stream.try_clone() else { return };
    let mut reader = BufReader::new(clone);
    let mut out = stream;
    while let Some(req) = read_request(&mut reader) {
        if shared.state.lock().unwrap().ipp_down {
            return;
        }
        let closing = req.header("connection").is_some_and(|v| v.eq_ignore_ascii_case("close"));
        let resp = match IppMessage::decode_prefix(&req.body) {
            Ok((m, used)) => ipp_answer(&shared, &m, &req.body[used..]),
            Err(_) => ipp_response(0x0400, vec![]),
        };
        let body = resp.encode();
        let mut http = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        http.extend_from_slice(&body);
        if out.write_all(&http).is_err() || closing {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_status_reflects_levels_icons_and_codes() {
        let st = State {
            asleep: false,
            roller_cleaning: false,
            cleaning_types: Vec::new(),
            model_js: None,
            words: Words::default(),
            firmware: FirmwareState {
                version: "1.060".into(),
                update_available: None,
                next_version: "1.061".into(),
                restart_ms: 0,
                down_from: None,
                down_until: None,
            },
            levels: [10, 0, 70, 60, 60],
            support_code: "1300".into(),
            config_xml: String::new(),
            busy_polls: 1,
            job_polls: 2,
            chmp_down: false,
            ipp_down: false,
            scan: ScanState {
                pages: 1,
                busy_posts: 0,
                reject_status: None,
                image: None,
                max_edge: 100,
                state: "Idle".to_string(),
                next_id: 1,
                jobs: Vec::new(),
                requests: Vec::new(),
            },
            prints: PrintState {
                received: Vec::new(),
                next_id: 1,
                jobs: Vec::new(),
                polls_to_complete: 1,
                reject: None,
                abort_jobs: false,
                identify: Vec::new(),
                accepting: true,
                forget_finished: false,
            },
        };
        let xml = print_status_xml(&st);
        assert!(xml.contains("<ivec:status>printing</ivec:status>"));
        assert!(xml.contains("<ivec:current_support_code>1300</ivec:current_support_code>"));
        let m = xml.split("<ivec:color>M</ivec:color>").nth(1).unwrap();
        assert!(
            m.starts_with("\n<ivec:icon>warning</ivec:icon>\n<ivec:level>10</ivec:level>"),
            "{m:.80}"
        );
        let bk = xml.split("<ivec:color>BK</ivec:color>").nth(1).unwrap();
        assert!(
            bk.contains("<ivec:icon>error</ivec:icon>")
                && bk.contains("<ivec:level>0</ivec:level>")
        );
    }

    #[test]
    fn power_on_needs_the_power_on_mode_like_the_real_printer() {
        let f = FakePrinter::start();
        f.set_asleep(true);
        let bare = "<ivec:param_set servicetype=\"device\"></ivec:param_set>";
        let empty = "<ivec:poweronmode></ivec:poweronmode>";
        let blank = "<ivec:poweronmode> </ivec:poweronmode>";
        for body in [bare, empty, blank] {
            let r = answer(&f.shared, "PowerOn", "device", body);
            assert!(r.contains("<ivec:response>NG</ivec:response>"), "{body}: {r}");
            assert!(
                r.contains("<ivec:response_detail>ParameterError</ivec:response_detail>"),
                "{r}"
            );
            assert!(f.state().asleep, "{body}: a refused PowerOn leaves the printer asleep");
        }
        let ok = "<ivec:param_set servicetype=\"device\">\
                  <ivec:poweronmode>ModeA</ivec:poweronmode></ivec:param_set>";
        let r = answer(&f.shared, "PowerOn", "device", ok);
        assert!(r.contains("<ivec:response>OK</ivec:response>"), "{r}");
        assert!(!f.state().asleep, "PowerOn with the mode wakes the printer");
    }

    #[test]
    fn set_configuration_updates_simple_fields() {
        let cfg = apply_set_configuration(
            DEVICE_CFG,
            "<ivec:drylevel>5</ivec:drylevel><ivec:papergap>wide</ivec:papergap>",
        );
        assert!(cfg.contains("<ivec:drylevel>5</ivec:drylevel>"));
        assert!(cfg.contains("<ivec:papergap>wide</ivec:papergap>"));
        assert!(!cfg.contains("<ivec:drylevel>3</ivec:drylevel>"));
    }
}
