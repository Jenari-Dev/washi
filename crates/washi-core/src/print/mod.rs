// SPDX-License-Identifier: GPL-2.0-or-later
//! Printing through IPP.
//!
//! * Photos (JPEG / PNG / BMP) are converted to an sRGB JPEG laid out on the paper
//!   ([`photo`]) and sent as `image/jpeg`.
//! * Documents (PDF; Office files through LibreOffice) are rasterised page by page with
//!   poppler ([`document`]) into a PWG raster spool file and sent as `image/pwg-raster`,
//!   because the printer accepts no PDF.
//! * [`plan_print`] builds the exact IPP request of every job without touching the network
//!   (the CLI's dry run). [`run_print`] needs a [`Confirm`]: it reads the printer's
//!   capabilities and `media-col-database` (read-only), validates the job, sends it and
//!   follows it until the printer reports it completed.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use washi_net::NetError;
use washi_net::ipp::{
    IppClient, IppError, JobState, Margins, MediaDatabase, MediaSpec, PrintColorMode, PrintJob,
    Quality, Scaling, Sides,
};

use crate::Confirm;
use crate::error::{Error, Result};
use crate::printer::Printer;
use crate::tools::Tools;

pub mod caps;
pub mod document;
pub mod layout;
pub mod paper;
pub mod photo;

pub use paper::{DISC_PAPER, PAPER, PaperSize, Source, media_type};

/// Where a spooled raster or JPEG goes before it is sent.
fn spool_dir() -> PathBuf {
    std::env::temp_dir()
}

// ---------------------------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------------------------

/// Two-sided printing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Duplex {
    Long,
    Short,
}

impl Duplex {
    pub fn sides(self) -> Sides {
        match self {
            Duplex::Long => Sides::TwoSidedLongEdge,
            Duplex::Short => Sides::TwoSidedShortEdge,
        }
    }

    pub fn parse(s: &str) -> Result<Option<Duplex>> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "off" | "none" | "one-sided" => Ok(None),
            "long" | "long-edge" => Ok(Some(Duplex::Long)),
            "short" | "short-edge" => Ok(Some(Duplex::Short)),
            other => Err(Error::Invalid(format!("duplex must be long or short, got {other:?}"))),
        }
    }
}

/// "Orientation" of the Print screens: how the sheet is used.
///
/// * `Auto` (the default): each photo or page is turned to fill the sheet, whichever way it
///   was made.
/// * `Portrait` / `Landscape`: the sheet is used upright / on its side and the content is *not*
///   turned to suit it (a landscape photo on a portrait sheet stays small).
///
/// The printer always takes the paper portrait; a landscape sheet is composed sideways and
/// turned a quarter clockwise onto the paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Orientation {
    #[default]
    Auto,
    Portrait,
    Landscape,
}

impl Orientation {
    pub fn parse(s: &str) -> Result<Orientation> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Orientation::Auto),
            "portrait" => Ok(Orientation::Portrait),
            "landscape" => Ok(Orientation::Landscape),
            other => Err(Error::Invalid(format!(
                "orientation must be auto, portrait or landscape, got {other:?}"
            ))),
        }
    }

    pub fn is_landscape(self) -> bool {
        self == Orientation::Landscape
    }
}

/// "Page Order" of Document Print: which sheet comes out of the printer first.
///
/// * `Normal`: the first sheet first.
/// * `Reverse`: the last sheet first, so that a printer that stacks the sheets face up delivers
///   them in reading order. Pages keep their place on a sheet (n-up), and the two sides of a
///   duplex sheet stay together: it is the *sheets* that are turned round, never the pages
///   within one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PageOrder {
    #[default]
    Normal,
    Reverse,
}

impl PageOrder {
    pub fn parse(s: &str) -> Result<PageOrder> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "normal" | "forward" => Ok(PageOrder::Normal),
            "reverse" | "reversed" => Ok(PageOrder::Reverse),
            other => {
                Err(Error::Invalid(format!("page order must be normal or reverse, got {other:?}")))
            }
        }
    }

    pub fn is_reverse(self) -> bool {
        self == PageOrder::Reverse
    }
}

pub fn parse_quality(s: &str) -> Result<Quality> {
    match s.trim().to_ascii_lowercase().as_str() {
        "draft" | "fast" => Ok(Quality::Draft),
        "normal" | "standard" => Ok(Quality::Normal),
        "high" | "best" => Ok(Quality::High),
        other => {
            Err(Error::Invalid(format!("quality must be draft, normal or high, got {other:?}")))
        }
    }
}

/// "Pages": `1-3,5`, `2-` (to the end), `-4` (up to 4). Spaces are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageSelection(Vec<(u32, u32)>);

impl PageSelection {
    pub fn parse(s: &str) -> Result<PageSelection> {
        let bad =
            |why: &str| Error::Invalid(format!("page selection {s:?}: {why} (example: 1-3,5)"));
        let mut ranges = Vec::new();
        for part in s.split(',').map(|p| p.replace(' ', "")).filter(|p| !p.is_empty()) {
            let num = |t: &str| {
                t.parse::<u32>().ok().filter(|n| *n > 0).ok_or_else(|| bad("not a page number"))
            };
            let range = match part.split_once('-') {
                None => {
                    let n = num(&part)?;
                    (n, n)
                }
                Some(("", b)) => (1, num(b)?),
                Some((a, "")) => (num(a)?, u32::MAX),
                Some((a, b)) => {
                    let (a, b) = (num(a)?, num(b)?);
                    if a > b {
                        return Err(bad("a range runs backwards"));
                    }
                    (a, b)
                }
            };
            ranges.push(range);
        }
        if ranges.is_empty() {
            return Err(bad("no pages"));
        }
        Ok(PageSelection(ranges))
    }

