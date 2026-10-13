// SPDX-License-Identifier: GPL-2.0-or-later
//! Settings flow: validate -> `StartJob`(device) -> `SetConfiguration` -> `EndJob` -> read back.

use std::fmt;

use serde::Serialize;
use washi_ivec::{
    DeviceCapability, DeviceConfig, DeviceSettings, JobId, OnOff, ReadOp, Request, Service,
    SilentKind, validate_hhmm,
};
use washi_net::Confirm;

use crate::error::{Error, Result};
use crate::plan::{Plan, Step};
use crate::printer::Printer;

/// A setting whose read-back value differs from what was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub field: &'static str,
    pub wanted: String,
    pub actual: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: wanted {}, printer reports {}", self.field, self.wanted, self.actual)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsReport {
    /// Settings before the change.
    pub before: DeviceConfig,
    /// Settings read back after the change.
    pub after: DeviceConfig,
    /// False when the printer already had these values and nothing was written.
    pub changed: bool,
}

fn allowed(field: &'static str, list: &[String], value: &str) -> Result<()> {
    if list.is_empty() {
        return Err(Error::Invalid(format!("this printer does not report support for {field}")));
    }
    if list.iter().any(|v| v.eq_ignore_ascii_case(value)) {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{field}: {value:?} is not allowed (allowed: {})",
            list.join(", ")
        )))
    }
}

fn check_power(
    field: &'static str,
    p: &washi_ivec::AutoPowerOff,
    ranges: &Option<washi_ivec::PowerRanges>,
) -> Result<()> {
    let r = ranges.as_ref().ok_or_else(|| {
        Error::Invalid(format!("this printer does not report support for {field}"))
    })?;
    allowed(field, &r.modes, p.mode.as_str())?;
    if p.mode == OnOff::On {
        let t = p.time.ok_or_else(|| {
            Error::Invalid(format!("{field}: a time in minutes is required when ON"))
        })?;
        if !r.times.contains(&t) {
            return Err(Error::Invalid(format!(
                "{field}: {t} minutes is not allowed (allowed: {})",
                r.times.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            )));
        }
    }
    Ok(())
}

/// Check a change set against what the printer reports it accepts. Pure.
pub fn validate_settings(changes: &DeviceSettings, cap: &DeviceCapability) -> Result<()> {
    if changes.is_empty() {
        return Err(Error::Invalid("no setting to change".into()));
    }
    if let Some(v) = changes.autopoweron {
        allowed("autopoweron", &cap.autopoweron, v.as_str())?;
    }
    if let Some(p) = &changes.autopoweroff {
        check_power("autopoweroff", p, &cap.autopoweroff)?;
    }
    if let Some(p) = &changes.autopoweroff_standalone {
        check_power("autopoweroff_standalone", p, &cap.autopoweroff_standalone)?;
    }
    if let Some(s) = &changes.silentmode {
        let modes = cap.silentmode.as_ref().map(|m| m.modes.clone()).unwrap_or_default();
        allowed("silentmode", &modes, s.kind.as_str())?;
        if s.kind == SilentKind::Time {
            let start = s
                .start
                .as_deref()
                .ok_or_else(|| Error::Invalid("silentmode: start time missing".into()))?;
            let end = s
                .end
                .as_deref()
                .ok_or_else(|| Error::Invalid("silentmode: end time missing".into()))?;
            validate_hhmm(start)?;
            validate_hhmm(end)?;
        }
    }
    if let Some(v) = &changes.papergap {
        allowed("papergap", &cap.papergap, v)?;
    }
    if let Some(v) = changes.drylevel {
        if cap.drylevel.is_empty() {
            return Err(Error::Invalid("this printer does not report support for drylevel".into()));
        }
        if !cap.drylevel.contains(&v) {
            return Err(Error::Invalid(format!(
                "drylevel: {v} is not allowed (allowed: {})",
                cap.drylevel.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
            )));
        }
    }
    if let Some(v) = changes.auto_printer_rom_update {
        allowed("auto_printer_rom_update", &cap.auto_printer_rom_update, v.as_str())?;
    }
    if let Some(v) = changes.mqtt_connection {
        allowed("mqtt_connection", &cap.mqtt_connection, v.as_str())?;
    }
    for (id, items) in &changes.setregistration {
        let ranges = cap.setregistration.get(id).ok_or_else(|| {
            Error::Invalid(format!(
                "this printer does not report manual alignment set {id} (it has: {})",
                cap.setregistration.keys().map(u8::to_string).collect::<Vec<_>>().join(", ")
            ))
        })?;
        for (sign, value) in items {
            let r = ranges.get(sign).ok_or_else(|| {
                Error::Invalid(format!(
                    "setregistration {id}: there is no value {sign:?} (it has: {})",
                    ranges.keys().cloned().collect::<Vec<_>>().join(", ")
                ))
            })?;
            if !r.contains(*value) {
                return Err(Error::Invalid(format!(
                    "setregistration {id}/{sign}: {value} is outside {}..{}",
                    r.min, r.max
                )));
            }
        }
    }
    Ok(())
}

