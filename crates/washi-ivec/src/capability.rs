// SPDX-License-Identifier: GPL-2.0-or-later
//! Typed views of `GetCapability device` and `GetCapability maintenance`.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::IvecError;
use crate::request::CleaningType;
use crate::response::Response;
use crate::xml::Node;

/// Inclusive integer range, from the `min:max` notation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Range {
    pub min: i32,
    pub max: i32,
}

impl Range {
    fn parse(s: &str) -> Option<Range> {
        let (a, b) = s.trim().split_once(':')?;
        Some(Range { min: a.trim().parse().ok()?, max: b.trim().parse().ok()? })
    }

    pub fn contains(&self, v: i32) -> bool {
        (self.min..=self.max).contains(&v)
    }
}

fn list(s: Option<&str>) -> Vec<String> {
    s.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PowerRanges {
    pub modes: Vec<String>,
    /// Minutes.
    pub times: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SilentRanges {
    /// `ON`, `OFF`, `time`.
    pub modes: Vec<String>,
}

fn power_ranges(ps: &Node, name: &str) -> Option<PowerRanges> {
    let p = ps.child(name)?;
    Some(PowerRanges {
        modes: list(p.child_text("mode")),
        times: list(p.child_text("time")).iter().filter_map(|t| t.parse().ok()).collect(),
    })
}

/// Allowed values for the device settings. Serial numbers in the same reply are not parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceCapability {
    pub manufacturer: String,
    pub model: String,
    pub firmver: String,
    /// `print`, `maintenance`, `pli`, `eis`.
    pub services: Vec<String>,
    pub autopoweron: Vec<String>,
    pub autopoweroff: Option<PowerRanges>,
    pub autopoweroff_standalone: Option<PowerRanges>,
    pub silentmode: Option<SilentRanges>,
    pub papergap: Vec<String>,
    pub drylevel: Vec<u8>,
    /// Manual head alignment ranges: id -> sign -> range.
    pub setregistration: BTreeMap<u8, BTreeMap<String, Range>>,
    pub auto_printer_rom_update: Vec<String>,
    pub mqtt_connection: Vec<String>,
    /// Pages of the printer's own web page (Remote UI) by name, as relative links:
    /// `maintenance` -> `index.html?page=PAGE_UTIL`.
    pub remote_pages: BTreeMap<String, String>,
}

impl DeviceCapability {
    pub fn from_response(r: &Response) -> Result<DeviceCapability, IvecError> {
        r.require_ok()?;
        let ps = r.param_set();
        let mut setregistration: BTreeMap<u8, BTreeMap<String, Range>> = BTreeMap::new();
        for reg in ps.children_named("setregistration") {
            let Some(id) = reg.attr("id").and_then(|i| i.parse::<u8>().ok()) else { continue };
            let items = setregistration.entry(id).or_default();
            for item in reg.children_named("item") {
                if let (Some(sign), Some(range)) = (item.attr("sign"), Range::parse(&item.text)) {
                    items.insert(sign.to_string(), range);
                }
            }
        }
        let remote_pages = ps
            .find(&["function_info", "remote_ui"])
            .map(|ui| {
                ui.children_named("pagelink")
                    .filter_map(|l| Some((l.attr("page")?.to_string(), l.text.clone())))
                    .filter(|(name, link)| !name.is_empty() && !link.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Ok(DeviceCapability {
            manufacturer: ps.child_text("manufacturer").unwrap_or_default().to_string(),
            model: ps.child_text("model").unwrap_or_default().to_string(),
            firmver: ps.child_text("firmver").unwrap_or_default().to_string(),
            services: list(ps.child_text("service")),
            autopoweron: list(ps.child_text("autopoweron")),
            autopoweroff: power_ranges(ps, "autopoweroff"),
            autopoweroff_standalone: power_ranges(ps, "autopoweroff_standalone"),
            silentmode: ps
                .child("silentmode")
                .map(|s| SilentRanges { modes: list(s.child_text("mode")) }),
            papergap: list(ps.child_text("papergap")),
            drylevel: list(ps.child_text("drylevel"))
                .iter()
                .filter_map(|d| d.parse().ok())
                .collect(),
            setregistration,
            auto_printer_rom_update: list(ps.child_text("auto_printer_rom_update")),
            mqtt_connection: list(ps.child_text("mqtt_connection")),
            remote_pages,
        })
    }

    pub fn parse(xml: &str) -> Result<DeviceCapability, IvecError> {
        Self::from_response(&Response::parse(xml)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CleaningCap {
    /// `regular`, `deep`.
    pub kind: String,
    /// `all`, `group1`, `group2`.
    pub inkgroups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestPrintCap {
    /// `nozzle_check`, `auto_registration`, `manual_registration`, `regi_check`.
    pub kind: String,
    pub inputbin: Option<String>,
    pub papersize: Option<String>,
    pub papertype: Option<String>,
    /// Sheets consumed.
    pub papernum: Option<u32>,
    /// Needs an `id` parameter.
    pub requires_id: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MaintenanceCapability {
    pub cleaning: Vec<CleaningCap>,
    pub testprint: Vec<TestPrintCap>,
    /// The printer lists `RollerCleaning` among its maintenance operations. The XK140 does not,
    /// so UIs keep the roller cleaning card hidden unless this is true.
    #[serde(default)]
    pub roller_cleaning: bool,
}

impl MaintenanceCapability {
    /// The cleaning types beyond regular and deep that Washi knows how to ask for (`roller`,
    /// `platen`), in the order the printer lists them. Unknown types are left out.
    pub fn extra_cleanings(&self) -> Vec<CleaningType> {
        let mut out: Vec<CleaningType> = Vec::new();
        for c in &self.cleaning {
            if let Some(t) = CleaningType::parse(&c.kind) {
                if !t.has_ink_group() && !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        out
    }

    pub fn from_response(r: &Response) -> Result<MaintenanceCapability, IvecError> {
        r.require_ok()?;
        let ps = r.param_set();
        let cleaning = ps
            .find(&["Cleaning"])
            .map(|c| {
                c.children_named("type")
                    .map(|t| CleaningCap {
                        kind: t.attr("value").unwrap_or_default().to_string(),
                        inkgroups: list(t.child_text("inkgroup")),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let testprint = ps
            .find(&["TestPrint"])
            .map(|c| {
                c.children_named("type")
                    .map(|t| TestPrintCap {
                        kind: t.attr("value").unwrap_or_default().to_string(),
                        inputbin: t.child_text("inputbin").map(str::to_string),
                        papersize: t.child_text("papersize").map(str::to_string),
                        papertype: t.child_text("papertype").map(str::to_string),
                        papernum: t.child_text("papernum").and_then(|n| n.parse().ok()),
                        requires_id: t.children_named("required_param").any(|p| p.text == "id"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let roller_cleaning =
            ps.children.iter().any(|c| c.name == "RollerCleaning" || c.name == "roller_cleaning");
        Ok(MaintenanceCapability { cleaning, testprint, roller_cleaning })
    }

    pub fn parse(xml: &str) -> Result<MaintenanceCapability, IvecError> {
        Self::from_response(&Response::parse(xml)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: &str = include_str!("../tests/fixtures/GetCapability-device.xml");
    const MAINT: &str = include_str!("../tests/fixtures/GetCapability-maintenance.xml");

    #[test]
    fn device_capability_fixture() {
        let c = DeviceCapability::parse(DEVICE).unwrap();
        assert_eq!(c.manufacturer, "CANON");
        assert_eq!(c.model, "XK140 series");
        assert_eq!(c.firmver, "1.060");
        assert_eq!(c.services, ["print", "maintenance", "pli", "eis"]);
        assert_eq!(c.autopoweron, ["ON", "OFF"]);
        let p = c.autopoweroff.unwrap();
        assert_eq!(p.modes, ["ON", "OFF"]);
        assert_eq!(p.times, [15, 30, 60, 120, 240]);
        assert_eq!(c.autopoweroff_standalone.unwrap().times.len(), 5);
        assert_eq!(c.silentmode.unwrap().modes, ["ON", "OFF", "time"]);
        assert_eq!(c.papergap, ["auto", "wide"]);
        assert_eq!(c.drylevel, [1, 2, 3, 4, 5]);
        assert_eq!(c.setregistration[&1]["A"], Range { min: -5, max: 5 });
        assert_eq!(c.setregistration[&1]["G"], Range { min: -3, max: 3 });
        assert_eq!(c.setregistration[&3]["e"], Range { min: -2, max: 2 });
        assert_eq!(c.auto_printer_rom_update, ["ON", "OFF"]);
        assert_eq!(c.mqtt_connection, ["ON", "OFF"]);
        assert_eq!(c.remote_pages["maintenance"], "index.html?page=PAGE_UTIL");
        assert_eq!(c.remote_pages["printerstatus"], "index.html?page=PAGE_INFO");
    }

    #[test]
    fn device_capability_has_no_serials() {
        let dbg = format!("{:?}", DeviceCapability::parse(DEVICE).unwrap());
        assert!(!dbg.contains("XXXXXXXXX") && !dbg.contains("0000000"));
    }

    #[test]
    fn maintenance_capability_fixture() {
        let m = MaintenanceCapability::parse(MAINT).unwrap();
        assert_eq!(m.cleaning.len(), 2);
        assert_eq!(m.cleaning[0].kind, "regular");
        assert_eq!(m.cleaning[1].kind, "deep");
        assert_eq!(m.cleaning[1].inkgroups, ["all", "group1", "group2"]);
        assert_eq!(m.testprint.len(), 4);
        assert!(!m.roller_cleaning, "the XK140 does not list roller cleaning");
        let manual = m.testprint.iter().find(|t| t.kind == "manual_registration").unwrap();
        assert!(manual.requires_id);
        assert_eq!(manual.papernum, Some(3));
        assert_eq!(manual.inputbin.as_deref(), Some("inputbin_p2"));
        assert_eq!(manual.papersize.as_deref(), Some("iso_a4_210x297mm"));
        assert_eq!(manual.papertype.as_deref(), Some("stationery"));
        let nozzle = m.testprint.iter().find(|t| t.kind == "nozzle_check").unwrap();
        assert!(!nozzle.requires_id);
        assert_eq!(nozzle.papernum, Some(1));
    }

    #[test]
    fn roller_cleaning_is_true_only_when_listed() {
        let with = MAINT.replace(
            "</ivec:TestPrint>",
            "</ivec:TestPrint><ivec:RollerCleaning><ivec:type value=\"regular\"/></ivec:RollerCleaning>",
        );
        assert!(MaintenanceCapability::parse(&with).unwrap().roller_cleaning);
    }

    #[test]
    fn extra_cleaning_types_are_picked_out() {
        let with = MAINT.replace(
            "</ivec:Cleaning>",
            "<ivec:type value=\"roller\"/><ivec:type value=\"choke\"/><ivec:type value=\"platen\"/></ivec:Cleaning>",
        );
        let m = MaintenanceCapability::parse(&with).unwrap();
        assert_eq!(m.cleaning.len(), 5);
        assert_eq!(m.extra_cleanings(), [CleaningType::Roller, CleaningType::Platen]);
        assert!(MaintenanceCapability::parse(MAINT).unwrap().extra_cleanings().is_empty());
    }

    #[test]
    fn range_parsing() {
        assert_eq!(Range::parse("-5:5"), Some(Range { min: -5, max: 5 }));
        assert_eq!(Range::parse("x"), None);
        assert!(Range { min: -2, max: 2 }.contains(-2));
        assert!(!Range { min: -2, max: 2 }.contains(3));
    }
}
