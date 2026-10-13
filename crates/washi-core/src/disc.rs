// SPDX-License-Identifier: GPL-2.0-or-later
//! Disc labels: model a printable disc, render a label image masked to the printable ring and
//! print it through the printer's disc tray.
//!
//! * The disc is 120 mm across; the printable ring runs from `inner` (17..45 mm, default 22)
//!   to `outer` (114..118 mm, default 116). Everything outside the ring is painted white, i.e.
//!   no ink reaches the tray or the hub.
//! * The label is rendered at 600 dpi on a 2834 x 2834 px canvas, the printable pixel area the
//!   printer itself reports for its disc size (`custom_canon_disc_120x120mm`).
//! * Printing: IPP `media-source` = `disc`, `media-type` = `com.canon-mtdisca` (printable disc,
//!   recommended) or `com.canon-mtdiscb`, `media-size` 120 x 120 mm (`om_square_120x120mm`),
//!   margins 0.1 mm: exactly the two entries of the printer's `media-col-database`.

mod ring;

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use image::{GrayImage, Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use washi_net::ipp::{Margins, MediaSpec, PrintColorMode, PrintJob, Quality};

use crate::Confirm;
use crate::error::{Error, Result};
use crate::print::paper::{DISC_MARGIN, DISC_PAPER};
use crate::print::{
    JobReport, PrintProgress, RunOptions, Spool, adopt_media, parse_quality, preflight, submit,
};
use crate::printer::Printer;
use crate::pwg::{ColorSpace, PageHeader, PwgWriter};
use crate::tools::run_capture;

pub use ring::{RingGlyph, RingText};

pub const DISC_DIAMETER_MM: f64 = 120.0;
pub const DPI: u32 = 600;
/// Printable pixels of the 120 mm disc size at 600 dpi (IVEC `printarea_border`).
pub const CANVAS_PX: u32 = 2834;

pub const INNER_RANGE_MM: (f64, f64) = (17.0, 45.0);
pub const OUTER_RANGE_MM: (f64, f64) = (114.0, 118.0);
pub const DEFAULT_INNER_MM: f64 = 22.0;
pub const DEFAULT_OUTER_MM: f64 = 116.0;

/// `a` = printable disc ("recommended"), `b` = other printable disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscMedia {
    A,
    B,
}

impl DiscMedia {
    pub fn keyword(self) -> &'static str {
        match self {
            DiscMedia::A => "com.canon-mtdisca",
            DiscMedia::B => "com.canon-mtdiscb",
        }
    }

    pub fn parse(s: &str) -> Result<DiscMedia> {
        match s.trim().to_ascii_lowercase().as_str() {
            "a" | "disca" | "recommended" => Ok(DiscMedia::A),
            "b" | "discb" | "other" => Ok(DiscMedia::B),
            other => Err(Error::Invalid(format!("disc media must be a or b, got {other:?}"))),
        }
    }
}

/// The printable ring.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DiscGeometry {
    pub inner_mm: f64,
    pub outer_mm: f64,
}

impl Default for DiscGeometry {
    fn default() -> Self {
        DiscGeometry { inner_mm: DEFAULT_INNER_MM, outer_mm: DEFAULT_OUTER_MM }
    }
}

impl DiscGeometry {
    pub fn validate(&self) -> Result<()> {
        let ok = |v: f64, (lo, hi): (f64, f64)| v.is_finite() && v >= lo && v <= hi;
        if !ok(self.inner_mm, INNER_RANGE_MM) {
            return Err(Error::Invalid(format!(
                "inner printable diameter must be {}..{} mm, got {}",
                INNER_RANGE_MM.0, INNER_RANGE_MM.1, self.inner_mm
            )));
        }
        if !ok(self.outer_mm, OUTER_RANGE_MM) {
            return Err(Error::Invalid(format!(
                "outer printable diameter must be {}..{} mm, got {}",
                OUTER_RANGE_MM.0, OUTER_RANGE_MM.1, self.outer_mm
            )));
        }
        Ok(())
    }
}

/// How the background picture is sized before `scale` is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundFit {
    /// Cover the whole printable circle (the picture's shorter side spans the outer diameter).
    Cover,
    /// The whole picture fits inside the printable circle.
    Contain,
}

impl BackgroundFit {
    pub fn parse(s: &str) -> Result<BackgroundFit> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cover" | "fill" => Ok(BackgroundFit::Cover),
            "contain" | "fit" => Ok(BackgroundFit::Contain),
            other => Err(Error::Invalid(format!("fit must be cover or contain, got {other:?}"))),
        }
    }
}

/// A picture behind the text, placed relative to the disc centre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Background {
    pub path: PathBuf,
    pub fit: BackgroundFit,
    /// 1.0 = the size `fit` gives.
    pub scale: f64,
    /// Shift of the picture's centre from the disc centre (right, down), in mm.
    pub offset_mm: (f64, f64),
    /// Clockwise rotation about the picture's centre.
    pub rotation_deg: f32,
}

impl Background {
    pub fn new(path: impl Into<PathBuf>) -> Background {
        Background {
            path: path.into(),
            fit: BackgroundFit::Cover,
            scale: 1.0,
            offset_mm: (0.0, 0.0),
            rotation_deg: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    pub fn parse(s: &str) -> Result<Align> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" => Ok(Align::Left),
            "center" | "centre" | "middle" => Ok(Align::Center),
            "right" => Ok(Align::Right),
            other => {
                Err(Error::Invalid(format!("align must be left, center or right, got {other:?}")))
            }
        }
    }
}

/// A text item. `x_mm` / `y_mm` are measured from the disc centre (right, down); the text
/// block (lines separated by `\n`) is centred vertically on that point and aligned
/// horizontally to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextItem {
    pub text: String,
    pub x_mm: f64,
    pub y_mm: f64,
    pub size_pt: f64,
    pub color: [u8; 3],
    /// Clockwise rotation about (x, y).
    pub rotation_deg: f32,
    pub align: Align,
    /// A font file; `None` picks one that has the glyphs (fontconfig, then common locations).
    pub font: Option<PathBuf>,
    /// "Follow the ring": the letters are placed on a circle about the disc centre instead of in
    /// a straight line. Then `x_mm`, `y_mm` and `rotation_deg` are not used, and the text is one
    /// line (line breaks become spaces).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring: Option<RingText>,
}