/// `1: A=0 B=1 | 2: I=0`, for messages and the change report.
fn registration_text(
    sets: &std::collections::BTreeMap<u8, std::collections::BTreeMap<String, i32>>,
) -> String {
    if sets.is_empty() {
        return "(not reported)".to_string();
    }
    sets.iter()
        .map(|(id, items)| {
            format!(
                "{id}: {}",
                items.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn show<T: fmt::Display>(v: &Option<T>) -> String {
    match v {
        Some(v) => v.to_string(),
        None => "(not reported)".to_string(),
    }
}

/// Compare the settings that were requested with what the printer reports. Fields the change
/// set leaves alone are not compared. Pure.
pub fn diff_settings(changes: &DeviceSettings, current: &DeviceConfig) -> Vec<Mismatch> {
    let mut out = Vec::new();
    let mut push = |field: &'static str, wanted: String, actual: String| {
        out.push(Mismatch { field, wanted, actual });
    };
    if let Some(w) = changes.autopoweron {
        if current.autopoweron != Some(w) {
            push("autopoweron", w.to_string(), show(&current.autopoweron));
        }
    }
    for (field, want, have) in [
        ("autopoweroff", &changes.autopoweroff, &current.autopoweroff),
        (
            "autopoweroff_standalone",
            &changes.autopoweroff_standalone,
            &current.autopoweroff_standalone,
        ),
    ] {
        if let Some(w) = want {
            let ok = have.as_ref().is_some_and(|h| {
                h.mode == Some(w.mode)
                    && (w.mode == OnOff::Off || w.time.is_none() || h.time == w.time)
            });
            if !ok {
                let wanted = match (w.mode, w.time) {
                    (OnOff::On, Some(t)) => format!("ON {t} min"),
                    (m, _) => m.to_string(),
                };
                let actual = have
                    .as_ref()
                    .map(|h| format!("{} {:?} min", show(&h.mode), h.time))
                    .unwrap_or_else(|| "(not reported)".into());
                push(field, wanted, actual);
            }
        }
    }
    if let Some(w) = &changes.silentmode {
        let ok = current.silentmode.as_ref().is_some_and(|h| {
            h.mode == Some(w.kind)
                && (w.kind != SilentKind::Time || (h.start == w.start && h.end == w.end))
        });
        if !ok {
            let wanted = if w.kind == SilentKind::Time {
                format!(
                    "time {}-{}",
                    w.start.as_deref().unwrap_or("?"),
                    w.end.as_deref().unwrap_or("?")
                )
            } else {
                w.kind.as_str().to_string()
            };
            let actual = current
                .silentmode
                .as_ref()
                .map(|h| {
                    format!(
                        "{} {}-{}",
                        show(&h.mode),
                        h.start.as_deref().unwrap_or("?"),
                        h.end.as_deref().unwrap_or("?")
                    )
                })
                .unwrap_or_else(|| "(not reported)".into());
            push("silentmode", wanted, actual);
        }
    }
    if let Some(w) = &changes.papergap {
        if !current.papergap.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(w)) {
            push("papergap", w.clone(), show(&current.papergap));
        }
    }
    if let Some(w) = changes.drylevel {
        if current.drylevel != Some(w) {
            push("drylevel", w.to_string(), show(&current.drylevel));
        }
    }
    if let Some(w) = changes.auto_printer_rom_update {
        if current.auto_printer_rom_update != Some(w) {
            push("auto_printer_rom_update", w.to_string(), show(&current.auto_printer_rom_update));
        }
    }
    if let Some(w) = changes.mqtt_connection {
        if current.mqtt_connection != Some(w) {
            push("mqtt_connection", w.to_string(), show(&current.mqtt_connection));
        }
    }
    if !changes.setregistration.is_empty() {
        let mut wanted = std::collections::BTreeMap::new();
        let mut actual = std::collections::BTreeMap::new();
        for (id, items) in &changes.setregistration {
            for (sign, v) in items {
                let have = current.setregistration.get(id).and_then(|m| m.get(sign));
                if have != Some(v) {
                    wanted
                        .entry(*id)
                        .or_insert_with(std::collections::BTreeMap::new)
                        .insert(sign.clone(), *v);
                    if let Some(h) = have {
                        actual
                            .entry(*id)
                            .or_insert_with(std::collections::BTreeMap::new)
                            .insert(sign.clone(), *h);
                    }
                }
            }
        }
        if !wanted.is_empty() {
            push("setregistration", registration_text(&wanted), registration_text(&actual));
        }
    }
    out
}

/// One setting whose value differs between two readings of the device configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigChange {
    pub field: &'static str,
    pub before: String,
    pub after: String,
}

fn timer(p: &Option<washi_ivec::PowerTimer>) -> String {
    match p {
        None => "(not reported)".to_string(),
        Some(p) => match (p.mode, p.time) {
            (Some(OnOff::On), Some(t)) => format!("ON {t} min"),
            (Some(m), _) => m.to_string(),
            (None, _) => "(not reported)".to_string(),
        },
    }
}

fn quiet(q: &Option<washi_ivec::SilentMode>) -> String {
    match q {
        None => "(not reported)".to_string(),
        Some(q) => match q.mode {
            Some(SilentKind::Time) => format!(
                "time {}-{}",
                q.start.as_deref().unwrap_or("?"),
                q.end.as_deref().unwrap_or("?")
            ),
            Some(k) => k.to_string(),
            None => "(not reported)".to_string(),
        },
    }
}

/// The settings that differ between two readings, with display values. Used to report what an
/// `ApplySettings` really changed. The stored quiet-mode window only counts while the mode is
/// `time`. Pure.
pub fn config_changes(before: &DeviceConfig, after: &DeviceConfig) -> Vec<ConfigChange> {
    let mut out = Vec::new();
    let mut add = |field: &'static str, b: String, a: String| {
        if b != a {
            out.push(ConfigChange { field, before: b, after: a });
        }
    };
    add("autopoweron", show(&before.autopoweron), show(&after.autopoweron));
    add("autopoweroff", timer(&before.autopoweroff), timer(&after.autopoweroff));
    add(
        "autopoweroff_standalone",
        timer(&before.autopoweroff_standalone),
        timer(&after.autopoweroff_standalone),
    );
    add("silentmode", quiet(&before.silentmode), quiet(&after.silentmode));
    add("papergap", show(&before.papergap), show(&after.papergap));
    add("drylevel", show(&before.drylevel), show(&after.drylevel));
    add(
        "auto_printer_rom_update",
        show(&before.auto_printer_rom_update),
        show(&after.auto_printer_rom_update),
    );
    add("mqtt_connection", show(&before.mqtt_connection), show(&after.mqtt_connection));
    add(
        "setregistration",
        registration_text(&before.setregistration),
        registration_text(&after.setregistration),
    );
    out
}

