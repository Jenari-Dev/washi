// SPDX-License-Identifier: GPL-2.0-or-later
//! [`PrinterSnapshot`]: one combined, serialisable picture of a printer.
//!
//! The printer auto-powers-off. While it sleeps the IVEC print/maintenance services answer
//! `NG Suspended`, but `GetStatus device` and IPP keep working. A snapshot therefore merges
//! whatever answered:
//!
//! * IVEC `GetStatus print` (when awake): state, support code, trays, ink tanks with models;
//! * IPP `marker-levels` (when asleep): ink levels, mapped onto the IVEC colour codes, with the
//!   tank models remembered from the last awake read;
//! * IVEC `GetStatus device` / `GetCapability device`: counters and device information;
//! * the previous snapshot (the cache file): anything nothing answered for, flagged in
//!   [`PrinterSnapshot::from_cache`].
//!
//! [`PrinterSnapshot::compose`] is pure; [`Printer::observe`](crate::Printer::observe) and
//! [`Printer::snapshot`](crate::Printer::snapshot) do the I/O.

use serde::{Deserialize, Serialize};
use washi_ivec::{DeviceCapability, DeviceStatus, Ink, InkIcon, PrintStatus, Tray};
use washi_net::ipp::{Marker, PrinterAttributes};

use crate::firmware::FirmwareUpdate;
use crate::lang::Language;
use crate::support::{Severity, classify};

/// Shown when the printer sleeps.
pub const SLEEPING_MESSAGE: &str = "Printer is in auto power-off; it wakes up when a job arrives.";
/// Shown when nothing answers.
pub const OFFLINE_MESSAGE: &str = "Printer cannot be reached on the network.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Power {
    /// IVEC print service answers (idle, printing, error, ...).
    On,
    /// Auto power-off: IVEC print service is `Suspended`; IPP and device status still answer.
    Sleeping,
    /// Nothing answers.
    #[default]
    Offline,
}

impl Power {
    pub fn as_str(self) -> &'static str {
        match self {
            Power::On => "on",
            Power::Sleeping => "sleeping",
            Power::Offline => "offline",
        }
    }

    /// Stable key for translation tables.
    pub fn message_key(self) -> &'static str {
        match self {
            Power::On => "power.on",
            Power::Sleeping => "power.sleeping",
            Power::Offline => "power.offline",
        }
    }

    /// English fallback text for [`Power::message_key`].
    pub fn message(self) -> &'static str {
        match self {
            Power::On => "Printer is on.",
            Power::Sleeping => SLEEPING_MESSAGE,
            Power::Offline => OFFLINE_MESSAGE,
        }
    }
}

/// How the printer is attached to the network, from IVEC `GetStatus device`
/// (`network_interface`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Connection {
    /// A wireless interface in `infrastructure_mode`.
    Wifi,
    /// A wired interface.
    Lan,
    /// The printer did not say (or has not been read yet).
    #[default]
    Unknown,
}

impl Connection {
    pub fn from_kind(kind: Option<&str>) -> Connection {
        match kind {
            Some("wifi") => Connection::Wifi,
            Some("lan") => Connection::Lan,
            _ => Connection::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Connection::Wifi => "wifi",
            Connection::Lan => "lan",
            Connection::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InkStatus {
    Ok,
    Low,
    Empty,
    #[default]
    Unknown,
}

/// Where the ink levels of a snapshot came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InkSource {
    /// IVEC `marker_info` (printer awake).
    Ivec,
    /// IPP `marker-levels` (printer asleep).
    Ipp,
    /// The previous snapshot; nothing answered.
    Cache,
    #[default]
    None,
}

/// One ink tank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InkLevel {
    /// `PGBK`, `BK`, `C`, `M`, `Y` (or the IPP marker name for tanks Washi does not know).
    pub color: String,
    /// Tank model such as `XKI-N21<M>SETUP`; remembered from the last awake read.
    pub model: Option<String>,
    /// Percent remaining (the printer reports 10 % steps).
    pub level: Option<u8>,
    pub status: InkStatus,
    /// `#RRGGBB` swatch colour (IPP `marker-colors`), when known.
    pub rgb: Option<String>,
    /// Display order reported by the printer.
    pub order: Option<u32>,
}

/// A printer error: the Canon "support code" with the family it belongs to (see
/// [`crate::support`]), a severity and an English text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Support {
    pub code: String,
    /// `support.<family>`, e.g. `support.paper-jam`; look this up in a translation table.
    pub message_key: String,
    /// English text of the family (it names the code when the family is unknown).
    pub message: String,
    /// Part of the family: informational notices do not turn the printer into an error.
    #[serde(default)]
    pub severity: Severity,
}

