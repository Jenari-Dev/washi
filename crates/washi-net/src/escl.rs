// SPDX-License-Identifier: GPL-2.0-or-later
//! eSCL (the protocol behind AirScan / Mopria Scan, `_uscan._tcp`) client for the flatbed of a
//! Canon inkjet.
//!
//! * [`ScannerCapabilities`] / [`ScannerStatus`]: tolerant parsers (unknown elements are ignored,
//!   everything the device may omit is optional). The device serial number and UUID are *not*
//!   copied into the typed structure.
//! * [`ScanSettings`]: the `ScanSettings` document for `POST /eSCL/ScanJobs`.
//! * [`EsclClient`]: `GET ScannerCapabilities` / `ScannerStatus` are reads. Everything that makes
//!   the scanner move (`POST ScanJobs`, `GET .../NextDocument`, `DELETE`) needs a [`Confirm`].
//!
//! A scan is `POST ScanJobs` -> `201 Created` + `Location` -> `GET <job>/NextDocument` until the
//! scanner answers `404` (a platen scan yields exactly one page, so the second request is the
//! `404`) -> done. `DELETE <job>` cancels.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::Serialize;
use thiserror::Error;
use washi_ivec::{Node, escape};

use crate::client::Confirm;
use crate::error::NetError;
use crate::http::{Endpoint, HttpResponse, MAX_BODY, MAX_SCAN_BODY, request_once};

pub const DEFAULT_ESCL_ROOT: &str = "/eSCL";
pub const NS_SCAN: &str = "http://schemas.hp.com/imaging/escl/2011/05/03";
pub const NS_PWG: &str = "http://www.pwg.org/schemas/2010/12/sm";

/// `ScanRegion` units are 1/300 inch.
pub const UNITS_PER_INCH: u32 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EsclError {
    #[error("malformed eSCL reply: {0}")]
    Parse(String),
    #[error("the scanner refused the scan job (HTTP {status}): {hint}")]
    JobRejected { status: u16, hint: &'static str },
    #[error("the scanner is busy (HTTP 503)")]
    Busy,
    #[error("the scanner did not say where the job lives (no Location header)")]
    NoLocation,
    #[error("invalid scan settings: {0}")]
    Invalid(String),
}

// ---------------------------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------------------------

/// Where the original lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum InputSource {
    Platen,
    /// Automatic document feeder (`Feeder` in a request).
    Feeder,
}

impl InputSource {
    pub fn as_str(self) -> &'static str {
        match self {
            InputSource::Platen => "Platen",
            InputSource::Feeder => "Feeder",
        }
    }
}

/// `ColorMode` values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ColorMode {
    BlackAndWhite1,
    Grayscale8,
    Grayscale16,
    Rgb24,
    Rgb48,
    Other(String),
}

impl ColorMode {
    pub fn parse(s: &str) -> ColorMode {
        match s.trim() {
            "BlackAndWhite1" => ColorMode::BlackAndWhite1,
            "Grayscale8" => ColorMode::Grayscale8,
            "Grayscale16" => ColorMode::Grayscale16,
            "RGB24" => ColorMode::Rgb24,
            "RGB48" => ColorMode::Rgb48,
            other => ColorMode::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            ColorMode::BlackAndWhite1 => "BlackAndWhite1",
            ColorMode::Grayscale8 => "Grayscale8",
            ColorMode::Grayscale16 => "Grayscale16",
            ColorMode::Rgb24 => "RGB24",
            ColorMode::Rgb48 => "RGB48",
            ColorMode::Other(s) => s,
        }
    }
}

/// `Intent` values: presets that tune the scanner's image processing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Intent {
    Document,
    TextAndGraphic,
    Photo,
    Preview,
    Auto,
    Other(String),
}

impl Intent {
    pub fn parse(s: &str) -> Intent {
        match s.trim() {
            "Document" => Intent::Document,
            "TextAndGraphic" => Intent::TextAndGraphic,
            "Photo" => Intent::Photo,
            "Preview" => Intent::Preview,
            "Auto" => Intent::Auto,
            other => Intent::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Intent::Document => "Document",
            Intent::TextAndGraphic => "TextAndGraphic",
            Intent::Photo => "Photo",
            Intent::Preview => "Preview",
            Intent::Auto => "Auto",
            Intent::Other(s) => s,
        }
    }
}

/// An `x * y` resolution in dpi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Resolution {
    pub x: u32,
    pub y: u32,
}

/// A `Min` / `Max` / `Normal` / `Step` range (`BrightnessSupport`, `CompressionFactorSupport`,
/// `XResolutionRange`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RangeCap {
    pub min: i32,
    pub max: i32,
    pub normal: Option<i32>,
    pub step: Option<i32>,
}

impl RangeCap {
    fn from_node(n: &Node) -> Option<RangeCap> {
        let num = |name: &str| n.child_text(name).and_then(|t| t.trim().parse::<i32>().ok());
        Some(RangeCap {
            min: num("Min")?,
            max: num("Max")?,
            normal: num("Normal"),
            step: num("Step"),
        })
    }

    pub fn contains(&self, v: i32) -> bool {
        if v < self.min || v > self.max {
            return false;
        }
        match self.step {
            Some(s) if s > 1 => (v - self.min) % s == 0,
            _ => true,
        }
    }
}

