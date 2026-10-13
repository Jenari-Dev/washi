// SPDX-License-Identifier: GPL-2.0-or-later
//! Scanning through the printer's eSCL interface: build the request ([`ScanRequest`] ->
//! [`ScanSettings`]), show it without sending ([`plan_scan`]), run it ([`run_scan`]).
//!
//! The scanner delivers a JPEG per page. Colour modes the scanner lacks are done on the host:
//! the XK140 offers `RGB24` and `Grayscale8` only, so "Black and white" is a grey scan that is
//! thresholded when the page is rendered. "Auto" original size scans the whole platen and crops
//! the object on the glass on the host ([`super::detect::auto_crop`]).

use serde::{Deserialize, Serialize};
use washi_net::escl::{ColorMode, Intent, ScanSettings, ScannerCapabilities};

use super::correct::OutputMode;
use super::detect;
use super::sizes::SizeChoice;
use super::{ImageCorrection, Page};
use crate::Confirm;
use crate::error::{Error, Result};
use crate::printer::Printer;

/// "Original Type".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OriginalKind {
    Document,
    Photo,
}

impl OriginalKind {
    pub fn parse(s: &str) -> Option<OriginalKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "document" | "doc" | "text" => Some(OriginalKind::Document),
            "photo" | "picture" => Some(OriginalKind::Photo),
            _ => None,
        }
    }
}

/// What the Scan screen asks for.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScanRequest {
    pub size: SizeChoice,
    pub resolution: u32,
    pub mode: OutputMode,
    pub kind: OriginalKind,
    /// Send the eSCL `Intent` (Document / Photo). Turn it off when a scanner rejects it.
    pub use_intent: bool,
    /// For `SizeChoice::Auto`: crop the biggest object on the platen on the host. Turn it off
    /// when the caller wants the whole platen (multi-photo crop, auto scan).
    pub auto_crop: bool,
}

impl Default for ScanRequest {
    fn default() -> Self {
        ScanRequest {
            size: SizeChoice::Fixed(super::sizes::SIZES[0]),
            resolution: 300,
            mode: OutputMode::Color,
            kind: OriginalKind::Document,
            use_intent: true,
            auto_crop: true,
        }
    }
}

/// Resolutions of the Scan screen's dropdown (the XK140 offers exactly these).
pub const RESOLUTIONS: [u32; 7] = [75, 100, 150, 200, 300, 600, 1200];

impl ScanRequest {
    /// The eSCL settings. With `caps` the region is cut to the scan area and colour modes the
    /// scanner lacks are replaced; without, the XK140's behaviour is assumed.
    pub fn settings(&self, caps: Option<&ScannerCapabilities>) -> ScanSettings {
        let platen = caps.and_then(|c| c.platen());
        let wants_gray = self.mode != OutputMode::Color;
        let color_mode = if wants_gray
            && platen.is_none_or(|p| {
                p.color_modes.is_empty() || p.supports_color_mode(&ColorMode::Grayscale8)
            }) {
            ColorMode::Grayscale8
        } else {
            ColorMode::Rgb24
        };
        let intent = match (self.use_intent, self.kind) {
            (false, _) => None,
            (true, OriginalKind::Document) => Some(Intent::Document),
            (true, OriginalKind::Photo) => Some(Intent::Photo),
        }
        .filter(|i| platen.is_none_or(|p| p.intents.is_empty() || p.supports_intent(i)));
        ScanSettings {
            intent,
            region: Some(self.size.region(platen)),
            color_mode,
            x_resolution: self.resolution,
            y_resolution: self.resolution,
            ..ScanSettings::default()
        }
    }

    /// Check what can be checked without a scanner.
    pub fn validate(&self) -> Result<()> {
        if !RESOLUTIONS.contains(&self.resolution) {
            return Err(Error::Invalid(format!(
                "resolution {} dpi is not offered (choose one of: {})",
                self.resolution,
                RESOLUTIONS.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            )));
        }
        Ok(())
    }
}