impl TextItem {
    pub fn new(text: impl Into<String>) -> TextItem {
        TextItem {
            text: text.into(),
            x_mm: 0.0,
            y_mm: 32.0,
            size_pt: 18.0,
            color: [0, 0, 0],
            rotation_deg: 0.0,
            align: Align::Center,
            font: None,
            ring: None,
        }
    }
}

/// Everything on a label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscLabel {
    pub geometry: DiscGeometry,
    pub media: DiscMedia,
    pub background_color: [u8; 3],
    pub background: Option<Background>,
    pub texts: Vec<TextItem>,
}

impl Default for DiscLabel {
    fn default() -> Self {
        DiscLabel {
            geometry: DiscGeometry::default(),
            media: DiscMedia::A,
            background_color: [255, 255, 255],
            background: None,
            texts: Vec::new(),
        }
    }
}

pub fn parse_color(s: &str) -> Result<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    let bad = || Error::Invalid(format!("{s:?} is not a colour (use #RRGGBB)"));
    if h.len() != 6 || !h.is_ascii() {
        return Err(bad());
    }
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| bad());
    Ok([v(0)?, v(2)?, v(4)?])
}

// ---------------------------------------------------------------------------------------------
// Fonts and text
// ---------------------------------------------------------------------------------------------

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF))
}

/// Ask fontconfig for a font file (and face index) that covers `sample`.
fn fc_match(pattern: &str) -> Option<(PathBuf, u32)> {
    let mut c = std::process::Command::new("fc-match");
    c.args(["-f", "%{file}|%{index}\n", pattern]);
    let o = run_capture(c, std::time::Duration::from_secs(10), "fc-match").ok()?;
    let line = String::from_utf8_lossy(&o.stdout).lines().next()?.to_string();
    let (file, index) = line.split_once('|')?;
    let path = PathBuf::from(file);
    path.exists().then(|| (path, index.trim().parse().unwrap_or(0)))
}

/// The font file fontconfig picks for a family name such as `Noto Sans` or `DejaVu Serif:bold`
/// (what a GUI's font picker returns). `None` when fontconfig is missing or the name is not a
/// plain pattern. The best match is returned even when the family itself is not installed.
pub fn find_font_family(family: &str) -> Option<PathBuf> {
    let f = family.trim();
    let plain = !f.is_empty()
        && f.len() <= 80
        && !f.starts_with('-')
        && f.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | ',' | ':' | '=' | '.'));
    if !plain {
        return None;
    }
    fc_match(f).map(|(p, _)| p)
}

/// A font file for `sample`: fontconfig first, then a few well-known paths.
pub fn find_font(sample: &str) -> Option<(PathBuf, u32)> {
    if let Some(p) = std::env::var_os("WASHI_FONT").map(PathBuf::from).filter(|p| p.exists()) {
        return Some((p, 0));
    }
    let pattern = if has_cjk(sample) { "sans:lang=ja" } else { "sans" };
    if let Some(f) = fc_match(pattern) {
        return Some(f);
    }
    [
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
    .map(|p| (p, 0))
}

struct LoadedFont {
    data: Vec<u8>,
    index: u32,
}

impl LoadedFont {
    fn load(item: &TextItem) -> Result<LoadedFont> {
        let (path, index) = match &item.font {
            Some(p) => (p.clone(), 0),
            None => find_font(&item.text).ok_or_else(|| {
                Error::Invalid("no font found for the label text; pass a font file".into())
            })?,
        };
        let data = std::fs::read(&path)
            .map_err(|e| Error::io(format!("cannot read the font {}", path.display()), e))?;
        FontRef::try_from_slice_and_index(&data, index)
            .map_err(|_| Error::Invalid(format!("{} is not a usable font file", path.display())))?;
        Ok(LoadedFont { data, index })
    }

    fn font(&self) -> FontRef<'_> {
        // Checked in `load`.
        FontRef::try_from_slice_and_index(&self.data, self.index).expect("font was validated")
    }
}

/// The scale for a font whose em is `px_size` pixels.
fn px_scale(font: &FontRef<'_>, px_size: f32) -> PxScale {
    font.pt_to_px_scale(px_size).unwrap_or(PxScale::from(px_size))
}

/// Coverage mask of one line of text, and the baseline-to-top ascent in pixels.
fn render_line(font: &FontRef<'_>, px_size: f32, line: &str) -> GrayImage {
    let scale = px_scale(font, px_size);
    let scaled = font.as_scaled(scale);
    let ascent = scaled.ascent();
    let height = (scaled.ascent() - scaled.descent()).ceil().max(1.0);
    let mut x = 0.0f32;
    let mut prev: Option<GlyphId> = None;
    let mut glyphs = Vec::new();
    for ch in line.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(p) = prev {
            x += scaled.kern(p, id);
        }
        glyphs.push(id.with_scale_and_position(scale, point(x + 1.0, ascent + 1.0)));
        x += scaled.h_advance(id);
        prev = Some(id);
    }
    let (w, h) = ((x.ceil() as u32 + 3).max(1), height as u32 + 3);
    let mut cov = GrayImage::new(w, h);
    for g in glyphs {
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, c| {
                let (px, py) = (b.min.x as i64 + i64::from(gx), b.min.y as i64 + i64::from(gy));
                if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                    let cur = f32::from(cov.get_pixel(px as u32, py as u32).0[0]);
                    cov.put_pixel(
                        px as u32,
                        py as u32,
                        image::Luma([(cur + c * 255.0).min(255.0) as u8]),
                    );
                }
            });
        }
    }
    cov
}

/// A text that follows the ring: every letter is rendered by itself and set on the circle where
/// [`RingText::layout`] puts it.
fn draw_ring_text(
    img: &mut RgbImage,
    font: &FontRef<'_>,
    t: &TextItem,
    ring: &RingText,
    size: f32,
    centre: f64,
    px_per_mm: f64,
) {
    let scaled = font.as_scaled(px_scale(font, size));
    let chars: Vec<char> = t.text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let advances_px: Vec<f64> =
        chars.iter().map(|c| f64::from(scaled.h_advance(scaled.glyph_id(*c)))).collect();
    let advances_mm: Vec<f64> = advances_px.iter().map(|a| a / px_per_mm).collect();
    let ascent = f64::from(scaled.ascent());
    for ((ch, adv), g) in chars.iter().zip(&advances_px).zip(ring.layout(&advances_mm, t.align)) {
        if ch.is_whitespace() {
            continue;
        }
        let cov = render_line(font, size, &ch.to_string());
        // `render_line` starts the letter 1 px in and puts its baseline `ascent + 1` px down; the
        // point to pin on the ring is the middle of the letter's advance, on the baseline.
        let layer_anchor = (1.0 + adv / 2.0, ascent + 1.0);
        let anchor = (centre + g.x_mm * px_per_mm, centre + g.y_mm * px_per_mm);
        composite_layer(img, &cov, anchor, layer_anchor, g.rotation_deg as f32, t.color);
    }
}

