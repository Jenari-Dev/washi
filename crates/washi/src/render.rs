// SPDX-License-Identifier: GPL-2.0-or-later
//! Plain-text rendering of printer data. Pure functions, unit-tested.

use std::fmt::Write as _;
use std::time::Duration;

use washi_core::ivec::{
    DeviceCapability, DeviceConfig, DeviceStatus, MaintenanceCapability, PowerRanges, Request,
    pretty,
};
use washi_core::{InkLevel, InkSource, InkStatus, Plan, Power, PrinterSnapshot, Step};

pub fn ink_name(code: &str) -> &str {
    washi_core::ink_display_name(code)
}

pub fn ink_bar(level: Option<u8>, width: usize) -> String {
    let Some(level) = level else { return "?".repeat(width) };
    let filled = (usize::from(level.min(100)) * width).div_ceil(100).min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

pub fn tray_name(bin: &str) -> String {
    match bin {
        "inputbin_p1" => "Rear tray".to_string(),
        "inputbin_p2" => "Cassette".to_string(),
        other => other.to_string(),
    }
}

/// `iso_a4_210x297mm` -> `A4 (210x297 mm)`.
pub fn pretty_size(s: &str) -> String {
    let parts: Vec<&str> = s.split('_').collect();
    if let Some((last, rest)) = parts.split_last() {
        if let Some(dim) = last.strip_suffix("mm").filter(|d| d.contains('x')) {
            let name: Vec<&str> = rest
                .iter()
                .copied()
                .filter(|p| !matches!(*p, "iso" | "jis" | "na" | "om" | "prc"))
                .collect();
            let name = name.join(" ");
            let name = if name.len() <= 3 { name.to_uppercase() } else { name };
            return if name.is_empty() {
                format!("{dim} mm")
            } else {
                format!("{name} ({dim} mm)")
            };
        }
    }
    s.to_string()
}

pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 60 { format!("{}m{:02}s", s / 60, s % 60) } else { format!("{s}s") }
}

/// "just now", "5 min ago", "3 h ago", "2 d ago".
pub fn fmt_ago(secs: u64) -> String {
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86400),
    }
}

fn ink_flag(i: &InkLevel) -> &'static str {
    match i.status {
        InkStatus::Ok => "",
        InkStatus::Low => "  low",
        InkStatus::Empty => "  EMPTY",
        InkStatus::Unknown if i.level.is_none() => "  level unknown",
        InkStatus::Unknown => "  check printer",
    }
}

/// `washi status`: power, state, error, ink tanks and paper. `now` is the Unix time used for
/// "last awake ... ago".
pub fn render_snapshot(host: &str, s: &PrinterSnapshot, now: u64) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "Printer   {host}");
    let _ = writeln!(o, "Power     {}", s.power.as_str());
    if let Some(a) = &s.address {
        let link = match s.connection {
            washi_core::Connection::Wifi => " (Wi-Fi)",
            washi_core::Connection::Lan => " (wired LAN)",
            washi_core::Connection::Unknown => "",
        };
        let _ = writeln!(o, "Address   {a}{link}");
    }
    if s.power != Power::On {
        let _ = writeln!(o, "          {}", s.power.message());
    }
    if let Some(state) = &s.state {
        let via = if s.power == Power::Sleeping { " (reported over IPP)" } else { "" };
        let _ = writeln!(o, "State     {state}{via}");
    }
    if let Some(t) = s.last_awake.filter(|_| s.power != Power::On) {
        let _ = writeln!(o, "Awake     last seen {}", fmt_ago(now.saturating_sub(t)));
    }
    if let Some(sup) = &s.support {
        let _ =
            writeln!(o, "Support   code {} ({}): {}", sup.code, sup.severity.as_str(), sup.message);
    }
    if let Some(a) = &s.activity {
        let _ = writeln!(o, "Activity  {a}");
    }

    let (heading, note) = match s.ink_source {
        InkSource::Ivec => ("Ink", ""),
        InkSource::Ipp => ("Ink", " (IPP marker levels)"),
        InkSource::Cache => ("Ink", " (last known; printer not answering)"),
        InkSource::None => ("Ink", " (not available)"),
    };
    let _ = writeln!(o, "\n{heading}{note}");
    let mut inks: Vec<_> = s.inks.iter().collect();
    inks.sort_by_key(|i| i.order.unwrap_or(u32::MAX));
    for i in inks {
        let level = i.level.map(|l| format!("{l:>3}%")).unwrap_or_else(|| "  ?%".to_string());
        let row = format!(
            "  {:<5} {:<14} {} {}  {}{}",
            i.color,
            ink_name(&i.color),
            ink_bar(i.level, 20),
            level,
            i.model.as_deref().unwrap_or(""),
            ink_flag(i)
        );
        let _ = writeln!(o, "{}", row.trim_end());
    }

    if s.trays.is_empty() && s.power != Power::On {
        let _ = writeln!(o, "\nPaper     not known until the printer has been awake once");
    }
    if !s.trays.is_empty() {
        let stale = s.from_cache.iter().any(|f| f == "trays");
        let _ = writeln!(o, "\nPaper{}", if stale { " (last known)" } else { "" });
        for t in &s.trays {
            let size = match (&t.paper_size, t.width, t.height) {
                (Some(s), _, _) => pretty_size(s),
                (None, Some(w), Some(h)) => {
                    format!("{} x {} mm", f64::from(w) / 100.0, f64::from(h) / 100.0)
                }
                _ => "unknown size".to_string(),
            };
            let kind = t.paper_type.as_deref().unwrap_or("unknown type");
            let _ = writeln!(o, "  {:<10} ({})  {size}, {kind}", tray_name(&t.bin), t.bin);
        }
    }
    if let Some(d) = &s.device {
        if !d.model.is_empty() {
            let _ =
                writeln!(o, "\nDevice    {} {} (firmware {})", d.manufacturer, d.model, d.firmware);
        }
    }
    for w in &s.warnings {
        let _ = writeln!(o, "\nnote: {w}");
    }
    o
}

