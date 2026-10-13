// SPDX-License-Identifier: GPL-2.0-or-later
//! Typed view of `GetConfiguration device`.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::IvecError;
use crate::response::Response;
use crate::settings::{OnOff, SilentKind};
use crate::xml::Node;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PowerTimer {
    pub mode: Option<OnOff>,
    /// Minutes.
    pub time: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SilentMode {
    pub mode: Option<SilentKind>,
    /// `HHMM`.
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceConfig {
    pub autopoweron: Option<OnOff>,
    pub autopoweroff: Option<PowerTimer>,
    pub autopoweroff_standalone: Option<PowerTimer>,
    pub silentmode: Option<SilentMode>,
    /// `auto` or `wide`.
    pub papergap: Option<String>,
    pub drylevel: Option<u8>,
    /// Manual head alignment values: id -> sign -> value.
    pub setregistration: BTreeMap<u8, BTreeMap<String, i32>>,
    pub auto_printer_rom_update: Option<OnOff>,
    pub mqtt_connection: Option<OnOff>,
}

fn on_off(node: &Node, name: &str) -> Option<OnOff> {
    node.child_text(name).and_then(|s| s.parse().ok())
}

fn power(node: &Node, name: &str) -> Option<PowerTimer> {
    let p = node.child(name)?;
    Some(PowerTimer {
        mode: on_off(p, "mode"),
        time: p.child_text("time").and_then(|t| t.parse().ok()),
    })
}

impl DeviceConfig {
    pub fn from_response(r: &Response) -> Result<DeviceConfig, IvecError> {
        r.require_ok()?;
        let ps = r.param_set();
        let silentmode = ps.child("silentmode").map(|s| SilentMode {
            mode: s.child_text("mode").and_then(|m| m.parse().ok()),
            start: s.child_text("starttime").filter(|t| !t.is_empty()).map(str::to_string),
            end: s.child_text("endtime").filter(|t| !t.is_empty()).map(str::to_string),
        });
        let mut setregistration: BTreeMap<u8, BTreeMap<String, i32>> = BTreeMap::new();
        for reg in ps.children_named("setregistration") {
            let Some(id) = reg.attr("id").and_then(|i| i.parse::<u8>().ok()) else { continue };
            let items = setregistration.entry(id).or_default();
            for item in reg.children_named("item") {
                if let (Some(sign), Ok(v)) = (item.attr("sign"), item.text.parse::<i32>()) {
                    items.insert(sign.to_string(), v);
                }
            }
        }
        Ok(DeviceConfig {
            autopoweron: on_off(ps, "autopoweron"),
            autopoweroff: power(ps, "autopoweroff"),
            autopoweroff_standalone: power(ps, "autopoweroff_standalone"),
            silentmode,
            papergap: ps.child_text("papergap").map(str::to_string),
            drylevel: ps.child_text("drylevel").and_then(|d| d.parse().ok()),
            setregistration,
            auto_printer_rom_update: on_off(ps, "auto_printer_rom_update"),
            mqtt_connection: on_off(ps, "mqtt_connection"),
        })
    }

    pub fn parse(xml: &str) -> Result<DeviceConfig, IvecError> {
        Self::from_response(&Response::parse(xml)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = include_str!("../tests/fixtures/GetConfiguration-device.xml");

    #[test]
    fn device_config_fixture() {
        let c = DeviceConfig::parse(CONFIG).unwrap();
        assert_eq!(c.autopoweron, Some(OnOff::On));
        assert_eq!(c.autopoweroff, Some(PowerTimer { mode: Some(OnOff::On), time: Some(30) }));
        assert_eq!(c.autopoweroff_standalone.as_ref().unwrap().time, Some(30));
        let q = c.silentmode.unwrap();
        assert_eq!(q.mode, Some(SilentKind::Off));
        assert_eq!(q.start.as_deref(), Some("2100"));
        assert_eq!(q.end.as_deref(), Some("0700"));
        assert_eq!(c.papergap.as_deref(), Some("auto"));
        assert_eq!(c.drylevel, Some(3));
        assert_eq!(c.setregistration.len(), 3);
        assert_eq!(c.setregistration[&1]["D"], 1);
        assert_eq!(c.setregistration[&1].len(), 8);
        assert_eq!(c.setregistration[&2].len(), 5);
        assert_eq!(c.setregistration[&3]["e"], 0);
        assert_eq!(c.auto_printer_rom_update, Some(OnOff::On));
        assert_eq!(c.mqtt_connection, Some(OnOff::On));
    }

    #[test]
    fn missing_and_unknown_fields() {
        let xml = CONFIG
            .replace(
                "<ivec:drylevel>3</ivec:drylevel>",
                "<vcn:future_setting>x</vcn:future_setting>",
            )
            .replace("<ivec:papergap>auto</ivec:papergap>", "");
        let c = DeviceConfig::parse(&xml).unwrap();
        assert_eq!(c.drylevel, None);
        assert_eq!(c.papergap, None);
        assert_eq!(c.autopoweron, Some(OnOff::On));
    }

    #[test]
    fn time_window_mode() {
        let xml = CONFIG.replace("<ivec:mode>OFF</ivec:mode>", "<ivec:mode>time</ivec:mode>");
        assert_eq!(
            DeviceConfig::parse(&xml).unwrap().silentmode.unwrap().mode,
            Some(SilentKind::Time)
        );
    }

    #[test]
    fn maintenance_configuration_is_not_a_device_config_but_parses_ok() {
        let c =
            DeviceConfig::parse(include_str!("../tests/fixtures/GetConfiguration-maintenance.xml"))
                .unwrap();
        assert_eq!(c.autopoweron, None);
        assert!(c.setregistration.is_empty());
    }
}