/// What `apply_device_settings` would send, without touching the network.
pub fn plan_settings(changes: &DeviceSettings) -> Result<Plan> {
    if changes.is_empty() {
        return Err(Error::Invalid("no setting to change".into()));
    }
    let job = JobId::standard();
    Ok(Plan {
        title: "change device settings".to_string(),
        steps: vec![
            Step::Poll(
                "read GetCapability device and GetConfiguration device; validate the change against the allowed values"
                    .to_string(),
            ),
            Step::Send(Request::start_job(Service::Device, job.clone())),
            Step::Send(Request::set_device_settings(job.clone(), changes.clone())),
            Step::Send(Request::end_job(Service::Device, job)),
            Step::Poll("read GetConfiguration device again and compare with the requested values".to_string()),
        ],
    })
}

impl Printer {
    /// Validate against `GetCapability`, write with `StartJob`/`SetConfiguration`/`EndJob`
    /// (`EndJob` is attempted even if the write fails), then read the configuration back and
    /// fail with [`Error::Mismatch`] if the printer does not report the requested values.
    pub fn apply_device_settings(
        &self,
        changes: &DeviceSettings,
        confirm: Confirm,
    ) -> Result<SettingsReport> {
        // Reads first: nothing is written if the request is not acceptable.
        let cap = self.device_capability()?;
        validate_settings(changes, &cap)?;
        let before = self.device_config()?;
        if diff_settings(changes, &before).is_empty() {
            return Ok(SettingsReport { after: before.clone(), before, changed: false });
        }

        let job = JobId::standard();
        let mut client = self.client();
        match client.execute(&Request::start_job(Service::Device, job.clone()), confirm) {
            Ok(r) => {
                r.require_ok()?;
            }
            Err(e) => {
                let _ = client.execute(&Request::end_job(Service::Device, job), confirm);
                client.close();
                return Err(e.into());
            }
        }
        let write = client
            .execute(&Request::set_device_settings(job.clone(), changes.clone()), confirm)
            .map_err(Error::from)
            .and_then(|r| r.require_ok().map(|_| ()).map_err(Error::from));
        let end = client
            .execute(&Request::end_job(Service::Device, job), confirm)
            .map_err(Error::from)
            .and_then(|r| crate::maintenance::end_job_outcome(&r));
        client.close();
        match (write, end) {
            (Ok(()), Ok(())) => {}
            (Ok(()), Err(e)) | (Err(e), Ok(())) => return Err(e),
            (Err(e), Err(cleanup)) => {
                return Err(Error::WithCleanupFailure {
                    error: Box::new(e),
                    cleanup: cleanup.to_string(),
                });
            }
        }

        let after = {
            let mut c = self.client();
            let resp =
                c.query(&Request::Get { op: ReadOp::GetConfiguration, service: Service::Device })?;
            DeviceConfig::from_response(&resp)?
        };
        let mismatches = diff_settings(changes, &after);
        if mismatches.is_empty() {
            Ok(SettingsReport { before, after, changed: true })
        } else {
            Err(Error::Mismatch(mismatches))
        }
    }
}