    /// The selected pages of a document with `total` pages: sorted, without duplicates.
    pub fn resolve(&self, total: u32) -> Result<Vec<u32>> {
        let mut out: Vec<u32> = Vec::new();
        for (a, b) in &self.0 {
            if *a > total {
                return Err(Error::Invalid(format!(
                    "page {a} does not exist (the document has {total} page(s))"
                )));
            }
            if *b != u32::MAX && *b > total {
                return Err(Error::Invalid(format!(
                    "page {b} does not exist (the document has {total} page(s))"
                )));
            }
            out.extend(*a..=(*b).min(total));
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }
}

/// What the Photo Print / Document Print side panels offer.
#[derive(Debug, Clone)]
pub struct PrintOptions {
    pub paper: PaperSize,
    /// IPP media-type keyword; `None` = `stationery` for documents, `photographic` for photos.
    pub media_type: Option<String>,
    pub source: Source,
    pub borderless: bool,
    pub quality: Quality,
    pub color: bool,
    pub copies: u32,
    /// Documents only.
    pub pages: Option<PageSelection>,
    /// Pages (or photos) per sheet: 1, 2, 4, 6, 9 or 16.
    pub layout: u8,
    /// Documents only.
    pub duplex: Option<Duplex>,
    pub orientation: Orientation,
    /// Documents only.
    pub page_order: PageOrder,
    pub job_name: Option<String>,
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions {
            paper: *PaperSize::parse("a4").unwrap_or(&PAPER[0]),
            media_type: None,
            source: Source::Auto,
            borderless: false,
            quality: Quality::Normal,
            color: true,
            copies: 1,
            pages: None,
            layout: 1,
            duplex: None,
            orientation: Orientation::Auto,
            page_order: PageOrder::Normal,
            job_name: None,
        }
    }
}

/// The JSON form of [`PrintOptions`] used by the daemon (`PrintFile`). Unknown keys are
/// errors, so a typo is never a silent no-op.
///
/// ```json
/// { "confirm": true, "size": "a4", "media_type": "plain", "source": "cassette",
///   "quality": "normal", "color": true, "copies": 1, "pages": "1-3,5", "layout": 1 }
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintSpec {
    #[serde(default)]
    pub confirm: bool,
    pub size: Option<String>,
    pub media_type: Option<String>,
    pub source: Option<String>,
    pub borderless: Option<bool>,
    pub quality: Option<String>,
    pub color: Option<bool>,
    pub copies: Option<u32>,
    pub pages: Option<String>,
    pub layout: Option<u8>,
    pub duplex: Option<String>,
    /// `auto` (default), `portrait` or `landscape`.
    pub orientation: Option<String>,
    /// `normal` (default) or `reverse`: the last sheet first (documents).
    pub page_order: Option<String>,
    pub job_name: Option<String>,
}

impl PrintSpec {
    pub fn from_json(json: &str) -> Result<PrintSpec> {
        serde_json::from_str(json)
            .map_err(|e| Error::Invalid(format!("invalid print options: {e}")))
    }

    pub fn to_options(&self) -> Result<PrintOptions> {
        let mut o = PrintOptions::default();
        if let Some(s) = &self.size {
            o.paper = *PaperSize::parse(s)?;
        }
        if let Some(t) = &self.media_type {
            o.media_type = Some(media_type(t)?);
        }
        if let Some(s) = &self.source {
            o.source = Source::parse(s)?;
        }
        o.borderless = self.borderless.unwrap_or(false);
        if let Some(q) = &self.quality {
            o.quality = parse_quality(q)?;
        }
        o.color = self.color.unwrap_or(true);
        o.copies = self.copies.unwrap_or(1);
        if let Some(p) = &self.pages {
            o.pages = Some(PageSelection::parse(p)?);
        }
        o.layout = self.layout.unwrap_or(1);
        if let Some(d) = &self.duplex {
            o.duplex = Duplex::parse(d)?;
        }
        if let Some(v) = &self.orientation {
            o.orientation = Orientation::parse(v)?;
        }
        if let Some(v) = &self.page_order {
            o.page_order = PageOrder::parse(v)?;
        }
        o.job_name = self.job_name.clone();
        Ok(o)
    }
}

