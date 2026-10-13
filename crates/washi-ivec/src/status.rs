// SPDX-License-Identifier: GPL-2.0-or-later
//! Typed views of `GetStatus` replies.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::IvecError;
use crate::response::Response;
use crate::xml::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InkIcon {
    None,
    Warning,
    Error,
    Unknown,
}

impl InkIcon {
    fn parse(s: &str) -> InkIcon {
        match s.to_ascii_lowercase().as_str() {
            "" | "none" => InkIcon::None,
            "warning" => InkIcon::Warning,
            "error" => InkIcon::Error,
            _ => InkIcon::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ink {
    /// Tank model, e.g. `XKI-N21<M>SETUP`.
    pub model: String,
    /// `BK`, `PGBK`, `C`, `M`, `Y`.
    pub color: String,
    pub icon: InkIcon,
    /// Percent remaining (the printer reports 10 % steps).
    pub level: Option<u8>,
    pub order: Option<u32>,
}

/// Current paper in one input bin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tray {
    /// `inputbin_p1` (rear tray), `inputbin_p2` (cassette).
    pub bin: String,
    pub paper_type: Option<String>,
    pub paper_size: Option<String>,
    /// Hundredths of a millimetre.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Contents of a non-empty `ivec:jobinfo`: child elements and attributes flattened to pairs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobInfo {
    pub fields: Vec<(String, String)>,
}

impl JobInfo {
    pub(crate) fn from_node(node: Option<&Node>) -> Option<JobInfo> {
        let node = node.filter(|n| !n.is_blank())?;
        let mut fields: Vec<(String, String)> = node.attrs.clone();
        let flat = node.flatten();
        if flat.is_empty() && !node.text.is_empty() {
            fields.push((node.name.clone(), node.text.clone()));
        }
        fields.extend(flat);
        Some(JobInfo { fields })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PrintStatus {
    /// `idle`, ... (firmware-defined).
    pub status: String,
    pub status_detail: String,
    /// Error / support code (empty when none).
    pub support_code: String,
    pub trays: Vec<Tray>,
    pub inks: Vec<Ink>,
    pub jobinfo: Option<JobInfo>,
}

fn parse_u32(s: &str) -> Option<u32> {
    s.trim().parse().ok()
}

fn tray_index(trays: &mut Vec<Tray>, bin: &str) -> usize {
    if let Some(i) = trays.iter().position(|t| t.bin == bin) {
        return i;
    }
    trays.push(Tray {
        bin: bin.to_string(),
        paper_type: None,
        paper_size: None,
        width: None,
        height: None,
    });
    trays.len() - 1
}

impl PrintStatus {
    pub fn from_response(r: &Response) -> Result<PrintStatus, IvecError> {
        r.require_ok()?;
        let ps = r.param_set();
        let mut trays: Vec<Tray> = Vec::new();
        for n in &ps.children {
            let Some(bin) = n.attr("physicalinputbin") else { continue };
            match n.name.as_str() {
                "currentpapertype" => {
                    let i = tray_index(&mut trays, bin);
                    trays[i].paper_type = Some(n.text.clone());
                }
                "currentpapersize" => {
                    let i = tray_index(&mut trays, bin);
                    trays[i].paper_size = Some(n.text.clone());
                }
                "current_papersize_width" => {
                    let i = tray_index(&mut trays, bin);
                    trays[i].width = parse_u32(&n.text);
                }
                "current_papersize_height" => {
                    let i = tray_index(&mut trays, bin);
                    trays[i].height = parse_u32(&n.text);
                }
                _ => {}
            }
        }
        let inks = ps
            .find(&["marker_info"])
            .map(|mi| {
                mi.children_named("ink")
                    .map(|i| Ink {
                        model: i.child_text("model").unwrap_or_default().to_string(),
                        color: i.child_text("color").unwrap_or_default().to_string(),
                        icon: InkIcon::parse(i.child_text("icon").unwrap_or_default()),
                        level: i.child_text("level").and_then(|l| l.parse().ok()),
                        order: i.child_text("order").and_then(parse_u32),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(PrintStatus {
            status: ps.child_text("status").unwrap_or_default().to_string(),
            status_detail: ps.child_text("status_detail").unwrap_or_default().to_string(),
            support_code: ps.child_text("current_support_code").unwrap_or_default().to_string(),
            trays,
            inks,
            jobinfo: JobInfo::from_node(ps.child("jobinfo")),
        })
    }

    pub fn parse(xml: &str) -> Result<PrintStatus, IvecError> {
        Self::from_response(&Response::parse(xml)?)
    }
}

/// Lifetime counters from `GetStatus device`. Cloud redirect URLs, hardware addresses and
/// contract flags in the same reply are intentionally not parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceStatus {
    /// Sheets fed per input bin.
    pub total_paper_num: BTreeMap<String, u32>,
    pub ink_replacement_count: BTreeMap<String, u32>,
    pub genuineink_replacement_count: BTreeMap<String, u32>,
    pub head_replacement_count: Option<u32>,
    /// e.g. `unavailable`, `available`.
    pub networkupdate_available: Option<String>,
    /// How the printer is attached to the network: `wifi` (a wireless interface in
    /// `infrastructure_mode`), `lan` (a wired interface) or `None` when the reply does not say.
    /// Hardware addresses in the same reply are never parsed.
    #[serde(default)]
    pub connection: Option<String>,
}

/// `wifi` / `lan` from the `network_interface` block of `GetStatus device`. A wired interface
/// that lists any communication method wins over Wi-Fi; Wi-Fi needs `infrastructure_mode`
/// (the other modes, `access_point_mode` and `wi_fi_direct_mode`, are the printer acting as an
/// access point, not a link to your network).
pub fn connection_kind(ps: &Node) -> Option<String> {
    let ni = ps.child("network_interface")?;
    let mut wifi = false;
    let mut lan = false;
    for set in ni.children_named("interface_set") {
        let id = set.attr("id").unwrap_or_default().to_ascii_lowercase();
        let methods: Vec<String> = set
            .child("communication_method")
            .map(|m| m.children_named("item").map(|i| i.text.trim().to_ascii_lowercase()).collect())
            .unwrap_or_default();
        let wired_id = id.starts_with("wired") || id.starts_with("eth") || id.starts_with("lan");
        if wired_id && !methods.is_empty() {
            lan = true;
        } else if id.starts_with("wireless") || id.starts_with("wlan") || id.starts_with("wifi") {
            wifi |= methods.iter().any(|m| m == "infrastructure_mode");
        }
    }
    if lan {
        Some("lan".to_string())
    } else if wifi {
        Some("wifi".to_string())
    } else {
        None
    }
}

impl DeviceStatus {
    pub fn from_response(r: &Response) -> Result<DeviceStatus, IvecError> {
        r.require_ok()?;
        let ps = r.param_set();
        let collect = |name: &str, key: &str| -> BTreeMap<String, u32> {
            ps.children_named(name)
                .filter_map(|n| Some((n.attr(key)?.to_string(), parse_u32(&n.text)?)))
                .collect()
        };
        Ok(DeviceStatus {
            total_paper_num: collect("total_paper_num", "physicalinputbin"),
            ink_replacement_count: collect("ink_replacement_count", "color"),
            genuineink_replacement_count: collect("genuineink_replacement_count", "color"),
            head_replacement_count: ps.child_text("head_replacement_count").and_then(parse_u32),
            networkupdate_available: ps.child_text("networkupdate_available").map(str::to_string),
            connection: connection_kind(ps),
        })
    }

    pub fn parse(xml: &str) -> Result<DeviceStatus, IvecError> {
        Self::from_response(&Response::parse(xml)?)
    }
}

/// `GetStatus maintenance`: a running maintenance job shows up in `jobinfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MaintenanceStatus {
    pub jobinfo: Option<JobInfo>,
}

impl MaintenanceStatus {
    pub fn from_response(r: &Response) -> Result<MaintenanceStatus, IvecError> {
        r.require_ok()?;
        Ok(MaintenanceStatus { jobinfo: JobInfo::from_node(r.param_set().child("jobinfo")) })
    }

    pub fn parse(xml: &str) -> Result<MaintenanceStatus, IvecError> {
        Self::from_response(&Response::parse(xml)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINT: &str = include_str!("../tests/fixtures/GetStatus-print.xml");
    const DEVICE: &str = include_str!("../tests/fixtures/GetStatus-device.xml");
    const MAINT: &str = include_str!("../tests/fixtures/GetStatus-maintenance.xml");

    #[test]
    fn print_status_fixture() {
        let s = PrintStatus::parse(PRINT).unwrap();
        assert_eq!(s.status, "idle");
        assert_eq!(s.support_code, "");
        assert_eq!(s.trays.len(), 2);
        assert_eq!(s.trays[0].bin, "inputbin_p1");
        assert_eq!(s.trays[0].paper_type.as_deref(), Some("stationery"));
        assert_eq!(s.trays[0].paper_size.as_deref(), Some("iso_a4_210x297mm"));
        assert_eq!(s.trays[1].width, Some(21000));
        assert_eq!(s.trays[1].height, Some(29700));
        assert_eq!(s.inks.len(), 5);
        let m = &s.inks[0];
        assert_eq!((m.color.as_str(), m.level, m.order), ("M", Some(60), Some(1)));
        assert_eq!(m.model, "XKI-N21<M>SETUP");
        assert_eq!(m.icon, InkIcon::None);
        let pgbk = s.inks.iter().find(|i| i.color == "PGBK").unwrap();
        assert_eq!(pgbk.model, "XKI-N20<PGBK>SETUP");
        assert_eq!(pgbk.level, Some(60));
        assert!(s.jobinfo.is_none());
    }

    #[test]
    fn print_status_does_not_leak_identifiers() {
        let json = format!("{:?}", PrintStatus::parse(PRINT).unwrap());
        assert!(!json.contains("REDACTED") && !json.to_lowercase().contains("gci"));
    }

    #[test]
    fn device_status_fixture() {
        let d = DeviceStatus::parse(DEVICE).unwrap();
        assert_eq!(d.total_paper_num.get("inputbin_p1"), Some(&6));
        assert_eq!(d.total_paper_num.get("inputbin_p2"), Some(&54));
        assert_eq!(d.ink_replacement_count.len(), 5);
        assert_eq!(d.ink_replacement_count.get("PGBK"), Some(&1));
        assert_eq!(d.genuineink_replacement_count.get("C"), Some(&1));
        assert_eq!(d.head_replacement_count, Some(1));
        assert_eq!(d.networkupdate_available.as_deref(), Some("unavailable"));
        assert_eq!(d.connection.as_deref(), Some("wifi"), "infrastructure_mode means Wi-Fi");
        let dbg = format!("{d:?}");
        assert!(!dbg.contains("00:00") && !dbg.contains("REDACTED"));
    }

    #[test]
    fn connection_kind_from_the_interface_sets() {
        let wired = DEVICE
            .replace("id=\"wireless0\"", "id=\"wired0\"")
            .replace("<vcn:item>infrastructure_mode</vcn:item>", "<vcn:item>wired_lan</vcn:item>");
        assert_eq!(DeviceStatus::parse(&wired).unwrap().connection.as_deref(), Some("lan"));
        let ap_only = DEVICE.replace("infrastructure_mode", "access_point_mode");
        assert_eq!(DeviceStatus::parse(&ap_only).unwrap().connection, None);
        let none = DEVICE
            .replace("<ivec:network_interface>", "<ivec:x>")
            .replace("</ivec:network_interface>", "</ivec:x>");
        assert_eq!(DeviceStatus::parse(&none).unwrap().connection, None);
    }

    #[test]
    fn maintenance_status_idle_and_running() {
        assert!(MaintenanceStatus::parse(MAINT).unwrap().jobinfo.is_none());
        let running = MAINT.replace(
            "<ivec:jobinfo/>",
            "<ivec:jobinfo><ivec:jobID>00000002</ivec:jobID><ivec:state>running</ivec:state></ivec:jobinfo>",
        );
        let j = MaintenanceStatus::parse(&running).unwrap().jobinfo.unwrap();
        assert!(j.fields.contains(&("jobID".to_string(), "00000002".to_string())));
        assert!(j.fields.contains(&("state".to_string(), "running".to_string())));
    }

    #[test]
    fn unknown_elements_and_missing_fields_are_tolerated() {
        let xml = PRINT
            .replace(
                "<ivec:status>idle</ivec:status>",
                "<ivec:status>printing</ivec:status><vcn:novel a=\"1\"><x/></vcn:novel>",
            )
            .replace("<ivec:level>60</ivec:level>", "");
        let s = PrintStatus::parse(&xml).unwrap();
        assert_eq!(s.status, "printing");
        assert_eq!(s.inks[0].level, None);
    }

    #[test]
    fn ng_reply_is_an_error() {
        assert!(PrintStatus::parse(include_str!("../tests/fixtures/GetStatus-eis.xml")).is_err());
        assert!(DeviceStatus::parse(include_str!("../tests/fixtures/GetStatus-scan.xml")).is_err());
    }
}
