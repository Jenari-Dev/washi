// SPDX-License-Identifier: GPL-2.0-or-later
//! Maintenance flow: `StartJob` -> operation -> poll until the job ends -> `EndJob`.

use std::thread::sleep;
use std::time::{Duration, Instant};

use washi_ivec::{
    CleaningType, InkGroup, JobId, JobInfo, MaintenanceStatus, PrintStatus, Request, Response,
    Service, TestPrintKind,
};
use washi_net::{ChmpClient, Confirm};

use crate::error::{Error, Result};
use crate::plan::{Plan, Step};
use crate::printer::Printer;

/// A maintenance operation of the Utilities / Maintenance menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintOp {
    Clean {
        kind: CleaningType,
        group: InkGroup,
    },
    /// Print the nozzle check pattern (1 sheet).
    NozzleCheck,
    /// Automatic print head alignment (2 sheets).
    AlignAuto,
    /// Print the alignment check sheet (1 sheet).
    AlignCheck,
    /// Print the manual alignment pattern for set 1..=3 (3 sheets).
    AlignManual(u8),
    RollerClean,
}

impl MaintOp {
    pub fn label(&self) -> String {
        match self {
            MaintOp::Clean { kind: CleaningType::Roller, .. } => {
                "paper feed roller cleaning".to_string()
            }
            MaintOp::Clean { kind: CleaningType::Platen, .. } => {
                "bottom plate cleaning".to_string()
            }
            MaintOp::Clean { kind, group } => {
                format!("{} cleaning (ink group {})", kind.as_str(), group.as_str())
            }
            MaintOp::NozzleCheck => "nozzle check".to_string(),
            MaintOp::AlignAuto => "automatic print head alignment".to_string(),
            MaintOp::AlignCheck => "alignment check".to_string(),
            MaintOp::AlignManual(id) => format!("manual alignment pattern {id}"),
            MaintOp::RollerClean => "paper feed roller cleaning".to_string(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let MaintOp::AlignManual(id) = self {
            if !(1..=3).contains(id) {
                return Err(Error::Invalid(format!(
                    "manual alignment id must be 1, 2 or 3, got {id}"
                )));
            }
        }
        Ok(())
    }

    /// The operation request inside the job bracket.
    pub fn request(&self, job: &JobId) -> Request {
        let job = job.clone();
        match self {
            MaintOp::Clean { kind, group } => Request::cleaning(job, *kind, *group),
            MaintOp::NozzleCheck => Request::test_print(job, TestPrintKind::NozzleCheck),
            MaintOp::AlignAuto => Request::test_print(job, TestPrintKind::AutoRegistration),
            MaintOp::AlignCheck => Request::test_print(job, TestPrintKind::RegiCheck),
            MaintOp::AlignManual(id) => {
                Request::test_print(job, TestPrintKind::ManualRegistration(*id))
            }
            MaintOp::RollerClean => Request::roller_cleaning(job),
        }
    }

    /// What `run_maintenance` would send, without touching the network.
    pub fn plan(&self) -> Result<Plan> {
        self.validate()?;
        let job = JobId::standard();
        Ok(Plan {
            title: self.label(),
            steps: vec![
                Step::Send(Request::start_job(Service::Maintenance, job.clone())),
                Step::Send(self.request(&job)),
                Step::Poll(
                    "poll GetStatus print + GetStatus maintenance until the printer is idle again"
                        .to_string(),
                ),
                Step::Send(Request::end_job(Service::Maintenance, job)),
            ],
        })
    }
}

/// Timing of the completion wait.
#[derive(Debug, Clone)]
pub struct MaintOptions {
    /// Give up waiting for the job to end.
    pub timeout: Duration,
    pub poll_interval: Duration,
    /// Do not accept "idle" as completion before this long if the printer was never seen busy.
    pub start_grace: Duration,
    /// If the printer is idle but `jobinfo` still lists a job for this long, call it finished.
    pub stuck_job_grace: Duration,
}

impl Default for MaintOptions {
    fn default() -> Self {
        MaintOptions {
            timeout: Duration::from_secs(600),
            poll_interval: Duration::from_secs(2),
            start_grace: Duration::from_secs(8),
            stuck_job_grace: Duration::from_secs(20),
        }
    }
}

/// Progress events for UIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// `StartJob` was accepted.
    JobStarted,
    /// The operation request was accepted; the printer starts working.
    CommandAccepted { operation: String },
    /// A status observation (on change, and every ~10 s).
    Waiting { elapsed: Duration, status: String, support_code: String, job: Option<JobInfo> },
    /// The printer reports it is done.
    JobEnded,
    /// `EndJob` is being sent.
    Ending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintReport {
    pub operation: String,
    pub elapsed: Duration,
}

fn is_suspended(r: &Response) -> bool {
    !r.ok && r.detail.eq_ignore_ascii_case("Suspended")
}

/// Result of an `EndJob`. The XK140 closes a finished maintenance job by itself and then answers
/// `EndJob` with `NG NotStart` (observed 2026-10-13, traced nozzle check) — the job is over either
/// way, so that reply counts as success.
pub(crate) fn end_job_outcome(r: &Response) -> Result<()> {
    if !r.ok && r.detail.eq_ignore_ascii_case("NotStart") {
        return Ok(());
    }
    r.require_ok().map(|_| ()).map_err(Error::from)
}

fn is_idle(s: &PrintStatus) -> bool {
    s.status.eq_ignore_ascii_case("idle")
}

/// Wait for the job to end. The completion signal is: print status `idle` (and no `jobinfo`)
/// on two consecutive polls, after the printer was seen busy or `start_grace` has passed.
fn wait_for_end(
    client: &mut ChmpClient,
    opts: &MaintOptions,
    progress: &mut dyn FnMut(Progress),
) -> Result<()> {
    let t0 = Instant::now();
    let mut saw_busy = false;
    let mut idle_polls = 0u32;
    let mut stuck_since: Option<Instant> = None;
    let mut last_emit: Option<(Instant, String, String, bool)> = None;
    let mut last_status = String::new();
    let mut last_code = String::new();
    loop {
        if t0.elapsed() > opts.timeout {
            return Err(Error::Timeout(format!(
                "the printer still reports {:?}{} after {:?}",
                last_status,
                if last_code.is_empty() {
                    String::new()
                } else {
                    format!(" (support code {last_code})")
                },
                opts.timeout
            )));
        }
        // While a maintenance job runs the XK140 answers `GetStatus print` with `NG Suspended`
        // (observed on the first real nozzle check): the print service is held by the job, so
        // that reply means "busy", not an error.
        let pr = client.query(&Request::get_status(Service::Print))?;
        let ps = if is_suspended(&pr) {
            PrintStatus { status: "suspended".to_string(), ..Default::default() }
        } else {
            PrintStatus::from_response(&pr)?
        };
        let mr = client.query(&Request::get_status(Service::Maintenance))?;
        let ms = if is_suspended(&mr) {
            MaintenanceStatus { jobinfo: None }
        } else {
            MaintenanceStatus::from_response(&mr)?
        };
        let idle = is_idle(&ps);
        let job_listed = ms.jobinfo.is_some();
        last_status = ps.status.clone();
        last_code = ps.support_code.clone();

        let key = (ps.status.clone(), ps.support_code.clone(), job_listed);
        let due = match &last_emit {
            None => true,
            Some((at, s, c, j)) => {
                (s, c, j) != (&key.0, &key.1, &key.2) || at.elapsed() >= Duration::from_secs(10)
            }
        };
        if due {
            progress(Progress::Waiting {
                elapsed: t0.elapsed(),
                status: ps.status.clone(),
                support_code: ps.support_code.clone(),
                job: ms.jobinfo.clone(),
            });
            last_emit = Some((Instant::now(), key.0, key.1, key.2));
        }

        if !idle || job_listed {
            saw_busy = true;
        }
        if idle && !job_listed {
            idle_polls += 1;
        } else {
            idle_polls = 0;
        }
        if idle && job_listed {
            let since = *stuck_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= opts.stuck_job_grace {
                return Ok(());
            }
        } else {
            stuck_since = None;
        }
        if idle_polls >= 2 && (saw_busy || t0.elapsed() >= opts.start_grace) {
            return Ok(());
        }
        sleep(opts.poll_interval);
    }
}

impl Printer {
    /// Run a maintenance operation: `StartJob` (maintenance, job `00000002`, bidi on), wait for
    /// OK, the operation, poll until the job ends, `EndJob`. `EndJob` is attempted even when the
    /// operation or the wait failed.
    pub fn run_maintenance(
        &self,
        op: &MaintOp,
        confirm: Confirm,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<MaintReport> {
        op.validate()?;
        let job = JobId::standard();
        let mut client = self.client();
        let t0 = Instant::now();

        match client.execute(&Request::start_job(Service::Maintenance, job.clone()), confirm) {
            Ok(r) => {
                r.require_ok()?;
            }
            Err(e) => {
                // The command may have reached the printer even though the reply was lost.
                let _ = client.execute(&Request::end_job(Service::Maintenance, job), confirm);
                client.close();
                return Err(e.into());
            }
        }
        progress(Progress::JobStarted);

        let outcome = (|| -> Result<()> {
            client.execute(&op.request(&job), confirm)?.require_ok()?;
            progress(Progress::CommandAccepted { operation: op.label() });
            wait_for_end(&mut client, &self.maint, progress)?;
            progress(Progress::JobEnded);
            Ok(())
        })();

        progress(Progress::Ending);
        let end = client
            .execute(&Request::end_job(Service::Maintenance, job), confirm)
            .map_err(Error::from)
            .and_then(|r| end_job_outcome(&r));
        client.close();

        match (outcome, end) {
            (Ok(()), Ok(())) => Ok(MaintReport { operation: op.label(), elapsed: t0.elapsed() }),
            (Ok(()), Err(e)) => Err(e),
            (Err(e), Ok(())) => Err(e),
            (Err(e), Err(cleanup)) => {
                Err(Error::WithCleanupFailure { error: Box::new(e), cleanup: cleanup.to_string() })
            }
        }
    }

    /// `PowerOn` (device service, no job): wake a printer that is in auto power-off.
    ///
    /// Verified on a real XK140 (firmware 1.060, 2026-10-14). The request carries the power-on
    /// mode `ModeA` (see [`Request::PowerOn`]); without it the printer answers
    /// `NG ParameterError`. The reply only says the command was accepted: look at the print
    /// status afterwards to see the printer come up (it takes a few seconds).
    pub fn power_on(&self, confirm: Confirm) -> Result<()> {
        let mut client = self.client();
        let r = client.execute(&Request::power_on(), confirm)?;
        r.require_ok()?;
        client.close();
        Ok(())
    }

    /// `PowerOff` (device service, no job).
    pub fn power_off(&self, confirm: Confirm) -> Result<()> {
        let mut client = self.client();
        let r = client.execute(&Request::power_off(), confirm)?;
        r.require_ok()?;
        client.close();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_brackets_the_operation() {
        let plan = MaintOp::NozzleCheck.plan().unwrap();
        let ops: Vec<_> = plan.requests().map(|r| r.operation()).collect();
        assert_eq!(ops, ["StartJob", "TestPrint", "EndJob"]);
        assert!(matches!(plan.steps[2], Step::Poll(_)));
        assert!(plan.requests().all(|r| !r.is_read_only()));
    }

    #[test]
    fn op_to_request_mapping() {
        let job = JobId::standard();
        let x = |op: MaintOp| op.request(&job).to_xml();
        assert!(
            x(MaintOp::Clean { kind: CleaningType::Deep, group: InkGroup::Group1 })
                .contains("<ivec:type>deep</ivec:type><ivec:inkgroup>group1</ivec:inkgroup>")
        );
        assert!(x(MaintOp::NozzleCheck).contains("nozzle_check"));
        assert!(x(MaintOp::AlignAuto).contains("auto_registration"));
        assert!(x(MaintOp::AlignCheck).contains("regi_check"));
        assert!(x(MaintOp::AlignManual(2)).contains("<ivec:id>2</ivec:id>"));
        assert!(
            x(MaintOp::RollerClean).contains("<ivec:operation>RollerCleaning</ivec:operation>")
        );
        // The roller and the bottom plate are `Cleaning` types without an ink group.
        let roller = x(MaintOp::Clean { kind: CleaningType::Roller, group: InkGroup::All });
        assert!(roller.contains("<ivec:operation>Cleaning</ivec:operation>"), "{roller}");
        assert!(roller.contains("<ivec:type>roller</ivec:type>") && !roller.contains("inkgroup"));
        let platen = x(MaintOp::Clean { kind: CleaningType::Platen, group: InkGroup::All });
        assert!(platen.contains("<ivec:type>platen</ivec:type>") && !platen.contains("inkgroup"));
    }

    #[test]
    fn manual_alignment_id_is_validated() {
        assert!(MaintOp::AlignManual(0).plan().is_err());
        assert!(MaintOp::AlignManual(4).plan().is_err());
        assert!(MaintOp::AlignManual(3).plan().is_ok());
    }
}