/// What one input source (platen or feeder) can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceCaps {
    pub source: InputSource,
    /// The feeder scans both sides (`AdfDuplexInputCaps`).
    pub duplex: bool,
    /// Scan area limits in 1/300 inch.
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
    pub max_x_offset: Option<u32>,
    pub max_y_offset: Option<u32>,
    pub max_scan_regions: u32,
    pub color_modes: Vec<ColorMode>,
    pub default_color_mode: Option<ColorMode>,
    /// `ContentType` values (`Photo`, `Text`, `TextAndPhoto`, `Halftone`, ...).
    pub content_types: Vec<String>,
    /// MIME types (`DocumentFormat` and `DocumentFormatExt`, without duplicates).
    pub formats: Vec<String>,
    pub default_format: Option<String>,
    /// Discrete resolutions; empty when only a range is offered.
    pub resolutions: Vec<Resolution>,
    pub default_resolution: Option<Resolution>,
    pub x_resolution_range: Option<RangeCap>,
    pub y_resolution_range: Option<RangeCap>,
    pub color_spaces: Vec<String>,
    pub binary_renderings: Vec<String>,
    pub intents: Vec<Intent>,
    pub max_optical_x_resolution: Option<u32>,
    pub max_optical_y_resolution: Option<u32>,
    pub brightness: Option<RangeCap>,
    pub contrast: Option<RangeCap>,
    pub sharpen: Option<RangeCap>,
    pub threshold: Option<RangeCap>,
    pub noise_removal: Option<RangeCap>,
}

impl SourceCaps {
    /// Is this resolution offered?
    pub fn supports_resolution(&self, dpi: u32) -> bool {
        if self.resolutions.iter().any(|r| r.x == dpi && r.y == dpi) {
            return true;
        }
        let in_range = |r: &Option<RangeCap>| {
            r.as_ref().is_some_and(|r| i32::try_from(dpi).is_ok_and(|d| r.contains(d)))
        };
        in_range(&self.x_resolution_range) && in_range(&self.y_resolution_range)
    }

    /// The offered resolutions as a flat list of "square" dpi values (for UIs and error text).
    pub fn square_resolutions(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.resolutions.iter().filter(|r| r.x == r.y).map(|r| r.x).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn supports_color_mode(&self, m: &ColorMode) -> bool {
        self.color_modes.contains(m)
    }

    pub fn supports_format(&self, mime: &str) -> bool {
        self.formats.iter().any(|f| f.eq_ignore_ascii_case(mime))
    }

    pub fn supports_intent(&self, i: &Intent) -> bool {
        self.intents.contains(i)
    }
}

/// `GET /eSCL/ScannerCapabilities`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScannerCapabilities {
    /// eSCL version the scanner speaks (e.g. `2.9`).
    pub version: String,
    pub make_and_model: Option<String>,
    pub manufacturer: Option<String>,
    pub sources: Vec<SourceCaps>,
    pub compression_factor: Option<RangeCap>,
    pub media_types: Vec<String>,
}

fn num_u32(n: &Node, name: &str) -> Option<u32> {
    n.child_text(name).and_then(|t| t.trim().parse().ok())
}

fn texts(n: &Node, container: &str, item: &str) -> Vec<String> {
    n.child(container)
        .map(|c| c.children_named(item).map(|i| i.text.clone()).collect())
        .unwrap_or_default()
}

fn is_default(n: &Node) -> bool {
    n.attr("default").is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

/// First descendant (depth-first) with this local name.
fn deep<'a>(n: &'a Node, name: &str) -> Option<&'a Node> {
    for c in &n.children {
        if c.name == name {
            return Some(c);
        }
        if let Some(f) = deep(c, name) {
            return Some(f);
        }
    }
    None
}

fn push_unique<T: PartialEq>(v: &mut Vec<T>, item: T) {
    if !v.contains(&item) {
        v.push(item);
    }
}

fn parse_source(caps: &Node, source: InputSource, duplex: bool) -> SourceCaps {
    let mut out = SourceCaps {
        source,
        duplex,
        min_width: num_u32(caps, "MinWidth").unwrap_or(1),
        max_width: num_u32(caps, "MaxWidth").unwrap_or(0),
        min_height: num_u32(caps, "MinHeight").unwrap_or(1),
        max_height: num_u32(caps, "MaxHeight").unwrap_or(0),
        max_x_offset: num_u32(caps, "MaxXOffset"),
        max_y_offset: num_u32(caps, "MaxYOffset"),
        max_scan_regions: num_u32(caps, "MaxScanRegions").unwrap_or(1),
        color_modes: Vec::new(),
        default_color_mode: None,
        content_types: Vec::new(),
        formats: Vec::new(),
        default_format: None,
        resolutions: Vec::new(),
        default_resolution: None,
        x_resolution_range: None,
        y_resolution_range: None,
        color_spaces: Vec::new(),
        binary_renderings: Vec::new(),
        intents: texts(caps, "SupportedIntents", "Intent")
            .iter()
            .map(|s| Intent::parse(s))
            .collect(),
        max_optical_x_resolution: num_u32(caps, "MaxOpticalXResolution"),
        max_optical_y_resolution: num_u32(caps, "MaxOpticalYResolution"),
        brightness: deep(caps, "BrightnessSupport").and_then(RangeCap::from_node),
        contrast: deep(caps, "ContrastSupport").and_then(RangeCap::from_node),
        sharpen: deep(caps, "SharpenSupport").and_then(RangeCap::from_node),
        threshold: deep(caps, "ThresholdSupport").and_then(RangeCap::from_node),
        noise_removal: deep(caps, "NoiseRemovalSupport").and_then(RangeCap::from_node),
    };
    let profiles: Vec<&Node> = caps
        .child("SettingProfiles")
        .map(|p| p.children_named("SettingProfile").collect())
        .unwrap_or_default();
    for p in profiles {
        if let Some(modes) = p.child("ColorModes") {
            for m in modes.children_named("ColorMode") {
                let mode = ColorMode::parse(&m.text);
                if is_default(m) && out.default_color_mode.is_none() {
                    out.default_color_mode = Some(mode.clone());
                }
                push_unique(&mut out.color_modes, mode);
            }
        }
        for t in texts(p, "ContentTypes", "ContentType") {
            push_unique(&mut out.content_types, t);
        }
        if let Some(fmts) = p.child("DocumentFormats") {
            for f in fmts
                .children
                .iter()
                .filter(|c| c.name == "DocumentFormat" || c.name == "DocumentFormatExt")
            {
                if is_default(f) && out.default_format.is_none() {
                    out.default_format = Some(f.text.clone());
                }
                push_unique(&mut out.formats, f.text.clone());
            }
        }
        if let Some(res) = p.child("SupportedResolutions") {
            if let Some(d) = res.child("DiscreteResolutions") {
                for r in d.children_named("DiscreteResolution") {
                    if let (Some(x), Some(y)) =
                        (num_u32(r, "XResolution"), num_u32(r, "YResolution"))
                    {
                        let value = Resolution { x, y };
                        let marked = r.child("XResolution").is_some_and(is_default)
                            || r.child("YResolution").is_some_and(is_default);
                        if marked && out.default_resolution.is_none() {
                            out.default_resolution = Some(value);
                        }
                        push_unique(&mut out.resolutions, value);
                    }
                }
            }
            if let Some(r) = res.child("ResolutionRange") {
                out.x_resolution_range = r.child("XResolutionRange").and_then(RangeCap::from_node);
                out.y_resolution_range = r.child("YResolutionRange").and_then(RangeCap::from_node);
            }
        }
        for t in texts(p, "ColorSpaces", "ColorSpace") {
            push_unique(&mut out.color_spaces, t);
        }
        for t in texts(p, "BinaryRenderings", "BinaryRendering") {
            push_unique(&mut out.binary_renderings, t);
        }
    }
    out
}