impl Support {
    /// `None` for an empty code.
    pub fn from_code(code: &str) -> Option<Support> {
        let code = code.trim();
        if code.is_empty() {
            return None;
        }
        let info = classify(code);
        Some(Support {
            code: code.to_string(),
            message_key: info.key.to_string(),
            message: info.text(Language::En, code),
            severity: info.severity,
        })
    }

    /// The message in a language.
    pub fn text(&self, lang: Language) -> String {
        classify(&self.code).text(lang, &self.code)
    }
}

/// Model, firmware and services (no serial numbers).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub services: Vec<String>,
}

/// The combined picture. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PrinterSnapshot {
    pub power: Power,
    /// IVEC status (`idle`, ...) when awake, else the IPP `printer-state`.
    pub state: Option<String>,
    /// IPP `printer-state-reasons`, when IPP was queried.
    pub state_reasons: Vec<String>,
    /// Current error, when the printer reports a support code (awake only).
    pub support: Option<Support>,
    pub inks: Vec<InkLevel>,
    pub ink_source: InkSource,
    pub trays: Vec<Tray>,
    pub device: Option<DeviceInfo>,
    /// Lifetime counters (`GetStatus device`).
    pub counters: Option<DeviceStatus>,
    /// Whether the printer offers a firmware update (`networkupdate_available` of
    /// `GetStatus device`; remembered while the printer is not answering).
    pub firmware_update: FirmwareUpdate,
    /// `firmware_update` is what the printer said the last time it was on, not a reading made
    /// now: a printer in auto power-off does not check for updates and says `unavailable`
    /// whatever is true, so that answer is not used while it sleeps.
    pub firmware_update_remembered: bool,
    /// `wifi`, `lan` or `unknown` (`network_interface` of `GetStatus device`; remembered while
    /// the printer is not answering).
    pub connection: Connection,
    /// The printer's IPv4 address as this computer resolves its host name (display only; the
    /// host name stays the key everywhere).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Fields (`inks`, `trays`, `device`, `counters`) that nothing answered for and that were
    /// carried over from the previous snapshot.
    pub from_cache: Vec<String>,
    /// Non-fatal problems while reading (never contain host names or identifiers).
    pub warnings: Vec<String>,
    /// What the daemon is doing with the printer right now (e.g. a maintenance job).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
    /// Unix seconds of the last time the IVEC print service answered.
    pub last_awake: Option<u64>,
    /// Unix seconds of this reading.
    pub last_updated: u64,
}

impl PrinterSnapshot {
    /// Equal apart from the timestamps (`last_updated`, and `last_awake` while the printer is on,
    /// where it just tracks "now"). Used to decide whether anything changed.
    pub fn content_eq(&self, other: &PrinterSnapshot) -> bool {
        let mut a = self.clone();
        a.last_updated = other.last_updated;
        if a.power == Power::On && other.power == Power::On {
            a.last_awake = other.last_awake;
        }
        a == *other
    }

    /// The ink tank of a colour code.
    pub fn ink(&self, color: &str) -> Option<&InkLevel> {
        self.inks.iter().find(|i| i.color == color)
    }
}

/// What the IVEC print service said.
#[derive(Debug, Clone)]
pub enum PrintProbe {
    Awake(PrintStatus),
    /// `NG Suspended`: auto power-off.
    Suspended,
    /// `NG DeviceBusy`.
    Busy,
    /// Any other `NG` (the detail, or a short description of a garbled reply).
    Refused(String),
    /// No TCP connection / no reply (a short, identifier-free reason).
    Unreachable(String),
}

/// Everything one refresh learned, before merging.
#[derive(Debug, Clone)]
pub struct Observation {
    pub print: PrintProbe,
    pub device_status: Option<DeviceStatus>,
    pub device_cap: Option<DeviceCapability>,
    /// `device_cap` is `None` because the cached device information was recent enough, not
    /// because reading it failed. The cached copy then does not count as stale.
    pub device_cap_current: bool,
    pub ipp: Option<PrinterAttributes>,
    pub warnings: Vec<String>,
}

/// IPP `marker-names` to IVEC colour code: `Black(PGBK)` -> `PGBK`, `Black(BK)` -> `BK`,
/// `Cyan` -> `C`, `Magenta` -> `M`, `Yellow` -> `Y`.
pub fn ink_code_from_marker(name: &str) -> Option<&'static str> {
    let n = name.trim().to_ascii_lowercase();
    if n.contains("pgbk") || n.contains("pigment") {
        Some("PGBK")
    } else if n.contains("(bk)") || n == "black" || n == "bk" {
        Some("BK")
    } else if n.contains("cyan") {
        Some("C")
    } else if n.contains("magenta") {
        Some("M")
    } else if n.contains("yellow") {
        Some("Y")
    } else {
        None
    }
}

