// SPDX-License-Identifier: GPL-2.0-or-later
//! Device settings as sent in `SetConfiguration` (a partial update: only `Some` fields are sent).

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::Serialize;

use crate::error::IvecError;
use crate::xml::escape;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OnOff {
    On,
    Off,
}

impl OnOff {
    pub fn as_str(self) -> &'static str {
        match self {
            OnOff::On => "ON",
            OnOff::Off => "OFF",
        }
    }
}

impl fmt::Display for OnOff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OnOff {
    type Err = IvecError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_uppercase().as_str() {
            "ON" => Ok(OnOff::On),
            "OFF" => Ok(OnOff::Off),
            other => {
                Err(IvecError::invalid("on/off", format!("expected ON or OFF, got {other:?}")))
            }
        }
    }
}

/// Validate a `HHMM` time of day.
pub fn validate_hhmm(s: &str) -> Result<(), IvecError> {
    let ok = s.len() == 4
        && s.bytes().all(|b| b.is_ascii_digit())
        && s[..2].parse::<u8>().is_ok_and(|h| h < 24)
        && s[2..].parse::<u8>().is_ok_and(|m| m < 60);
    if ok { Ok(()) } else { Err(IvecError::invalid("time", format!("{s:?} is not HHMM"))) }
}

/// Auto power off. `time` (minutes) is required when `mode` is `On`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AutoPowerOff {
    pub mode: OnOff,
    pub time: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SilentKind {
    #[serde(rename = "ON")]
    On,
    #[serde(rename = "OFF")]
    Off,
    /// Quiet during a daily time window.
    #[serde(rename = "time")]
    Time,
}

impl SilentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SilentKind::On => "ON",
            SilentKind::Off => "OFF",
            SilentKind::Time => "time",
        }
    }
}

