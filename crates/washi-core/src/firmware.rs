// SPDX-License-Identifier: GPL-2.0-or-later
//! Firmware: the version, whether the printer says an update is available, and the update itself.
//!
//! **The update is UNVERIFIED on a real printer.** The version and the availability are reads
//! (`GetCapability device` -> `firmver`, `GetStatus device` -> `networkupdate_available`); the
//! only value seen so far is `unavailable`. The update request is the one Canon's own app sends:
//! `VendorCmd` with `<vcn:ijoperation>UpdateFRom</vcn:ijoperation>` on the `device` service,
//! without a job. The printer then fetches and installs the firmware by itself, restarts, and
//! comes back, which takes several minutes. Washi sends the request, waits for the printer to go
//! away and come back, and reads the version again. Nothing is flashed from this computer, and
//! the printer must not be switched off meanwhile.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use washi_ivec::{Request, Service};
use washi_net::Confirm;

use crate::error::{Error, Result};
use crate::plan::{Plan, Step};
use crate::printer::Printer;

/// What the printer says about a firmware update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FirmwareUpdate {
    /// The printer did not say (not read yet, or no such field).
    #[default]
    Unknown,
    /// `unavailable`: the firmware is current.
    Unavailable,
    /// `available`, or any other value the printer reports.
    Available,
}

impl FirmwareUpdate {
    /// From `networkupdate_available`. Only `unavailable` means "no update"; every other
    /// non-empty value counts as an update being offered.
    pub fn from_reported(value: Option<&str>) -> FirmwareUpdate {
        match value.map(str::trim) {
            None | Some("") => FirmwareUpdate::Unknown,
            Some(v) if v.eq_ignore_ascii_case("unavailable") => FirmwareUpdate::Unavailable,
            Some(_) => FirmwareUpdate::Available,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FirmwareUpdate::Unknown => "unknown",
            FirmwareUpdate::Unavailable => "unavailable",
            FirmwareUpdate::Available => "available",
        }
    }
}

/// Version and update availability, read now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirmwareStatus {
    pub version: String,
    pub update: FirmwareUpdate,
    /// The raw `networkupdate_available` value.
    pub reported: Option<String>,
    /// The printer is in auto power-off. It does not check for updates then and says
    /// `unavailable` whatever is true, so `update` is not that answer: see `remembered`.
    pub asleep: bool,
    /// `update` is what the printer said the last time it was on (from the earlier snapshot
    /// given), `unknown` when there is none.
    pub remembered: bool,
}

/// Timing of the update wait.
#[derive(Debug, Clone)]
pub struct FirmwareOptions {
    pub poll_interval: Duration,
    /// How long to wait for the printer to go away. If it never does, no update was installed.
    pub gone_timeout: Duration,
    /// How long to wait for the printer to come back.
    pub back_timeout: Duration,
    /// Consecutive failed (or successful) reads that count as gone (or back).
    pub stable_polls: u32,
}

impl Default for FirmwareOptions {
    fn default() -> Self {
        FirmwareOptions {
            poll_interval: Duration::from_secs(3),
            gone_timeout: Duration::from_secs(10 * 60),
            back_timeout: Duration::from_secs(30 * 60),
            stable_polls: 2,
        }
    }
}

/// Progress events for UIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirmwareProgress {
    /// The printer accepted the update command.
    Sent,
    /// The printer stopped answering: it is restarting.
    Restarting,
    /// Still waiting; `restarted` says which wait it is.
    Waiting { elapsed: Duration, restarted: bool },
    /// The printer answers again.
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareReport {
    /// The version before the update (empty if it could not be read).
    pub before: String,
    /// The version after the printer came back (empty if it could not be read).
    pub after: String,
    /// The printer went away and came back. When false, nothing was installed.
    pub restarted: bool,
    pub elapsed: Duration,
}

/// What an update would send, without touching the network.
pub fn plan_update() -> Plan {
    Plan {
        title: "firmware update (UpdateFRom, unverified)".to_string(),
        steps: vec![
            Step::Send(Request::update_firmware()),
            Step::Poll(
                "poll GetStatus device until the printer stops answering and then answers again, \
                 then read GetCapability device for the new version"
                    .to_string(),
            ),
        ],
    }
}

impl Printer {
    /// The firmware version and whether an update is offered (three reads).
    pub fn firmware_status(&self) -> Result<FirmwareStatus> {
        self.firmware_status_with(None)
    }

    /// Like [`Printer::firmware_status`]. A printer in auto power-off answers `unavailable` to
    /// the question even when an update is on offer, so there that answer is ignored and the
    /// one from the last time the printer was on is used instead: `last` is the snapshot kept
    /// from then (`unknown` without one). `available` is believed whenever it is read.
    pub fn firmware_status_with(
        &self,
        last: Option<&crate::snapshot::PrinterSnapshot>,
    ) -> Result<FirmwareStatus> {
        let cap = self.device_capability()?;
        let status = self.device_status()?;
        let asleep = !self.print_service_awake();
        let said = FirmwareUpdate::from_reported(status.networkupdate_available.as_deref());
        let (update, remembered) = if asleep && said != FirmwareUpdate::Available {
            match crate::snapshot::carried_update(last) {
                Some((v, _)) => (v, true),
                None => (FirmwareUpdate::Unknown, false),
            }
        } else {
            (said, false)
        };
        Ok(FirmwareStatus {
            version: cap.firmver,
            update,
            reported: status.networkupdate_available,
            asleep,
            remembered,
        })
    }

