// SPDX-License-Identifier: GPL-2.0-or-later
//! Print jobs over IPP: `Print-Job`, `Validate-Job`, `Get-Job-Attributes`, `Get-Jobs`,
//! `Cancel-Job` and `Identify-Printer`.
//!
//! Reads (`Get-Job-Attributes`, `Get-Jobs`, the capability and media queries) are free.
//! Everything that starts, changes or cancels device activity takes a [`Confirm`]. The request
//! builders ([`IppClient::print_job_request`] and friends) never touch the network, so the CLI
//! can show the exact request in a dry run.

use std::fmt::Write as _;
use std::io::Read;

use serde::Serialize;

use super::media::MediaDatabase;
use super::{
    IppAttribute, IppClient, IppError, IppGroup, IppMessage, IppValue, op, status_name, tag,
};
use crate::client::Confirm;
use crate::error::NetError;

/// IPP version used for the job operations (the printer offers 1.1 and 2.0).
const JOB_IPP_VERSION: (u8, u8) = (2, 0);

/// `sides`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Sides {
    OneSided,
    TwoSidedLongEdge,
    TwoSidedShortEdge,
}

impl Sides {
    pub fn as_str(self) -> &'static str {
        match self {
            Sides::OneSided => "one-sided",
            Sides::TwoSidedLongEdge => "two-sided-long-edge",
            Sides::TwoSidedShortEdge => "two-sided-short-edge",
        }
    }

    pub fn is_duplex(self) -> bool {
        self != Sides::OneSided
    }
}

/// `print-quality` (3 draft, 4 normal, 5 high).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Quality {
    Draft,
    Normal,
    High,
}

impl Quality {
    pub fn enum_value(self) -> i32 {
        match self {
            Quality::Draft => 3,
            Quality::Normal => 4,
            Quality::High => 5,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Draft => "draft",
            Quality::Normal => "normal",
            Quality::High => "high",
        }
    }
}

/// `print-color-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PrintColorMode {
    Color,
    Monochrome,
    Auto,
}

impl PrintColorMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PrintColorMode::Color => "color",
            PrintColorMode::Monochrome => "monochrome",
            PrintColorMode::Auto => "auto",
        }
    }
}

/// `print-scaling` (only meaningful for formats the printer scales itself, i.e. JPEG).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Scaling {
    None,
    Fill,
    Fit,
    AutoFit,
    Auto,
}

impl Scaling {
    pub fn as_str(self) -> &'static str {
        match self {
            Scaling::None => "none",
            Scaling::Fill => "fill",
            Scaling::Fit => "fit",
            Scaling::AutoFit => "auto-fit",
            Scaling::Auto => "auto",
        }
    }
}

/// Margins in 1/100 mm (`media-top-margin` ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Margins {
    pub top: i32,
    pub bottom: i32,
    pub left: i32,
    pub right: i32,
}

impl Margins {
    pub const ZERO: Margins = Margins { top: 0, bottom: 0, left: 0, right: 0 };

    pub fn is_zero(&self) -> bool {
        *self == Margins::ZERO
    }
}

/// The `media-col` of a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaSpec {
    /// `media-size` in 1/100 mm.
    pub width: i32,
    pub height: i32,
    /// e.g. `stationery`, `photographic`, `com.canon-mtdisca`.
    pub media_type: Option<String>,
    /// `auto`, `main`, `rear` or `disc`.
    pub source: Option<String>,
    pub margins: Option<Margins>,
}

impl MediaSpec {
    pub fn to_value(&self) -> IppValue {
        let int = |v: i32| vec![IppValue::Integer(v)];
        let mut m: Vec<(String, Vec<IppValue>)> = Vec::new();
        if let Some(g) = &self.margins {
            m.push(("media-bottom-margin".into(), int(g.bottom)));
            m.push(("media-left-margin".into(), int(g.left)));
            m.push(("media-right-margin".into(), int(g.right)));
        }
        m.push((
            "media-size".into(),
            vec![IppValue::Collection(vec![
                ("x-dimension".into(), int(self.width)),
                ("y-dimension".into(), int(self.height)),
            ])],
        ));
        if let Some(s) = &self.source {
            m.push(("media-source".into(), vec![IppValue::Keyword(s.clone())]));
        }
        if let Some(g) = &self.margins {
            m.push(("media-top-margin".into(), int(g.top)));
        }
        if let Some(t) = &self.media_type {
            m.push(("media-type".into(), vec![IppValue::Keyword(t.clone())]));
        }
        IppValue::Collection(m)
    }
}

