// SPDX-License-Identifier: GPL-2.0-or-later
//! Washi core: the printer handle, configuration file, and the multi-step flows (maintenance,
//! settings) shared by the CLI and later front-ends.
//!
//! Safety model: reads are free; every write goes through a [`Confirm`] token, and the flows
//! also exist as [`Plan`]s that list the exact requests without touching the network.

mod cache;
mod config;
pub mod disc;
mod error;
pub mod firmware;
mod lang;
pub mod links;
pub mod localization;
mod maintenance;
pub mod pdf;
mod plan;
pub mod print;
mod printer;
pub mod pwg;
pub mod scan;
mod settings;
mod snapshot;
mod spec;
pub mod support;
pub mod tools;

pub use washi_ivec as ivec;
pub use washi_net as net;
pub use washi_net::Confirm;

pub use cache::{SnapshotCache, printer_key};
pub use config::{
    Config, DaemonConfig, MIN_POLL_INTERVAL_SECS, NotificationConfig, PrinterConfig, UiConfig,
};
pub use error::{Error, Result};
pub use firmware::{
    FirmwareOptions, FirmwareProgress, FirmwareReport, FirmwareStatus, FirmwareUpdate,
};
pub use lang::{Language, LanguageSetting, system_language};
pub use links::{Link, LinkSource, WebLinks, WebSources};
pub use localization::{
    CachedIcon, CachedStrings, FetchedIcon, FetchedStrings, LocalizationCache, MAX_AGE_SECS,
};
pub use maintenance::{MaintOp, MaintOptions, MaintReport, Progress};
pub use plan::{Plan, Step};
pub use printer::{HostSource, Printer, resolve_host, resolve_ipv4, split_host_port, unix_now};
pub use settings::{
    ConfigChange, Mismatch, SettingsReport, config_changes, diff_settings, plan_settings,
    validate_settings,
};
pub use snapshot::{
    Connection, DeviceInfo, InkLevel, InkSource, InkStatus, OFFLINE_MESSAGE, Observation, Power,
    PrintProbe, PrinterSnapshot, SLEEPING_MESSAGE, Support, ink_code_from_marker, ink_display_name,
};
pub use spec::{MaintSpec, SettingsSpec, parse_auto_power_off, parse_quiet};
pub use support::{Severity, SupportInfo};
pub use tools::Tools;

/// Plan for `PowerOn` (no job bracket, power-on mode `ModeA`). Verified on a real XK140
/// (firmware 1.060, 2026-10-14): the printer wakes up.
pub fn plan_power_on() -> Plan {
    Plan {
        title: "wake the printer (PowerOn, mode ModeA)".to_string(),
        steps: vec![Step::Send(washi_ivec::Request::power_on())],
    }
}

/// Plan for `PowerOff` (no job bracket).
pub fn plan_power_off() -> Plan {
    Plan {
        title: "power off".to_string(),
        steps: vec![Step::Send(washi_ivec::Request::power_off())],
    }
}