#[cfg(test)]
mod tests {
    use washi_ivec::{AutoPowerOff, SilentSetting};

    use super::*;

    const CAP: &str = include_str!("../../washi-ivec/tests/fixtures/GetCapability-device.xml");
    const CFG: &str = include_str!("../../washi-ivec/tests/fixtures/GetConfiguration-device.xml");

    fn cap() -> DeviceCapability {
        DeviceCapability::parse(CAP).unwrap()
    }

    fn cfg() -> DeviceConfig {
        DeviceConfig::parse(CFG).unwrap()
    }

    #[test]
    fn validation_accepts_allowed_values() {
        let ok = DeviceSettings {
            autopoweron: Some(OnOff::Off),
            autopoweroff: Some(AutoPowerOff { mode: OnOff::On, time: Some(120) }),
            silentmode: Some(SilentSetting {
                kind: SilentKind::Time,
                start: Some("2200".into()),
                end: Some("0630".into()),
            }),
            papergap: Some("wide".into()),
            drylevel: Some(5),
            auto_printer_rom_update: Some(OnOff::Off),
            ..Default::default()
        };
        validate_settings(&ok, &cap()).unwrap();
    }

    #[test]
    fn validation_rejects_out_of_range() {
        let c = cap();
        let bad = |s: DeviceSettings| validate_settings(&s, &c).unwrap_err().to_string();
        assert!(
            bad(DeviceSettings { drylevel: Some(6), ..Default::default() }).contains("drylevel")
        );
        assert!(
            bad(DeviceSettings { drylevel: Some(0), ..Default::default() }).contains("drylevel")
        );
        assert!(
            bad(DeviceSettings { papergap: Some("narrow".into()), ..Default::default() })
                .contains("papergap")
        );
        assert!(
            bad(DeviceSettings {
                autopoweroff: Some(AutoPowerOff { mode: OnOff::On, time: Some(45) }),
                ..Default::default()
            })
            .contains("45")
        );
        assert!(
            bad(DeviceSettings {
                autopoweroff: Some(AutoPowerOff { mode: OnOff::On, time: None }),
                ..Default::default()
            })
            .contains("required")
        );
        assert!(
            bad(DeviceSettings {
                silentmode: Some(SilentSetting {
                    kind: SilentKind::Time,
                    start: Some("2500".into()),
                    end: Some("0700".into())
                }),
                ..Default::default()
            })
            .contains("HHMM")
        );
        assert!(bad(DeviceSettings::default()).contains("no setting"));
    }

    #[test]
    fn unsupported_setting_is_rejected() {
        let mut c = cap();
        c.mqtt_connection.clear();
        let s = DeviceSettings { mqtt_connection: Some(OnOff::Off), ..Default::default() };
        assert!(validate_settings(&s, &c).unwrap_err().to_string().contains("does not report"));
    }