impl PrintOptions {
    pub fn validate(&self) -> Result<()> {
        layout::check_layout(self.layout)?;
        if !(1..=99).contains(&self.copies) {
            return Err(Error::Invalid(format!("copies must be 1 to 99, got {}", self.copies)));
        }
        if self.source == Source::Disc {
            return Err(Error::Invalid(
                "the disc tray is used by `washi disc`, not by file printing".into(),
            ));
        }
        if self.borderless && self.layout > 1 {
            return Err(Error::Invalid("borderless printing needs one page per sheet".into()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------------------------

/// What kind of file this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FileKind {
    Photo,
    Pdf,
    Office,
}

/// Look at the file's first bytes (and, for Office files, its extension).
pub fn classify_file(path: &Path) -> Result<FileKind> {
    use std::io::Read;
    let mut head = [0u8; 8];
    let n = File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
    let head = &head[..n];
    if head.starts_with(b"%PDF-") {
        return Ok(FileKind::Pdf);
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(&[0x89, b'P', b'N', b'G'])
        || head.starts_with(b"BM")
    {
        return Ok(FileKind::Photo);
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if matches!(
        ext.as_str(),
        "doc"
            | "docx"
            | "xls"
            | "xlsx"
            | "ppt"
            | "pptx"
            | "odt"
            | "ods"
            | "odp"
            | "rtf"
            | "txt"
            | "csv"
    ) {
        return Ok(FileKind::Office);
    }
    Err(Error::Invalid(format!(
        "{}: not a printable file (use JPEG, PNG, BMP, PDF, or an Office document)",
        path.display()
    )))
}

/// What a planned job prints.
#[derive(Debug, Clone)]
pub enum JobSource {
    /// One or more photos on one sheet.
    Photos(Vec<PathBuf>),
    Pdf {
        path: PathBuf,
        pages: Option<Vec<u32>>,
    },
    Office {
        path: PathBuf,
        pages: Option<PageSelection>,
    },
}

/// One IPP job in a plan.
#[derive(Debug, Clone)]
pub struct PlannedJob {
    pub label: String,
    pub source: JobSource,
    pub job: PrintJob,
    /// Pages of the input that will be printed, when known without converting.
    pub input_pages: Option<u32>,
    /// Sheets of paper, when known.
    pub sheets: Option<u32>,
    pub notes: Vec<String>,
}

/// Everything `run_print` will do, built without touching the network.
#[derive(Debug, Clone)]
pub struct PrintPlan {
    pub printer_uri: String,
    pub options: PrintOptions,
    pub jobs: Vec<PlannedJob>,
    pub warnings: Vec<String>,
}

impl PrintPlan {
    /// The IPP requests, as the CLI dry run shows them.
    pub fn describe(&self, ipp: &IppClient) -> String {
        let mut out = String::new();
        for (i, j) in self.jobs.iter().enumerate() {
            out.push_str(&format!("Job {}/{}: {}\n", i + 1, self.jobs.len(), j.label));
            for n in &j.notes {
                out.push_str(&format!("  - {n}\n"));
            }
            out.push_str("  first Validate-Job, then:\n");
            for line in ipp.print_job_request(&j.job).describe().lines() {
                out.push_str(&format!("    {line}\n"));
            }
            out.push_str(&format!("    + document data ({})\n", j.job.format));
        }
        out
    }
}

fn default_media_type(opts: &PrintOptions, kind: FileKind) -> String {
    opts.media_type.clone().unwrap_or_else(|| match kind {
        FileKind::Photo => "photographic".to_string(),
        _ => "stationery".to_string(),
    })
}

fn media_spec(opts: &PrintOptions, kind: FileKind) -> MediaSpec {
    let margins = if opts.borderless {
        Margins::ZERO
    } else {
        let (top, bottom, left, right) = paper::DEFAULT_MARGINS;
        Margins { top, bottom, left, right }
    };
    MediaSpec {
        width: opts.paper.width,
        height: opts.paper.height,
        media_type: Some(default_media_type(opts, kind)),
        source: Some(opts.source.ipp().to_string()),
        margins: Some(margins),
    }
}

fn base_job(opts: &PrintOptions, kind: FileKind, name: &str) -> PrintJob {
    let mut job = PrintJob::new(
        name,
        if kind == FileKind::Photo { "image/jpeg" } else { "image/pwg-raster" },
    );
    job.copies = opts.copies;
    job.quality = opts.quality;
    job.color = if opts.color { PrintColorMode::Color } else { PrintColorMode::Monochrome };
    job.media = Some(media_spec(opts, kind));
    if kind == FileKind::Photo {
        job.scaling = Some(if opts.borderless { Scaling::Fill } else { Scaling::Fit });
    } else if let Some(d) = opts.duplex {
        job.sides = d.sides();
    }
    job
}

fn file_label(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into())
}

/// Plan the jobs for `files`. Reads the files' first bytes and, for PDFs, runs `pdfinfo`;
/// never contacts the printer.
pub fn plan_print(
    printer: &Printer,
    files: &[PathBuf],
    opts: &PrintOptions,
    tools: &Tools,
) -> Result<PrintPlan> {
    opts.validate()?;
    if files.is_empty() {
        return Err(Error::Invalid("no files to print".into()));
    }
    let mut jobs: Vec<PlannedJob> = Vec::new();
    let mut warnings = Vec::new();
    let mut photos: Vec<PathBuf> = Vec::new();
    let mut doc_jobs: Vec<PlannedJob> = Vec::new();
    for f in files {
        match classify_file(f)? {
            FileKind::Photo => {
                if opts.pages.is_some() {
                    return Err(Error::Invalid(format!(
                        "--pages applies to documents, not to the photo {}",
                        file_label(f)
                    )));
                }
                if opts.duplex.is_some() {
                    return Err(Error::Invalid(
                        "two-sided printing is for documents, not photos".into(),
                    ));
                }
                if opts.page_order.is_reverse() {
                    return Err(Error::Invalid(
                        "the page order is for documents, not photos".into(),
                    ));
                }
                photos.push(f.clone());
            }
            kind @ (FileKind::Pdf | FileKind::Office) => {
                let label = file_label(f);
                let mut job = base_job(opts, kind, &label);
                job.name = label.clone();
                let mut notes = vec![format!(
                    "{} -> rasterised at 600 dpi ({}) into PWG raster, {} per sheet",
                    if kind == FileKind::Pdf {
                        "PDF"
                    } else {
                        "Office document (converted to PDF with LibreOffice first)"
                    },
                    if opts.color { "sRGB" } else { "grey" },
                    opts.layout
                )];
                let (source, input_pages, sheets);
                if kind == FileKind::Pdf {
                    if !tools.can_print_pdf() {
                        warnings.push(format!(
                            "{label}: poppler (pdftoppm, pdfinfo) is not installed; this job cannot be rendered"
                        ));
                        source = JobSource::Pdf { path: f.clone(), pages: None };
                        input_pages = None;
                        sheets = None;
                    } else {
                        let info = document::pdf_info(tools, f)?;
                        let pages = match &opts.pages {
                            Some(sel) => sel.resolve(info.pages)?,
                            None => (1..=info.pages).collect(),
                        };
                        let (_, s) =
                            document::count_sides(pages.len(), opts.layout, opts.duplex.is_some());
                        input_pages = Some(pages.len() as u32);
                        sheets = Some(s);
                        notes.push(format!(
                            "{} of {} page(s) -> {} sheet(s){}{}",
                            pages.len(),
                            info.pages,
                            s,
                            if opts.duplex.is_some() { ", two-sided" } else { "" },
                            if opts.page_order.is_reverse() { ", last sheet first" } else { "" }
                        ));
                        source = JobSource::Pdf { path: f.clone(), pages: Some(pages) };
                    }
                } else {
                    if !tools.can_convert_office() {
                        warnings.push(format!("{label}: LibreOffice (soffice) is not installed; this job cannot be converted"));
                    }
                    notes.push("page count is known after the conversion".into());
                    source = JobSource::Office { path: f.clone(), pages: opts.pages.clone() };
                    input_pages = None;
                    sheets = None;
                }
                doc_jobs.push(PlannedJob { label, source, job, input_pages, sheets, notes });
            }
        }
    }
    // Photos: one job per photo, or one per sheet of `layout` photos.
    let per_sheet = usize::from(opts.layout);
    for (i, group) in photos.chunks(per_sheet).enumerate() {
        let label = if per_sheet == 1 {
            file_label(&group[0])
        } else {
            format!("photo sheet {} ({} photos)", i + 1, group.len())
        };
        let job = base_job(opts, FileKind::Photo, &label);
        let mut notes = vec![format!(
            "JPEG (sRGB) laid out on {}{}",
            opts.paper.label,
            if opts.borderless { ", borderless (fill)" } else { ", with margins (fit)" }
        )];
        if per_sheet > 1 {
            notes.push(format!("{per_sheet} photos per sheet"));
        }
        jobs.push(PlannedJob {
            label,
            source: JobSource::Photos(group.to_vec()),
            job,
            input_pages: Some(group.len() as u32),
            sheets: Some(1),
            notes,
        });
    }
    jobs.extend(doc_jobs);
    let mut plan = PrintPlan {
        printer_uri: printer.ipp().printer_uri(),
        options: opts.clone(),
        jobs,
        warnings,
    };
    if let Some(n) = opts.job_name.as_deref().filter(|n| !n.is_empty()) {
        for j in &mut plan.jobs {
            j.job.name = n.to_string();
        }
    }
    Ok(plan)
}

// ---------------------------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------------------------

/// Progress of [`run_print`] (and of disc printing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintProgress {
    /// Reading capabilities and the media database.
    Preflight,
    /// Rendering job `job` of `jobs`: sheet side `side` of `sides`.
    Preparing {
        job: usize,
        jobs: usize,
        side: u32,
        sides: u32,
    },
    /// `Validate-Job` passed; `ignored` lists attributes the printer will drop.
    Validated {
        ignored: Vec<String>,
    },
    Sending {
        job: usize,
        jobs: usize,
        bytes: u64,
    },
    Submitted {
        job: usize,
        jobs: usize,
        id: i32,
    },
    State {
        id: i32,
        state: JobState,
        reasons: Vec<String>,
        impressions: Option<i32>,
    },
    Finished {
        id: i32,
        state: JobState,
    },
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Compare the job with the printer's `media-col-database` before sending.
    pub check_media: bool,
    /// Follow each job until the printer says it is done.
    pub wait: bool,
    pub timeout: Duration,
    pub poll_interval: Duration,
    /// Setting this cancels: before the next sheet is rendered, between polls (then
    /// `Cancel-Job` is sent).
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            check_media: true,
            wait: true,
            timeout: Duration::from_secs(30 * 60),
            poll_interval: Duration::from_millis(1500),
            cancel: None,
        }
    }
}

impl RunOptions {
    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|c| c.load(Ordering::SeqCst))
    }
}