impl fmt::Display for SilentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SilentKind {
    type Err = IvecError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "on" => Ok(SilentKind::On),
            "off" => Ok(SilentKind::Off),
            "time" => Ok(SilentKind::Time),
            other => Err(IvecError::invalid("silentmode", format!("unknown mode {other:?}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SilentSetting {
    pub kind: SilentKind,
    /// `HHMM`; sent only for [`SilentKind::Time`].
    pub start: Option<String>,
    pub end: Option<String>,
}

/// A partial device-settings update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DeviceSettings {
    pub autopoweron: Option<OnOff>,
    pub autopoweroff: Option<AutoPowerOff>,
    pub autopoweroff_standalone: Option<AutoPowerOff>,
    pub silentmode: Option<SilentSetting>,
    /// `auto` or `wide`.
    pub papergap: Option<String>,
    /// 1..=5.
    pub drylevel: Option<u8>,
    pub auto_printer_rom_update: Option<OnOff>,
    pub mqtt_connection: Option<OnOff>,
    /// Manual print head alignment values: set id (1..=3) -> sign (`A`..`H`, `I`..`M`, `a`..`e`)
    /// -> value. Sent as `<ivec:setregistration id="N"><ivec:item sign="X">v</ivec:item>..`,
    /// the shape the printer reports in `GetConfiguration`. **Writing is unverified.**
    pub setregistration: BTreeMap<u8, BTreeMap<String, i32>>,
}

impl DeviceSettings {
    pub fn is_empty(&self) -> bool {
        *self == DeviceSettings::default()
    }

    /// The setting elements for a `SetConfiguration` param set (without `jobID`).
    pub fn to_xml_fragment(&self) -> String {
        let mut x = String::new();
        if let Some(v) = self.autopoweron {
            x.push_str(&format!("<ivec:autopoweron>{}</ivec:autopoweron>", v.as_str()));
        }
        if let Some(v) = &self.autopoweroff {
            x.push_str(&power_xml("autopoweroff", v));
        }
        if let Some(v) = &self.autopoweroff_standalone {
            x.push_str(&power_xml("autopoweroff_standalone", v));
        }
        if let Some(v) = &self.silentmode {
            x.push_str("<ivec:silentmode>");
            x.push_str(&format!("<ivec:mode>{}</ivec:mode>", v.kind.as_str()));
            if v.kind == SilentKind::Time {
                if let Some(s) = &v.start {
                    x.push_str(&format!("<ivec:starttime>{}</ivec:starttime>", escape(s)));
                }
                if let Some(e) = &v.end {
                    x.push_str(&format!("<ivec:endtime>{}</ivec:endtime>", escape(e)));
                }
            }
            x.push_str("</ivec:silentmode>");
        }
        if let Some(v) = &self.papergap {
            x.push_str(&format!("<ivec:papergap>{}</ivec:papergap>", escape(v)));
        }
        if let Some(v) = self.drylevel {
            x.push_str(&format!("<ivec:drylevel>{v}</ivec:drylevel>"));
        }
        if let Some(v) = self.auto_printer_rom_update {
            x.push_str(&format!(
                "<vcn:auto_printer_rom_update>{}</vcn:auto_printer_rom_update>",
                v.as_str()
            ));
        }
        if let Some(v) = self.mqtt_connection {
            x.push_str(&format!("<vcn:mqtt_connection>{}</vcn:mqtt_connection>", v.as_str()));
        }
        for (id, items) in &self.setregistration {
            x.push_str(&format!("<ivec:setregistration id=\"{id}\">"));
            for (sign, value) in items {
                x.push_str(&format!("<ivec:item sign=\"{}\">{value}</ivec:item>", escape(sign)));
            }
            x.push_str("</ivec:setregistration>");
        }
        x
    }
}

fn power_xml(tag: &str, p: &AutoPowerOff) -> String {
    let mut x = format!("<ivec:{tag}><ivec:mode>{}</ivec:mode>", p.mode.as_str());
    if let (OnOff::On, Some(t)) = (p.mode, p.time) {
        x.push_str(&format!("<ivec:time>{t}</ivec:time>"));
    }
    x.push_str(&format!("</ivec:{tag}>"));
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hhmm_validation() {
        assert!(validate_hhmm("2100").is_ok());
        assert!(validate_hhmm("0000").is_ok());
        assert!(validate_hhmm("2359").is_ok());
        assert!(validate_hhmm("2400").is_err());
        assert!(validate_hhmm("0960").is_err());
        assert!(validate_hhmm("900").is_err());
        assert!(validate_hhmm("09:00").is_err());
    }

    #[test]
    fn fragment_is_partial_and_escaped() {
        let s = DeviceSettings {
            papergap: Some("a<b".into()),
            drylevel: Some(4),
            autopoweroff: Some(AutoPowerOff { mode: OnOff::On, time: Some(60) }),
            ..Default::default()
        };
        let x = s.to_xml_fragment();
        assert!(x.contains("<ivec:papergap>a&lt;b</ivec:papergap>"));
        assert!(x.contains("<ivec:drylevel>4</ivec:drylevel>"));
        assert!(x.contains("<ivec:autopoweroff><ivec:mode>ON</ivec:mode><ivec:time>60</ivec:time></ivec:autopoweroff>"));
        assert!(!x.contains("autopoweron"));
        assert!(DeviceSettings::default().is_empty());
        assert!(!s.is_empty());
    }

    #[test]
    fn registration_values_are_sent_per_set() {
        let mut items = BTreeMap::new();
        items.insert("A".to_string(), -2);
        items.insert("B".to_string(), 3);
        let s =
            DeviceSettings { setregistration: BTreeMap::from([(1, items)]), ..Default::default() };
        assert!(!s.is_empty());
        let x = s.to_xml_fragment();
        assert_eq!(
            x,
            "<ivec:setregistration id=\"1\"><ivec:item sign=\"A\">-2</ivec:item>\
             <ivec:item sign=\"B\">3</ivec:item></ivec:setregistration>"
        );
    }

    #[test]
    fn power_off_omits_time_and_quiet_window_includes_times() {
        let s = DeviceSettings {
            autopoweroff: Some(AutoPowerOff { mode: OnOff::Off, time: Some(30) }),
            silentmode: Some(SilentSetting {
                kind: SilentKind::Time,
                start: Some("2100".into()),
                end: Some("0700".into()),
            }),
            ..Default::default()
        };
        let x = s.to_xml_fragment();
        assert!(x.contains("<ivec:autopoweroff><ivec:mode>OFF</ivec:mode></ivec:autopoweroff>"));
        assert!(x.contains("<ivec:mode>time</ivec:mode><ivec:starttime>2100</ivec:starttime><ivec:endtime>0700</ivec:endtime>"));
    }
}