impl ScannerCapabilities {
    pub fn parse(xml: &str) -> Result<ScannerCapabilities, EsclError> {
        let root = Node::parse(xml).map_err(|e| EsclError::Parse(e.to_string()))?;
        if root.name != "ScannerCapabilities" {
            return Err(EsclError::Parse(format!(
                "expected ScannerCapabilities, got {}",
                root.name
            )));
        }
        let mut sources = Vec::new();
        if let Some(c) = root.find(&["Platen", "PlatenInputCaps"]) {
            sources.push(parse_source(c, InputSource::Platen, false));
        }
        if let Some(adf) = root.child("Adf") {
            if let Some(c) = adf.child("AdfSimplexInputCaps") {
                sources.push(parse_source(c, InputSource::Feeder, false));
            }
            if let Some(c) = adf.child("AdfDuplexInputCaps") {
                sources.push(parse_source(c, InputSource::Feeder, true));
            }
        }
        Ok(ScannerCapabilities {
            version: root.child_text("Version").unwrap_or_default().to_string(),
            make_and_model: root.child_text("MakeAndModel").map(str::to_string),
            manufacturer: root.child_text("Manufacturer").map(str::to_string),
            sources,
            compression_factor: root
                .child("CompressionFactorSupport")
                .and_then(RangeCap::from_node),
            media_types: texts(&root, "SupportedMediaTypes", "MediaType"),
        })
    }

    pub fn platen(&self) -> Option<&SourceCaps> {
        self.sources.iter().find(|s| s.source == InputSource::Platen)
    }

    pub fn source(&self, source: InputSource) -> Option<&SourceCaps> {
        self.sources.iter().find(|s| s.source == source)
    }
}

/// `ScannerStatus/State`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ScannerState {
    Idle,
    Processing,
    Testing,
    Stopped,
    Down,
    Other(String),
}

impl ScannerState {
    pub fn as_str(&self) -> &str {
        match self {
            ScannerState::Idle => "Idle",
            ScannerState::Processing => "Processing",
            ScannerState::Testing => "Testing",
            ScannerState::Stopped => "Stopped",
            ScannerState::Down => "Down",
            ScannerState::Other(s) => s,
        }
    }
}

/// One entry of `ScannerStatus/Jobs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanJobInfo {
    pub uri: String,
    pub state: String,
    pub images_completed: Option<u32>,
    pub images_to_transfer: Option<u32>,
    pub reasons: Vec<String>,
}

/// `GET /eSCL/ScannerStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScannerStatus {
    pub version: String,
    pub state: ScannerState,
    pub adf_state: Option<String>,
    pub jobs: Vec<ScanJobInfo>,
}