/// How one job ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobReport {
    pub label: String,
    pub job_id: i32,
    pub state: JobState,
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PrintReport {
    pub jobs: Vec<JobReport>,
}

/// A document ready to send.
pub enum Spool {
    Memory(Vec<u8>),
    File(tempfile::NamedTempFile),
}

impl Spool {
    fn len(&self) -> u64 {
        match self {
            Spool::Memory(v) => v.len() as u64,
            Spool::File(f) => f.as_file().metadata().map_or(0, |m| m.len()),
        }
    }
}

/// What the preflight learned from the printer.
#[derive(Debug, Clone, Default)]
pub struct Preflight {
    pub sheet_back: String,
    pub raster_types: Vec<String>,
    pub media: MediaDatabase,
    pub max_jpeg_edge: Option<i32>,
    pub warnings: Vec<String>,
}

/// Read-only: capabilities and (optionally) the media database; refuse early when the printer
/// is not accepting jobs or cannot take the format.
pub fn preflight(ipp: &IppClient, format: &str, check_media: bool) -> Result<Preflight> {
    let caps = ipp.print_capabilities()?;
    let mut pf = Preflight {
        sheet_back: caps.raster_sheet_back.clone().unwrap_or_else(|| "normal".to_string()),
        raster_types: caps.raster_types.clone(),
        max_jpeg_edge: caps.jpeg_max_dimension,
        ..Preflight::default()
    };
    if caps.accepting_jobs == Some(false) {
        return Err(Error::Invalid(
            "the printer is not accepting jobs right now (check its display)".into(),
        ));
    }
    if caps.state.as_deref() == Some("stopped") {
        return Err(Error::Invalid(format!(
            "the printer is stopped ({}); fix the problem on its display first",
            caps.state_reasons.join(", ")
        )));
    }
    if !caps.document_formats.is_empty() && !caps.supports_format(format) {
        return Err(Error::Invalid(format!(
            "the printer does not accept {format} (it takes: {})",
            caps.document_formats.join(", ")
        )));
    }
    if check_media {
        pf.media = ipp.media_database()?;
        if pf.media.is_empty() {
            pf.warnings.push(
                "the printer returned no media database; the paper combination is not checked"
                    .into(),
            );
        }
    }
    Ok(pf)
}