/// One line for `washi watch`.
pub fn watch_line(s: &PrinterSnapshot, clock: &str) -> String {
    let mut o = format!("[{clock}] {:<8}", s.power.as_str());
    if let Some(state) = &s.state {
        let _ = write!(o, " {state:<10}");
    }
    let mut inks: Vec<_> = s.inks.iter().collect();
    inks.sort_by_key(|i| i.order.unwrap_or(u32::MAX));
    for i in inks {
        match i.level {
            Some(l) => {
                let _ = write!(o, " {}={l}%", i.color);
            }
            None => {
                let _ = write!(o, " {}=?", i.color);
            }
        }
        match i.status {
            InkStatus::Low => o.push('!'),
            InkStatus::Empty => o.push_str("!!"),
            _ => {}
        }
    }
    if let Some(sup) = &s.support {
        let _ = write!(o, " error={}", sup.code);
    }
    if let Some(a) = &s.activity {
        let _ = write!(o, " ({a})");
    }
    o
}

pub fn render_info(cap: &DeviceCapability, st: &DeviceStatus) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "Model       {} {}", cap.manufacturer, cap.model);
    let _ = writeln!(o, "Firmware    {}", cap.firmver);
    let _ = writeln!(o, "Services    {}", cap.services.join(", "));
    if let Some(u) = &st.networkupdate_available {
        let _ = writeln!(o, "Update      network firmware update: {u}");
    }
    let _ = writeln!(o, "\nSheets fed per tray");
    for (bin, n) in &st.total_paper_num {
        let _ = writeln!(o, "  {:<10} ({bin})  {n}", tray_name(bin));
    }
    let _ = writeln!(o, "\nInk tank replacements (all / genuine)");
    for (color, n) in &st.ink_replacement_count {
        let g = st.genuineink_replacement_count.get(color).map_or("?".to_string(), u32::to_string);
        let _ = writeln!(o, "  {:<5} {:<14} {n} / {g}", color, ink_name(color));
    }
    if let Some(h) = st.head_replacement_count {
        let _ = writeln!(o, "\nPrint head replacements  {h}");
    }
    o
}

fn power_allowed(r: &Option<PowerRanges>) -> String {
    match r {
        Some(r) => {
            let times: Vec<String> = r.times.iter().map(u32::to_string).collect();
            format!("{} / {} min", r.modes.join(", "), times.join(", "))
        }
        None => "-".to_string(),
    }
}

fn power_current(p: &Option<washi_core::ivec::PowerTimer>) -> String {
    match p {
        Some(p) => match (p.mode, p.time) {
            (Some(m), Some(t)) => format!("{m}, {t} min"),
            (Some(m), None) => m.to_string(),
            _ => "?".to_string(),
        },
        None => "-".to_string(),
    }
}

fn opt<T: ToString>(v: &Option<T>) -> String {
    v.as_ref().map_or("-".to_string(), ToString::to_string)
}

fn list(v: &[String]) -> String {
    if v.is_empty() { "-".to_string() } else { v.join(", ") }
}