/// Display name of an ink colour code.
pub fn ink_display_name(code: &str) -> &str {
    match code {
        "BK" => "Black",
        "PGBK" => "Pigment black",
        "C" => "Cyan",
        "M" => "Magenta",
        "Y" => "Yellow",
        other => other,
    }
}

fn status_from_ivec(icon: InkIcon, level: Option<u8>) -> InkStatus {
    match icon {
        InkIcon::Error => InkStatus::Empty,
        InkIcon::Warning => {
            if level == Some(0) {
                InkStatus::Empty
            } else {
                InkStatus::Low
            }
        }
        InkIcon::None => {
            if level == Some(0) {
                InkStatus::Empty
            } else {
                InkStatus::Ok
            }
        }
        InkIcon::Unknown => InkStatus::Unknown,
    }
}

fn status_from_marker(m: &Marker) -> InkStatus {
    match (m.level, m.low_level) {
        (None, _) => InkStatus::Unknown,
        (Some(0), _) => InkStatus::Empty,
        (Some(l), Some(low)) if l <= low => InkStatus::Low,
        _ => InkStatus::Ok,
    }
}

fn ink_from_ivec(i: &Ink) -> InkLevel {
    InkLevel {
        color: i.color.clone(),
        model: Some(i.model.clone()).filter(|m| !m.is_empty()),
        level: i.level,
        status: status_from_ivec(i.icon, i.level),
        rgb: None,
        order: i.order,
    }
}

/// IPP markers as ink tanks; tank model and display order are taken from the previous snapshot.
fn inks_from_ipp(markers: &[Marker], prev: &[InkLevel]) -> Vec<InkLevel> {
    markers
        .iter()
        .enumerate()
        .map(|(idx, m)| {
            let color = ink_code_from_marker(&m.name).unwrap_or(&m.name).to_string();
            let known = prev.iter().find(|p| p.color == color);
            InkLevel {
                model: known.and_then(|p| p.model.clone()),
                level: m.level,
                status: status_from_marker(m),
                rgb: m.color.clone(),
                order: known.and_then(|p| p.order).or(u32::try_from(idx + 1).ok()),
                color,
            }
        })
        .collect()
}

fn ipp_state(ipp: Option<&PrinterAttributes>) -> Option<String> {
    ipp?.state.map(|s| s.as_str().to_string())
}

/// What an earlier snapshot knew about firmware updates that can still be relied on: the value
/// and whether it was itself remembered. A printer that was asleep and said `unavailable` is not
/// relied on (snapshots kept by older versions hold that).
pub(crate) fn carried_update(prev: Option<&PrinterSnapshot>) -> Option<(FirmwareUpdate, bool)> {
    let p = prev?;
    match p.firmware_update {
        FirmwareUpdate::Unknown => None,
        FirmwareUpdate::Unavailable
            if p.power == Power::Sleeping && !p.firmware_update_remembered =>
        {
            None
        }
        v => Some((v, p.firmware_update_remembered)),
    }
}

/// The firmware update state of a new snapshot and whether it is remembered. A printer in auto
/// power-off does not check for updates: it answers `unavailable` while an update is really on
/// offer (seen on a real XK140). So a sleeping printer's `unavailable` (or silence) is ignored
/// and the value from the last time it was on is kept, `Unknown` when there is none. `available`
/// can be trusted whenever it is read. `fresh` says whether the counters were read now (else
/// they are carried over and so is the state).
fn firmware_update_of(
    power: Power,
    fresh: bool,
    reported: Option<&str>,
    prev: Option<&PrinterSnapshot>,
) -> (FirmwareUpdate, bool) {
    if !fresh {
        return carried_update(prev).unwrap_or((FirmwareUpdate::Unknown, false));
    }
    let now = FirmwareUpdate::from_reported(reported);
    if power == Power::Sleeping && now != FirmwareUpdate::Available {
        return match carried_update(prev) {
            Some((v, _)) => (v, true),
            None => (FirmwareUpdate::Unknown, false),
        };
    }
    (now, false)
}