/// Check the job's media against the database and adopt the database entry's margins.
pub fn adopt_media(job: &mut PrintJob, db: &MediaDatabase, borderless: bool) -> Result<()> {
    if db.is_empty() {
        return Ok(());
    }
    let Some(m) = job.media.as_mut() else { return Ok(()) };
    match db.find(m.width, m.height, m.source.as_deref(), m.media_type.as_deref(), borderless) {
        Some(entry) => {
            if let Some(g) = entry.margins() {
                m.margins = Some(g);
            }
            Ok(())
        }
        None => {
            let types = db.types_for(m.width, m.height, m.source.as_deref());
            let sizes = db.sizes(m.source.as_deref());
            let size = PaperSize::by_dimensions(m.width, m.height).map_or_else(
                || format!("{}x{} (1/100 mm)", m.width, m.height),
                |p| p.label.to_string(),
            );
            let why = if sizes.contains(&(m.width, m.height)) {
                format!(
                    "no {} entry for {size} from the {} source with media type {} (it offers for this size: {})",
                    if borderless { "borderless" } else { "bordered" },
                    m.source.as_deref().unwrap_or("any"),
                    m.media_type.as_deref().unwrap_or("any"),
                    if types.is_empty() { "nothing".into() } else { types.join(", ") }
                )
            } else {
                format!(
                    "{size} is not offered from the {} source",
                    m.source.as_deref().unwrap_or("any")
                )
            };
            Err(Error::Invalid(format!(
                "the printer cannot print that: {why}. Run `washi media` to see what it offers"
            )))
        }
    }
}

fn wait_for_job(
    ipp: &IppClient,
    id: i32,
    opts: &RunOptions,
    progress: &mut dyn FnMut(PrintProgress),
    confirm: Confirm,
    user: &str,
) -> Result<JobState> {
    let started = Instant::now();
    let mut last: Option<(JobState, Vec<String>, Option<i32>)> = None;
    loop {
        if opts.cancelled() {
            ipp.cancel_job(id, user, confirm)?;
            return Err(Error::Cancelled);
        }
        let j = match ipp.get_job_attributes(id) {
            Ok(j) => j,
            // Printers forget finished jobs: "not found" / "gone" after we saw the job accepted
            // means it is over (and, with nothing said against it, it printed).
            Err(NetError::Ipp(IppError::Rejected { code: 0x0406 | 0x0407, .. }))
                if last.is_some() =>
            {
                progress(PrintProgress::Finished { id, state: JobState::Completed });
                return Ok(JobState::Completed);
            }
            Err(e) => return Err(e.into()),
        };
        if let Some(state) = j.state {
            let cur = (state, j.state_reasons.clone(), j.impressions_completed);
            if last.as_ref() != Some(&cur) {
                progress(PrintProgress::State {
                    id,
                    state,
                    reasons: j.state_reasons.clone(),
                    impressions: j.impressions_completed,
                });
                last = Some(cur);
            }
            if state.is_terminal() {
                progress(PrintProgress::Finished { id, state });
                return Ok(state);
            }
        }
        if started.elapsed() > opts.timeout {
            return Err(Error::Timeout(format!(
                "job {id} was not finished after {} minutes; it may still be printing",
                opts.timeout.as_secs() / 60
            )));
        }
        std::thread::sleep(opts.poll_interval);
    }
}

/// Validate, send and (optionally) follow one prepared job.
#[allow(clippy::too_many_arguments)]
pub fn submit(
    ipp: &IppClient,
    job: &PrintJob,
    spool: &mut Spool,
    label: &str,
    index: (usize, usize),
    opts: &RunOptions,
    confirm: Confirm,
    progress: &mut dyn FnMut(PrintProgress),
) -> Result<JobReport> {
    let v = ipp.validate_job(job, confirm)?;
    progress(PrintProgress::Validated { ignored: v.ignored });
    let len = spool.len();
    progress(PrintProgress::Sending { job: index.0, jobs: index.1, bytes: len });
    let attrs = match spool {
        Spool::Memory(bytes) => {
            ipp.print_job(job, &mut std::io::Cursor::new(&bytes[..]), len, confirm)?
        }
        Spool::File(f) => {
            f.as_file_mut()
                .seek(SeekFrom::Start(0))
                .map_err(|e| Error::io("cannot rewind the spool file", e))?;
            ipp.print_job(job, f.as_file_mut(), len, confirm)?
        }
    };
    let id = attrs.id.ok_or_else(|| {
        Error::Invalid("the printer accepted the job but did not give it an id".into())
    })?;
    progress(PrintProgress::Submitted { job: index.0, jobs: index.1, id });
    let state = if opts.wait {
        wait_for_job(ipp, id, opts, progress, confirm, &job.user)?
    } else {
        attrs.state.unwrap_or(JobState::Pending)
    };
    match state {
        JobState::Aborted => Err(Error::Invalid(format!(
            "the printer aborted job {id}; check its display for the reason"
        ))),
        JobState::Canceled => Err(Error::Invalid(format!("job {id} was cancelled at the printer"))),
        _ => Ok(JobReport { label: label.to_string(), job_id: id, state, ignored: attrs.ignored }),
    }
}