/// What a scan would do, without doing it.
#[derive(Debug, Clone)]
pub struct ScanPlan {
    pub settings: ScanSettings,
    /// The HTTP requests, as text.
    pub request: String,
    /// What happens on the host afterwards.
    pub host_steps: Vec<String>,
}

pub fn plan_scan(printer: &Printer, req: &ScanRequest) -> Result<ScanPlan> {
    req.validate()?;
    let settings = req.settings(None);
    let request = printer.escl().describe_scan(&settings);
    let mut host_steps = vec![format!(
        "decode the JPEG page(s) as {} dpi {}",
        req.resolution,
        if req.mode == OutputMode::Color { "colour" } else { "grey" }
    )];
    if req.size == SizeChoice::Auto && req.auto_crop {
        host_steps.push("auto-crop the object on the platen (the whole platen is scanned)".into());
    }
    if req.mode == OutputMode::BlackWhite {
        host_steps
            .push("convert to black and white on the host (the scanner has no 1-bit mode)".into());
    }
    Ok(ScanPlan { settings, request, host_steps })
}

/// Progress of a running scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanProgress {
    /// Capabilities were read and the request validated.
    Checked,
    /// The scanner accepted the job.
    Started,
    /// Page `index` (0-based) arrived: `bytes` of JPEG.
    PageReceived {
        index: usize,
        bytes: usize,
    },
    Done {
        pages: usize,
    },
}

/// One scanned page and the bytes the scanner sent.
#[derive(Debug, Clone)]
pub struct ScannedPage {
    pub page: Page,
    /// The page as a JPEG: exactly what the scanner sent, or (for "Auto" size) the cropped
    /// image re-encoded.
    pub jpeg: Vec<u8>,
}

fn decode_page(req: &ScanRequest, data: &[u8]) -> Result<ScannedPage> {
    let mut page = Page::from_bytes(data, req.resolution)?;
    let mut jpeg = data.to_vec();
    if req.size == SizeChoice::Auto && req.auto_crop {
        let rgb = page.original().to_rgb8();
        if let Some(cropped) = detect::auto_crop(&rgb) {
            let img = image::DynamicImage::ImageRgb8(cropped);
            jpeg = super::save::encode_jpeg(&img, 92, req.resolution)?;
            page = Page::new(img, req.resolution);
        }
    }
    page.set_correction(ImageCorrection { color_mode: req.mode, ..ImageCorrection::default() });
    Ok(ScannedPage { page, jpeg })
}