pub fn render_settings(cfg: &DeviceConfig, cap: &DeviceCapability) -> String {
    let silent = cfg.silentmode.as_ref().map_or("-".to_string(), |s| {
        let mode = s.mode.map_or("?".to_string(), |m| m.to_string());
        match (&s.start, &s.end) {
            (Some(a), Some(b)) => format!("{mode} (window {a}-{b})"),
            _ => mode,
        }
    });
    let dry = if cap.drylevel.is_empty() {
        "-".to_string()
    } else {
        format!("{}-{}", cap.drylevel.iter().min().unwrap(), cap.drylevel.iter().max().unwrap())
    };
    let rows: Vec<(&str, String, String)> = vec![
        ("Auto power on", opt(&cfg.autopoweron), list(&cap.autopoweron)),
        ("Auto power off", power_current(&cfg.autopoweroff), power_allowed(&cap.autopoweroff)),
        (
            "Auto power off (no network)",
            power_current(&cfg.autopoweroff_standalone),
            power_allowed(&cap.autopoweroff_standalone),
        ),
        (
            "Quiet mode",
            silent,
            cap.silentmode.as_ref().map_or("-".to_string(), |s| {
                format!("{} (time = daily window, HHMM-HHMM)", s.modes.join(", "))
            }),
        ),
        ("Paper gap", opt(&cfg.papergap), list(&cap.papergap)),
        ("Ink drying time (dry level)", opt(&cfg.drylevel), dry),
        (
            "Auto firmware update",
            opt(&cfg.auto_printer_rom_update),
            list(&cap.auto_printer_rom_update),
        ),
        ("Cloud link (MQTT)", opt(&cfg.mqtt_connection), list(&cap.mqtt_connection)),
    ];
    let w0 = rows.iter().map(|r| r.0.len()).max().unwrap_or(0);
    let w1 = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0).max("Current".len());
    let mut o = String::new();
    let _ = writeln!(o, "{:<w0$}  {:<w1$}  Allowed", "Setting", "Current");
    for (name, cur, allowed) in rows {
        let _ = writeln!(o, "{name:<w0$}  {cur:<w1$}  {allowed}");
    }
    if !cfg.setregistration.is_empty() {
        let _ = writeln!(o, "\nHead alignment values (manual alignment sets)");
        for (id, items) in &cfg.setregistration {
            let vals: Vec<String> = items.iter().map(|(k, v)| format!("{k}={v}")).collect();
            let range = cap
                .setregistration
                .get(id)
                .and_then(|m| m.values().next())
                .map(|r| format!("  [{}..{}]", r.min, r.max))
                .unwrap_or_default();
            let _ = writeln!(o, "  set {id}: {}{range}", vals.join(" "));
        }
    }
    o
}

pub fn render_cap_device(cap: &DeviceCapability) -> String {
    let mut o = String::new();
    let _ =
        writeln!(o, "Device      {} {} (firmware {})", cap.manufacturer, cap.model, cap.firmver);
    let _ = writeln!(o, "Services    {}", cap.services.join(", "));
    let _ = writeln!(o, "\nAllowed settings");
    let _ = writeln!(o, "  autopoweron                {}", list(&cap.autopoweron));
    let _ = writeln!(o, "  autopoweroff               {}", power_allowed(&cap.autopoweroff));
    let _ =
        writeln!(o, "  autopoweroff_standalone    {}", power_allowed(&cap.autopoweroff_standalone));
    let _ = writeln!(
        o,
        "  silentmode                 {}",
        cap.silentmode.as_ref().map_or("-".to_string(), |s| s.modes.join(", "))
    );
    let _ = writeln!(o, "  papergap                   {}", list(&cap.papergap));
    let _ = writeln!(
        o,
        "  drylevel                   {}",
        cap.drylevel.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
    );
    let _ = writeln!(o, "  auto_printer_rom_update    {}", list(&cap.auto_printer_rom_update));
    let _ = writeln!(o, "  mqtt_connection            {}", list(&cap.mqtt_connection));
    for (id, items) in &cap.setregistration {
        let vals: Vec<String> =
            items.iter().map(|(k, r)| format!("{k} {}..{}", r.min, r.max)).collect();
        let _ = writeln!(o, "  setregistration {id}          {}", vals.join(", "));
    }
    o
}