/// One print job: the operation and job-template attributes. The document itself is passed to
/// [`IppClient::print_job`] separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrintJob {
    /// `job-name`.
    pub name: String,
    /// `requesting-user-name`.
    pub user: String,
    /// `document-format`, e.g. `image/pwg-raster` or `image/jpeg`.
    pub format: String,
    pub copies: u32,
    pub sides: Sides,
    pub quality: Quality,
    pub color: PrintColorMode,
    pub media: Option<MediaSpec>,
    pub scaling: Option<Scaling>,
    /// `printer-resolution` in dpi (the XK140 offers 600).
    pub resolution_dpi: Option<u32>,
    /// `ipp-attribute-fidelity`: true makes the printer refuse a job whose attributes it cannot
    /// honour instead of substituting.
    pub fidelity: bool,
}

impl PrintJob {
    pub fn new(name: impl Into<String>, format: impl Into<String>) -> PrintJob {
        PrintJob {
            name: name.into(),
            user: "washi".to_string(),
            format: format.into(),
            copies: 1,
            sides: Sides::OneSided,
            quality: Quality::Normal,
            color: PrintColorMode::Color,
            media: None,
            scaling: None,
            resolution_dpi: Some(600),
            fidelity: false,
        }
    }

    fn operation_attributes(&self, with_format: bool) -> Vec<IppAttribute> {
        let mut v = vec![
            IppAttribute::single("requesting-user-name", IppValue::Name(self.user.clone())),
            IppAttribute::single("job-name", IppValue::Name(self.name.clone())),
            IppAttribute::single("ipp-attribute-fidelity", IppValue::Boolean(self.fidelity)),
        ];
        if with_format {
            v.push(IppAttribute::single(
                "document-format",
                IppValue::MimeMediaType(self.format.clone()),
            ));
        }
        v
    }

    fn job_attributes(&self) -> Vec<IppAttribute> {
        let mut v = vec![
            IppAttribute::single("copies", IppValue::Integer(self.copies.clamp(1, 99) as i32)),
            IppAttribute::single("sides", IppValue::Keyword(self.sides.as_str().into())),
            IppAttribute::single("print-quality", IppValue::Enum(self.quality.enum_value())),
            IppAttribute::single("print-color-mode", IppValue::Keyword(self.color.as_str().into())),
        ];
        if let Some(m) = &self.media {
            v.push(IppAttribute::single("media-col", m.to_value()));
        }
        if let Some(dpi) = self.resolution_dpi {
            let dpi = dpi as i32;
            v.push(IppAttribute::single(
                "printer-resolution",
                IppValue::Resolution { x: dpi, y: dpi, units: 3 },
            ));
        }
        if let Some(s) = self.scaling {
            v.push(IppAttribute::single("print-scaling", IppValue::Keyword(s.as_str().into())));
        }
        v
    }
}

/// `job-state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum JobState {
    Pending,
    PendingHeld,
    Processing,
    ProcessingStopped,
    Canceled,
    Aborted,
    Completed,
    Unknown(i32),
}

impl JobState {
    pub fn from_enum(v: i32) -> JobState {
        match v {
            3 => JobState::Pending,
            4 => JobState::PendingHeld,
            5 => JobState::Processing,
            6 => JobState::ProcessingStopped,
            7 => JobState::Canceled,
            8 => JobState::Aborted,
            9 => JobState::Completed,
            other => JobState::Unknown(other),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::PendingHeld => "pending-held",
            JobState::Processing => "processing",
            JobState::ProcessingStopped => "processing-stopped",
            JobState::Canceled => "canceled",
            JobState::Aborted => "aborted",
            JobState::Completed => "completed",
            JobState::Unknown(_) => "unknown",
        }
    }

    /// The job will not change any more.
    pub fn is_terminal(self) -> bool {
        matches!(self, JobState::Canceled | JobState::Aborted | JobState::Completed)
    }
}

/// The job group of a reply (`Print-Job`, `Get-Job-Attributes`, `Get-Jobs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobAttributes {
    pub id: Option<i32>,
    pub uri: Option<String>,
    pub state: Option<JobState>,
    pub state_reasons: Vec<String>,
    pub state_message: Option<String>,
    pub impressions_completed: Option<i32>,
    pub sheets_completed: Option<i32>,
    /// Attributes the printer ignored or substituted (`unsupported-attributes` group); only
    /// filled on a successful reply with `successful-ok-ignored-or-substituted-attributes`.
    pub ignored: Vec<String>,
}