fn spool_file() -> Result<tempfile::NamedTempFile> {
    tempfile::Builder::new()
        .prefix("washi-spool-")
        .tempfile_in(spool_dir())
        .map_err(|e| Error::io("cannot create the spool file", e))
}

/// Print a plan. **Prints**, so it needs a [`Confirm`].
pub fn run_print(
    printer: &Printer,
    plan: &PrintPlan,
    opts: &RunOptions,
    tools: &Tools,
    confirm: Confirm,
    progress: &mut dyn FnMut(PrintProgress),
) -> Result<PrintReport> {
    let ipp = printer.ipp();
    let needs_raster = plan.jobs.iter().any(|j| j.job.format == "image/pwg-raster");
    progress(PrintProgress::Preflight);
    let pf = preflight(
        &ipp,
        if needs_raster { "image/pwg-raster" } else { "image/jpeg" },
        opts.check_media,
    )?;
    let mut report = PrintReport::default();
    let total = plan.jobs.len();
    for (i, pj) in plan.jobs.iter().enumerate() {
        if opts.cancelled() {
            return Err(Error::Cancelled);
        }
        let mut job = pj.job.clone();
        adopt_media(&mut job, &pf.media, plan.options.borderless)?;
        let mut spool =
            prepare(pj, &plan.options, &pf, tools, (i, total), opts, progress, &mut job)?;
        let r = submit(&ipp, &job, &mut spool, &pj.label, (i, total), opts, confirm, progress)?;
        report.jobs.push(r);
    }
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    pj: &PlannedJob,
    opts: &PrintOptions,
    pf: &Preflight,
    tools: &Tools,
    index: (usize, usize),
    run: &RunOptions,
    progress: &mut dyn FnMut(PrintProgress),
    job: &mut PrintJob,
) -> Result<Spool> {
    match &pj.source {
        JobSource::Photos(files) => {
            let images: Vec<_> =
                files.iter().map(|f| photo::load_photo(f)).collect::<Result<_>>()?;
            let canvas = if images.len() == 1 && opts.layout == 1 {
                photo::compose_single_oriented(
                    &images[0],
                    &opts.paper,
                    opts.borderless,
                    opts.orientation,
                )
            } else {
                photo::compose_sheet_oriented(&images, &opts.paper, opts.layout, opts.orientation)
            };
            if let Some(max) = pf.max_jpeg_edge {
                if canvas.width() as i32 > max || canvas.height() as i32 > max {
                    return Err(Error::Invalid(format!(
                        "the image is larger than the printer's {max} px limit"
                    )));
                }
            }
            Ok(Spool::Memory(photo::encode_for_printer(&canvas)?))
        }
        JobSource::Pdf { path, pages } => {
            rasterize_pdf(path, pages.as_deref(), opts, pf, tools, index, run, progress, job)
        }
        JobSource::Office { path, pages } => {
            let work = tempfile::tempdir()
                .map_err(|e| Error::io("cannot create a temporary directory", e))?;
            let pdf = document::convert_office(tools, path, work.path())?;
            let info = document::pdf_info(tools, &pdf)?;
            let sel = match pages {
                Some(s) => s.resolve(info.pages)?,
                None => (1..=info.pages).collect(),
            };
            rasterize_pdf(&pdf, Some(&sel), opts, pf, tools, index, run, progress, job)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_pdf(
    pdf: &Path,
    pages: Option<&[u32]>,
    opts: &PrintOptions,
    pf: &Preflight,
    tools: &Tools,
    index: (usize, usize),
    run: &RunOptions,
    progress: &mut dyn FnMut(PrintProgress),
    job: &mut PrintJob,
) -> Result<Spool> {
    let info = document::pdf_info(tools, pdf)?;
    let all: Vec<u32>;
    let pages = match pages {
        Some(p) => p,
        None => {
            all = (1..=info.pages).collect();
            &all
        }
    };
    let need = if opts.color { "srgb_8" } else { "sgray_8" };
    if !pf.raster_types.is_empty() && !pf.raster_types.iter().any(|t| t == need) {
        return Err(Error::Invalid(format!(
            "the printer does not take {need} raster (it offers: {})",
            pf.raster_types.join(", ")
        )));
    }
    let media_type = job.media.as_ref().and_then(|m| m.media_type.clone()).unwrap_or_default();
    let spec = document::RasterSpec {
        pdf,
        info: &info,
        pages,
        paper: opts.paper,
        layout: opts.layout,
        mono: !opts.color,
        sides: job.sides,
        sheet_back: &pf.sheet_back,
        media_type: &media_type,
        quality: job.quality.enum_value() as u32,
        dpi: 600,
        orientation: opts.orientation,
        reverse: opts.page_order.is_reverse(),
    };
    let file = spool_file()?;
    let mut writer = BufWriter::with_capacity(
        1 << 20,
        file.reopen().map_err(|e| Error::io("cannot open the spool file", e))?,
    );
    let cancel = run.cancel.clone();
    let stats = document::rasterize(
        tools,
        &spec,
        &mut writer,
        &mut |side, sides| {
            progress(PrintProgress::Preparing { job: index.0, jobs: index.1, side, sides })
        },
        &|| cancel.as_ref().is_some_and(|c| c.load(Ordering::SeqCst)),
    )?;
    std::io::Write::flush(&mut writer).map_err(|e| Error::io("cannot write the spool file", e))?;
    drop(writer);
    let _ = stats;
    Ok(Spool::File(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_selections() {
        let sel = |s: &str| PageSelection::parse(s).unwrap();
        assert_eq!(sel("1-3,5").resolve(10).unwrap(), [1, 2, 3, 5]);
        assert_eq!(sel("1-6, 3, 12-14").resolve(14).unwrap(), [1, 2, 3, 4, 5, 6, 12, 13, 14]);
        assert_eq!(sel("8-").resolve(10).unwrap(), [8, 9, 10]);
        assert_eq!(sel("-2").resolve(10).unwrap(), [1, 2]);
        assert_eq!(sel("5,3,5").resolve(5).unwrap(), [3, 5], "sorted, no duplicates");
        assert!(sel("4").resolve(3).unwrap_err().to_string().contains("page 4 does not exist"));
        assert!(sel("2-9").resolve(5).is_err());
        for bad in ["", "0", "a", "3-1", "1,,x", "--2", "1-2-3", " , "] {
            assert!(PageSelection::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn option_parsers() {
        assert_eq!(Duplex::parse("long").unwrap(), Some(Duplex::Long));
        assert_eq!(Duplex::parse("short-edge").unwrap(), Some(Duplex::Short));
        assert_eq!(Duplex::parse("off").unwrap(), None);
        assert!(Duplex::parse("both").is_err());
        assert_eq!(parse_quality("HIGH").unwrap(), Quality::High);
        assert!(parse_quality("ultra").is_err());
        assert_eq!(Duplex::Short.sides(), Sides::TwoSidedShortEdge);
    }

    #[test]
    fn options_are_validated() {
        let ok = PrintOptions::default();
        assert!(ok.validate().is_ok());
        assert!(PrintOptions { copies: 0, ..ok.clone() }.validate().is_err());
        assert!(PrintOptions { copies: 100, ..ok.clone() }.validate().is_err());
        assert!(PrintOptions { layout: 3, ..ok.clone() }.validate().is_err());
        assert!(PrintOptions { source: Source::Disc, ..ok.clone() }.validate().is_err());
        assert!(PrintOptions { borderless: true, layout: 2, ..ok.clone() }.validate().is_err());
        assert!(PrintOptions { borderless: true, ..ok }.validate().is_ok());
    }

    #[test]
    fn json_specs() {
        let s = PrintSpec::from_json(
            r#"{"confirm":true,"size":"letter","media_type":"photo","source":"rear","borderless":true,
                "quality":"high","color":false,"copies":2,"pages":"1-2","layout":1,"duplex":"short"}"#,
        )
        .unwrap();
        assert!(s.confirm);
        let o = s.to_options().unwrap();
        assert_eq!(o.paper.key, "letter");
        assert_eq!(o.media_type.as_deref(), Some("photographic"));
        assert_eq!(
            (o.source, o.borderless, o.quality, o.color, o.copies),
            (Source::Rear, true, Quality::High, false, 2)
        );
        assert_eq!(o.duplex, Some(Duplex::Short));
        assert_eq!(o.pages.unwrap().resolve(5).unwrap(), [1, 2]);
        assert!(!PrintSpec::from_json("{}").unwrap().confirm);
        assert!(PrintSpec::from_json(r#"{"colour":true}"#).is_err(), "typos are errors");
        assert!(PrintSpec::from_json(r#"{"size":"a3"}"#).unwrap().to_options().is_err());
        assert!(PrintSpec::from_json("nope").is_err());
        let d = PrintSpec::default().to_options().unwrap();
        assert_eq!((d.paper.key, d.copies, d.layout), ("a4", 1, 1));
    }

    #[test]
    fn files_are_classified_by_content() {
        let dir = tempfile::tempdir().unwrap();
        let w = |name: &str, bytes: &[u8]| {
            let p = dir.path().join(name);
            std::fs::write(&p, bytes).unwrap();
            p
        };
        assert_eq!(classify_file(&w("a.dat", b"%PDF-1.7 ...")).unwrap(), FileKind::Pdf);
        assert_eq!(classify_file(&w("b", &[0xFF, 0xD8, 0xFF, 0xE0])).unwrap(), FileKind::Photo);
        assert_eq!(
            classify_file(&w("c.png", &[0x89, b'P', b'N', b'G', 13, 10])).unwrap(),
            FileKind::Photo
        );
        assert_eq!(classify_file(&w("d.bmp", b"BM....")).unwrap(), FileKind::Photo);
        assert_eq!(classify_file(&w("e.DOCX", b"PK\x03\x04")).unwrap(), FileKind::Office);
        assert!(classify_file(&w("f.exe", b"MZ")).is_err());
        assert!(classify_file(&dir.path().join("missing.pdf")).is_err());
    }

    #[test]
    fn media_is_checked_against_the_database() {
        use washi_net::ipp::MediaEntry;
        let entry = |w, h, m: [i32; 4], src: &str, ty: &str| MediaEntry {
            width: w,
            height: h,
            top: Some(m[0]),
            bottom: Some(m[1]),
            left: Some(m[2]),
            right: Some(m[3]),
            source: Some(src.into()),
            media_type: Some(ty.into()),
        };
        let db = MediaDatabase {
            entries: vec![
                entry(21000, 29700, [500, 500, 340, 340], "auto", "stationery"),
                entry(21000, 29700, [800, 1270, 560, 560], "rear", "stationery"),
                entry(21000, 29700, [0, 0, 0, 0], "rear", "com.canon-mtglossy"),
            ],
        };
        let mut job = base_job(
            &PrintOptions { source: Source::Rear, ..PrintOptions::default() },
            FileKind::Pdf,
            "x",
        );
        adopt_media(&mut job, &db, false).unwrap();
        let g = job.media.as_ref().unwrap().margins.unwrap();
        assert_eq!(
            (g.top, g.bottom, g.left, g.right),
            (800, 1270, 560, 560),
            "the database entry's margins are used"
        );

        let borderless = PrintOptions {
            source: Source::Rear,
            media_type: Some("com.canon-mtglossy".into()),
            borderless: true,
            ..PrintOptions::default()
        };
        let mut job = base_job(&borderless, FileKind::Photo, "p");
        adopt_media(&mut job, &db, true).unwrap();
        assert!(job.media.as_ref().unwrap().margins.unwrap().is_zero());

        // Borderless plain paper does not exist: the error names what does.
        let mut job = base_job(
            &PrintOptions { source: Source::Rear, borderless: true, ..PrintOptions::default() },
            FileKind::Pdf,
            "x",
        );
        let e = adopt_media(&mut job, &db, true).unwrap_err().to_string();
        assert!(
            e.contains("no borderless entry")
                && e.contains("com.canon-mtglossy")
                && e.contains("washi media"),
            "{e}"
        );
        // A size the source lacks.
        let mut job = base_job(
            &PrintOptions { paper: *PaperSize::parse("a5").unwrap(), ..PrintOptions::default() },
            FileKind::Pdf,
            "x",
        );
        assert!(adopt_media(&mut job, &db, false).unwrap_err().to_string().contains("not offered"));
        // An empty database accepts everything.
        assert!(adopt_media(&mut job, &MediaDatabase::default(), false).is_ok());
    }

    #[test]
    fn planned_jobs_carry_the_right_ipp_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let jpg = dir.path().join("p.jpg");
        std::fs::write(&jpg, [0xFF, 0xD8, 0xFF, 0xE0, 0, 0]).unwrap();
        let pdf = dir.path().join("d.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 x").unwrap();
        let printer = Printer::new("printer.example");
        let none = Tools::default();
        let opts = PrintOptions {
            borderless: true,
            media_type: Some("com.canon-mtluster".into()),
            source: Source::Rear,
            copies: 2,
            ..PrintOptions::default()
        };
        let plan = plan_print(&printer, std::slice::from_ref(&jpg), &opts, &none).unwrap();
        let j = &plan.jobs[0].job;
        assert_eq!(j.format, "image/jpeg");
        assert_eq!(j.scaling, Some(Scaling::Fill));
        assert_eq!(j.copies, 2);
        let m = j.media.as_ref().unwrap();
        assert_eq!((m.width, m.height), (21000, 29700));
        assert_eq!(m.media_type.as_deref(), Some("com.canon-mtluster"));
        assert_eq!(m.source.as_deref(), Some("rear"));
        assert!(m.margins.unwrap().is_zero());

        // A PDF without poppler is planned (with a warning) so the dry run can still show it.
        let plan = plan_print(&printer, &[pdf], &PrintOptions::default(), &none).unwrap();
        assert_eq!(plan.jobs[0].job.format, "image/pwg-raster");
        assert_eq!(
            plan.jobs[0].job.media.as_ref().unwrap().media_type.as_deref(),
            Some("stationery")
        );
        assert!(plan.warnings.iter().any(|w| w.contains("poppler")));
        let text = plan.describe(&printer.ipp());
        assert!(
            text.contains("Validate-Job")
                && text.contains("Print-Job")
                && text.contains("image/pwg-raster"),
            "{text}"
        );

        // Photos: one job per photo, or per sheet.
        let many: Vec<PathBuf> = std::iter::repeat_n(jpg.clone(), 5).collect();
        assert_eq!(
            plan_print(&printer, &many, &PrintOptions::default(), &none).unwrap().jobs.len(),
            5
        );
        let sheets = plan_print(
            &printer,
            &many,
            &PrintOptions { layout: 4, ..PrintOptions::default() },
            &none,
        )
        .unwrap();
        assert_eq!(sheets.jobs.len(), 2);
        assert!(sheets.jobs[1].label.contains("1 photos"), "{}", sheets.jobs[1].label);
        // Misplaced options are refused.
        assert!(
            plan_print(
                &printer,
                std::slice::from_ref(&jpg),
                &PrintOptions {
                    pages: Some(PageSelection::parse("1").unwrap()),
                    ..PrintOptions::default()
                },
                &none
            )
            .is_err()
        );
        assert!(
            plan_print(
                &printer,
                &[jpg],
                &PrintOptions { duplex: Some(Duplex::Long), ..PrintOptions::default() },
                &none
            )
            .is_err()
        );
        assert!(plan_print(&printer, &[], &PrintOptions::default(), &none).is_err());
    }
}
