// SPDX-License-Identifier: GPL-2.0-or-later
//! Textual and JSON descriptions of settings changes and maintenance operations, shared by the
//! CLI (`washi set ...`) and the daemon's D-Bus API (JSON in, validated here).

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use washi_ivec::{
    AutoPowerOff, CleaningType, DeviceSettings, InkGroup, OnOff, SilentKind, SilentSetting,
    validate_hhmm,
};

use crate::error::{Error, Result};
use crate::maintenance::MaintOp;

/// `off`, or 15 / 30 / 60 / 120 / 240 minutes.
pub fn parse_auto_power_off(value: &str) -> Result<AutoPowerOff> {
    let v = value.trim().to_ascii_lowercase();
    if v == "off" {
        return Ok(AutoPowerOff { mode: OnOff::Off, time: None });
    }
    match v.parse::<u32>() {
        Ok(m @ (15 | 30 | 60 | 120 | 240)) => Ok(AutoPowerOff { mode: OnOff::On, time: Some(m) }),
        _ => Err(Error::Invalid(format!(
            "auto power off must be off, 15, 30, 60, 120 or 240 (minutes), got {value:?}"
        ))),
    }
}

/// Quiet mode: `off`, `on`, or a daily window `HHMM-HHMM`.
pub fn parse_quiet(value: &str) -> Result<SilentSetting> {
    let v = value.trim().to_ascii_lowercase();
    match v.as_str() {
        "off" => Ok(SilentSetting { kind: SilentKind::Off, start: None, end: None }),
        "on" => Ok(SilentSetting { kind: SilentKind::On, start: None, end: None }),
        _ => {
            let Some((a, b)) = v.split_once('-') else {
                return Err(Error::Invalid(format!(
                    "quiet mode must be off, on, or HHMM-HHMM (e.g. 2100-0700), got {value:?}"
                )));
            };
            validate_hhmm(a)?;
            validate_hhmm(b)?;
            Ok(SilentSetting {
                kind: SilentKind::Time,
                start: Some(a.to_string()),
                end: Some(b.to_string()),
            })
        }
    }
}

/// `"on"`, `"off"`, `true` or `false` in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toggle(pub OnOff);

impl<'de> Deserialize<'de> for Toggle {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Bool(true) => Ok(Toggle(OnOff::On)),
            Raw::Bool(false) => Ok(Toggle(OnOff::Off)),
            Raw::Text(s) => s.parse().map(Toggle).map_err(serde::de::Error::custom),
        }
    }
}

/// `"off"`, `"60"` or `60` in JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerSpec(pub AutoPowerOff);

impl<'de> Deserialize<'de> for PowerSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Minutes(u32),
            Text(String),
        }
        let text = match Raw::deserialize(d)? {
            Raw::Minutes(m) => m.to_string(),
            Raw::Text(s) => s,
        };
        parse_auto_power_off(&text).map(PowerSpec).map_err(serde::de::Error::custom)
    }
}

/// A device-settings change as JSON (the daemon's `ApplySettings`). Unknown keys are errors so
/// that typos never turn into silent no-ops.
///
/// ```json
/// { "confirm": true, "dry_level": 4, "auto_power_off": 60, "quiet": "2100-0700" }
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSpec {
    /// Must be `true` for anything that writes (mirrors the CLI's `--confirm`).
    #[serde(default)]
    pub confirm: bool,
    pub auto_power_on: Option<Toggle>,
    pub auto_power_off: Option<PowerSpec>,
    pub auto_power_off_standalone: Option<PowerSpec>,
    /// `off`, `on` or `HHMM-HHMM`.
    pub quiet: Option<String>,
    pub dry_level: Option<u8>,
    /// `auto` or `wide` (the printer's own `papergap`).
    pub paper_gap: Option<String>,
    /// "Prevent paper abrasion": `true` / `"on"` = `papergap` `wide`, `false` / `"off"` =
    /// `auto`. The same setting as `paper_gap`; give one of the two.
    pub paper_abrasion: Option<Toggle>,
    pub auto_firmware: Option<Toggle>,
    pub cloud_link: Option<Toggle>,
    /// Manual print head alignment values: set id (`"1"`..`"3"`) -> sign (`"A"`..) -> value,
    /// e.g. `{"1": {"A": 1, "B": -2}}`. Ranges are checked against what the printer reports.
    /// Send whole sets (every sign of the id): what the printer does with a partial set is not
    /// known.
    pub setregistration: Option<BTreeMap<String, BTreeMap<String, i32>>>,
}

