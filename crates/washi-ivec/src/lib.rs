// SPDX-License-Identifier: GPL-2.0-or-later
//! Canon IVEC: the XML command language spoken over CHMP by Canon inkjet printers.
//!
//! This crate does no I/O. It builds request envelopes ([`Request`]) and parses replies
//! ([`Response`] and the typed views [`PrintStatus`], [`DeviceStatus`], [`DeviceConfig`],
//! [`DeviceCapability`], [`MaintenanceCapability`], [`MaintenanceStatus`]).
//!
//! Parsing is deliberately tolerant: unknown elements and attributes are ignored, and every
//! typed field that the device may omit is optional. Device identifiers (serial numbers,
//! hardware addresses, `gci*` blocks, cloud redirect URLs) are never copied into typed
//! structs; [`redact`] blanks them in raw XML for display.

mod capability;
mod config;
mod error;
mod request;
mod response;
mod settings;
mod status;
mod xml;

pub use capability::{CleaningCap, SilentRanges};
pub use capability::{DeviceCapability, MaintenanceCapability, PowerRanges, Range, TestPrintCap};
pub use config::{DeviceConfig, PowerTimer, SilentMode};
pub use error::IvecError;
pub use request::{
    CleaningType, InkGroup, JobId, NS_IVEC, NS_VCN, ReadOp, Request, Service, TestPrintKind,
    VendorCommand,
};
pub use response::Response;
pub use settings::{AutoPowerOff, DeviceSettings, OnOff, SilentKind, SilentSetting, validate_hhmm};
pub use status::{DeviceStatus, Ink, InkIcon, JobInfo, MaintenanceStatus, PrintStatus, Tray};
pub use xml::{Node, escape, pretty, redact};