/// Scan one job (one platen page) and hand every page to `on_page` as soon as it has been
/// received and decoded. **Moves the scanner**, so it needs a [`Confirm`]. Reads the
/// capabilities first (read-only) and refuses a request the scanner would not take.
pub fn run_scan_streaming(
    printer: &Printer,
    req: &ScanRequest,
    confirm: Confirm,
    progress: &mut dyn FnMut(ScanProgress),
    on_page: &mut dyn FnMut(usize, ScannedPage) -> Result<()>,
) -> Result<usize> {
    req.validate()?;
    let escl = printer.escl();
    let caps = escl.capabilities()?;
    let settings = req.settings(Some(&caps));
    settings.validate(&caps).map_err(|e| Error::Invalid(e.to_string()))?;
    // A busy scanner (or one with its cover open) says so in its status: ask before starting.
    if let Ok(status) = escl.status() {
        if !status.is_idle() {
            return Err(Error::Invalid(format!(
                "the scanner is {} right now; wait until it is idle and make sure the cover is closed",
                status.state.as_str().to_ascii_lowercase()
            )));
        }
    }
    progress(ScanProgress::Checked);
    let mut started = false;
    let mut failure: Option<Error> = None;
    let mut index = 0usize;
    let result = escl.scan(&settings, confirm, |doc| {
        if !started {
            started = true;
            progress(ScanProgress::Started);
        }
        progress(ScanProgress::PageReceived { index, bytes: doc.data.len() });
        let step = decode_page(req, &doc.data).and_then(|sp| on_page(index, sp));
        index += 1;
        match step {
            Ok(()) => Ok(()),
            Err(e) => {
                // Remember the real error; the transport only needs to know it must stop.
                let msg = e.to_string();
                failure = Some(e);
                Err(washi_net::NetError::Http(msg))
            }
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    let n = result?;
    progress(ScanProgress::Done { pages: n });
    Ok(n)
}

/// Like [`run_scan_streaming`], collecting the pages.
pub fn run_scan(
    printer: &Printer,
    req: &ScanRequest,
    confirm: Confirm,
    progress: &mut dyn FnMut(ScanProgress),
) -> Result<Vec<ScannedPage>> {
    let mut pages = Vec::new();
    run_scan_streaming(printer, req, confirm, progress, &mut |_, sp| {
        pages.push(sp);
        Ok(())
    })?;
    Ok(pages)
}

/// The JSON form of a [`ScanRequest`] used by the daemon (`StartScan`). Unknown keys are
/// errors. Every key is optional except `confirm`, which must be `true` to scan.
///
/// ```json
/// { "confirm": true, "size": "a4", "resolution": 300, "mode": "color", "type": "document" }
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanSpec {
    #[serde(default)]
    pub confirm: bool,
    pub size: Option<String>,
    pub resolution: Option<u32>,
    /// `color`, `gray` or `bw`.
    pub mode: Option<String>,
    /// `document` or `photo`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// Send the eSCL Intent (default true).
    pub intent: Option<bool>,
    /// For `size: "auto"`: crop the object on the host (default true).
    pub auto_crop: Option<bool>,
}

impl ScanSpec {
    pub fn from_json(json: &str) -> Result<ScanSpec> {
        serde_json::from_str(json)
            .map_err(|e| Error::Invalid(format!("invalid scan settings: {e}")))
    }

    pub fn to_request(&self) -> Result<ScanRequest> {
        let mut r = ScanRequest::default();
        if let Some(s) = &self.size {
            r.size = SizeChoice::parse(s)?;
        }
        if let Some(v) = self.resolution {
            r.resolution = v;
        }
        if let Some(m) = &self.mode {
            r.mode = OutputMode::parse(m).ok_or_else(|| {
                Error::Invalid(format!("mode must be color, gray or bw, got {m:?}"))
            })?;
        }
        if let Some(k) = &self.kind {
            r.kind = OriginalKind::parse(k).ok_or_else(|| {
                Error::Invalid(format!("type must be document or photo, got {k:?}"))
            })?;
        }
        r.use_intent = self.intent.unwrap_or(true);
        r.auto_crop = self.auto_crop.unwrap_or(true);
        r.validate()?;
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> ScannerCapabilities {
        ScannerCapabilities::parse(include_str!(
            "../../../washi-net/tests/fixtures/escl-capabilities-xk140.xml"
        ))
        .unwrap()
    }

    #[test]
    fn settings_follow_the_request() {
        let req = ScanRequest {
            size: SizeChoice::parse("a5").unwrap(),
            resolution: 600,
            mode: OutputMode::Grayscale,
            kind: OriginalKind::Photo,
            use_intent: true,
            auto_crop: true,
        };
        let s = req.settings(Some(&caps()));
        assert_eq!(s.color_mode, ColorMode::Grayscale8);
        assert_eq!((s.x_resolution, s.y_resolution), (600, 600));
        assert_eq!(s.intent, Some(Intent::Photo));
        let r = s.region.unwrap();
        assert_eq!((r.width, r.height), (1748, 2480));
        assert_eq!(s.format, "image/jpeg");
        assert!(s.validate(&caps()).is_ok());
    }

    #[test]
    fn black_and_white_is_a_grey_scan_and_auto_is_the_whole_platen() {
        let req = ScanRequest {
            mode: OutputMode::BlackWhite,
            size: SizeChoice::Auto,
            ..ScanRequest::default()
        };
        let s = req.settings(Some(&caps()));
        assert_eq!(s.color_mode, ColorMode::Grayscale8, "the XK140 has no BlackAndWhite1");
        let r = s.region.unwrap();
        assert_eq!((r.width, r.height), (2550, 3508));
        assert_eq!(ScanRequest::default().settings(None).color_mode, ColorMode::Rgb24);
        // No intent when asked not to, or when the scanner does not know it.
        assert_eq!(
            ScanRequest { use_intent: false, ..ScanRequest::default() }.settings(None).intent,
            None
        );
        let mut c = caps();
        c.sources[0].intents = vec![Intent::Photo];
        assert_eq!(ScanRequest::default().settings(Some(&c)).intent, None);
    }

    #[test]
    fn json_specs() {
        let s = ScanSpec::from_json(r#"{"confirm":true,"size":"letter","resolution":600,"mode":"gray","type":"photo","intent":false}"#).unwrap();
        assert!(s.confirm);
        let r = s.to_request().unwrap();
        assert_eq!(
            (r.size.label(), r.resolution, r.mode, r.kind, r.use_intent),
            ("Letter", 600, OutputMode::Grayscale, OriginalKind::Photo, false)
        );
        assert!(ScanSpec::from_json("{}").unwrap().to_request().is_ok());
        assert!(!ScanSpec::from_json("{}").unwrap().confirm);
        assert!(ScanSpec::from_json(r#"{"dpi":300}"#).is_err(), "typos are errors");
        assert!(ScanSpec::from_json(r#"{"resolution":250}"#).unwrap().to_request().is_err());
        assert!(ScanSpec::from_json(r#"{"mode":"sepia"}"#).unwrap().to_request().is_err());
        assert!(ScanSpec::from_json(r#"{"size":"a3"}"#).unwrap().to_request().is_err());
    }

    #[test]
    fn requests_are_validated() {
        assert!(ScanRequest::default().validate().is_ok());
        let e = ScanRequest { resolution: 250, ..ScanRequest::default() }
            .validate()
            .unwrap_err()
            .to_string();
        assert!(e.contains("250") && e.contains("1200"), "{e}");
        assert_eq!(OriginalKind::parse("Photo"), Some(OriginalKind::Photo));
        assert_eq!(OriginalKind::parse("slide"), None);
    }

    #[test]
    fn the_plan_shows_the_request_and_the_host_steps() {
        let p = Printer::new("printer.example");
        let plan = plan_scan(
            &p,
            &ScanRequest {
                size: SizeChoice::Auto,
                mode: OutputMode::BlackWhite,
                ..ScanRequest::default()
            },
        )
        .unwrap();
        assert!(plan.request.starts_with("POST http://printer.example/eSCL/ScanJobs"));
        assert!(plan.request.contains("<pwg:Width>2550</pwg:Width>"));
        assert_eq!(plan.host_steps.len(), 3);
        assert!(plan_scan(&p, &ScanRequest { resolution: 1, ..ScanRequest::default() }).is_err());
    }

    #[test]
    fn decoded_pages_carry_the_requested_colour_mode() {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            30,
            20,
            image::Rgb([200, 100, 50]),
        ));
        let jpeg = super::super::save::encode_jpeg(&img, 90, 300).unwrap();
        let req = ScanRequest { mode: OutputMode::BlackWhite, ..ScanRequest::default() };
        let sp = decode_page(&req, &jpeg).unwrap();
        assert_eq!(sp.jpeg, jpeg);
        assert_eq!(sp.page.edits.correction.color_mode, OutputMode::BlackWhite);
        assert_eq!(sp.page.dpi, 300);
        assert!(decode_page(&req, b"junk").is_err());
    }
}