impl PrinterSnapshot {
    /// Merge one [`Observation`] with the previous snapshot (`prev`, usually from the cache).
    pub fn compose(obs: Observation, prev: Option<&PrinterSnapshot>, now: u64) -> PrinterSnapshot {
        let Observation { print, device_status, device_cap, device_cap_current, ipp, mut warnings } =
            obs;
        let mut from_cache: Vec<String> = Vec::new();

        let mut state: Option<String> = None;
        let mut support: Option<Support> = None;
        let mut fresh_trays: Vec<Tray> = Vec::new();
        let mut ivec_inks: Vec<Ink> = Vec::new();
        let mut ivec_answered = true;
        let power = match print {
            PrintProbe::Awake(ps) => {
                state = Some(ps.status.clone()).filter(|s| !s.is_empty());
                support = Support::from_code(&ps.support_code);
                fresh_trays = ps.trays;
                ivec_inks = ps.inks;
                Power::On
            }
            PrintProbe::Busy => {
                state = Some("busy".to_string());
                Power::On
            }
            PrintProbe::Refused(detail) => {
                warnings.push(format!("the printer refused GetStatus print: {detail}"));
                Power::On
            }
            PrintProbe::Suspended => {
                ivec_answered = false;
                Power::Sleeping
            }
            PrintProbe::Unreachable(why) => {
                ivec_answered = false;
                if ipp.is_some() {
                    warnings.push(format!("CHMP is not answering ({why}); IPP is"));
                    Power::On
                } else {
                    Power::Offline
                }
            }
        };
        if state.is_none() && power != Power::Offline {
            state = ipp_state(ipp.as_ref());
        }

        // Inks.
        let prev_inks: &[InkLevel] = prev.map_or(&[], |p| p.inks.as_slice());
        let (inks, ink_source) = if !ivec_inks.is_empty() {
            (ivec_inks.iter().map(ink_from_ivec).collect(), InkSource::Ivec)
        } else if let Some(markers) = ipp.as_ref().map(|a| &a.markers).filter(|m| !m.is_empty()) {
            (inks_from_ipp(markers, prev_inks), InkSource::Ipp)
        } else if !prev_inks.is_empty() {
            from_cache.push("inks".to_string());
            (prev_inks.to_vec(), InkSource::Cache)
        } else {
            (Vec::new(), InkSource::None)
        };

        // Trays: only the print service reports them.
        let trays = if !fresh_trays.is_empty() {
            fresh_trays
        } else if let Some(p) = prev.filter(|p| !p.trays.is_empty()) {
            from_cache.push("trays".to_string());
            p.trays.clone()
        } else {
            Vec::new()
        };

        // Device information.
        let device = if let Some(cap) = device_cap {
            Some(DeviceInfo {
                manufacturer: cap.manufacturer,
                model: cap.model,
                firmware: cap.firmver,
                services: cap.services,
            })
        } else if let Some(d) = prev.and_then(|p| p.device.clone()) {
            if !device_cap_current {
                from_cache.push("device".to_string());
            }
            Some(d)
        } else {
            ipp.as_ref().and_then(|a| {
                (a.make_and_model.is_some() || a.firmware_version.is_some()).then(|| DeviceInfo {
                    manufacturer: String::new(),
                    model: a.make_and_model.clone().unwrap_or_default(),
                    firmware: a.firmware_version.clone().unwrap_or_default(),
                    services: Vec::new(),
                })
            })
        };

        // Counters.
        let fresh_counters = device_status.is_some();
        let counters = if device_status.is_some() {
            device_status
        } else if let Some(c) = prev.and_then(|p| p.counters.clone()) {
            from_cache.push("counters".to_string());
            Some(c)
        } else {
            None
        };

        let (firmware_update, firmware_update_remembered) = firmware_update_of(
            power,
            fresh_counters,
            counters.as_ref().and_then(|c| c.networkupdate_available.as_deref()),
            prev,
        );

        let last_awake = if power == Power::On && ivec_answered {
            Some(now)
        } else {
            prev.and_then(|p| p.last_awake)
        };

        PrinterSnapshot {
            power,
            state,
            state_reasons: ipp.as_ref().map(|a| a.state_reasons.clone()).unwrap_or_default(),
            support,
            inks,
            ink_source,
            trays,
            device,
            connection: Connection::from_kind(
                counters.as_ref().and_then(|c| c.connection.as_deref()),
            ),
            firmware_update,
            firmware_update_remembered,
            counters,
            address: prev.and_then(|p| p.address.clone()),
            from_cache,
            warnings,
            activity: None,
            last_awake,
            last_updated: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use washi_net::ipp::PrinterState;

    use super::*;

    const PRINT: &str = include_str!("../../washi-ivec/tests/fixtures/GetStatus-print.xml");
    const DEVICE: &str = include_str!("../../washi-ivec/tests/fixtures/GetStatus-device.xml");
    const CAP: &str = include_str!("../../washi-ivec/tests/fixtures/GetCapability-device.xml");

    fn awake_obs() -> Observation {
        Observation {
            print: PrintProbe::Awake(PrintStatus::parse(PRINT).unwrap()),
            device_status: Some(DeviceStatus::parse(DEVICE).unwrap()),
            device_cap: Some(DeviceCapability::parse(CAP).unwrap()),
            device_cap_current: false,
            ipp: None,
            warnings: vec![],
        }
    }

    fn marker(name: &str, level: Option<u8>, color: &str) -> Marker {
        Marker {
            name: name.into(),
            color: Some(color.into()),
            kind: Some("ink-cartridge".into()),
            level,
            low_level: Some(15),
            high_level: Some(100),
        }
    }

    /// Markers in the XK140's IPP order, with levels different from the IVEC fixture.
    fn ipp_attrs(levels: [Option<u8>; 5]) -> PrinterAttributes {
        let names = [
            ("Magenta", "#F200FF"),
            ("Black(BK)", "#000000"),
            ("Yellow", "#FFDA00"),
            ("Black(PGBK)", "#000000"),
            ("Cyan", "#00CFFF"),
        ];
        PrinterAttributes {
            state: Some(PrinterState::Idle),
            state_reasons: vec!["none".into()],
            markers: names.iter().zip(levels).map(|((n, c), l)| marker(n, l, c)).collect(),
            make_and_model: Some("Canon XK140 series".into()),
            firmware_version: Some("1.060".into()),
            ..PrinterAttributes::default()
        }
    }

    fn sleeping_obs(levels: [Option<u8>; 5]) -> Observation {
        Observation {
            print: PrintProbe::Suspended,
            device_status: Some(DeviceStatus::parse(DEVICE).unwrap()),
            device_cap: None,
            device_cap_current: false,
            ipp: Some(ipp_attrs(levels)),
            warnings: vec![],
        }
    }

    #[test]
    fn marker_names_map_to_ivec_colour_codes() {
        for (name, code) in [
            ("Black(PGBK)", Some("PGBK")),
            ("Black(BK)", Some("BK")),
            ("black", Some("BK")),
            ("Cyan", Some("C")),
            ("Magenta", Some("M")),
            ("Yellow", Some("Y")),
            ("Photo Gray", None),
        ] {
            assert_eq!(ink_code_from_marker(name), code, "{name}");
        }
    }

    #[test]
    fn awake_snapshot_uses_ivec_everywhere() {
        let s = PrinterSnapshot::compose(awake_obs(), None, 1000);
        assert_eq!(s.power, Power::On);
        assert_eq!(s.state.as_deref(), Some("idle"));
        assert_eq!(s.support, None);
        assert_eq!(s.ink_source, InkSource::Ivec);
        assert_eq!(s.inks.len(), 5);
        let m = s.ink("M").unwrap();
        assert_eq!((m.level, m.status, m.order), (Some(60), InkStatus::Ok, Some(1)));
        assert_eq!(m.model.as_deref(), Some("XKI-N21<M>SETUP"));
        assert_eq!(s.trays.len(), 2);
        assert_eq!(s.device.as_ref().unwrap().model, "XK140 series");
        assert_eq!(s.device.as_ref().unwrap().firmware, "1.060");
        assert_eq!(s.counters.as_ref().unwrap().total_paper_num["inputbin_p2"], 54);
        assert_eq!(s.connection, Connection::Wifi);
        assert_eq!(s.address, None, "the address is resolved by Printer::snapshot, not compose");
        assert!(s.from_cache.is_empty() && s.warnings.is_empty());
        assert_eq!((s.last_awake, s.last_updated), (Some(1000), 1000));
    }

    #[test]
    fn sleeping_snapshot_takes_ink_from_ipp_and_models_from_the_cache() {
        let awake = PrinterSnapshot::compose(awake_obs(), None, 1000);
        let asleep = PrinterSnapshot::compose(
            sleeping_obs([Some(50), Some(80), Some(70), Some(60), Some(40)]),
            Some(&awake),
            2000,
        );
        assert_eq!(asleep.power, Power::Sleeping);
        assert_eq!(asleep.state.as_deref(), Some("idle"), "IPP state while asleep");
        assert_eq!(asleep.state_reasons, ["none"]);
        assert_eq!(asleep.ink_source, InkSource::Ipp);
        let colors: Vec<_> = asleep.inks.iter().map(|i| i.color.as_str()).collect();
        assert_eq!(colors, ["M", "BK", "Y", "PGBK", "C"]);
        let c = asleep.ink("C").unwrap();
        assert_eq!((c.level, c.rgb.as_deref()), (Some(40), Some("#00CFFF")));
        assert_eq!(c.model.as_deref(), Some("XKI-N21<C>SETUP"), "tank model remembered");
        assert_eq!(asleep.ink("PGBK").unwrap().model.as_deref(), Some("XKI-N20<PGBK>SETUP"));
        assert_eq!(asleep.ink("PGBK").unwrap().order, Some(4));
        // Trays and device info come from the cache; the counters were read fresh.
        assert_eq!(asleep.trays, awake.trays);
        assert_eq!(asleep.device, awake.device);
        assert_eq!(asleep.from_cache, ["trays", "device"]);
        assert_eq!(asleep.last_awake, Some(1000), "last awake time is carried over");
        assert_eq!(asleep.last_updated, 2000);
        assert_eq!(asleep.support, None);
    }

    #[test]
    fn recent_cached_device_info_is_not_flagged_as_stale() {
        let awake = PrinterSnapshot::compose(awake_obs(), None, 1000);
        let obs = Observation {
            device_cap: None,
            device_cap_current: true,
            ..sleeping_obs([Some(60); 5])
        };
        let s = PrinterSnapshot::compose(obs, Some(&awake), 1100);
        assert_eq!(s.device, awake.device);
        assert_eq!(s.from_cache, ["trays"]);
    }

    #[test]
    fn sleeping_without_a_cache_still_reports_ink_and_basic_device_info() {
        let s = PrinterSnapshot::compose(
            sleeping_obs([Some(60), Some(80), Some(70), Some(60), Some(60)]),
            None,
            5,
        );
        assert_eq!(s.power, Power::Sleeping);
        assert_eq!(s.inks.len(), 5);
        assert!(s.inks.iter().all(|i| i.model.is_none()), "models unknown until seen awake");
        assert_eq!(s.inks[2].order, Some(3), "IPP order is used when nothing is cached");
        assert_eq!(s.device.as_ref().unwrap().model, "Canon XK140 series");
        assert_eq!(s.device.as_ref().unwrap().firmware, "1.060");
        assert_eq!(s.last_awake, None);
    }

    #[test]
    fn ink_status_from_both_sources() {
        // IVEC: the printer's icon decides.
        let mut obs = awake_obs();
        if let PrintProbe::Awake(ps) = &mut obs.print {
            ps.inks[0].icon = InkIcon::Warning;
            ps.inks[1].icon = InkIcon::Error;
            ps.inks[2].level = Some(0);
        }
        let s = PrinterSnapshot::compose(obs, None, 1);
        let st = |c: &str| s.ink(c).unwrap().status;
        assert_eq!(
            (st("M"), st("BK"), st("Y"), st("C")),
            (InkStatus::Low, InkStatus::Empty, InkStatus::Empty, InkStatus::Ok)
        );
        // IPP: level against the low-level threshold.
        let s = PrinterSnapshot::compose(
            sleeping_obs([Some(10), Some(15), Some(0), None, Some(16)]),
            None,
            1,
        );
        let st = |c: &str| s.ink(c).unwrap().status;
        assert_eq!(st("M"), InkStatus::Low);
        assert_eq!(st("BK"), InkStatus::Low, "at the threshold counts as low");
        assert_eq!(st("Y"), InkStatus::Empty);
        assert_eq!(st("PGBK"), InkStatus::Unknown);
        assert_eq!(st("C"), InkStatus::Ok);
    }

    #[test]
    fn support_code_becomes_message_key_and_text() {
        let mut obs = awake_obs();
        if let PrintProbe::Awake(ps) = &mut obs.print {
            ps.support_code = "1300".into();
        }
        let s = PrinterSnapshot::compose(obs, None, 1);
        let sup = s.support.unwrap();
        assert_eq!(sup.code, "1300");
        assert_eq!(sup.message_key, "support.paper-jam");
        assert_eq!(sup.severity, Severity::Error);
        assert!(sup.message.contains("jammed"), "{}", sup.message);
        assert!(sup.text(Language::Ja).contains("つまって"));

        let unknown = Support::from_code("9999").unwrap();
        assert_eq!(unknown.message_key, "support.unknown");
        assert_eq!(unknown.severity, Severity::Warning);
        assert!(unknown.message.contains("9999") && unknown.message.contains("support"));
        assert_eq!(Support::from_code("  "), None);
    }

    #[test]
    fn offline_keeps_the_last_known_data_from_the_cache() {
        let awake = PrinterSnapshot::compose(awake_obs(), None, 1000);
        let off = PrinterSnapshot::compose(
            Observation {
                print: PrintProbe::Unreachable("connection failed".into()),
                device_status: None,
                device_cap: None,
                device_cap_current: false,
                ipp: None,
                warnings: vec!["IPP is not answering either".into()],
            },
            Some(&awake),
            3000,
        );
        assert_eq!(off.power, Power::Offline);
        assert_eq!(off.state, None);
        assert_eq!(off.ink_source, InkSource::Cache);
        assert_eq!(off.inks, awake.inks);
        assert_eq!(off.trays, awake.trays);
        assert_eq!(off.counters, awake.counters);
        assert_eq!(off.from_cache, ["inks", "trays", "device", "counters"]);
        assert_eq!(off.last_awake, Some(1000));
        assert_eq!(off.connection, Connection::Wifi, "the link type is remembered");
        assert_eq!(off.last_updated, 3000);
        assert_eq!(off.support, None, "an unknown error state is not guessed");

        let nothing = PrinterSnapshot::compose(
            Observation {
                print: PrintProbe::Unreachable("x".into()),
                device_status: None,
                device_cap: None,
                device_cap_current: false,
                ipp: None,
                warnings: vec![],
            },
            None,
            1,
        );
        assert_eq!(nothing.power, Power::Offline);
        assert_eq!(nothing.ink_source, InkSource::None);
        assert!(nothing.inks.is_empty() && nothing.device.is_none());
    }

    #[test]
    fn chmp_down_but_ipp_up_counts_as_on_with_a_warning() {
        let s = PrinterSnapshot::compose(
            Observation {
                print: PrintProbe::Unreachable("timeout".into()),
                device_status: None,
                device_cap: None,
                device_cap_current: false,
                ipp: Some(ipp_attrs([Some(60); 5])),
                warnings: vec![],
            },
            None,
            9,
        );
        assert_eq!(s.power, Power::On);
        assert_eq!(s.ink_source, InkSource::Ipp);
        assert_eq!(s.last_awake, None, "IVEC never answered");
        assert!(s.warnings[0].contains("CHMP is not answering"), "{:?}", s.warnings);
    }

    #[test]
    fn busy_and_refused_print_service_are_on() {
        let busy = PrinterSnapshot::compose(
            Observation { print: PrintProbe::Busy, ..awake_obs() },
            None,
            1,
        );
        assert_eq!((busy.power, busy.state.as_deref()), (Power::On, Some("busy")));
        assert_eq!(busy.ink_source, InkSource::None, "no IPP, no cache: no ink");
        let with_ipp = PrinterSnapshot::compose(
            Observation {
                print: PrintProbe::Busy,
                ipp: Some(ipp_attrs([Some(60); 5])),
                ..awake_obs()
            },
            None,
            1,
        );
        assert_eq!(with_ipp.ink_source, InkSource::Ipp);
        let refused = PrinterSnapshot::compose(
            Observation { print: PrintProbe::Refused("DataError".into()), ..awake_obs() },
            None,
            1,
        );
        assert_eq!(refused.power, Power::On);
        assert!(refused.warnings[0].contains("DataError"));
    }

    #[test]
    fn content_eq_ignores_only_the_timestamp() {
        let a = PrinterSnapshot::compose(awake_obs(), None, 10);
        let b = PrinterSnapshot::compose(awake_obs(), None, 99);
        assert!(a.content_eq(&b) && b.content_eq(&a));
        let mut c = b.clone();
        c.inks[0].level = Some(10);
        assert!(!a.content_eq(&c));
        // While asleep, `last_awake` is data, not "now".
        let mut d = a.clone();
        let mut e = a.clone();
        d.power = Power::Sleeping;
        e.power = Power::Sleeping;
        e.last_awake = Some(1);
        assert!(!d.content_eq(&e));
    }

    #[test]
    fn json_shape_is_stable_and_round_trips() {
        let s = PrinterSnapshot::compose(awake_obs(), None, 42);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["power"], "on");
        assert_eq!(json["ink_source"], "ivec");
        assert_eq!(json["inks"][0]["status"], "ok");
        assert_eq!(json["last_updated"], 42);
        assert!(json.get("activity").is_none(), "unset activity is omitted");
        let back: PrinterSnapshot = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
        // Unknown / missing keys from another version are tolerated.
        let sparse: PrinterSnapshot =
            serde_json::from_str(r#"{"power":"sleeping","future":1}"#).unwrap();
        assert_eq!(sparse.power, Power::Sleeping);
    }

    #[test]
    fn snapshots_do_not_carry_identifiers() {
        let s = PrinterSnapshot::compose(awake_obs(), None, 1);
        let json = serde_json::to_string(&s).unwrap().to_lowercase();
        for needle in ["redacted", "gci", "serial", "snma"] {
            assert!(!json.contains(needle), "snapshot JSON mentions {needle}");
        }
    }

    #[test]
    fn connection_json_and_address_carry_over() {
        let mut awake = PrinterSnapshot::compose(awake_obs(), None, 1);
        awake.address = Some("192.0.2.7".into());
        let json = serde_json::to_value(&awake).unwrap();
        assert_eq!(
            (json["connection"].as_str(), json["address"].as_str()),
            (Some("wifi"), Some("192.0.2.7"))
        );
        let off = PrinterSnapshot::compose(
            Observation {
                print: PrintProbe::Unreachable("x".into()),
                device_status: None,
                device_cap: None,
                device_cap_current: false,
                ipp: None,
                warnings: vec![],
            },
            Some(&awake),
            2,
        );
        assert_eq!(off.address.as_deref(), Some("192.0.2.7"));
        let sparse: PrinterSnapshot = serde_json::from_str(r#"{"power":"on"}"#).unwrap();
        assert_eq!(sparse.connection, Connection::Unknown);
        assert_eq!(Connection::from_kind(Some("lan")).as_str(), "lan");
        assert_eq!(Connection::from_kind(Some("bluetooth")), Connection::Unknown);
    }

    #[test]
    fn power_messages() {
        assert_eq!(Power::Sleeping.message(), SLEEPING_MESSAGE);
        assert_eq!(Power::Offline.message_key(), "power.offline");
        assert_eq!(Power::On.as_str(), "on");
    }

    fn with_update(mut obs: Observation, reported: &str) -> Observation {
        let mut st = obs.device_status.take().unwrap();
        st.networkupdate_available = Some(reported.to_string());
        obs.device_status = Some(st);
        obs
    }

    #[test]
    fn a_sleeping_printers_unavailable_is_not_an_answer() {
        // Awake and `unavailable`: up to date, for real.
        let awake = PrinterSnapshot::compose(awake_obs(), None, 1000);
        assert_eq!(
            (awake.firmware_update, awake.firmware_update_remembered),
            (FirmwareUpdate::Unavailable, false)
        );
        // Asleep, `unavailable`, and nothing from an awake reading: unknown.
        let asleep = PrinterSnapshot::compose(sleeping_obs([Some(50); 5]), None, 2000);
        assert_eq!(
            (asleep.firmware_update, asleep.firmware_update_remembered),
            (FirmwareUpdate::Unknown, false)
        );
        // Asleep, `unavailable`, after an awake `available`: the update is still on offer, and
        // it is marked as remembered.
        let offered = PrinterSnapshot::compose(with_update(awake_obs(), "available"), None, 3000);
        assert_eq!(offered.firmware_update, FirmwareUpdate::Available);
        let asleep = PrinterSnapshot::compose(sleeping_obs([Some(50); 5]), Some(&offered), 4000);
        assert_eq!(
            (asleep.firmware_update, asleep.firmware_update_remembered),
            (FirmwareUpdate::Available, true)
        );
        // It stays so across several sleeping readings...
        let again = PrinterSnapshot::compose(sleeping_obs([Some(50); 5]), Some(&asleep), 5000);
        assert_eq!(
            (again.firmware_update, again.firmware_update_remembered),
            (FirmwareUpdate::Available, true)
        );
        // ...and the printer being unreachable does not change what was known.
        let off = Observation {
            print: PrintProbe::Unreachable("down".into()),
            device_status: None,
            device_cap: None,
            ipp: None,
            ..awake_obs()
        };
        let offline = PrinterSnapshot::compose(off, Some(&again), 5500);
        assert_eq!(offline.power, Power::Offline);
        assert_eq!(
            (offline.firmware_update, offline.firmware_update_remembered),
            (FirmwareUpdate::Available, true)
        );
        // `available` read while asleep can be trusted and is not marked remembered.
        let mut seen_asleep = sleeping_obs([Some(50); 5]);
        seen_asleep = with_update(seen_asleep, "available");
        let s = PrinterSnapshot::compose(seen_asleep, None, 6000);
        assert_eq!(
            (s.firmware_update, s.firmware_update_remembered),
            (FirmwareUpdate::Available, false)
        );
        // The printer wakes up and says `unavailable`: now it is believed.
        let woke = PrinterSnapshot::compose(awake_obs(), Some(&again), 7000);
        assert_eq!(
            (woke.firmware_update, woke.firmware_update_remembered),
            (FirmwareUpdate::Unavailable, false)
        );
    }

    #[test]
    fn a_sleeping_unavailable_kept_by_an_older_version_is_not_carried_forward() {
        let mut old = PrinterSnapshot::compose(awake_obs(), None, 1000);
        old.power = Power::Sleeping;
        old.firmware_update = FirmwareUpdate::Unavailable;
        old.firmware_update_remembered = false;
        let s = PrinterSnapshot::compose(sleeping_obs([Some(50); 5]), Some(&old), 2000);
        assert_eq!(
            (s.firmware_update, s.firmware_update_remembered),
            (FirmwareUpdate::Unknown, false)
        );
    }
}