impl ScannerStatus {
    pub fn parse(xml: &str) -> Result<ScannerStatus, EsclError> {
        let root = Node::parse(xml).map_err(|e| EsclError::Parse(e.to_string()))?;
        if root.name != "ScannerStatus" {
            return Err(EsclError::Parse(format!("expected ScannerStatus, got {}", root.name)));
        }
        let state = match root.child_text("State").unwrap_or_default() {
            "Idle" => ScannerState::Idle,
            "Processing" => ScannerState::Processing,
            "Testing" => ScannerState::Testing,
            "Stopped" => ScannerState::Stopped,
            "Down" => ScannerState::Down,
            other => ScannerState::Other(other.to_string()),
        };
        let jobs = root
            .child("Jobs")
            .map(|j| {
                j.children_named("JobInfo")
                    .map(|ji| ScanJobInfo {
                        uri: ji.child_text("JobUri").unwrap_or_default().to_string(),
                        state: ji.child_text("JobState").unwrap_or_default().to_string(),
                        images_completed: num_u32(ji, "ImagesCompleted"),
                        images_to_transfer: num_u32(ji, "ImagesToTransfer"),
                        reasons: texts(ji, "JobStateReasons", "JobStateReason"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(ScannerStatus {
            version: root.child_text("Version").unwrap_or_default().to_string(),
            state,
            adf_state: root.child_text("AdfState").map(str::to_string),
            jobs,
        })
    }

    pub fn is_idle(&self) -> bool {
        self.state == ScannerState::Idle
    }
}

// ---------------------------------------------------------------------------------------------
// ScanSettings
// ---------------------------------------------------------------------------------------------

/// The area to scan, in 1/300 inch from the platen's reference corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScanRegion {
    pub x_offset: u32,
    pub y_offset: u32,
    pub width: u32,
    pub height: u32,
}

impl ScanRegion {
    /// A region at the reference corner sized in millimetres (rounded to 1/300 inch).
    pub fn from_mm(width_mm: f64, height_mm: f64) -> ScanRegion {
        ScanRegion {
            x_offset: 0,
            y_offset: 0,
            width: mm_to_units(width_mm),
            height: mm_to_units(height_mm),
        }
    }
}

/// Millimetres to 1/300 inch.
pub fn mm_to_units(mm: f64) -> u32 {
    (mm / 25.4 * f64::from(UNITS_PER_INCH)).round().max(1.0) as u32
}

/// The body of `POST /eSCL/ScanJobs`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanSettings {
    /// `pwg:Version` the client claims to speak.
    pub version: String,
    pub intent: Option<Intent>,
    pub region: Option<ScanRegion>,
    pub input_source: InputSource,
    pub color_mode: ColorMode,
    pub x_resolution: u32,
    pub y_resolution: u32,
    /// `image/jpeg` or `application/pdf`.
    pub format: String,
    pub brightness: Option<i32>,
    pub contrast: Option<i32>,
    pub sharpen: Option<i32>,
    pub threshold: Option<i32>,
}

impl Default for ScanSettings {
    fn default() -> Self {
        ScanSettings {
            version: "2.1".to_string(),
            intent: None,
            region: None,
            input_source: InputSource::Platen,
            color_mode: ColorMode::Rgb24,
            x_resolution: 300,
            y_resolution: 300,
            format: "image/jpeg".to_string(),
            brightness: None,
            contrast: None,
            sharpen: None,
            threshold: None,
        }
    }
}

impl ScanSettings {
    /// The `ScanSettings` XML document.
    pub fn to_xml(&self) -> String {
        let mut x = String::new();
        x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        x.push_str(&format!(
            "<scan:ScanSettings xmlns:pwg=\"{NS_PWG}\" xmlns:scan=\"{NS_SCAN}\">\n"
        ));
        x.push_str(&format!("  <pwg:Version>{}</pwg:Version>\n", escape(&self.version)));
        if let Some(i) = &self.intent {
            x.push_str(&format!("  <scan:Intent>{}</scan:Intent>\n", escape(i.as_str())));
        }
        if let Some(r) = &self.region {
            x.push_str("  <pwg:ScanRegions>\n    <pwg:ScanRegion>\n");
            x.push_str(&format!("      <pwg:Height>{}</pwg:Height>\n", r.height));
            x.push_str(
                "      <pwg:ContentRegionUnits>escl:ThreeHundredthsOfInches</pwg:ContentRegionUnits>\n",
            );
            x.push_str(&format!("      <pwg:Width>{}</pwg:Width>\n", r.width));
            x.push_str(&format!("      <pwg:XOffset>{}</pwg:XOffset>\n", r.x_offset));
            x.push_str(&format!("      <pwg:YOffset>{}</pwg:YOffset>\n", r.y_offset));
            x.push_str("    </pwg:ScanRegion>\n  </pwg:ScanRegions>\n");
        }
        x.push_str(&format!(
            "  <pwg:DocumentFormat>{0}</pwg:DocumentFormat>\n  <scan:DocumentFormatExt>{0}</scan:DocumentFormatExt>\n",
            escape(&self.format)
        ));
        x.push_str(&format!(
            "  <pwg:InputSource>{}</pwg:InputSource>\n",
            self.input_source.as_str()
        ));
        x.push_str(&format!(
            "  <scan:ColorMode>{}</scan:ColorMode>\n",
            escape(self.color_mode.as_str())
        ));
        x.push_str(&format!("  <scan:XResolution>{}</scan:XResolution>\n", self.x_resolution));
        x.push_str(&format!("  <scan:YResolution>{}</scan:YResolution>\n", self.y_resolution));
        for (tag, v) in [
            ("Brightness", self.brightness),
            ("Contrast", self.contrast),
            ("Sharpen", self.sharpen),
            ("Threshold", self.threshold),
        ] {
            if let Some(v) = v {
                x.push_str(&format!("  <scan:{tag}>{v}</scan:{tag}>\n"));
            }
        }
        x.push_str("</scan:ScanSettings>\n");
        x
    }

    /// Check the request against what the scanner offers; the message names the allowed values.
    pub fn validate(&self, caps: &ScannerCapabilities) -> Result<(), EsclError> {
        let invalid = |m: String| Err(EsclError::Invalid(m));
        let Some(src) = caps.source(self.input_source) else {
            return invalid(format!("this scanner has no {} input", self.input_source.as_str()));
        };
        if self.x_resolution != self.y_resolution || !src.supports_resolution(self.x_resolution) {
            let list = src.square_resolutions();
            return invalid(format!(
                "{}x{} dpi is not offered (available: {})",
                self.x_resolution,
                self.y_resolution,
                list.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            ));
        }
        if !src.color_modes.is_empty() && !src.supports_color_mode(&self.color_mode) {
            return invalid(format!(
                "colour mode {} is not offered (available: {})",
                self.color_mode.as_str(),
                src.color_modes.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
        if !src.formats.is_empty() && !src.supports_format(&self.format) {
            return invalid(format!(
                "format {} is not offered (available: {})",
                self.format,
                src.formats.join(", ")
            ));
        }
        if let Some(i) = &self.intent {
            if !src.intents.is_empty() && !src.supports_intent(i) {
                return invalid(format!("intent {} is not offered", i.as_str()));
            }
        }
        if let Some(r) = &self.region {
            if r.width == 0 || r.height == 0 {
                return invalid("the scan region is empty".to_string());
            }
            if src.max_width > 0
                && (r.x_offset.saturating_add(r.width) > src.max_width
                    || r.y_offset.saturating_add(r.height) > src.max_height)
            {
                return invalid(format!(
                    "the scan region ({}x{} at {},{}) exceeds the scan area of {}x{} (1/300 inch)",
                    r.width, r.height, r.x_offset, r.y_offset, src.max_width, src.max_height
                ));
            }
        }
        for (name, v, range) in [
            ("brightness", self.brightness, &src.brightness),
            ("contrast", self.contrast, &src.contrast),
            ("sharpen", self.sharpen, &src.sharpen),
            ("threshold", self.threshold, &src.threshold),
        ] {
            if let Some(v) = v {
                match range {
                    Some(r) if r.contains(v) => {}
                    Some(r) => {
                        return invalid(format!("{name} {v} is outside {}..{}", r.min, r.max));
                    }
                    None => return invalid(format!("this scanner has no {name} setting")),
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EsclOptions {
    pub port: u16,
    pub root: String,
    pub connect_timeout: Duration,
    /// Socket timeout for the small requests (capabilities, status, job creation).
    pub io_timeout: Duration,
    /// How long `GET NextDocument` may take (a 1200 dpi page needs a minute or more).
    pub page_timeout: Duration,
    /// How long to keep retrying `503 Busy`.
    pub busy_timeout: Duration,
    pub busy_retry: Duration,
}

impl Default for EsclOptions {
    fn default() -> Self {
        EsclOptions {
            port: 80,
            root: DEFAULT_ESCL_ROOT.to_string(),
            connect_timeout: Duration::from_secs(5),
            io_timeout: Duration::from_secs(15),
            page_timeout: Duration::from_secs(300),
            busy_timeout: Duration::from_secs(30),
            busy_retry: Duration::from_millis(750),
        }
    }
}

/// A scan job on the scanner (`/eSCL/ScanJobs/<id>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanJob {
    /// Absolute path of the job, e.g. `/eSCL/ScanJobs/1234`.
    pub path: String,
}

/// One scanned page.
#[derive(Debug, Clone)]
pub struct ScannedDocument {
    pub content_type: String,
    pub data: Vec<u8>,
}

/// eSCL client: one connection per request (`Connection: close`).
#[derive(Debug, Clone)]
pub struct EsclClient {
    host: String,
    opts: EsclOptions,
}

/// `http://host:port/path` -> `/path`; paths pass through.
fn location_path(location: &str, root: &str) -> String {
    let loc = location.trim();
    let path = match loc.find("://") {
        Some(i) => {
            let after = &loc[i + 3..];
            after.find('/').map_or("/", |p| &after[p..])
        }
        None => loc,
    };
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{root}/{}", path.trim_start_matches('/'))
    }
}

impl EsclClient {
    pub fn new(host: impl Into<String>) -> Self {
        Self::with_options(host, EsclOptions::default())
    }

    pub fn with_options(host: impl Into<String>, opts: EsclOptions) -> Self {
        EsclClient { host: host.into(), opts }
    }

    pub fn options(&self) -> &EsclOptions {
        &self.opts
    }

    fn endpoint(&self, io_timeout: Duration) -> Endpoint<'_> {
        Endpoint {
            host: &self.host,
            port: self.opts.port,
            connect_timeout: self.opts.connect_timeout,
            io_timeout,
        }
    }

    fn host_header(&self) -> String {
        self.endpoint(self.opts.io_timeout).host_header(80)
    }

    /// `http://host[:port]` for display.
    pub fn origin(&self) -> String {
        format!("http://{}", self.host_header())
    }

    pub fn scan_jobs_path(&self) -> String {
        format!("{}/ScanJobs", self.opts.root)
    }

    fn get(&self, path: &str, io_timeout: Duration, max: usize) -> Result<HttpResponse, NetError> {
        let headers = [("Host", self.host_header()), ("Accept", "*/*".to_string())];
        request_once(self.endpoint(io_timeout), "GET", path, &headers, &[], None, max)
    }

    fn get_text(&self, name: &str) -> Result<String, NetError> {
        let path = format!("{}/{name}", self.opts.root);
        let r = self.get(&path, self.opts.io_timeout, MAX_BODY)?;
        if r.status != 200 {
            return Err(NetError::Status { status: r.status });
        }
        Ok(String::from_utf8_lossy(&r.body).into_owned())
    }

    /// `GET /eSCL/ScannerCapabilities` (read-only).
    pub fn capabilities_xml(&self) -> Result<String, NetError> {
        self.get_text("ScannerCapabilities")
    }

    /// `GET /eSCL/ScannerCapabilities`, parsed (read-only).
    pub fn capabilities(&self) -> Result<ScannerCapabilities, NetError> {
        Ok(ScannerCapabilities::parse(&self.capabilities_xml()?)?)
    }

    /// `GET /eSCL/ScannerStatus` (read-only).
    pub fn status(&self) -> Result<ScannerStatus, NetError> {
        Ok(ScannerStatus::parse(&self.get_text("ScannerStatus")?)?)
    }

    /// The exact request `create_job` would send, as text (for dry runs). Touches nothing.
    pub fn describe_scan(&self, settings: &ScanSettings) -> String {
        let xml = settings.to_xml();
        let mut s = String::new();
        s.push_str(&format!("POST {}{} HTTP/1.1\n", self.origin(), self.scan_jobs_path()));
        s.push_str(&format!(
            "Host: {}\nContent-Type: text/xml\nContent-Length: {}\n\n",
            self.host_header(),
            xml.len()
        ));
        s.push_str(&xml);
        s.push_str(&format!(
            "\nthen, with the job URL from the Location header of the 201 reply:\n  GET {}{}/ScanJobs/<job>/NextDocument   (repeated until the scanner answers 404)\n  DELETE {}{}/ScanJobs/<job>               (only to cancel)\n",
            self.origin(),
            self.opts.root,
            self.origin(),
            self.opts.root
        ));
        s
    }

    /// `POST /eSCL/ScanJobs`. Starts a scan: needs a [`Confirm`].
    pub fn create_job(
        &self,
        settings: &ScanSettings,
        _confirm: Confirm,
    ) -> Result<ScanJob, NetError> {
        let body = settings.to_xml();
        let deadline = Instant::now() + self.opts.busy_timeout;
        loop {
            let headers = [("Host", self.host_header()), ("Content-Type", "text/xml".to_string())];
            let r = request_once(
                self.endpoint(self.opts.io_timeout),
                "POST",
                &self.scan_jobs_path(),
                &headers,
                body.as_bytes(),
                None,
                MAX_BODY,
            )?;
            match r.status {
                200..=202 => {
                    let loc = r.header("location").ok_or(EsclError::NoLocation)?;
                    return Ok(ScanJob { path: location_path(loc, &self.opts.root) });
                }
                503 if Instant::now() < deadline => sleep(self.opts.busy_retry),
                503 => return Err(EsclError::Busy.into()),
                status => {
                    let hint = match status {
                        400 => "the scanner did not understand the settings",
                        409 => "the scanner is occupied by another job or the cover is open",
                        413 => "the scanner cannot take that many pages or such a large area",
                        415 => "the scanner does not support that document format",
                        _ => "unexpected reply",
                    };
                    return Err(EsclError::JobRejected { status, hint }.into());
                }
            }
        }
    }

    /// `GET <job>/NextDocument`: the next page, or `None` once the scanner answers 404. Moves
    /// the scanner, so it needs a [`Confirm`].
    pub fn next_document(
        &self,
        job: &ScanJob,
        _confirm: Confirm,
    ) -> Result<Option<ScannedDocument>, NetError> {
        let path = format!("{}/NextDocument", job.path.trim_end_matches('/'));
        let deadline = Instant::now() + self.opts.busy_timeout;
        loop {
            let r = self.get(&path, self.opts.page_timeout, MAX_SCAN_BODY)?;
            match r.status {
                200 => {
                    let content_type = r.header("content-type").unwrap_or("").to_string();
                    return Ok(Some(ScannedDocument { content_type, data: r.body }));
                }
                404 => return Ok(None),
                503 if Instant::now() < deadline => sleep(self.opts.busy_retry),
                503 => return Err(EsclError::Busy.into()),
                status => return Err(NetError::Status { status }),
            }
        }
    }

    /// `DELETE <job>`: cancel (or clean up) a job. A job that is already gone counts as done.
    pub fn cancel(&self, job: &ScanJob, _confirm: Confirm) -> Result<(), NetError> {
        let headers = [("Host", self.host_header())];
        let r = request_once(
            self.endpoint(self.opts.io_timeout),
            "DELETE",
            &job.path,
            &headers,
            &[],
            None,
            MAX_BODY,
        )?;
        match r.status {
            200 | 202 | 204 | 404 => Ok(()),
            status => Err(NetError::Status { status }),
        }
    }

    /// Run a whole scan: create the job and fetch pages until the scanner says 404. `on_page`
    /// gets each page as soon as it arrives. Returns the number of pages. On an error after the
    /// job was created the job is cancelled (best effort).
    pub fn scan(
        &self,
        settings: &ScanSettings,
        confirm: Confirm,
        mut on_page: impl FnMut(ScannedDocument) -> Result<(), NetError>,
    ) -> Result<usize, NetError> {
        let job = self.create_job(settings, confirm)?;
        let mut pages = 0usize;
        loop {
            match self.next_document(&job, confirm) {
                Ok(Some(doc)) => {
                    pages += 1;
                    if let Err(e) = on_page(doc) {
                        let _ = self.cancel(&job, confirm);
                        return Err(e);
                    }
                }
                Ok(None) => return Ok(pages),
                Err(e) => {
                    let _ = self.cancel(&job, confirm);
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: &str = include_str!("../tests/fixtures/escl-capabilities-xk140.xml");
    const STATUS: &str = include_str!("../tests/fixtures/escl-status-idle.xml");

    #[test]
    fn xk140_capabilities_parse() {
        let c = ScannerCapabilities::parse(CAPS).unwrap();
        assert_eq!(c.version, "2.9");
        assert_eq!(c.make_and_model.as_deref(), Some("Canon XK140 series"));
        assert_eq!(c.manufacturer.as_deref(), Some("Canon"));
        assert_eq!(c.sources.len(), 1, "platen only");
        let p = c.platen().unwrap();
        assert!(!p.duplex);
        assert_eq!((p.max_width, p.max_height), (2550, 3508));
        assert_eq!((p.min_width, p.min_height), (1, 1));
        assert_eq!((p.max_x_offset, p.max_y_offset), (Some(2549), Some(3507)));
        assert_eq!(p.color_modes, [ColorMode::Grayscale8, ColorMode::Rgb24]);
        assert_eq!(p.default_color_mode, Some(ColorMode::Rgb24));
        assert_eq!(p.formats, ["image/jpeg", "application/pdf"]);
        assert_eq!(p.default_format.as_deref(), Some("image/jpeg"));
        assert_eq!(p.square_resolutions(), [75, 100, 150, 200, 300, 600, 1200]);
        assert_eq!(p.default_resolution, Some(Resolution { x: 300, y: 300 }));
        assert_eq!(
            p.intents,
            [
                Intent::Document,
                Intent::Photo,
                Intent::Preview,
                Intent::TextAndGraphic,
                Intent::Auto
            ]
        );
        assert_eq!(p.content_types, ["Photo", "Text", "TextAndPhoto", "Halftone", "Thru", "Auto"]);
        assert_eq!(p.color_spaces, ["sRGB"]);
        assert_eq!(p.max_optical_x_resolution, Some(1200));
        assert!(p.brightness.is_none() && p.contrast.is_none() && p.sharpen.is_none());
        assert!(p.supports_resolution(600) && !p.supports_resolution(400));
        assert_eq!(
            c.compression_factor,
            Some(RangeCap { min: 0, max: 1, normal: Some(1), step: Some(1) })
        );
        assert_eq!(c.media_types, ["BackgroundReduction"]);
        // The unit's identity is not carried into the typed structure.
        assert!(!format!("{c:?}").contains("000000000000"));
    }

    #[test]
    fn ranges_feeder_and_resolution_ranges() {
        let xml = r#"<scan:ScannerCapabilities xmlns:scan="s" xmlns:pwg="p">
          <pwg:Version>2.6</pwg:Version>
          <scan:Platen><scan:PlatenInputCaps>
            <scan:MaxWidth>2550</scan:MaxWidth><scan:MaxHeight>3507</scan:MaxHeight>
            <scan:SettingProfiles><scan:SettingProfile>
              <scan:ColorModes><scan:ColorMode>BlackAndWhite1</scan:ColorMode>
                <scan:ColorMode>RGB24</scan:ColorMode></scan:ColorModes>
              <scan:SupportedResolutions><scan:ResolutionRange>
                <scan:XResolutionRange><scan:Min>75</scan:Min><scan:Max>600</scan:Max><scan:Normal>300</scan:Normal><scan:Step>25</scan:Step></scan:XResolutionRange>
                <scan:YResolutionRange><scan:Min>75</scan:Min><scan:Max>600</scan:Max><scan:Normal>300</scan:Normal><scan:Step>25</scan:Step></scan:YResolutionRange>
              </scan:ResolutionRange></scan:SupportedResolutions>
            </scan:SettingProfile></scan:SettingProfiles>
            <scan:BrightnessSupport><scan:Min>0</scan:Min><scan:Max>2000</scan:Max><scan:Normal>1000</scan:Normal><scan:Step>1</scan:Step></scan:BrightnessSupport>
            <scan:SharpenSupport><scan:Min>0</scan:Min><scan:Max>100</scan:Max></scan:SharpenSupport>
          </scan:PlatenInputCaps></scan:Platen>
          <scan:Adf>
            <scan:AdfSimplexInputCaps><scan:MaxWidth>2550</scan:MaxWidth><scan:MaxHeight>4200</scan:MaxHeight></scan:AdfSimplexInputCaps>
            <scan:AdfDuplexInputCaps><scan:MaxWidth>2550</scan:MaxWidth><scan:MaxHeight>4200</scan:MaxHeight></scan:AdfDuplexInputCaps>
          </scan:Adf>
        </scan:ScannerCapabilities>"#;
        let c = ScannerCapabilities::parse(xml).unwrap();
        assert_eq!(c.sources.len(), 3);
        let p = c.platen().unwrap();
        assert!(p.resolutions.is_empty());
        assert!(
            p.supports_resolution(250)
                && !p.supports_resolution(260)
                && !p.supports_resolution(700)
        );
        assert_eq!(
            p.brightness,
            Some(RangeCap { min: 0, max: 2000, normal: Some(1000), step: Some(1) })
        );
        assert_eq!(p.sharpen.map(|r| (r.min, r.max)), Some((0, 100)));
        assert!(p.contrast.is_none());
        assert!(p.color_modes.contains(&ColorMode::BlackAndWhite1));
        assert_eq!(c.sources.iter().filter(|s| s.source == InputSource::Feeder).count(), 2);
        assert!(c.sources.iter().any(|s| s.duplex));
    }

    #[test]
    fn bad_documents_are_errors() {
        assert!(ScannerCapabilities::parse("").is_err());
        assert!(ScannerCapabilities::parse("<a/>").is_err());
        assert!(ScannerStatus::parse("<ScannerCapabilities/>").is_err());
        assert!(ScannerCapabilities::parse("<ScannerCapabilities/>").unwrap().sources.is_empty());
    }

    #[test]
    fn status_parses_idle_and_jobs() {
        let s = ScannerStatus::parse(STATUS).unwrap();
        assert!(s.is_idle());
        assert_eq!(s.version, "2.9");
        assert!(s.jobs.is_empty());
        let busy = r#"<scan:ScannerStatus xmlns:scan="s" xmlns:pwg="p"><pwg:Version>2.1</pwg:Version>
            <pwg:State>Processing</pwg:State><scan:Jobs><scan:JobInfo><pwg:JobUri>/eSCL/ScanJobs/1</pwg:JobUri>
            <pwg:JobUuid>x</pwg:JobUuid><scan:ImagesCompleted>1</scan:ImagesCompleted>
            <pwg:JobState>Processing</pwg:JobState><pwg:JobStateReasons><pwg:JobStateReason>JobScanning</pwg:JobStateReason></pwg:JobStateReasons>
            </scan:JobInfo></scan:Jobs></scan:ScannerStatus>"#;
        let s = ScannerStatus::parse(busy).unwrap();
        assert_eq!(s.state, ScannerState::Processing);
        assert_eq!(s.jobs[0].uri, "/eSCL/ScanJobs/1");
        assert_eq!(s.jobs[0].images_completed, Some(1));
        assert_eq!(s.jobs[0].reasons, ["JobScanning"]);
        assert_eq!(
            ScannerStatus::parse("<ScannerStatus><State>Down</State></ScannerStatus>")
                .unwrap()
                .state,
            ScannerState::Down
        );
    }

    #[test]
    fn scan_settings_xml_is_exact() {
        let s = ScanSettings {
            intent: Some(Intent::Document),
            region: Some(ScanRegion { x_offset: 0, y_offset: 0, width: 2480, height: 3508 }),
            color_mode: ColorMode::Grayscale8,
            x_resolution: 150,
            y_resolution: 150,
            format: "application/pdf".into(),
            ..ScanSettings::default()
        };
        let want = [
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<scan:ScanSettings xmlns:pwg=\"http://www.pwg.org/schemas/2010/12/sm\" xmlns:scan=\"http://schemas.hp.com/imaging/escl/2011/05/03\">",
            "  <pwg:Version>2.1</pwg:Version>",
            "  <scan:Intent>Document</scan:Intent>",
            "  <pwg:ScanRegions>",
            "    <pwg:ScanRegion>",
            "      <pwg:Height>3508</pwg:Height>",
            "      <pwg:ContentRegionUnits>escl:ThreeHundredthsOfInches</pwg:ContentRegionUnits>",
            "      <pwg:Width>2480</pwg:Width>",
            "      <pwg:XOffset>0</pwg:XOffset>",
            "      <pwg:YOffset>0</pwg:YOffset>",
            "    </pwg:ScanRegion>",
            "  </pwg:ScanRegions>",
            "  <pwg:DocumentFormat>application/pdf</pwg:DocumentFormat>",
            "  <scan:DocumentFormatExt>application/pdf</scan:DocumentFormatExt>",
            "  <pwg:InputSource>Platen</pwg:InputSource>",
            "  <scan:ColorMode>Grayscale8</scan:ColorMode>",
            "  <scan:XResolution>150</scan:XResolution>",
            "  <scan:YResolution>150</scan:YResolution>",
            "</scan:ScanSettings>",
            "",
        ]
        .join("\n");
        assert_eq!(s.to_xml(), want);
        // And it is well-formed XML that our own parser reads back.
        let n = Node::parse(&s.to_xml()).unwrap();
        assert_eq!(n.name, "ScanSettings");
        assert_eq!(n.find(&["ScanRegions", "ScanRegion", "Width"]).unwrap().text, "2480");
    }

    #[test]
    fn minimal_settings_have_no_region_or_intent() {
        let x = ScanSettings::default().to_xml();
        assert!(!x.contains("ScanRegions") && !x.contains("Intent"));
        assert!(x.contains("<scan:ColorMode>RGB24</scan:ColorMode>"));
        let with =
            ScanSettings { brightness: Some(900), threshold: Some(128), ..ScanSettings::default() }
                .to_xml();
        assert!(with.contains("<scan:Brightness>900</scan:Brightness>"));
        assert!(with.contains("<scan:Threshold>128</scan:Threshold>"));
    }

    #[test]
    fn validation_names_the_allowed_values() {
        let caps = ScannerCapabilities::parse(CAPS).unwrap();
        let ok = ScanSettings {
            region: Some(ScanRegion { x_offset: 0, y_offset: 0, width: 2550, height: 3508 }),
            ..ScanSettings::default()
        };
        assert!(ok.validate(&caps).is_ok());
        let e = ScanSettings { x_resolution: 400, y_resolution: 400, ..ok.clone() }
            .validate(&caps)
            .unwrap_err()
            .to_string();
        assert!(e.contains("400x400") && e.contains("75, 100, 150, 200, 300, 600, 1200"), "{e}");
        let e = ScanSettings { color_mode: ColorMode::BlackAndWhite1, ..ok.clone() }
            .validate(&caps)
            .unwrap_err()
            .to_string();
        assert!(e.contains("BlackAndWhite1") && e.contains("Grayscale8, RGB24"), "{e}");
        let e =
            ScanSettings { format: "image/png".into(), ..ok.clone() }.validate(&caps).unwrap_err();
        assert!(e.to_string().contains("image/jpeg"));
        let big = ScanSettings {
            region: Some(ScanRegion { x_offset: 100, y_offset: 0, width: 2550, height: 3508 }),
            ..ok.clone()
        };
        assert!(big.validate(&caps).unwrap_err().to_string().contains("exceeds"));
        assert!(
            ScanSettings { brightness: Some(10), ..ok.clone() }
                .validate(&caps)
                .unwrap_err()
                .to_string()
                .contains("no brightness")
        );
        assert!(ScanSettings { input_source: InputSource::Feeder, ..ok }.validate(&caps).is_err());
    }

    #[test]
    fn location_headers_are_normalised() {
        let r = DEFAULT_ESCL_ROOT;
        assert_eq!(location_path("http://192.0.2.1/eSCL/ScanJobs/42", r), "/eSCL/ScanJobs/42");
        assert_eq!(location_path("https://h:443/eSCL/ScanJobs/42/", r), "/eSCL/ScanJobs/42/");
        assert_eq!(location_path("/eSCL/ScanJobs/7", r), "/eSCL/ScanJobs/7");
        assert_eq!(location_path("ScanJobs/7", r), "/eSCL/ScanJobs/7");
        assert_eq!(location_path("http://h", r), "/");
    }

    #[test]
    fn mm_conversion_matches_paper_sizes() {
        assert_eq!(mm_to_units(210.0), 2480);
        assert_eq!(mm_to_units(297.0), 3508);
        assert_eq!(mm_to_units(215.9), 2550);
        assert_eq!(mm_to_units(279.4), 3300);
        let r = ScanRegion::from_mm(148.0, 210.0);
        assert_eq!((r.width, r.height), (1748, 2480));
    }

    #[test]
    fn describe_scan_lists_the_whole_sequence() {
        let c = EsclClient::new("printer.local");
        let d = c.describe_scan(&ScanSettings::default());
        assert!(d.starts_with("POST http://printer.local/eSCL/ScanJobs HTTP/1.1"), "{d}");
        assert!(
            d.contains("Content-Type: text/xml")
                && d.contains("NextDocument")
                && d.contains("DELETE")
        );
        let o = EsclOptions { port: 8080, ..EsclOptions::default() };
        assert!(EsclClient::with_options("127.0.0.1", o).origin().ends_with("127.0.0.1:8080"));
    }
}