    #[test]
    fn diff_detects_changes_and_ignores_untouched_fields() {
        let current = cfg();
        let same = DeviceSettings {
            drylevel: Some(3),
            papergap: Some("AUTO".into()),
            autopoweroff: Some(AutoPowerOff { mode: OnOff::On, time: Some(30) }),
            silentmode: Some(SilentSetting { kind: SilentKind::Off, start: None, end: None }),
            ..Default::default()
        };
        assert_eq!(diff_settings(&same, &current), vec![]);

        let different = DeviceSettings {
            drylevel: Some(4),
            autopoweroff: Some(AutoPowerOff { mode: OnOff::On, time: Some(60) }),
            silentmode: Some(SilentSetting {
                kind: SilentKind::Time,
                start: Some("2200".into()),
                end: Some("0700".into()),
            }),
            auto_printer_rom_update: Some(OnOff::Off),
            ..Default::default()
        };
        let fields: Vec<_> = diff_settings(&different, &current).iter().map(|m| m.field).collect();
        assert_eq!(fields, ["autopoweroff", "silentmode", "drylevel", "auto_printer_rom_update"]);

        // Power-off OFF does not care about the stored time.
        let off = DeviceSettings {
            autopoweroff: Some(AutoPowerOff { mode: OnOff::Off, time: None }),
            ..Default::default()
        };
        assert_eq!(diff_settings(&off, &current).len(), 1);
    }

    #[test]
    fn config_changes_lists_only_real_differences() {
        let a = cfg();
        assert_eq!(config_changes(&a, &a), vec![]);
        let mut b = a.clone();
        b.drylevel = Some(4);
        b.autopoweroff = Some(washi_ivec::PowerTimer { mode: Some(OnOff::On), time: Some(60) });
        b.papergap = Some("wide".into());
        // The stored window is irrelevant while the mode is off.
        if let Some(q) = b.silentmode.as_mut() {
            q.start = Some("2200".into());
        }
        let ch = config_changes(&a, &b);
        let fields: Vec<_> = ch.iter().map(|c| c.field).collect();
        assert_eq!(fields, ["autopoweroff", "papergap", "drylevel"]);
        assert_eq!(ch[0].before, "ON 30 min");
        assert_eq!(ch[0].after, "ON 60 min");
        assert_eq!((ch[2].before.as_str(), ch[2].after.as_str()), ("3", "4"));
        // Window changes show up once the mode is `time`.
        if let Some(q) = b.silentmode.as_mut() {
            q.mode = Some(SilentKind::Time);
        }
        let ch = config_changes(&a, &b);
        let q = ch.iter().find(|c| c.field == "silentmode").unwrap();
        assert_eq!(q.before, "OFF");
        assert_eq!(q.after, "time 2200-0700");
    }

    #[test]
    fn plan_is_job_bracketed_and_never_empty() {
        let s = DeviceSettings { drylevel: Some(2), ..Default::default() };
        let plan = plan_settings(&s).unwrap();
        let ops: Vec<_> = plan.requests().map(|r| r.operation()).collect();
        assert_eq!(ops, ["StartJob", "SetConfiguration", "EndJob"]);
        assert!(plan_settings(&DeviceSettings::default()).is_err());
    }

    #[test]
    fn registration_values_are_checked_against_the_capability() {
        let c = cap();
        let one = |id: u8, sign: &str, v: i32| DeviceSettings {
            setregistration: std::collections::BTreeMap::from([(
                id,
                std::collections::BTreeMap::from([(sign.to_string(), v)]),
            )]),
            ..Default::default()
        };
        validate_settings(&one(1, "A", -5), &c).unwrap();
        validate_settings(&one(1, "G", 3), &c).unwrap();
        validate_settings(&one(3, "e", 2), &c).unwrap();
        let bad = |s: DeviceSettings| validate_settings(&s, &c).unwrap_err().to_string();
        assert!(bad(one(1, "A", 6)).contains("outside -5..5"));
        assert!(bad(one(1, "G", 4)).contains("outside -3..3"), "G and H are -3..3");
        assert!(bad(one(3, "e", 3)).contains("outside -2..2"));
        assert!(bad(one(4, "A", 0)).contains("manual alignment set 4"));
        assert!(bad(one(2, "A", 0)).contains("no value"));
    }

    #[test]
    fn registration_read_back_is_compared_and_reported() {
        let mut want = std::collections::BTreeMap::new();
        want.insert("D".to_string(), 3); // the fixture has D = 1
        want.insert("A".to_string(), 0); // unchanged
        let changes = DeviceSettings {
            setregistration: std::collections::BTreeMap::from([(1, want)]),
            ..Default::default()
        };
        let before = cfg();
        let m = diff_settings(&changes, &before);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].field, "setregistration");
        assert_eq!((m[0].wanted.as_str(), m[0].actual.as_str()), ("1: D=3", "1: D=1"));
        let mut after = before.clone();
        after.setregistration.get_mut(&1).unwrap().insert("D".into(), 3);
        assert!(diff_settings(&changes, &after).is_empty());
        let reported = config_changes(&before, &after);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].field, "setregistration");
        assert!(reported[0].before.contains("D=1") && reported[0].after.contains("D=3"));
        assert!(config_changes(&before, &before).is_empty());
    }
}