fn job_from_group(g: &IppGroup) -> JobAttributes {
    let int = |name: &str| g.get(name).and_then(|a| a.values.first()).and_then(IppValue::as_i32);
    let text = |name: &str| {
        g.get(name).and_then(|a| a.values.first()).and_then(IppValue::as_str).map(str::to_string)
    };
    JobAttributes {
        id: int("job-id"),
        uri: text("job-uri"),
        state: int("job-state").map(JobState::from_enum),
        state_reasons: g
            .get("job-state-reasons")
            .map(|a| a.values.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        state_message: text("job-state-message").filter(|m| !m.is_empty()),
        impressions_completed: int("job-impressions-completed"),
        sheets_completed: int("job-media-sheets-completed"),
        ignored: Vec::new(),
    }
}

fn check_status(msg: &IppMessage) -> Result<(), IppError> {
    if msg.code >= 0x0100 {
        return Err(IppError::Rejected {
            code: msg.code,
            name: status_name(msg.code),
            detail: msg.rejection_detail(),
        });
    }
    Ok(())
}

impl JobAttributes {
    /// The first job group of a successful reply. A reply with a client or server error status
    /// is an [`IppError::Rejected`] that names the attributes the printer did not like.
    pub fn from_message(msg: &IppMessage) -> Result<JobAttributes, IppError> {
        check_status(msg)?;
        let g = msg.group(tag::JOB_ATTRIBUTES).ok_or(IppError::NoJobAttributes)?;
        let mut job = job_from_group(g);
        job.ignored = msg
            .groups_with(tag::UNSUPPORTED_ATTRIBUTES)
            .flat_map(|g| g.attributes.iter().map(|a| a.name.clone()))
            .collect();
        Ok(job)
    }

    /// Every job group of a `Get-Jobs` reply (possibly none).
    pub fn all_from_message(msg: &IppMessage) -> Result<Vec<JobAttributes>, IppError> {
        check_status(msg)?;
        Ok(msg.groups_with(tag::JOB_ATTRIBUTES).map(job_from_group).collect())
    }
}

/// Outcome of `Validate-Job`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidateReply {
    pub status: u16,
    pub status_name: &'static str,
    /// Attributes the printer would ignore or substitute.
    pub ignored: Vec<String>,
}

/// `identify-actions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum IdentifyAction {
    Flash,
    Sound,
}

impl IdentifyAction {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentifyAction::Flash => "flash",
            IdentifyAction::Sound => "sound",
        }
    }
}

/// `which-jobs` for `Get-Jobs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichJobs {
    NotCompleted,
    Completed,
}

/// What the printer says about printing (read-only).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PrintCapabilities {
    pub accepting_jobs: Option<bool>,
    pub state: Option<String>,
    pub state_reasons: Vec<String>,
    pub document_formats: Vec<String>,
    /// `srgb_8`, `sgray_8`, ...
    pub raster_types: Vec<String>,
    /// dpi values of `pwg-raster-document-resolution-supported`.
    pub raster_resolutions: Vec<(i32, i32)>,
    pub raster_sheet_back: Option<String>,
    pub media_sources: Vec<String>,
    pub media_types: Vec<String>,
    pub sides: Vec<String>,
    pub copies_max: Option<i32>,
    pub jpeg_max_dimension: Option<i32>,
    pub jpeg_max_kilobytes: Option<i32>,
    pub job_creation_attributes: Vec<String>,
}

/// The attributes `print_capabilities` asks for.
pub const PRINT_ATTRIBUTES: &[&str] = &[
    "printer-state",
    "printer-state-reasons",
    "printer-is-accepting-jobs",
    "document-format-supported",
    "pwg-raster-document-type-supported",
    "pwg-raster-document-resolution-supported",
    "pwg-raster-document-sheet-back",
    "media-source-supported",
    "media-type-supported",
    "sides-supported",
    "copies-supported",
    "jpeg-x-dimension-supported",
    "jpeg-k-octets-supported",
    "job-creation-attributes-supported",
];