    fn answers(&self) -> bool {
        self.client().query(&Request::get_status(Service::Device)).is_ok()
    }

    /// Ask the printer to update its firmware, then wait until it has restarted and read the new
    /// version. **Unverified on a real printer.** Refuses unless the printer says an update is
    /// available. WRITES: needs a [`Confirm`].
    pub fn update_firmware(
        &self,
        confirm: Confirm,
        opts: &FirmwareOptions,
        progress: &mut dyn FnMut(FirmwareProgress),
    ) -> Result<FirmwareReport> {
        let before = self.firmware_status()?;
        if before.asleep && (before.update != FirmwareUpdate::Available || before.remembered) {
            return Err(Error::Invalid(
                "the printer is in auto power-off and only checks for firmware updates while it \
                 is on: wake it first (`washi power on`) and try again"
                    .to_string(),
            ));
        }
        match before.update {
            FirmwareUpdate::Available => {}
            FirmwareUpdate::Unavailable => {
                return Err(Error::Invalid(
                    "the printer says its firmware is up to date: no update is available"
                        .to_string(),
                ));
            }
            FirmwareUpdate::Unknown => {
                return Err(Error::Invalid(
                    "the printer does not say whether a firmware update is available".to_string(),
                ));
            }
        }
        let t0 = Instant::now();
        let mut client = self.client();
        client.execute(&Request::update_firmware(), confirm)?.require_ok()?;
        client.close();
        progress(FirmwareProgress::Sent);

        // Phase 1: the printer goes away.
        let mut failures = 0u32;
        let mut restarted = false;
        let gone_deadline = Instant::now() + opts.gone_timeout;
        while Instant::now() < gone_deadline {
            sleep(opts.poll_interval);
            if self.answers() {
                failures = 0;
                progress(FirmwareProgress::Waiting { elapsed: t0.elapsed(), restarted: false });
            } else {
                failures += 1;
                if failures >= opts.stable_polls {
                    restarted = true;
                    break;
                }
            }
        }
        if !restarted {
            // It never went away: nothing was installed (or the printer did not need to restart).
            let after = self.device_capability().map(|c| c.firmver).unwrap_or_default();
            return Ok(FirmwareReport {
                before: before.version,
                after,
                restarted: false,
                elapsed: t0.elapsed(),
            });
        }
        progress(FirmwareProgress::Restarting);

        // Phase 2: the printer comes back.
        let mut ok = 0u32;
        let back_deadline = Instant::now() + opts.back_timeout;
        loop {
            if Instant::now() >= back_deadline {
                return Err(Error::Timeout(format!(
                    "the printer did not come back within {:?} after the firmware update",
                    opts.back_timeout
                )));
            }
            sleep(opts.poll_interval);
            if self.answers() {
                ok += 1;
                if ok >= opts.stable_polls {
                    break;
                }
            } else {
                ok = 0;
                progress(FirmwareProgress::Waiting { elapsed: t0.elapsed(), restarted: true });
            }
        }
        progress(FirmwareProgress::Back);
        let after = self.device_capability().map(|c| c.firmver).unwrap_or_default();
        Ok(FirmwareReport { before: before.version, after, restarted: true, elapsed: t0.elapsed() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unavailable_means_no_update() {
        use FirmwareUpdate::*;
        assert_eq!(FirmwareUpdate::from_reported(None), Unknown);
        assert_eq!(FirmwareUpdate::from_reported(Some("")), Unknown);
        assert_eq!(FirmwareUpdate::from_reported(Some("  ")), Unknown);
        assert_eq!(FirmwareUpdate::from_reported(Some("unavailable")), Unavailable);
        assert_eq!(FirmwareUpdate::from_reported(Some("Unavailable")), Unavailable);
        assert_eq!(FirmwareUpdate::from_reported(Some("available")), Available);
        assert_eq!(FirmwareUpdate::from_reported(Some("downloading")), Available);
        assert_eq!(FirmwareUpdate::Available.as_str(), "available");
    }

    #[test]
    fn the_plan_is_one_vendor_command() {
        let plan = plan_update();
        let xml: Vec<_> = plan.requests().map(|r| r.to_xml()).collect();
        assert_eq!(xml.len(), 1);
        assert!(xml[0].contains("<ivec:operation>VendorCmd</ivec:operation>"), "{}", xml[0]);
        assert!(xml[0].contains("<vcn:ijoperation>UpdateFRom</vcn:ijoperation>"));
        assert!(xml[0].contains("servicetype=\"device\""));
        assert!(!xml[0].contains("jobID"), "no job bracket");
        assert!(plan.requests().all(|r| !r.is_read_only()));
    }
}