fn registration_sets(
    raw: &BTreeMap<String, BTreeMap<String, i32>>,
) -> Result<BTreeMap<u8, BTreeMap<String, i32>>> {
    let mut out = BTreeMap::new();
    for (id, items) in raw {
        let n = id.trim().parse::<u8>().ok().filter(|n| (1..=3).contains(n)).ok_or_else(|| {
            Error::Invalid(format!("setregistration id must be 1, 2 or 3, got {id:?}"))
        })?;
        if items.is_empty() {
            return Err(Error::Invalid(format!("setregistration {n} has no values")));
        }
        for sign in items.keys() {
            if sign.chars().count() != 1 || !sign.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(Error::Invalid(format!(
                    "setregistration {n}: {sign:?} is not a sign (a single letter, e.g. A or e)"
                )));
            }
        }
        out.insert(n, items.clone());
    }
    Ok(out)
}

impl SettingsSpec {
    pub fn from_json(json: &str) -> Result<SettingsSpec> {
        serde_json::from_str(json).map_err(|e| Error::Invalid(format!("settings JSON: {e}")))
    }

    /// The change set; an empty spec is an error.
    pub fn to_settings(&self) -> Result<DeviceSettings> {
        if self.paper_gap.is_some() && self.paper_abrasion.is_some() {
            return Err(Error::Invalid(
                "paper_gap and paper_abrasion are the same setting: give only one".into(),
            ));
        }
        let papergap = match (&self.paper_gap, self.paper_abrasion) {
            (Some(v), _) => Some(v.trim().to_ascii_lowercase()),
            (None, Some(Toggle(OnOff::On))) => Some("wide".to_string()),
            (None, Some(Toggle(OnOff::Off))) => Some("auto".to_string()),
            (None, None) => None,
        };
        let s = DeviceSettings {
            autopoweron: self.auto_power_on.map(|t| t.0),
            autopoweroff: self.auto_power_off.as_ref().map(|p| p.0),
            autopoweroff_standalone: self.auto_power_off_standalone.as_ref().map(|p| p.0),
            silentmode: self.quiet.as_deref().map(parse_quiet).transpose()?,
            papergap,
            drylevel: self.dry_level,
            auto_printer_rom_update: self.auto_firmware.map(|t| t.0),
            mqtt_connection: self.cloud_link.map(|t| t.0),
            setregistration: match &self.setregistration {
                Some(raw) => registration_sets(raw)?,
                None => BTreeMap::new(),
            },
        };
        if s.is_empty() {
            return Err(Error::Invalid("no setting to change".into()));
        }
        Ok(s)
    }
}

/// A maintenance operation as JSON (the daemon's `RunMaintenance`).
///
/// ```json
/// { "confirm": true, "op": "clean", "kind": "deep", "group": "all" }
/// { "confirm": true, "op": "nozzle_check" }
/// { "confirm": true, "op": "align_manual", "id": 2 }
/// ```
///
/// `op` is one of `clean`, `nozzle_check`, `align_auto`, `align_check`, `align_manual`,
/// `roller_clean`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintSpec {
    /// Must be `true` (mirrors the CLI's `--confirm`).
    #[serde(default)]
    pub confirm: bool,
    pub op: String,
    /// `clean` only: `regular` (default) or `deep`.
    pub kind: Option<String>,
    /// `clean` only: `all` (default), `1`/`group1`, `2`/`group2`.
    pub group: Option<String>,
    /// `align_manual` only: 1..=3.
    pub id: Option<u8>,
}

impl MaintSpec {
    pub fn from_json(json: &str) -> Result<MaintSpec> {
        serde_json::from_str(json).map_err(|e| Error::Invalid(format!("maintenance JSON: {e}")))
    }