impl PrintCapabilities {
    pub fn from_message(msg: &IppMessage) -> Result<PrintCapabilities, IppError> {
        check_status(msg)?;
        let g = msg.group(tag::PRINTER_ATTRIBUTES).ok_or(IppError::NoPrinterAttributes)?;
        let strings = |name: &str| -> Vec<String> {
            g.get(name)
                .map(|a| a.values.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        let range_upper = |name: &str| -> Option<i32> {
            g.get(name).and_then(|a| {
                a.values.iter().find_map(|v| match v {
                    IppValue::Range { upper, .. } => Some(*upper),
                    _ => None,
                })
            })
        };
        Ok(PrintCapabilities {
            accepting_jobs: g.get("printer-is-accepting-jobs").and_then(|a| {
                a.values.iter().find_map(|v| match v {
                    IppValue::Boolean(b) => Some(*b),
                    _ => None,
                })
            }),
            state: g
                .get("printer-state")
                .and_then(|a| a.values.first())
                .and_then(IppValue::as_i32)
                .map(|s| {
                    match s {
                        3 => "idle",
                        4 => "processing",
                        5 => "stopped",
                        _ => "unknown",
                    }
                    .to_string()
                }),
            state_reasons: strings("printer-state-reasons"),
            document_formats: strings("document-format-supported"),
            raster_types: strings("pwg-raster-document-type-supported"),
            raster_resolutions: g
                .get("pwg-raster-document-resolution-supported")
                .map(|a| {
                    a.values
                        .iter()
                        .filter_map(|v| match v {
                            IppValue::Resolution { x, y, .. } => Some((*x, *y)),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            raster_sheet_back: strings("pwg-raster-document-sheet-back").into_iter().next(),
            media_sources: strings("media-source-supported"),
            media_types: strings("media-type-supported"),
            sides: strings("sides-supported"),
            copies_max: range_upper("copies-supported"),
            jpeg_max_dimension: range_upper("jpeg-x-dimension-supported"),
            jpeg_max_kilobytes: range_upper("jpeg-k-octets-supported"),
            job_creation_attributes: strings("job-creation-attributes-supported"),
        })
    }

    pub fn supports_format(&self, mime: &str) -> bool {
        self.document_formats.iter().any(|f| f.eq_ignore_ascii_case(mime))
    }
}

impl IppClient {
    fn job_message(
        &self,
        code: u16,
        request_id: u32,
        ops: Vec<IppAttribute>,
        job: Vec<IppAttribute>,
    ) -> IppMessage {
        let mut operation = self.base_operation_attributes();
        operation.extend(ops);
        let mut groups = vec![IppGroup::new(tag::OPERATION_ATTRIBUTES, operation)];
        if !job.is_empty() {
            groups.push(IppGroup::new(tag::JOB_ATTRIBUTES, job));
        }
        IppMessage { version: JOB_IPP_VERSION, code, request_id, groups }
    }

    /// The `Print-Job` request (without the document that follows it on the wire).
    pub fn print_job_request(&self, job: &PrintJob) -> IppMessage {
        self.job_message(op::PRINT_JOB, 2, job.operation_attributes(true), job.job_attributes())
    }

    /// The `Validate-Job` request: same attributes as `Print-Job`, no document.
    pub fn validate_job_request(&self, job: &PrintJob) -> IppMessage {
        self.job_message(op::VALIDATE_JOB, 1, job.operation_attributes(true), job.job_attributes())
    }

    pub fn get_job_attributes_request(&self, job_id: i32) -> IppMessage {
        self.job_message(
            op::GET_JOB_ATTRIBUTES,
            1,
            vec![
                IppAttribute::single("job-id", IppValue::Integer(job_id)),
                IppAttribute::keywords(
                    "requested-attributes",
                    &[
                        "job-id",
                        "job-uri",
                        "job-state",
                        "job-state-reasons",
                        "job-state-message",
                        "job-impressions-completed",
                        "job-media-sheets-completed",
                    ],
                ),
            ],
            vec![],
        )
    }

    pub fn cancel_job_request(&self, job_id: i32, user: &str) -> IppMessage {
        self.job_message(
            op::CANCEL_JOB,
            1,
            vec![
                IppAttribute::single("job-id", IppValue::Integer(job_id)),
                IppAttribute::single("requesting-user-name", IppValue::Name(user.into())),
            ],
            vec![],
        )
    }

    pub fn identify_request(&self, action: IdentifyAction, user: &str) -> IppMessage {
        self.job_message(
            op::IDENTIFY_PRINTER,
            1,
            vec![
                IppAttribute::single("requesting-user-name", IppValue::Name(user.into())),
                IppAttribute::keywords("identify-actions", &[action.as_str()]),
            ],
            vec![],
        )
    }

    /// `Validate-Job`: would the printer accept this job? Moves no paper, but it is a job
    /// operation, so it needs a [`Confirm`].
    pub fn validate_job(
        &self,
        job: &PrintJob,
        _confirm: Confirm,
    ) -> Result<ValidateReply, NetError> {
        let reply =
            self.exchange(&self.validate_job_request(job), None, self.options().io_timeout)?;
        check_status(&reply)?;
        Ok(ValidateReply {
            status: reply.code,
            status_name: status_name(reply.code),
            ignored: reply
                .groups_with(tag::UNSUPPORTED_ATTRIBUTES)
                .flat_map(|g| g.attributes.iter().map(|a| a.name.clone()))
                .collect(),
        })
    }

    /// `Print-Job`: send `len` bytes of `document` as one job. **Prints.** Needs a [`Confirm`].
    pub fn print_job(
        &self,
        job: &PrintJob,
        document: &mut dyn Read,
        len: u64,
        _confirm: Confirm,
    ) -> Result<JobAttributes, NetError> {
        let reply = self.exchange(
            &self.print_job_request(job),
            Some((document, len)),
            self.options().job_timeout,
        )?;
        Ok(JobAttributes::from_message(&reply)?)
    }

    /// `Get-Job-Attributes` (read-only).
    pub fn get_job_attributes(&self, job_id: i32) -> Result<JobAttributes, NetError> {
        let reply = self.exchange(
            &self.get_job_attributes_request(job_id),
            None,
            self.options().io_timeout,
        )?;
        Ok(JobAttributes::from_message(&reply)?)
    }

    /// `Get-Jobs` (read-only).
    pub fn get_jobs(&self, which: WhichJobs) -> Result<Vec<JobAttributes>, NetError> {
        let which = match which {
            WhichJobs::NotCompleted => "not-completed",
            WhichJobs::Completed => "completed",
        };
        let msg = self.job_message(
            op::GET_JOBS,
            1,
            vec![
                IppAttribute::keywords("which-jobs", &[which]),
                IppAttribute::keywords(
                    "requested-attributes",
                    &["job-id", "job-state", "job-state-reasons"],
                ),
            ],
            vec![],
        );
        let reply = self.exchange(&msg, None, self.options().io_timeout)?;
        Ok(JobAttributes::all_from_message(&reply)?)
    }

    /// `Cancel-Job`. Needs a [`Confirm`].
    pub fn cancel_job(&self, job_id: i32, user: &str, _confirm: Confirm) -> Result<(), NetError> {
        let reply =
            self.exchange(&self.cancel_job_request(job_id, user), None, self.options().io_timeout)?;
        Ok(check_status(&reply)?)
    }

    /// `Identify-Printer` ("find my printer": flash the display or beep). Needs a [`Confirm`].
    pub fn identify(
        &self,
        action: IdentifyAction,
        user: &str,
        _confirm: Confirm,
    ) -> Result<(), NetError> {
        let reply =
            self.exchange(&self.identify_request(action, user), None, self.options().io_timeout)?;
        Ok(check_status(&reply)?)
    }

    /// What the printer accepts for printing (read-only).
    pub fn print_capabilities(&self) -> Result<PrintCapabilities, NetError> {
        let msg = self.get_printer_attributes_raw(PRINT_ATTRIBUTES)?;
        Ok(PrintCapabilities::from_message(&msg)?)
    }

    /// The printer's `media-col-database` (read-only). It must be asked for on its own: a broad
    /// request returns it empty.
    pub fn media_database(&self) -> Result<MediaDatabase, NetError> {
        let msg = self.get_printer_attributes_raw(&["media-col-database"])?;
        Ok(MediaDatabase::from_message(&msg)?)
    }
}

// ---------------------------------------------------------------------------------------------
// Human-readable dumps (dry runs)
// ---------------------------------------------------------------------------------------------

fn tag_name(t: u8) -> &'static str {
    match t {
        tag::OPERATION_ATTRIBUTES => "operation-attributes-tag",
        tag::JOB_ATTRIBUTES => "job-attributes-tag",
        tag::PRINTER_ATTRIBUTES => "printer-attributes-tag",
        tag::UNSUPPORTED_ATTRIBUTES => "unsupported-attributes-tag",
        _ => "group-tag",
    }
}

fn type_name(v: &IppValue) -> &'static str {
    match v {
        IppValue::Integer(_) => "integer",
        IppValue::Boolean(_) => "boolean",
        IppValue::Enum(_) => "enum",
        IppValue::OctetString(_) => "octetString",
        IppValue::DateTime(_) => "dateTime",
        IppValue::Resolution { .. } => "resolution",
        IppValue::Range { .. } => "rangeOfInteger",
        IppValue::Collection(_) => "collection",
        IppValue::Text(_) => "text",
        IppValue::Name(_) => "name",
        IppValue::Keyword(_) => "keyword",
        IppValue::Uri(_) => "uri",
        IppValue::UriScheme(_) => "uriScheme",
        IppValue::Charset(_) => "charset",
        IppValue::NaturalLanguage(_) => "naturalLanguage",
        IppValue::MimeMediaType(_) => "mimeMediaType",
        IppValue::OutOfBand(_) => "out-of-band",
        IppValue::Other { .. } => "unknown",
    }
}

fn value_text(v: &IppValue) -> String {
    match v {
        IppValue::Collection(members) => {
            let inner: Vec<String> = members
                .iter()
                .map(|(k, vals)| {
                    let vs: Vec<String> = vals.iter().map(value_text).collect();
                    format!("{k}={}", vs.join(","))
                })
                .collect();
            format!("{{{}}}", inner.join(" "))
        }
        other => other.to_string(),
    }
}

impl IppMessage {
    /// An `ipptool`-style listing: operation, then every group and attribute with its type.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{} (IPP {}.{}, request-id {})",
            op::name(self.code),
            self.version.0,
            self.version.1,
            self.request_id
        );
        for g in &self.groups {
            let _ = writeln!(out, "  {}", tag_name(g.tag));
            for a in &g.attributes {
                let ty = a.values.first().map_or("", type_name);
                let vals: Vec<String> = a.values.iter().map(value_text).collect();
                let multi = if a.values.len() > 1 { "1setOf " } else { "" };
                let _ = writeln!(out, "    {} ({multi}{ty}) = {}", a.name, vals.join(", "));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> IppClient {
        IppClient::new("printer.local")
    }

    fn a4_job() -> PrintJob {
        let mut job = PrintJob::new("report.pdf", "image/pwg-raster");
        job.media = Some(MediaSpec {
            width: 21000,
            height: 29700,
            media_type: Some("stationery".into()),
            source: Some("auto".into()),
            margins: Some(Margins { top: 500, bottom: 500, left: 340, right: 340 }),
        });
        job
    }

    fn get<'a>(g: &'a IppGroup, name: &str) -> &'a IppAttribute {
        g.get(name).unwrap_or_else(|| panic!("missing {name}"))
    }

    #[test]
    fn print_job_tlv_is_exact() {
        let bytes = client().print_job_request(&a4_job()).encode();
        // version 2.0, operation Print-Job (0x0002), request-id 2, operation-attributes group.
        assert_eq!(&bytes[..9], &[0x02, 0x00, 0x00, 0x02, 0, 0, 0, 2, 0x01]);
        let msg = IppMessage::decode(&bytes).unwrap();
        assert_eq!(msg.version, (2, 0));
        assert_eq!(msg.code, op::PRINT_JOB);
        let ops = msg.group(tag::OPERATION_ATTRIBUTES).unwrap();
        // The three mandatory attributes come first, in this order.
        let names: Vec<&str> = ops.attributes.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "attributes-charset",
                "attributes-natural-language",
                "printer-uri",
                "requesting-user-name",
                "job-name",
                "ipp-attribute-fidelity",
                "document-format"
            ]
        );
        assert_eq!(
            get(ops, "printer-uri").values[0],
            IppValue::Uri("ipp://printer.local:631/ipp/print".into())
        );
        assert_eq!(
            get(ops, "document-format").values[0],
            IppValue::MimeMediaType("image/pwg-raster".into())
        );
        assert_eq!(get(ops, "ipp-attribute-fidelity").values[0], IppValue::Boolean(false));
        let j = msg.group(tag::JOB_ATTRIBUTES).unwrap();
        assert_eq!(get(j, "copies").values[0], IppValue::Integer(1));
        assert_eq!(get(j, "sides").values[0], IppValue::Keyword("one-sided".into()));
        assert_eq!(get(j, "print-quality").values[0], IppValue::Enum(4));
        assert_eq!(get(j, "print-color-mode").values[0], IppValue::Keyword("color".into()));
        assert_eq!(
            get(j, "printer-resolution").values[0],
            IppValue::Resolution { x: 600, y: 600, units: 3 }
        );
        assert!(j.get("print-scaling").is_none(), "raster jobs carry no print-scaling");
        // The media-col is a nested collection with the margins from the media database.
        let IppValue::Collection(m) = &get(j, "media-col").values[0] else {
            panic!("not a collection")
        };
        let member = |n: &str| m.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(member("media-source"), [IppValue::Keyword("auto".into())]);
        assert_eq!(member("media-type"), [IppValue::Keyword("stationery".into())]);
        assert_eq!(member("media-top-margin"), [IppValue::Integer(500)]);
        assert_eq!(member("media-left-margin"), [IppValue::Integer(340)]);
        let IppValue::Collection(size) = &member("media-size")[0] else { panic!("no size") };
        assert_eq!(size[0], ("x-dimension".into(), vec![IppValue::Integer(21000)]));
        assert_eq!(size[1], ("y-dimension".into(), vec![IppValue::Integer(29700)]));
    }

    #[test]
    fn print_job_wire_bytes_for_a_minimal_job() {
        let mut job = PrintJob::new("x", "image/jpeg");
        job.resolution_dpi = None;
        let bytes = client().print_job_request(&job).encode();
        let mut want: Vec<u8> = vec![0x02, 0x00, 0x00, 0x02, 0, 0, 0, 2, 0x01];
        let put = |want: &mut Vec<u8>, t: u8, n: &str, v: &[u8]| {
            want.push(t);
            want.extend((n.len() as u16).to_be_bytes());
            want.extend(n.as_bytes());
            want.extend((v.len() as u16).to_be_bytes());
            want.extend(v);
        };
        put(&mut want, 0x47, "attributes-charset", b"utf-8");
        put(&mut want, 0x48, "attributes-natural-language", b"en");
        put(&mut want, 0x45, "printer-uri", b"ipp://printer.local:631/ipp/print");
        put(&mut want, 0x42, "requesting-user-name", b"washi");
        put(&mut want, 0x42, "job-name", b"x");
        put(&mut want, 0x22, "ipp-attribute-fidelity", &[0]);
        put(&mut want, 0x49, "document-format", b"image/jpeg");
        want.push(0x02);
        put(&mut want, 0x21, "copies", &1i32.to_be_bytes());
        put(&mut want, 0x44, "sides", b"one-sided");
        put(&mut want, 0x23, "print-quality", &4i32.to_be_bytes());
        put(&mut want, 0x44, "print-color-mode", b"color");
        want.push(0x03);
        assert_eq!(bytes, want);
    }

    #[test]
    fn jpeg_job_options_and_validate_job_share_the_attributes() {
        let mut job = a4_job();
        job.format = "image/jpeg".into();
        job.scaling = Some(Scaling::Fill);
        job.sides = Sides::TwoSidedShortEdge;
        job.quality = Quality::High;
        job.color = PrintColorMode::Monochrome;
        job.copies = 3;
        let c = client();
        let (p, v) = (c.print_job_request(&job), c.validate_job_request(&job));
        assert_eq!(v.code, op::VALIDATE_JOB);
        assert_eq!(p.groups, v.groups);
        let j = p.group(tag::JOB_ATTRIBUTES).unwrap();
        assert_eq!(get(j, "print-scaling").values[0], IppValue::Keyword("fill".into()));
        assert_eq!(get(j, "sides").values[0], IppValue::Keyword("two-sided-short-edge".into()));
        assert_eq!(get(j, "print-quality").values[0], IppValue::Enum(5));
        assert_eq!(get(j, "print-color-mode").values[0], IppValue::Keyword("monochrome".into()));
        assert_eq!(get(j, "copies").values[0], IppValue::Integer(3));
        job.copies = 500;
        assert_eq!(
            get(c.print_job_request(&job).group(tag::JOB_ATTRIBUTES).unwrap(), "copies").values[0],
            IppValue::Integer(99),
            "copies are clamped to what the printer offers"
        );
    }

    #[test]
    fn disc_media_col_has_the_database_shape() {
        let m = MediaSpec {
            width: 12000,
            height: 12000,
            media_type: Some("com.canon-mtdisca".into()),
            source: Some("disc".into()),
            margins: Some(Margins { top: 10, bottom: 10, left: 10, right: 10 }),
        };
        let IppValue::Collection(c) = m.to_value() else { panic!() };
        let names: Vec<&str> = c.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            [
                "media-bottom-margin",
                "media-left-margin",
                "media-right-margin",
                "media-size",
                "media-source",
                "media-top-margin",
                "media-type"
            ]
        );
        // And without margins only size, source and type remain.
        let bare = MediaSpec { margins: None, ..m };
        let IppValue::Collection(c) = bare.to_value() else { panic!() };
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn small_requests() {
        let c = client();
        let g = c.get_job_attributes_request(7);
        assert_eq!(g.code, op::GET_JOB_ATTRIBUTES);
        let ops = g.group(tag::OPERATION_ATTRIBUTES).unwrap();
        assert_eq!(get(ops, "job-id").values[0], IppValue::Integer(7));
        assert_eq!(get(ops, "requested-attributes").values.len(), 7);
        let x = c.cancel_job_request(7, "washi");
        assert_eq!(x.code, op::CANCEL_JOB);
        assert_eq!(
            get(x.group(tag::OPERATION_ATTRIBUTES).unwrap(), "job-id").values[0],
            IppValue::Integer(7)
        );
        let i = c.identify_request(IdentifyAction::Sound, "washi");
        assert_eq!(i.code, 0x003C);
        assert_eq!(
            get(i.group(tag::OPERATION_ATTRIBUTES).unwrap(), "identify-actions").values,
            [IppValue::Keyword("sound".into())]
        );
        assert!(op::is_read(op::GET_JOBS) && !op::is_read(op::PRINT_JOB));
        assert_eq!(op::name(op::IDENTIFY_PRINTER), "Identify-Printer");
    }

    fn reply(status: u16, groups: Vec<IppGroup>) -> IppMessage {
        let mut g = vec![IppGroup::new(
            tag::OPERATION_ATTRIBUTES,
            vec![
                IppAttribute::single("attributes-charset", IppValue::Charset("utf-8".into())),
                IppAttribute::single("status-message", IppValue::Text("because".into())),
            ],
        )];
        g.extend(groups);
        IppMessage { version: (2, 0), code: status, request_id: 2, groups: g }
    }

    fn job_group(id: i32, state: i32) -> IppGroup {
        IppGroup::new(
            tag::JOB_ATTRIBUTES,
            vec![
                IppAttribute::single("job-id", IppValue::Integer(id)),
                IppAttribute::single(
                    "job-uri",
                    IppValue::Uri(format!("ipp://printer.local/ipp/print/{id}")),
                ),
                IppAttribute::single("job-state", IppValue::Enum(state)),
                IppAttribute::keywords("job-state-reasons", &["job-printing"]),
                IppAttribute::single("job-impressions-completed", IppValue::Integer(2)),
            ],
        )
    }

    #[test]
    fn job_reply_parsing() {
        let j = JobAttributes::from_message(&reply(0, vec![job_group(12, 5)])).unwrap();
        assert_eq!(j.id, Some(12));
        assert_eq!(j.state, Some(JobState::Processing));
        assert!(!j.state.unwrap().is_terminal());
        assert_eq!(j.state_reasons, ["job-printing"]);
        assert_eq!(j.impressions_completed, Some(2));
        assert!(j.ignored.is_empty());
        assert!(JobState::Completed.is_terminal() && JobState::Canceled.is_terminal());
        assert_eq!(JobState::from_enum(99), JobState::Unknown(99));

        // successful-ok-ignored-or-substituted-attributes carries the list of ignored ones.
        let ignored = IppGroup::new(
            tag::UNSUPPORTED_ATTRIBUTES,
            vec![IppAttribute::single(
                "print-scaling",
                IppValue::OutOfBand(super::super::OutOfBand::Unsupported),
            )],
        );
        let j = JobAttributes::from_message(&reply(1, vec![job_group(3, 3), ignored])).unwrap();
        assert_eq!(j.ignored, ["print-scaling"]);
        assert_eq!(j.state, Some(JobState::Pending));

        let all =
            JobAttributes::all_from_message(&reply(0, vec![job_group(1, 9), job_group(2, 5)]))
                .unwrap();
        assert_eq!(all.len(), 2);
        assert!(JobAttributes::all_from_message(&reply(0, vec![])).unwrap().is_empty());
        assert_eq!(JobAttributes::from_message(&reply(0, vec![])), Err(IppError::NoJobAttributes));
    }

    #[test]
    fn refusals_name_the_unsupported_attributes() {
        let bad = IppGroup::new(
            tag::UNSUPPORTED_ATTRIBUTES,
            vec![IppAttribute::single(
                "media-col",
                IppValue::OutOfBand(super::super::OutOfBand::Unsupported),
            )],
        );
        let e = JobAttributes::from_message(&reply(0x040B, vec![bad])).unwrap_err();
        let text = e.to_string();
        assert!(
            text.contains("client-error-attributes-or-values-not-supported")
                && text.contains("because")
                && text.contains("unsupported: media-col"),
            "{text}"
        );
        assert!(matches!(e, IppError::Rejected { code: 0x040B, .. }));
        assert!(check_status(&reply(0x0507, vec![])).is_err());
        assert!(check_status(&reply(0x0002, vec![])).is_ok());
    }

    #[test]
    fn print_capabilities_parse() {
        let attrs = vec![
            IppAttribute::single("printer-state", IppValue::Enum(3)),
            IppAttribute::single("printer-is-accepting-jobs", IppValue::Boolean(true)),
            IppAttribute::keywords("printer-state-reasons", &["none"]),
            IppAttribute::new(
                "document-format-supported",
                ["image/jpeg", "image/pwg-raster"]
                    .map(|s| IppValue::MimeMediaType(s.into()))
                    .into(),
            ),
            IppAttribute::keywords("pwg-raster-document-type-supported", &["srgb_8", "sgray_8"]),
            IppAttribute::single(
                "pwg-raster-document-resolution-supported",
                IppValue::Resolution { x: 600, y: 600, units: 3 },
            ),
            IppAttribute::keywords("pwg-raster-document-sheet-back", &["rotated"]),
            IppAttribute::keywords("media-source-supported", &["auto", "main", "rear", "disc"]),
            IppAttribute::single("copies-supported", IppValue::Range { lower: 1, upper: 99 }),
            IppAttribute::single(
                "jpeg-x-dimension-supported",
                IppValue::Range { lower: 16, upper: 19200 },
            ),
            IppAttribute::single(
                "jpeg-k-octets-supported",
                IppValue::Range { lower: 0, upper: 16384 },
            ),
        ];
        let msg = reply(0, vec![IppGroup::new(tag::PRINTER_ATTRIBUTES, attrs)]);
        let c = PrintCapabilities::from_message(&msg).unwrap();
        assert_eq!(c.accepting_jobs, Some(true));
        assert_eq!(c.state.as_deref(), Some("idle"));
        assert!(c.supports_format("IMAGE/PWG-RASTER") && !c.supports_format("application/pdf"));
        assert_eq!(c.raster_types, ["srgb_8", "sgray_8"]);
        assert_eq!(c.raster_resolutions, [(600, 600)]);
        assert_eq!(c.raster_sheet_back.as_deref(), Some("rotated"));
        assert_eq!(
            (c.copies_max, c.jpeg_max_dimension, c.jpeg_max_kilobytes),
            (Some(99), Some(19200), Some(16384))
        );
        assert_eq!(c.media_sources.len(), 4);
    }

    #[test]
    fn describe_is_readable() {
        let d = client().print_job_request(&a4_job()).describe();
        assert!(d.starts_with("Print-Job (IPP 2.0, request-id 2)"), "{d}");
        assert!(d.contains("operation-attributes-tag") && d.contains("job-attributes-tag"));
        assert!(d.contains("printer-uri (uri) = ipp://printer.local:631/ipp/print"));
        assert!(d.contains("document-format (mimeMediaType) = image/pwg-raster"));
        assert!(d.contains("print-quality (enum) = 4"));
        assert!(d.contains("media-size={x-dimension=21000 y-dimension=29700}"), "{d}");
        assert!(d.contains("printer-resolution (resolution) = 600x600dpi"));
    }
}