pub fn render_cap_maintenance(cap: &MaintenanceCapability) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "Cleaning");
    for c in &cap.cleaning {
        let _ = writeln!(o, "  {:<8} ink groups: {}", c.kind, c.inkgroups.join(", "));
    }
    let _ = writeln!(o, "\nTest prints");
    for t in &cap.testprint {
        let _ = writeln!(
            o,
            "  {:<20} {} sheet(s) {} {} from {}{}",
            t.kind,
            t.papernum.map_or("?".to_string(), |n| n.to_string()),
            t.papersize.as_deref().map(pretty_size).unwrap_or_default(),
            t.papertype.as_deref().unwrap_or(""),
            t.inputbin.as_deref().map(tray_name).unwrap_or_default(),
            if t.requires_id { "  (needs an id: 1..3)" } else { "" }
        );
    }
    o
}

fn step_label(r: &Request) -> String {
    format!("{} ({})", r.operation(), r.service())
}

/// The dry-run listing: every request as indented XML (whitespace added for display only).
pub fn render_plan(plan: &Plan) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "DRY RUN: nothing was sent to the printer (no network access).");
    let _ = writeln!(o, "Plan: {}", plan.title);
    let _ =
        writeln!(o, "(XML is indented here for reading; it is sent without added whitespace.)\n");
    for (i, step) in plan.steps.iter().enumerate() {
        match step {
            Step::Send(r) => {
                let _ = writeln!(o, "{}. send {}", i + 1, step_label(r));
                let xml = pretty(&r.to_xml()).unwrap_or_else(|_| r.to_xml());
                for line in xml.lines() {
                    let _ = writeln!(o, "     {line}");
                }
            }
            Step::Poll(text) => {
                let _ = writeln!(o, "{}. {text}", i + 1);
            }
        }
        o.push('\n');
    }
    let _ = writeln!(o, "Run again with --confirm to do this for real.");
    o
}

#[cfg(test)]
mod tests {
    use washi_core::ivec::{
        DeviceCapability, DeviceConfig, DeviceStatus, MaintenanceCapability, PrintStatus,
    };
    use washi_core::{Observation, PrintProbe};

    use super::*;

    const PRINT: &str = include_str!("../../washi-ivec/tests/fixtures/GetStatus-print.xml");
    const DEVICE: &str = include_str!("../../washi-ivec/tests/fixtures/GetStatus-device.xml");
    const CAP: &str = include_str!("../../washi-ivec/tests/fixtures/GetCapability-device.xml");
    const CFG: &str = include_str!("../../washi-ivec/tests/fixtures/GetConfiguration-device.xml");
    const MCAP: &str =
        include_str!("../../washi-ivec/tests/fixtures/GetCapability-maintenance.xml");

    #[test]
    fn bars() {
        assert_eq!(ink_bar(Some(0), 10), "░░░░░░░░░░");
        assert_eq!(ink_bar(Some(100), 10), "██████████");
        assert_eq!(ink_bar(Some(60), 10), "██████░░░░");
        assert_eq!(ink_bar(Some(5), 10), "█░░░░░░░░░");
        assert_eq!(ink_bar(None, 4), "????");
        assert_eq!(ink_bar(Some(250), 4), "████");
    }

    #[test]
    fn sizes() {
        assert_eq!(pretty_size("iso_a4_210x297mm"), "A4 (210x297 mm)");
        assert_eq!(pretty_size("custom_canon_89x127mm"), "custom canon (89x127 mm)");
        assert_eq!(pretty_size("weird"), "weird");
    }

    fn awake() -> PrinterSnapshot {
        PrinterSnapshot::compose(
            Observation {
                print: PrintProbe::Awake(PrintStatus::parse(PRINT).unwrap()),
                device_status: Some(DeviceStatus::parse(DEVICE).unwrap()),
                device_cap: Some(DeviceCapability::parse(CAP).unwrap()),
                device_cap_current: false,
                ipp: None,
                warnings: vec![],
            },
            None,
            1000,
        )
    }

    #[test]
    fn status_text() {
        let s = render_snapshot("printer.local", &awake(), 1000);
        assert!(s.contains("Power     on"), "{s}");
        assert!(s.contains("State     idle"), "{s}");
        assert!(!s.contains("auto power-off"), "{s}");
        assert!(s.contains("Magenta") && s.contains("60%") && s.contains("XKI-N21<M>SETUP"), "{s}");
        assert!(s.contains("Rear tray") && s.contains("Cassette"), "{s}");
        assert!(s.contains("A4 (210x297 mm), stationery"), "{s}");
        assert!(s.contains("XK140 series (firmware 1.060)"), "{s}");
        // Ink order: M, BK, Y, PGBK, C as reported.
        let m = s.find("Magenta").unwrap();
        let pg = s.find("Pigment black").unwrap();
        assert!(m < pg);
    }

