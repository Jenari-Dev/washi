// SPDX-License-Identifier: GPL-2.0-or-later
//! Network access to Canon CHMP printers.
//!
//! * [`ChmpClient`]: Canon's HTTP management channel (CHMP 1.3.0) on a plain `TcpStream` with a
//!   minimal HTTP/1.1 implementation. Reads (`query`) are always allowed; anything that changes
//!   the printer goes through [`ChmpClient::execute`], which needs an explicit [`Confirm`].
//! * [`ipp`]: the IPP TLV codec and client. `Get-Printer-Attributes` / `Get-Job-Attributes` /
//!   `Get-Jobs` are reads (they keep answering `marker-levels` when the CHMP print service is
//!   asleep); `Print-Job`, `Validate-Job`, `Cancel-Job` and `Identify-Printer` need a [`Confirm`].
//! * [`escl`]: the eSCL scanner client. Capabilities and status are reads; creating a job and
//!   fetching pages need a [`Confirm`].
//! * [`web`]: the links the printer's own web server publishes (`model.js`), read-only.
//! * [`discover`]: mDNS discovery of `_canon-chmp._tcp`.

mod client;
mod discovery;
mod error;
pub mod escl;
pub mod http;
pub mod ipp;
mod tcp;
pub mod web;

pub use client::{ChmpClient, ClientOptions, Confirm, DEFAULT_PATH};
pub use discovery::{DiscoveredPrinter, discover, discover_first};
pub use error::NetError;
pub use escl::{EsclClient, EsclOptions, ScanSettings, ScannerCapabilities, ScannerStatus};
pub use ipp::{IppClient, IppOptions, PrinterAttributes};
pub use web::{ModelLinks, WebOptions, fetch_model_links};