    /// The validated operation.
    pub fn to_op(&self) -> Result<MaintOp> {
        let is_clean = self.op == "clean";
        if !is_clean && (self.kind.is_some() || self.group.is_some()) {
            return Err(Error::Invalid(format!("{:?} takes no kind or group", self.op)));
        }
        if self.op != "align_manual" && self.id.is_some() {
            return Err(Error::Invalid(format!("{:?} takes no id", self.op)));
        }
        let op = match self.op.as_str() {
            "clean" => {
                let kind = CleaningType::parse(self.kind.as_deref().unwrap_or("regular"))
                    .ok_or_else(|| {
                        Error::Invalid(format!(
                            "cleaning kind must be regular, deep, roller or platen, got {:?}",
                            self.kind.as_deref().unwrap_or_default()
                        ))
                    })?;
                if !kind.has_ink_group() && self.group.is_some() {
                    return Err(Error::Invalid(format!(
                        "{} cleaning takes no ink group",
                        kind.as_str()
                    )));
                }
                let group = self.group.as_deref().unwrap_or("all").parse::<InkGroup>()?;
                MaintOp::Clean { kind, group }
            }
            "nozzle_check" => MaintOp::NozzleCheck,
            "align_auto" => MaintOp::AlignAuto,
            "align_check" => MaintOp::AlignCheck,
            "align_manual" => MaintOp::AlignManual(self.id.ok_or_else(|| {
                Error::Invalid("align_manual needs an id (1, 2 or 3)".to_string())
            })?),
            "roller_clean" => MaintOp::RollerClean,
            other => {
                return Err(Error::Invalid(format!(
                    "unknown maintenance op {other:?} (clean, nozzle_check, align_auto, \
                     align_check, align_manual, roller_clean)"
                )));
            }
        };
        op.validate()?;
        Ok(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_parsers() {
        assert_eq!(parse_auto_power_off("off").unwrap().mode, OnOff::Off);
        assert_eq!(parse_auto_power_off("120").unwrap().time, Some(120));
        assert!(parse_auto_power_off("45").is_err());
        assert!(parse_auto_power_off("soon").is_err());
        assert_eq!(parse_quiet("OFF").unwrap().kind, SilentKind::Off);
        assert_eq!(parse_quiet("on").unwrap().kind, SilentKind::On);
        let w = parse_quiet("2100-0700").unwrap();
        assert_eq!(
            (w.kind, w.start.as_deref(), w.end.as_deref()),
            (SilentKind::Time, Some("2100"), Some("0700"))
        );
        assert!(parse_quiet("2100").is_err());
        assert!(parse_quiet("2500-0700").is_err());
        assert!(parse_quiet("21:00-07:00").is_err());
    }

    #[test]
    fn settings_json_maps_to_a_change_set() {
        let s = SettingsSpec::from_json(
            r#"{"confirm": true, "dry_level": 4, "auto_power_off": 60, "auto_power_on": "off",
                "quiet": "2100-0700", "paper_gap": "WIDE", "auto_firmware": false,
                "auto_power_off_standalone": "off", "cloud_link": "on"}"#,
        )
        .unwrap();
        assert!(s.confirm);
        let d = s.to_settings().unwrap();
        assert_eq!(d.drylevel, Some(4));
        assert_eq!(d.autopoweroff, Some(AutoPowerOff { mode: OnOff::On, time: Some(60) }));
        assert_eq!(d.autopoweroff_standalone, Some(AutoPowerOff { mode: OnOff::Off, time: None }));
        assert_eq!(d.autopoweron, Some(OnOff::Off));
        assert_eq!(d.papergap.as_deref(), Some("wide"));
        assert_eq!(d.auto_printer_rom_update, Some(OnOff::Off));
        assert_eq!(d.mqtt_connection, Some(OnOff::On));
        assert_eq!(d.silentmode.unwrap().kind, SilentKind::Time);
    }

    #[test]
    fn paper_abrasion_is_the_paper_gap_setting() {
        for (json, want) in [
            (r#"{"paper_abrasion": true}"#, "wide"),
            (r#"{"paper_abrasion": "ON"}"#, "wide"),
            (r#"{"paper_abrasion": false}"#, "auto"),
            (r#"{"paper_abrasion": "off"}"#, "auto"),
            (r#"{"paper_gap": "Auto"}"#, "auto"),
        ] {
            let d = SettingsSpec::from_json(json).unwrap().to_settings().unwrap();
            assert_eq!(d.papergap.as_deref(), Some(want), "{json}");
        }
        let both = SettingsSpec::from_json(r#"{"paper_abrasion": true, "paper_gap": "wide"}"#)
            .unwrap()
            .to_settings();
        assert!(both.is_err(), "two spellings of one setting are refused");
        assert!(SettingsSpec::from_json(r#"{"paper_abrasion": "maybe"}"#).is_err());
    }

    #[test]
    fn registration_values_come_from_json() {
        let s = SettingsSpec::from_json(
            r#"{"confirm": true, "setregistration": {"1": {"A": 2, "B": -1}, "3": {"e": 0}}}"#,
        )
        .unwrap();
        let d = s.to_settings().unwrap();
        assert_eq!(d.setregistration[&1]["A"], 2);
        assert_eq!(d.setregistration[&1]["B"], -1);
        assert_eq!(d.setregistration[&3]["e"], 0);
        assert!(!d.is_empty() && d.drylevel.is_none());
        let e = |j: &str| {
            SettingsSpec::from_json(j).and_then(|s| s.to_settings()).unwrap_err().to_string()
        };
        assert!(e(r#"{"setregistration": {"4": {"A": 1}}}"#).contains("1, 2 or 3"));
        assert!(e(r#"{"setregistration": {"x": {"A": 1}}}"#).contains("1, 2 or 3"));
        assert!(e(r#"{"setregistration": {"1": {}}}"#).contains("no values"));
        assert!(e(r#"{"setregistration": {"1": {"AB": 1}}}"#).contains("not a sign"));
        assert!(e(r#"{"setregistration": {"1": {"7": 1}}}"#).contains("not a sign"));
        assert!(e(r#"{"setregistration": {"1": {"A": "x"}}}"#).contains("settings JSON"));
    }

    #[test]
    fn settings_json_rejects_bad_input() {
        let e = |j: &str| SettingsSpec::from_json(j).and_then(|s| s.to_settings()).unwrap_err();
        assert!(e("{}").to_string().contains("no setting"));
        assert!(e(r#"{"confirm": true}"#).to_string().contains("no setting"));
        assert!(e(r#"{"dry_levl": 3}"#).to_string().contains("dry_levl"), "typos are errors");
        assert!(e(r#"{"auto_power_off": 45}"#).to_string().contains("45"));
        assert!(e(r#"{"auto_power_on": "maybe"}"#).to_string().contains("ON or OFF"));
        assert!(e(r#"{"quiet": "25:00"}"#).to_string().contains("quiet"));
        assert!(e("not json").to_string().contains("settings JSON"));
        assert!(e(r#"{"dry_level": 300}"#).to_string().contains("settings JSON"));
        // Range checks against the printer's capability happen later in validate_settings.
        assert_eq!(
            SettingsSpec::from_json(r#"{"dry_level": 9}"#).unwrap().to_settings().unwrap().drylevel,
            Some(9)
        );
    }

    #[test]
    fn maintenance_json_maps_to_ops() {
        let op = |j: &str| MaintSpec::from_json(j).and_then(|s| s.to_op());
        assert_eq!(op(r#"{"op":"nozzle_check"}"#).unwrap(), MaintOp::NozzleCheck);
        assert_eq!(
            op(r#"{"op":"clean"}"#).unwrap(),
            MaintOp::Clean { kind: CleaningType::Regular, group: InkGroup::All }
        );
        assert_eq!(
            op(r#"{"op":"clean","kind":"deep","group":"2"}"#).unwrap(),
            MaintOp::Clean { kind: CleaningType::Deep, group: InkGroup::Group2 }
        );
        assert_eq!(op(r#"{"op":"align_manual","id":3}"#).unwrap(), MaintOp::AlignManual(3));
        assert_eq!(op(r#"{"op":"align_auto"}"#).unwrap(), MaintOp::AlignAuto);
        assert_eq!(op(r#"{"op":"align_check"}"#).unwrap(), MaintOp::AlignCheck);
        assert_eq!(op(r#"{"op":"roller_clean"}"#).unwrap(), MaintOp::RollerClean);
        assert_eq!(
            op(r#"{"op":"clean","kind":"roller"}"#).unwrap(),
            MaintOp::Clean { kind: CleaningType::Roller, group: InkGroup::All }
        );
        assert_eq!(
            op(r#"{"op":"clean","kind":"platen"}"#).unwrap(),
            MaintOp::Clean { kind: CleaningType::Platen, group: InkGroup::All }
        );
        assert!(!MaintSpec::from_json(r#"{"op":"nozzle_check"}"#).unwrap().confirm);
    }

    #[test]
    fn maintenance_json_rejects_bad_input() {
        let e = |j: &str| MaintSpec::from_json(j).and_then(|s| s.to_op()).unwrap_err().to_string();
        assert!(e(r#"{"op":"align_manual"}"#).contains("needs an id"));
        assert!(e(r#"{"op":"align_manual","id":4}"#).contains("1, 2 or 3"));
        assert!(e(r#"{"op":"nozzle_check","id":1}"#).contains("no id"));
        assert!(e(r#"{"op":"nozzle_check","kind":"deep"}"#).contains("no kind"));
        assert!(e(r#"{"op":"clean","kind":"soft"}"#).contains("regular, deep, roller or platen"));
        assert!(e(r#"{"op":"clean","kind":"choke"}"#).contains("regular, deep, roller or platen"));
        assert!(e(r#"{"op":"clean","kind":"roller","group":"all"}"#).contains("no ink group"));
        assert!(e(r#"{"op":"clean","group":"3"}"#).contains("inkgroup"));
        assert!(e(r#"{"op":"format_disk"}"#).contains("unknown maintenance op"));
        assert!(e(r#"{"op":"power_off"}"#).contains("unknown maintenance op"));
        assert!(e(r#"{"opp":"clean"}"#).contains("opp"));
        assert!(e("[]").contains("maintenance JSON"));
    }
}