    #[test]
    fn sleeping_status_text_explains_auto_power_off() {
        let mut snap = awake();
        snap.power = Power::Sleeping;
        snap.ink_source = InkSource::Ipp;
        snap.from_cache = vec!["trays".into()];
        snap.last_awake = Some(1000);
        snap.inks[3].status = InkStatus::Low;
        snap.inks[4].level = None;
        snap.inks[4].status = InkStatus::Unknown;
        let s = render_snapshot("printer.local", &snap, 1000 + 7200);
        assert!(s.contains("Power     sleeping"), "{s}");
        assert!(s.contains("Printer is in auto power-off; it wakes up when a job arrives."), "{s}");
        assert!(s.contains("State     idle (reported over IPP)"), "{s}");
        assert!(s.contains("Awake     last seen 2 h ago"), "{s}");
        assert!(s.contains("Ink (IPP marker levels)"), "{s}");
        assert!(s.contains("Paper (last known)"), "{s}");
        assert!(s.contains("  low"), "{s}");
        assert!(s.contains("level unknown"), "{s}");
    }

    #[test]
    fn offline_status_text() {
        let mut snap = awake();
        snap.power = Power::Offline;
        snap.state = None;
        snap.ink_source = InkSource::Cache;
        let s = render_snapshot("printer.local", &snap, 1030);
        assert!(s.contains("Power     offline"), "{s}");
        assert!(s.contains("cannot be reached"), "{s}");
        assert!(s.contains("Ink (last known; printer not answering)"), "{s}");
        assert!(s.contains("Awake     last seen just now"), "{s}");
    }

    #[test]
    fn a_support_code_shows_its_severity_and_message() {
        let mut snap = awake();
        snap.support = washi_core::Support::from_code("1000");
        let s = render_snapshot("printer.local", &snap, 1030);
        assert!(s.contains("Support   code 1000 (warning): The rear tray is out of paper"), "{s}");
    }

    #[test]
    fn watch_line_summarises_a_snapshot() {
        let mut snap = awake();
        snap.inks[0].status = InkStatus::Low;
        snap.inks[2].status = InkStatus::Empty;
        snap.support = washi_core::Support::from_code("1300");
        let l = watch_line(&snap, "12:00:01");
        assert!(l.starts_with("[12:00:01] on"), "{l}");
        assert!(l.contains("M=60%!") && l.contains("Y=70%!!") && l.contains("C=60%"), "{l}");
        assert!(l.contains("error=1300"), "{l}");
    }

    #[test]
    fn ago() {
        assert_eq!(fmt_ago(5), "just now");
        assert_eq!(fmt_ago(300), "5 min ago");
        assert_eq!(fmt_ago(7300), "2 h ago");
        assert_eq!(fmt_ago(200_000), "2 d ago");
    }

    #[test]
    fn info_text() {
        let s = render_info(
            &DeviceCapability::parse(CAP).unwrap(),
            &DeviceStatus::parse(DEVICE).unwrap(),
        );
        assert!(s.contains("Firmware    1.060"), "{s}");
        assert!(s.contains("54"), "{s}");
        assert!(!s.contains("XXXXXXXXX"));
    }

    #[test]
    fn settings_text() {
        let s = render_settings(
            &DeviceConfig::parse(CFG).unwrap(),
            &DeviceCapability::parse(CAP).unwrap(),
        );
        assert!(s.contains("Auto power off"), "{s}");
        assert!(s.contains("ON, 30 min"), "{s}");
        assert!(s.contains("OFF (window 2100-0700)"), "{s}");
        assert!(s.contains("1-5"), "{s}");
        assert!(s.contains("set 1: A=0 B=0"), "{s}");
    }

    #[test]
    fn maintenance_caps_text() {
        let s = render_cap_maintenance(&MaintenanceCapability::parse(MCAP).unwrap());
        assert!(s.contains("deep") && s.contains("group1"), "{s}");
        assert!(s.contains("manual_registration") && s.contains("needs an id"), "{s}");
    }

    #[test]
    fn plan_text_shows_every_request() {
        let plan = washi_core::MaintOp::NozzleCheck.plan().unwrap();
        let s = render_plan(&plan);
        assert!(s.starts_with("DRY RUN"));
        assert!(
            s.contains("send StartJob (maintenance)")
                && s.contains("send TestPrint (maintenance)")
                && s.contains("send EndJob (maintenance)")
        );
        assert!(s.contains("<ivec:type>nozzle_check</ivec:type>"), "{s}");
        assert!(s.contains("--confirm"));
    }

    #[test]
    fn durations() {
        assert_eq!(fmt_duration(Duration::from_secs(9)), "9s");
        assert_eq!(fmt_duration(Duration::from_secs(125)), "2m05s");
    }
}