/// Composite a coverage mask in `color` onto `img`; the mask is rotated by `angle` degrees
/// clockwise about `anchor_layer`, which lands on `anchor` of the canvas.
fn composite_layer(
    img: &mut RgbImage,
    cov: &GrayImage,
    anchor: (f64, f64),
    anchor_layer: (f64, f64),
    angle: f32,
    color: [u8; 3],
) {
    let (lw, lh) = (f64::from(cov.width()), f64::from(cov.height()));
    let (s, c) = f64::from(angle).to_radians().sin_cos();
    // Bounding box of the rotated layer on the canvas.
    let corners = [(0.0, 0.0), (lw, 0.0), (lw, lh), (0.0, lh)].map(|(x, y): (f64, f64)| {
        let (dx, dy) = (x - anchor_layer.0, y - anchor_layer.1);
        (anchor.0 + dx * c - dy * s, anchor.1 + dx * s + dy * c)
    });
    let (min_x, max_x) =
        corners.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (min_y, max_y) =
        corners.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let (x0, x1) =
        ((min_x.floor().max(0.0)) as u32, (max_x.ceil().min(f64::from(img.width()))) as u32);
    let (y0, y1) =
        ((min_y.floor().max(0.0)) as u32, (max_y.ceil().min(f64::from(img.height()))) as u32);
    for y in y0..y1 {
        for x in x0..x1 {
            let (dx, dy) = (f64::from(x) + 0.5 - anchor.0, f64::from(y) + 0.5 - anchor.1);
            // Inverse rotation back into the layer.
            let lx = anchor_layer.0 + dx * c + dy * s;
            let ly = anchor_layer.1 - dx * s + dy * c;
            if lx < 0.0 || ly < 0.0 || lx >= lw || ly >= lh {
                continue;
            }
            let cv = sample_cov(cov, lx - 0.5, ly - 0.5);
            if cv <= 0.0 {
                continue;
            }
            let p = img.get_pixel_mut(x, y);
            for (px, ink) in p.0.iter_mut().zip(color) {
                *px = (f32::from(*px) * (1.0 - cv) + f32::from(ink) * cv).round() as u8;
            }
        }
    }
}

fn sample_cov(cov: &GrayImage, x: f64, y: f64) -> f32 {
    let (w, h) = cov.dimensions();
    let (xf, yf) = (x.clamp(0.0, f64::from(w - 1)), y.clamp(0.0, f64::from(h - 1)));
    let (x0, y0) = (xf.floor() as u32, yf.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = ((xf - f64::from(x0)) as f32, (yf - f64::from(y0)) as f32);
    let g = |px: u32, py: u32| f32::from(cov.get_pixel(px, py).0[0]) / 255.0;
    let top = g(x0, y0) * (1.0 - fx) + g(x1, y0) * fx;
    let bot = g(x0, y1) * (1.0 - fx) + g(x1, y1) * fx;
    top * (1.0 - fy) + bot * fy
}

// ---------------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------------

fn sample_bilinear(img: &RgbImage, x: f64, y: f64) -> Option<[u8; 3]> {
    let (w, h) = img.dimensions();
    if x < -0.5 || y < -0.5 || x > f64::from(w) - 0.5 || y > f64::from(h) - 0.5 {
        return None;
    }
    let xf = x.clamp(0.0, f64::from(w - 1));
    let yf = y.clamp(0.0, f64::from(h - 1));
    let (x0, y0) = (xf.floor() as u32, yf.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (xf - f64::from(x0), yf - f64::from(y0));
    let p = |a: u32, b: u32| img.get_pixel(a, b).0;
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    let mut out = [0u8; 3];
    for i in 0..3 {
        let top = f64::from(a[i]) * (1.0 - fx) + f64::from(b[i]) * fx;
        let bot = f64::from(c[i]) * (1.0 - fx) + f64::from(d[i]) * fx;
        out[i] = (top * (1.0 - fy) + bot * fy).round() as u8;
    }
    Some(out)
}

impl DiscLabel {
    /// Render at the printer's resolution ([`CANVAS_PX`] square). Loads the background picture
    /// from its path.
    pub fn render(&self) -> Result<RgbImage> {
        self.render_sized(CANVAS_PX)
    }

    /// Render with a canvas of `px` x `px` (previews: a smaller canvas draws the same label).
    pub fn render_sized(&self, px: u32) -> Result<RgbImage> {
        let bg = match &self.background {
            Some(b) => Some(crate::print::photo::load_photo(&b.path)?),
            None => None,
        };
        self.render_with(bg.as_ref(), px)
    }

    /// Render with an already loaded background picture (a GUI keeps it in memory).
    pub fn render_with(&self, background: Option<&RgbImage>, px: u32) -> Result<RgbImage> {
        self.geometry.validate()?;
        let px = px.clamp(64, 2 * CANVAS_PX);
        let k = f64::from(px) / f64::from(CANVAS_PX); // canvas scale relative to 600 dpi
        let px_per_mm = f64::from(DPI) / 25.4 * k;
        let centre = f64::from(px) / 2.0;
        let (r_in, r_out) =
            (self.geometry.inner_mm / 2.0 * px_per_mm, self.geometry.outer_mm / 2.0 * px_per_mm);
        let base = Rgb(self.background_color);
        let mut img = RgbImage::from_pixel(px, px, base);

        // Background picture: inverse-map every pixel of the ring into the picture.
        if let (Some(src), Some(spec)) = (background, &self.background) {
            let (sw, sh) = (f64::from(src.width()), f64::from(src.height()));
            let circle = self.geometry.outer_mm * px_per_mm; // outer diameter in px
            let fit = match spec.fit {
                BackgroundFit::Cover => circle / sw.min(sh),
                BackgroundFit::Contain => circle / (sw * sw + sh * sh).sqrt(),
            };
            let scale = (fit * spec.scale.max(0.01)).max(1e-6); // source px -> canvas px
            let (sin, cos) = f64::from(spec.rotation_deg).to_radians().sin_cos();
            let (cx, cy) =
                (centre + spec.offset_mm.0 * px_per_mm, centre + spec.offset_mm.1 * px_per_mm);
            let lo = (centre - r_out - 1.0).max(0.0) as u32;
            let hi = ((centre + r_out + 1.0).ceil() as u32).min(px);
            for y in lo..hi {
                for x in lo..hi {
                    let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
                    // Undo the rotation, then the scale; the picture's centre is (sw/2, sh/2).
                    let ux = (dx * cos + dy * sin) / scale + sw / 2.0 - 0.5;
                    let uy = (-dx * sin + dy * cos) / scale + sh / 2.0 - 0.5;
                    if let Some(c) = sample_bilinear(src, ux, uy) {
                        img.put_pixel(x, y, Rgb(c));
                    }
                }
            }
        }

        // Text.
        for t in &self.texts {
            if t.text.trim().is_empty() {
                continue;
            }
            let lf = LoadedFont::load(t)?;
            let font = lf.font();
            // `pt_to_px_scale` assumes 72 ppi; scale to the canvas resolution.
            let size = (t.size_pt * f64::from(DPI) / 72.0 * k) as f32;
            if let Some(ring) = &t.ring {
                ring.validate()?;
                draw_ring_text(&mut img, &font, t, ring, size, centre, px_per_mm);
                continue;
            }
            let lines: Vec<&str> = t.text.lines().collect();
            let line_h = f64::from(size) * 1.25;
            let (ts, tc) = f64::from(t.rotation_deg).to_radians().sin_cos();
            for (i, line) in lines.iter().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let cov = render_line(&font, size, line);
                let off = (i as f64 - (lines.len() as f64 - 1.0) / 2.0) * line_h;
                // The block is rotated about (x, y); this line sits `off` px below it.
                let (ax, ay) = (centre + t.x_mm * px_per_mm, centre + t.y_mm * px_per_mm);
                let anchor = (ax - off * ts, ay + off * tc);
                let lw = f64::from(cov.width());
                let layer_anchor = (
                    match t.align {
                        Align::Left => 0.0,
                        Align::Center => lw / 2.0,
                        Align::Right => lw,
                    },
                    f64::from(cov.height()) / 2.0,
                );
                composite_layer(&mut img, &cov, anchor, layer_anchor, t.rotation_deg, t.color);
            }
        }

        // The ring: anti-aliased edges, white everywhere else (no ink on the tray or the hub).
        let white = [255u8, 255, 255];
        for y in 0..px {
            for x in 0..px {
                let d = ((f64::from(x) + 0.5 - centre).powi(2)
                    + (f64::from(y) + 0.5 - centre).powi(2))
                .sqrt();
                let cov = (r_out - d + 0.5).clamp(0.0, 1.0).min((d - r_in + 0.5).clamp(0.0, 1.0));
                if cov >= 1.0 {
                    continue;
                }
                let p = img.get_pixel_mut(x, y);
                for (px, w) in p.0.iter_mut().zip(white) {
                    *px = (f64::from(*px) * cov + f64::from(w) * (1.0 - cov)).round() as u8;
                }
            }
        }
        Ok(img)
    }
}

// ---------------------------------------------------------------------------------------------
// JSON form (the daemon's PrintDisc)
// ---------------------------------------------------------------------------------------------

/// The JSON form of a label job. Unknown keys are errors.
///
/// ```json
/// { "confirm": true, "image": "/path/to/art.png", "fit": "cover",
///   "texts": [{"text": "My mix", "y_mm": 32, "size_pt": 20}], "inner_mm": 22, "media": "a" }
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscSpec {
    #[serde(default)]
    pub confirm: bool,
    pub image: Option<PathBuf>,
    pub fit: Option<String>,
    pub scale: Option<f64>,
    pub offset_mm: Option<(f64, f64)>,
    pub rotation: Option<f32>,
    pub background_color: Option<String>,
    #[serde(default)]
    pub texts: Vec<TextSpec>,
    pub inner_mm: Option<f64>,
    pub outer_mm: Option<f64>,
    pub media: Option<String>,
    pub quality: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSpec {
    pub text: String,
    pub x_mm: Option<f64>,
    pub y_mm: Option<f64>,
    pub size_pt: Option<f64>,
    pub color: Option<String>,
    pub rotation: Option<f32>,
    pub align: Option<String>,
    pub font: Option<PathBuf>,
    /// Follow the ring (see [`RingText`]); the position and rotation above are then unused.
    pub ring: Option<RingSpec>,
}

/// The JSON form of [`RingText`]; every key is optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RingSpec {
    pub radius_mm: Option<f64>,
    /// Degrees clockwise from the top of the disc.
    pub start_deg: Option<f64>,
    pub clockwise: Option<bool>,
    pub letter_spacing_pt: Option<f64>,
}

impl RingSpec {
    pub fn to_ring(&self) -> Result<RingText> {
        let d = RingText::default();
        let ring = RingText {
            radius_mm: self.radius_mm.unwrap_or(d.radius_mm),
            start_deg: self.start_deg.unwrap_or(d.start_deg),
            clockwise: self.clockwise.unwrap_or(d.clockwise),
            letter_spacing_pt: self.letter_spacing_pt.unwrap_or(d.letter_spacing_pt),
        };
        ring.validate()?;
        Ok(ring)
    }
}

impl DiscSpec {
    pub fn from_json(json: &str) -> Result<DiscSpec> {
        serde_json::from_str(json).map_err(|e| Error::Invalid(format!("invalid disc label: {e}")))
    }

    pub fn to_label(&self) -> Result<(DiscLabel, Quality)> {
        let mut l = DiscLabel::default();
        if let Some(v) = self.inner_mm {
            l.geometry.inner_mm = v;
        }
        if let Some(v) = self.outer_mm {
            l.geometry.outer_mm = v;
        }
        l.geometry.validate()?;
        if let Some(m) = &self.media {
            l.media = DiscMedia::parse(m)?;
        }
        if let Some(c) = &self.background_color {
            l.background_color = parse_color(c)?;
        }
        if let Some(p) = &self.image {
            let mut b = Background::new(p);
            if let Some(f) = &self.fit {
                b.fit = BackgroundFit::parse(f)?;
            }
            b.scale = self.scale.unwrap_or(1.0);
            b.offset_mm = self.offset_mm.unwrap_or((0.0, 0.0));
            b.rotation_deg = self.rotation.unwrap_or(0.0);
            l.background = Some(b);
        }
        for t in &self.texts {
            let mut item = TextItem::new(&t.text);
            item.x_mm = t.x_mm.unwrap_or(item.x_mm);
            item.y_mm = t.y_mm.unwrap_or(item.y_mm);
            item.size_pt = t.size_pt.unwrap_or(item.size_pt).clamp(4.0, 200.0);
            if let Some(c) = &t.color {
                item.color = parse_color(c)?;
            }
            item.rotation_deg = t.rotation.unwrap_or(0.0);
            if let Some(a) = &t.align {
                item.align = Align::parse(a)?;
            }
            item.font = t.font.clone();
            item.ring = t.ring.as_ref().map(RingSpec::to_ring).transpose()?;
            l.texts.push(item);
        }
        let quality = match &self.quality {
            Some(q) => parse_quality(q)?,
            None => Quality::Normal,
        };
        Ok((l, quality))
    }
}

// ---------------------------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------------------------

/// The IPP job for a disc label (media from the printer's database).
pub fn disc_job(label: &DiscLabel, quality: Quality) -> PrintJob {
    let mut job = PrintJob::new("disc label", "image/pwg-raster");
    job.quality = quality;
    job.color = PrintColorMode::Color;
    job.media = Some(MediaSpec {
        width: DISC_PAPER.width,
        height: DISC_PAPER.height,
        media_type: Some(label.media.keyword().to_string()),
        source: Some("disc".to_string()),
        margins: Some(Margins {
            top: DISC_MARGIN,
            bottom: DISC_MARGIN,
            left: DISC_MARGIN,
            right: DISC_MARGIN,
        }),
    });
    job
}

/// What printing a label would do, without doing it.
#[derive(Debug, Clone)]
pub struct DiscPlan {
    pub job: PrintJob,
    pub request: String,
    pub notes: Vec<String>,
}

pub fn plan_disc(printer: &Printer, label: &DiscLabel, quality: Quality) -> Result<DiscPlan> {
    label.geometry.validate()?;
    let job = disc_job(label, quality);
    let request = printer.ipp().print_job_request(&job).describe();
    let notes = vec![
        format!(
            "ring {} mm to {} mm of the 120 mm disc, rendered at 600 dpi ({} x {} px), outside the ring white",
            label.geometry.inner_mm, label.geometry.outer_mm, CANVAS_PX, CANVAS_PX
        ),
        "put the disc in the disc tray and insert the tray before printing".to_string(),
        "media size 120 x 120 mm, source `disc`: the entry the printer lists in its media-col-database".to_string(),
    ];
    Ok(DiscPlan { job, request, notes })
}

/// Render the label to a PWG raster spool file.
pub fn spool_label(
    label: &DiscLabel,
    job: &PrintJob,
    background: Option<&RgbImage>,
) -> Result<Spool> {
    let img = label.render_with(background, CANVAS_PX)?;
    let file = tempfile::Builder::new()
        .prefix("washi-spool-")
        .tempfile_in(std::env::temp_dir())
        .map_err(|e| Error::io("cannot create the spool file", e))?;
    let mut header = PageHeader::new(CANVAS_PX, CANVAS_PX, DPI, ColorSpace::SRgb);
    header.media_type = label.media.keyword().to_string();
    header.page_size_name = DISC_PAPER.ipp_name.to_string();
    header.quality = job.quality.enum_value() as u32;
    header.total_pages = 1;
    let out =
        BufWriter::new(file.reopen().map_err(|e| Error::io("cannot open the spool file", e))?);
    let mut w = PwgWriter::new(out);
    w.write_page(&header, img.as_raw()).map_err(|e| Error::io("cannot write the spool file", e))?;
    w.finish()
        .and_then(|mut o| std::io::Write::flush(&mut o))
        .map_err(|e| Error::io("cannot write the spool file", e))?;
    Ok(Spool::File(file))
}

/// Print a label on the disc tray. **Prints**, so it needs a [`Confirm`]. The printer's
/// `media-col-database` must list the disc entry for the chosen media type.
pub fn run_disc(
    printer: &Printer,
    label: &DiscLabel,
    quality: Quality,
    opts: &RunOptions,
    confirm: Confirm,
    progress: &mut dyn FnMut(PrintProgress),
) -> Result<JobReport> {
    label.geometry.validate()?;
    let ipp = printer.ipp();
    progress(PrintProgress::Preflight);
    let pf = preflight(&ipp, "image/pwg-raster", opts.check_media)?;
    let mut job = disc_job(label, quality);
    if opts.check_media && !pf.media.is_empty() {
        if pf.media.disc().is_empty() {
            return Err(Error::Invalid(
                "the printer's media database has no disc entry; is a disc tray supported?".into(),
            ));
        }
        adopt_media(&mut job, &pf.media, false)?;
    }
    if !pf.raster_types.is_empty() && !pf.raster_types.iter().any(|t| t == "srgb_8") {
        return Err(Error::Invalid("the printer does not take srgb_8 raster".into()));
    }
    progress(PrintProgress::Preparing { job: 0, jobs: 1, side: 0, sides: 1 });
    let mut spool = spool_label(label, &job, None)?;
    progress(PrintProgress::Preparing { job: 0, jobs: 1, side: 1, sides: 1 });
    submit(&ipp, &job, &mut spool, "disc label", (0, 1), opts, confirm, progress)
}

/// Does this path exist and look like something we can load as the background?
pub fn check_image(path: &Path) -> Result<()> {
    if File::open(path).is_err() {
        return Err(Error::Invalid(format!("cannot open the image {}", path.display())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PX: u32 = 600; // preview canvas for the tests

    fn solid(w: u32, h: u32, c: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb(c))
    }

    fn at(img: &RgbImage, dx_mm: f64, dy_mm: f64) -> [u8; 3] {
        // A point `dx_mm`, `dy_mm` from the disc centre on a PX-wide canvas.
        let ppm = f64::from(DPI) / 25.4 * f64::from(PX) / f64::from(CANVAS_PX);
        let c = f64::from(PX) / 2.0;
        img.get_pixel((c + dx_mm * ppm) as u32, (c + dy_mm * ppm) as u32).0
    }

    #[test]
    fn geometry_limits() {
        assert!(DiscGeometry::default().validate().is_ok());
        for (i, o) in [(17.0, 114.0), (45.0, 118.0), (22.0, 116.0)] {
            assert!(DiscGeometry { inner_mm: i, outer_mm: o }.validate().is_ok());
        }
        for (i, o) in
            [(16.9, 116.0), (45.1, 116.0), (22.0, 113.9), (22.0, 118.1), (f64::NAN, 116.0)]
        {
            assert!(DiscGeometry { inner_mm: i, outer_mm: o }.validate().is_err(), "{i} {o}");
        }
        // 120 mm is 2834.6 px at 600 dpi; the printer reports 2834 printable pixels.
        assert_eq!((DISC_DIAMETER_MM / 25.4 * f64::from(DPI)) as u32, CANVAS_PX);
    }

    #[test]
    fn the_ring_masks_everything_else() {
        let label = DiscLabel { background_color: [200, 30, 30], ..DiscLabel::default() };
        let img = label.render_sized(PX).unwrap();
        assert_eq!(img.dimensions(), (PX, PX));
        assert_eq!(at(&img, 0.0, 0.0), [255, 255, 255], "the hub hole is white");
        assert_eq!(at(&img, 8.0, 0.0), [255, 255, 255], "inside the inner diameter (22 mm)");
        assert_eq!(at(&img, 20.0, 0.0), [200, 30, 30], "inside the ring");
        assert_eq!(at(&img, 0.0, -50.0), [200, 30, 30]);
        assert_eq!(at(&img, 59.0, 0.0), [255, 255, 255], "outside 116 mm");
        assert_eq!(img.get_pixel(2, 2).0, [255, 255, 255], "the corners are white");
        // Custom diameters move the edges.
        let wide = DiscLabel {
            geometry: DiscGeometry { inner_mm: 40.0, outer_mm: 114.0 },
            background_color: [0, 0, 255],
            ..DiscLabel::default()
        }
        .render_sized(PX)
        .unwrap();
        assert_eq!(at(&wide, 15.0, 0.0), [255, 255, 255]);
        assert_eq!(at(&wide, 25.0, 0.0), [0, 0, 255]);
        // Edge pixels are anti-aliased: some pixel between the colours exists.
        assert!(img.pixels().any(|p| p.0[0] > 200 && p.0[0] < 255 && p.0[1] > 30 && p.0[1] < 255));
    }

    #[test]
    fn background_picture_covers_and_moves() {
        // Left half red, right half blue.
        let mut pic = solid(200, 100, [255, 0, 0]);
        for y in 0..100 {
            for x in 100..200 {
                pic.put_pixel(x, y, Rgb([0, 0, 255]));
            }
        }
        let mut label =
            DiscLabel { background: Some(Background::new("unused.png")), ..DiscLabel::default() };
        let cover = label.render_with(Some(&pic), PX).unwrap();
        assert_eq!(at(&cover, -30.0, 0.0), [255, 0, 0]);
        assert_eq!(at(&cover, 30.0, 0.0), [0, 0, 255]);
        assert_eq!(at(&cover, 0.0, -40.0)[1], 0, "covers the ring above the centre too");
        // Contain leaves the corners of the circle uncovered (the picture is 2:1).
        label.background.as_mut().unwrap().fit = BackgroundFit::Contain;
        let contain = label.render_with(Some(&pic), PX).unwrap();
        assert_eq!(
            at(&contain, 0.0, -40.0),
            [255, 255, 255],
            "above the picture: background colour"
        );
        assert_eq!(at(&contain, -30.0, 0.0), [255, 0, 0]);
        // Shifting by 40 mm to the right moves the red/blue border with it.
        label.background.as_mut().unwrap().fit = BackgroundFit::Cover;
        label.background.as_mut().unwrap().offset_mm = (40.0, 0.0);
        let moved = label.render_with(Some(&pic), PX).unwrap();
        assert_eq!(at(&moved, 30.0, 0.0), [255, 0, 0]);
        // Rotating by 90 degrees turns the left/right halves into top/bottom.
        label.background.as_mut().unwrap().offset_mm = (0.0, 0.0);
        label.background.as_mut().unwrap().rotation_deg = 90.0;
        let rot = label.render_with(Some(&pic), PX).unwrap();
        assert_eq!(at(&rot, 0.0, -30.0), [255, 0, 0]);
        assert_eq!(at(&rot, 0.0, 30.0), [0, 0, 255]);
        // Zooming in shows less of the picture.
        label.background.as_mut().unwrap().rotation_deg = 0.0;
        label.background.as_mut().unwrap().scale = 4.0;
        let zoom = label.render_with(Some(&pic), PX).unwrap();
        assert_eq!(at(&zoom, -30.0, 0.0), [255, 0, 0]);
    }

    #[test]
    fn text_is_drawn_inside_the_ring_and_can_rotate() {
        let Some((font, _)) = find_font("Washi") else {
            eprintln!("no font on this system; skipping the text test");
            return;
        };
        let mut t = TextItem::new("WASHI\nDISC");
        t.size_pt = 30.0;
        t.y_mm = 32.0;
        t.color = [255, 0, 0];
        let label = DiscLabel { texts: vec![t.clone()], ..DiscLabel::default() };
        let img = label.render_sized(PX).unwrap();
        let reds = |im: &RgbImage| {
            im.pixels().filter(|p| p.0[0] > 200 && p.0[1] < 80 && p.0[2] < 80).count()
        };
        let n = reds(&img);
        assert!(n > 200, "text pixels: {n}");
        // The text sits below the centre.
        let ys: Vec<u32> = img
            .enumerate_pixels()
            .filter(|(_, _, p)| p.0[0] > 200 && p.0[1] < 80)
            .map(|(_, y, _)| y)
            .collect();
        assert!(ys.iter().all(|y| *y > PX / 2), "all text below the centre line");
        // Rotated by 90 degrees the block becomes tall.
        t.rotation_deg = 90.0;
        t.x_mm = 35.0;
        t.y_mm = 0.0;
        t.size_pt = 14.0;
        let rot =
            DiscLabel { texts: vec![t.clone()], ..DiscLabel::default() }.render_sized(PX).unwrap();
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (u32::MAX, 0, u32::MAX, 0);
        for (x, y, p) in rot.enumerate_pixels() {
            if p.0[0] > 200 && p.0[1] < 140 {
                (min_x, max_x, min_y, max_y) =
                    (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y));
            }
        }
        assert!(
            max_y - min_y > max_x - min_x,
            "tall after a quarter turn: {}x{}",
            max_x - min_x,
            max_y - min_y
        );
        // An explicit font file works the same; a bad one is an error.
        t.font = Some(font);
        t.rotation_deg = 0.0;
        assert!(
            DiscLabel { texts: vec![t.clone()], ..DiscLabel::default() }.render_sized(PX).is_ok()
        );
        t.font = Some(PathBuf::from("/nonexistent/font.ttf"));
        assert!(DiscLabel { texts: vec![t], ..DiscLabel::default() }.render_sized(PX).is_err());
    }

    /// The red pixels of a render as (angle clockwise from the top in degrees, radius in mm).
    fn red_polar(img: &RgbImage) -> Vec<(f64, f64)> {
        let ppm = f64::from(DPI) / 25.4 * f64::from(img.width()) / f64::from(CANVAS_PX);
        let c = f64::from(img.width()) / 2.0;
        img.enumerate_pixels()
            .filter(|(_, _, p)| p.0[0] > 200 && p.0[1] < 90 && p.0[2] < 90)
            .map(|(x, y, _)| {
                let (dx, dy) = (f64::from(x) + 0.5 - c, f64::from(y) + 0.5 - c);
                (dx.atan2(-dy).to_degrees(), dx.hypot(dy) / ppm)
            })
            .collect()
    }

    fn ring_label(text: &str, ring: RingText, align: Align) -> DiscLabel {
        let mut t = TextItem::new(text);
        t.size_pt = 20.0;
        t.color = [255, 0, 0];
        t.align = align;
        t.ring = Some(ring);
        DiscLabel { texts: vec![t], ..DiscLabel::default() }
    }

    #[test]
    fn text_that_follows_the_ring_stands_on_the_circle() {
        if find_font("Washi").is_none() {
            eprintln!("no font on this system; skipping the ring text test");
            return;
        }
        let r = 46.0;
        let cap_mm = 20.0 * 25.4 / 72.0; // the em of the text, an upper bound of a letter's height
        // Clockwise, centred on the top: letters stand on the circle with their tops outwards.
        let ring =
            RingText { radius_mm: r, start_deg: 0.0, clockwise: true, letter_spacing_pt: 2.0 };
        let img = ring_label("WASHI DISC", ring, Align::Center).render_sized(PX).unwrap();
        let px = red_polar(&img);
        assert!(px.len() > 300, "ink: {}", px.len());
        let (rmin, rmax) =
            px.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
        assert!(rmin > r - 0.2 * cap_mm && rmax < r + 1.05 * cap_mm, "radii {rmin:.1}..{rmax:.1}");
        let amin = px.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        let amax = px.iter().map(|p| p.0).fold(f64::MIN, f64::max);
        assert!(
            amin < -5.0 && amax > 5.0,
            "spreads over both sides of the top: {amin:.1}..{amax:.1}"
        );
        assert!((amin + amax).abs() < 8.0, "centred on the top: {amin:.1}..{amax:.1}");
        assert!(px.iter().all(|p| p.0.abs() < 90.0), "nothing at the bottom half");

        // Counter-clockwise, centred on the bottom: tops point at the centre, so the letters
        // lie between the baseline and the centre.
        let ring =
            RingText { radius_mm: r, start_deg: 180.0, clockwise: false, letter_spacing_pt: 2.0 };
        let img = ring_label("WASHI DISC", ring, Align::Center).render_sized(PX).unwrap();
        let px = red_polar(&img);
        assert!(px.len() > 300);
        let (rmin, rmax) =
            px.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
        assert!(rmax < r + 0.2 * cap_mm && rmin > r - 1.05 * cap_mm, "radii {rmin:.1}..{rmax:.1}");
        assert!(px.iter().all(|p| p.0.abs() > 90.0), "nothing in the top half");
    }

    #[test]
    fn the_start_angle_and_alignment_place_the_text_on_the_ring() {
        if find_font("Washi").is_none() {
            return;
        }
        let at_start = |align: Align, clockwise: bool| {
            let ring =
                RingText { radius_mm: 46.0, start_deg: 90.0, clockwise, letter_spacing_pt: 0.0 };
            let img = ring_label("WASHI", ring, align).render_sized(PX).unwrap();
            let a: Vec<f64> = red_polar(&img).iter().map(|p| p.0).collect();
            (a.iter().cloned().fold(f64::MAX, f64::min), a.iter().cloned().fold(f64::MIN, f64::max))
        };
        // Left, clockwise: the text begins at 3 o'clock and runs on towards the bottom.
        let (lo, hi) = at_start(Align::Left, true);
        assert!(lo > 85.0 && hi > 100.0, "{lo:.1}..{hi:.1}");
        // Right, clockwise: the text ends at 3 o'clock.
        let (lo, hi) = at_start(Align::Right, true);
        assert!(hi < 95.0 && lo < 75.0, "{lo:.1}..{hi:.1}");
        // Left, counter-clockwise: from 3 o'clock back up towards the top.
        let (lo, hi) = at_start(Align::Left, false);
        assert!(hi < 95.0 && lo < 75.0, "{lo:.1}..{hi:.1}");
        // The same label drawn on a smaller canvas puts the text at the same angles.
        let ring =
            RingText { radius_mm: 46.0, start_deg: 90.0, clockwise: true, letter_spacing_pt: 0.0 };
        let l = ring_label("WASHI", ring, Align::Left);
        let spread = |px: u32| {
            let a: Vec<f64> = red_polar(&l.render_sized(px).unwrap()).iter().map(|p| p.0).collect();
            (a.iter().cloned().fold(f64::MAX, f64::min), a.iter().cloned().fold(f64::MIN, f64::max))
        };
        let (big, small) = (spread(PX), spread(PX / 2));
        assert!(
            (big.0 - small.0).abs() < 3.0 && (big.1 - small.1).abs() < 3.0,
            "{big:?} {small:?}"
        );
    }

    #[test]
    fn ring_text_json_and_limits() {
        let j = r#"{"texts": [{"text": "Mix", "size_pt": 12,
                      "ring": {"radius_mm": 40, "start_deg": 270, "clockwise": false,
                               "letter_spacing_pt": 4}}]}"#;
        let (label, _) = DiscSpec::from_json(j).unwrap().to_label().unwrap();
        assert_eq!(
            label.texts[0].ring,
            Some(RingText {
                radius_mm: 40.0,
                start_deg: 270.0,
                clockwise: false,
                letter_spacing_pt: 4.0
            })
        );
        // An empty ring object means the defaults; no ring key means a straight text.
        let (label, _) =
            DiscSpec::from_json(r#"{"texts": [{"text": "a", "ring": {}}, {"text": "b"}]}"#)
                .unwrap()
                .to_label()
                .unwrap();
        assert_eq!(label.texts[0].ring, Some(RingText::default()));
        assert_eq!(label.texts[1].ring, None);
        for bad in [
            r#"{"texts": [{"text": "a", "ring": {"radius_mm": 2}}]}"#,
            r#"{"texts": [{"text": "a", "ring": {"radius_mm": 70}}]}"#,
            r#"{"texts": [{"text": "a", "ring": {"letter_spacing_pt": 500}}]}"#,
        ] {
            assert!(DiscSpec::from_json(bad).unwrap().to_label().is_err(), "{bad}");
        }
        assert!(
            DiscSpec::from_json(r#"{"texts": [{"text": "a", "ring": {"radius": 40}}]}"#).is_err()
        );
        // A label with a ring text survives the round trip through serde.
        let (label, _) = DiscSpec::from_json(j).unwrap().to_label().unwrap();
        let back: DiscLabel =
            serde_json::from_str(&serde_json::to_string(&label).unwrap()).unwrap();
        assert_eq!(back, label);
        // A straight text is saved without the key.
        let straight = serde_json::to_string(&TextItem::new("x")).unwrap();
        assert!(!straight.contains("ring"), "{straight}");
    }

    #[test]
    fn colours_and_json_specs() {
        assert_eq!(parse_color("#ff8000").unwrap(), [255, 128, 0]);
        assert_eq!(parse_color("00FF7f").unwrap(), [0, 255, 127]);
        for bad in ["", "#fff", "red", "#gg0000", "#ff80000"] {
            assert!(parse_color(bad).is_err(), "{bad}");
        }
        let s = DiscSpec::from_json(
            r##"{"confirm":true,"image":"/tmp/a.png","fit":"contain","scale":1.5,"offset_mm":[2,-3],"rotation":10,
                 "background_color":"#102030","inner_mm":30,"outer_mm":115,"media":"b","quality":"high",
                 "texts":[{"text":"Hi","y_mm":10,"size_pt":24,"color":"#ff0000","align":"left","rotation":5}]}"##,
        )
        .unwrap();
        assert!(s.confirm);
        let (l, q) = s.to_label().unwrap();
        assert_eq!(q, Quality::High);
        assert_eq!(l.media, DiscMedia::B);
        assert_eq!((l.geometry.inner_mm, l.geometry.outer_mm), (30.0, 115.0));
        assert_eq!(l.background_color, [16, 32, 48]);
        let b = l.background.unwrap();
        assert_eq!(
            (b.fit, b.scale, b.offset_mm, b.rotation_deg),
            (BackgroundFit::Contain, 1.5, (2.0, -3.0), 10.0)
        );
        assert_eq!(l.texts[0].align, Align::Left);
        assert_eq!(
            (l.texts[0].y_mm, l.texts[0].size_pt, l.texts[0].color),
            (10.0, 24.0, [255, 0, 0])
        );
        assert!(DiscSpec::from_json(r#"{"inner":30}"#).is_err(), "unknown key");
        assert!(
            DiscSpec::from_json(r#"{"inner_mm":10}"#).unwrap().to_label().is_err(),
            "out of range"
        );
        assert!(DiscSpec::from_json(r#"{"media":"c"}"#).unwrap().to_label().is_err());
        assert!(!DiscSpec::from_json("{}").unwrap().confirm);
    }

    #[test]
    fn the_job_uses_the_disc_entry_of_the_media_database() {
        let job = disc_job(&DiscLabel::default(), Quality::High);
        let m = job.media.as_ref().unwrap();
        assert_eq!((m.width, m.height), (12000, 12000));
        assert_eq!(m.source.as_deref(), Some("disc"));
        assert_eq!(m.media_type.as_deref(), Some("com.canon-mtdisca"));
        assert_eq!(m.margins, Some(Margins { top: 10, bottom: 10, left: 10, right: 10 }));
        assert_eq!(job.format, "image/pwg-raster");
        let b =
            disc_job(&DiscLabel { media: DiscMedia::B, ..DiscLabel::default() }, Quality::Normal);
        assert_eq!(b.media.unwrap().media_type.as_deref(), Some("com.canon-mtdiscb"));
        assert_eq!(DiscMedia::parse("A").unwrap(), DiscMedia::A);
        assert!(DiscMedia::parse("c").is_err());
        let plan =
            plan_disc(&Printer::new("printer.example"), &DiscLabel::default(), Quality::Normal)
                .unwrap();
        assert!(
            plan.request.contains("media-source=disc") || plan.request.contains("disc"),
            "{}",
            plan.request
        );
        assert!(
            plan.request.contains("com.canon-mtdisca")
                && plan.request.contains("x-dimension=12000")
        );
    }

    #[test]
    fn the_spooled_raster_is_one_square_srgb_page() {
        let label = DiscLabel { background_color: [10, 200, 30], ..DiscLabel::default() };
        let job = disc_job(&label, Quality::Normal);
        let Spool::File(f) = spool_label(&label, &job, None).unwrap() else {
            panic!("expected a spool file")
        };
        let bytes = std::fs::read(f.path()).unwrap();
        let pages = crate::pwg::decode(&bytes).unwrap();
        assert_eq!(pages.len(), 1);
        let h = &pages[0].header;
        assert_eq!((h.width, h.height, h.dpi), (2834, 2834, 600));
        assert_eq!((h.page_width_pt, h.page_height_pt), (340, 340));
        assert_eq!(h.media_type, "com.canon-mtdisca");
        assert_eq!(h.page_size_name, "om_square_120x120mm");
        assert!(!h.duplex);
        let px = |x: usize, y: usize| &pages[0].pixels[(y * 2834 + x) * 3..(y * 2834 + x) * 3 + 3];
        assert_eq!(px(1417, 1417), [255, 255, 255], "hub");
        assert_eq!(px(1417, 400), [10, 200, 30], "ring");
        assert_eq!(px(5, 5), [255, 255, 255], "corner");
        assert!(bytes.len() < 400_000, "mostly flat colour compresses well: {}", bytes.len());
        assert!(check_image(Path::new("/nonexistent/x.png")).is_err());
    }
}
