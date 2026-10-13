// SPDX-License-Identifier: GPL-2.0-or-later
//! Geometry: quarter-turn rotation, slant (small-angle) correction with a deskew estimate, and
//! trimming with aspect-ratio presets.

use image::imageops::{self, FilterType};
use image::{GrayImage, Rgb, RgbImage};
use serde::{Deserialize, Serialize};

/// Slant correction range of the "Rotate/Crop" screen, in degrees either way.
pub const MAX_SLANT_DEGREES: f32 = 10.0;

/// A multiple of 90 degrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Rotation {
    #[default]
    None,
    /// 90 degrees clockwise.
    Right,
    Half,
    /// 90 degrees counter-clockwise.
    Left,
}

impl Rotation {
    pub fn quarter_turns(self) -> u8 {
        match self {
            Rotation::None => 0,
            Rotation::Right => 1,
            Rotation::Half => 2,
            Rotation::Left => 3,
        }
    }

    pub fn from_quarter_turns(n: i32) -> Rotation {
        match n.rem_euclid(4) {
            0 => Rotation::None,
            1 => Rotation::Right,
            2 => Rotation::Half,
            _ => Rotation::Left,
        }
    }

    pub fn then(self, other: Rotation) -> Rotation {
        Rotation::from_quarter_turns(
            i32::from(self.quarter_turns()) + i32::from(other.quarter_turns()),
        )
    }

    /// True when width and height swap.
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Right | Rotation::Left)
    }
}

pub fn rotate_quarters(img: &RgbImage, r: Rotation) -> RgbImage {
    match r {
        Rotation::None => img.clone(),
        Rotation::Right => imageops::rotate90(img),
        Rotation::Half => imageops::rotate180(img),
        Rotation::Left => imageops::rotate270(img),
    }
}

fn sample_bilinear(img: &RgbImage, x: f32, y: f32) -> Option<[f32; 3]> {
    let (w, h) = img.dimensions();
    if x < -0.5 || y < -0.5 || x > w as f32 - 0.5 || y > h as f32 - 0.5 {
        return None;
    }
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |xx: u32, yy: u32| img.get_pixel(xx, yy).0;
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    let mut out = [0f32; 3];
    for i in 0..3 {
        let top = f32::from(a[i]) * (1.0 - fx) + f32::from(b[i]) * fx;
        let bot = f32::from(c[i]) * (1.0 - fx) + f32::from(d[i]) * fx;
        out[i] = top * (1.0 - fy) + bot * fy;
    }
    Some(out)
}

/// Rotate the picture content by `degrees` (positive = clockwise) about its centre. The canvas
/// keeps its size; uncovered corners are filled with `fill`. Bilinear resampling.
pub fn rotate_degrees(img: &RgbImage, degrees: f32, fill: Rgb<u8>) -> RgbImage {
    if degrees.abs() < 1e-4 {
        return img.clone();
    }
    let (w, h) = img.dimensions();
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut out = RgbImage::from_pixel(w, h, fill);
    for y in 0..h {
        let dy = y as f32 + 0.5 - cy;
        for x in 0..w {
            let dx = x as f32 + 0.5 - cx;
            // Inverse of a clockwise rotation in y-down coordinates.
            let sx = cx + dx * cos + dy * sin - 0.5;
            let sy = cy - dx * sin + dy * cos - 0.5;
            if let Some(p) = sample_bilinear(img, sx, sy) {
                out.put_pixel(x, y, Rgb([p[0] as u8, p[1] as u8, p[2] as u8]));
            }
        }
    }
    out
}

fn otsu_threshold(gray: &GrayImage) -> u8 {
    let mut hist = [0u64; 256];
    for p in gray.pixels() {
        hist[usize::from(p.0[0])] += 1;
    }
    let total: u64 = hist.iter().sum();
    let sum_all: f64 = hist.iter().enumerate().map(|(i, c)| i as f64 * *c as f64).sum();
    let (mut w_b, mut sum_b, mut best, mut thr) = (0u64, 0f64, -1f64, 128u8);
    for (t, c) in hist.iter().enumerate() {
        w_b += c;
        if w_b == 0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0 {
            break;
        }
        sum_b += t as f64 * *c as f64;
        let (m_b, m_f) = (sum_b / w_b as f64, (sum_all - sum_b) / w_f as f64);
        let between = w_b as f64 * w_f as f64 * (m_b - m_f) * (m_b - m_f);
        if between > best {
            best = between;
            thr = t as u8;
        }
    }
    thr
}

/// Score of a candidate slant: how "banded" the projection of the ink pixels is when rows are
/// tilted by `deg` degrees (sum of squared differences of neighbouring row counts).
fn projection_score(points: &[(f32, f32)], deg: f32, rows: usize, origin: f32) -> f64 {
    let (sin, cos) = deg.to_radians().sin_cos();
    let mut hist = vec![0f64; rows + 2];
    for &(x, y) in points {
        let yr = y * cos + x * sin + origin;
        let i = yr.round();
        if i >= 0.0 && (i as usize) < rows {
            hist[i as usize] += 1.0;
        }
    }
    hist.windows(2).map(|w| (w[1] - w[0]) * (w[1] - w[0])).sum()
}

/// Estimate the angle that [`rotate_degrees`] has to apply to straighten text lines, in
/// degrees (within +-[`MAX_SLANT_DEGREES`]). Works on mostly-text pages by finding the tilt at
/// which the dark pixels line up in the sharpest rows. Returns 0 when there is no clear answer
/// (photos, blank pages).
pub fn estimate_skew(img: &RgbImage) -> f32 {
    let (w, h) = img.dimensions();
    if w < 16 || h < 16 {
        return 0.0;
    }
    let scale = (900.0 / w.max(h) as f32).min(1.0);
    let small = if scale < 1.0 {
        imageops::resize(
            img,
            ((w as f32 * scale) as u32).max(1),
            ((h as f32 * scale) as u32).max(1),
            FilterType::Triangle,
        )
    } else {
        img.clone()
    };
    let gray = imageops::grayscale(&small);
    let thr = otsu_threshold(&gray);
    // Ink = clearly darker than the paper.
    let cut = thr.min(200);
    let (sw, sh) = gray.dimensions();
    let points: Vec<(f32, f32)> = gray
        .enumerate_pixels()
        .filter(|(_, _, p)| p.0[0] < cut)
        .map(|(x, y, _)| (x as f32 - sw as f32 / 2.0, y as f32 - sh as f32 / 2.0))
        .collect();
    // Too little or too much ink: not a text page.
    let area = (sw * sh) as usize;
    if points.len() < 200 || points.len() > area * 6 / 10 {
        return 0.0;
    }
    let rows = (sw as usize).max(sh as usize) * 2;
    let origin = rows as f32 / 2.0;
    let score = |deg: f32| projection_score(&points, deg, rows, origin);

    let mut best = (0.0f32, f64::MIN);
    let mut deg = -MAX_SLANT_DEGREES;
    while deg <= MAX_SLANT_DEGREES + 1e-3 {
        let s = score(deg);
        if s > best.1 {
            best = (deg, s);
        }
        deg += 0.5;
    }
    let mut refined = best;
    let mut d = best.0 - 0.5;
    while d <= best.0 + 0.5 + 1e-3 {
        let s = score(d);
        if s > refined.1 {
            refined = (d, s);
        }
        d += 0.05;
    }
    let flat = score(0.0);
    // Require a clear gain over "already straight".
    if refined.1 < flat * 1.15 {
        return 0.0;
    }
    refined.0.clamp(-MAX_SLANT_DEGREES, MAX_SLANT_DEGREES)
}

/// A crop rectangle as fractions of the image (so it survives resolution changes).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl CropRect {
    pub const FULL: CropRect = CropRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };

    /// Clamp into the image and make sure it is at least `min` of each side.
    pub fn clamped(self, min: f32) -> CropRect {
        let w = self.w.clamp(min, 1.0);
        let h = self.h.clamp(min, 1.0);
        CropRect { x: self.x.clamp(0.0, 1.0 - w), y: self.y.clamp(0.0, 1.0 - h), w, h }
    }

    /// Pixel box `(x, y, w, h)` inside an image of `iw` x `ih`; at least 1x1.
    pub fn to_pixels(self, iw: u32, ih: u32) -> (u32, u32, u32, u32) {
        let r = self.clamped(1.0 / iw.max(ih).max(1) as f32);
        let x = ((r.x * iw as f32).round() as u32).min(iw.saturating_sub(1));
        let y = ((r.y * ih as f32).round() as u32).min(ih.saturating_sub(1));
        let w = ((r.w * iw as f32).round() as u32).clamp(1, iw - x);
        let h = ((r.h * ih as f32).round() as u32).clamp(1, ih - y);
        (x, y, w, h)
    }

    pub fn is_full(&self) -> bool {
        *self == CropRect::FULL
    }
}

pub fn crop(img: &RgbImage, r: CropRect) -> RgbImage {
    let (x, y, w, h) = r.to_pixels(img.width(), img.height());
    imageops::crop_imm(img, x, y, w, h).to_image()
}

/// "Trimming Aspect Ratio" presets of the Rotate/Crop screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectPreset {
    Custom,
    /// The aspect ratio of the whole image.
    Original,
    Square,
    R3x2,
    R4x3,
    R7x5,
    R10x8,
    R16x9,
}

impl AspectPreset {
    pub const ALL: [AspectPreset; 8] = [
        AspectPreset::Custom,
        AspectPreset::Original,
        AspectPreset::Square,
        AspectPreset::R3x2,
        AspectPreset::R4x3,
        AspectPreset::R7x5,
        AspectPreset::R10x8,
        AspectPreset::R16x9,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AspectPreset::Custom => "Custom",
            AspectPreset::Original => "Original aspect ratio",
            AspectPreset::Square => "Square",
            AspectPreset::R3x2 => "3:2",
            AspectPreset::R4x3 => "4:3",
            AspectPreset::R7x5 => "7:5",
            AspectPreset::R10x8 => "10:8",
            AspectPreset::R16x9 => "16:9",
        }
    }

    pub fn parse(s: &str) -> Option<AspectPreset> {
        match s.trim().to_ascii_lowercase().as_str() {
            "custom" | "free" => Some(AspectPreset::Custom),
            "original" => Some(AspectPreset::Original),
            "square" | "1:1" => Some(AspectPreset::Square),
            "3:2" => Some(AspectPreset::R3x2),
            "4:3" => Some(AspectPreset::R4x3),
            "7:5" => Some(AspectPreset::R7x5),
            "10:8" | "5:4" => Some(AspectPreset::R10x8),
            "16:9" => Some(AspectPreset::R16x9),
            _ => None,
        }
    }

    /// Long side over short side; `None` for Custom (and Original needs the image).
    fn ratio(self, iw: u32, ih: u32) -> Option<f32> {
        Some(match self {
            AspectPreset::Custom => return None,
            AspectPreset::Original => iw.max(ih) as f32 / iw.min(ih).max(1) as f32,
            AspectPreset::Square => 1.0,
            AspectPreset::R3x2 => 3.0 / 2.0,
            AspectPreset::R4x3 => 4.0 / 3.0,
            AspectPreset::R7x5 => 7.0 / 5.0,
            AspectPreset::R10x8 => 10.0 / 8.0,
            AspectPreset::R16x9 => 16.0 / 9.0,
        })
    }
}

/// Adjust `current` (default: the whole image) to the preset's aspect ratio by trimming its
/// longer side, keeping its centre. The long side of the ratio follows the image orientation
/// (landscape image -> landscape crop). `Custom` returns `current` unchanged.
pub fn fit_aspect(iw: u32, ih: u32, preset: AspectPreset, current: Option<CropRect>) -> CropRect {
    let cur = current.unwrap_or(CropRect::FULL);
    let Some(mut ratio) = preset.ratio(iw, ih) else { return cur };
    if iw == 0 || ih == 0 {
        return cur;
    }
    if ih > iw {
        ratio = 1.0 / ratio; // width / height of a portrait crop
    }
    // Work in pixels so the ratio is exact for the actual image.
    let (cw, ch) = (cur.w * iw as f32, cur.h * ih as f32);
    let (nw, nh) = if cw / ch > ratio { (ch * ratio, ch) } else { (cw, cw / ratio) };
    let (cx, cy) = ((cur.x + cur.w / 2.0) * iw as f32, (cur.y + cur.h / 2.0) * ih as f32);
    CropRect {
        x: (cx - nw / 2.0) / iw as f32,
        y: (cy - nh / 2.0) / ih as f32,
        w: nw / iw as f32,
        h: nh / ih as f32,
    }
    .clamped(0.001)
}

/// Synthetic images shared by the unit tests of the scan modules.
#[cfg(test)]
pub(crate) mod tests_support {
    use image::{Rgb, RgbImage};

    /// A light page with rows of dark dashes that look like lines of text.
    pub(crate) fn text_page(w: u32, h: u32) -> RgbImage {
        let mut img = RgbImage::from_pixel(w, h, Rgb([240, 240, 240]));
        let mut y = h / 12;
        while y + 6 < h * 11 / 12 {
            let mut x = w / 10;
            while x + 12 < w * 9 / 10 {
                let len = 8 + (x * 7 + y * 3) % 18;
                for yy in y..y + 5 {
                    for xx in x..(x + len).min(w * 9 / 10) {
                        img.put_pixel(xx, yy, Rgb([20, 20, 20]));
                    }
                }
                x += len + 5 + (x + y) % 7;
            }
            y += 14;
        }
        img
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::text_page;
    use super::*;

    #[test]
    fn quarter_turns_compose() {
        assert_eq!(Rotation::Right.then(Rotation::Right), Rotation::Half);
        assert_eq!(Rotation::Left.then(Rotation::Right), Rotation::None);
        assert_eq!(Rotation::from_quarter_turns(-1), Rotation::Left);
        assert_eq!(Rotation::from_quarter_turns(5), Rotation::Right);
        assert!(Rotation::Right.swaps_axes() && !Rotation::Half.swaps_axes());
        let mut img = RgbImage::new(4, 2);
        img.put_pixel(0, 0, Rgb([255, 0, 0]));
        let r = rotate_quarters(&img, Rotation::Right);
        assert_eq!(r.dimensions(), (2, 4));
        assert_eq!(r.get_pixel(1, 0).0, [255, 0, 0], "top-left goes to top-right");
        let l = rotate_quarters(&img, Rotation::Left);
        assert_eq!(l.get_pixel(0, 3).0, [255, 0, 0], "top-left goes to bottom-left");
        assert_eq!(rotate_quarters(&img, Rotation::Half).get_pixel(3, 1).0, [255, 0, 0]);
    }

    #[test]
    fn small_rotation_moves_content_clockwise_and_fills_corners() {
        let mut img = RgbImage::from_pixel(101, 101, Rgb([255, 255, 255]));
        // A vertical black bar at the right of centre.
        for y in 20..80 {
            for x in 70..74 {
                img.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        let r = rotate_degrees(&img, 90.0, Rgb([10, 20, 30]));
        // 90 degrees clockwise: the bar (right of centre) ends up below centre.
        let dark = |x: u32, y: u32| r.get_pixel(x, y).0[0] < 60;
        assert!(dark(50, 72), "bar is now horizontal below the centre");
        assert!(!dark(72, 50));
        let r = rotate_degrees(&img, 20.0, Rgb([10, 20, 30]));
        assert_eq!(r.dimensions(), (101, 101));
        assert_eq!(r.get_pixel(0, 0).0, [10, 20, 30], "uncovered corner uses the fill colour");
        assert_eq!(rotate_degrees(&img, 0.0, Rgb([0, 0, 0])), img);
    }

    #[test]
    fn deskew_recovers_the_tilt() {
        let page = text_page(600, 800);
        assert!(estimate_skew(&page).abs() < 0.3, "a straight page stays put");
        for tilt in [3.0f32, -2.0, 6.5] {
            // Tilt the page `tilt` degrees clockwise; the estimate must undo it.
            let tilted = rotate_degrees(&page, tilt, Rgb([240, 240, 240]));
            let est = estimate_skew(&tilted);
            assert!((est + tilt).abs() < 0.4, "tilt {tilt}: estimated correction {est}");
            let fixed = rotate_degrees(&tilted, est, Rgb([240, 240, 240]));
            assert!(estimate_skew(&fixed).abs() < 0.4);
        }
    }

    #[test]
    fn deskew_ignores_blank_and_photographic_pages() {
        assert_eq!(estimate_skew(&RgbImage::from_pixel(300, 300, Rgb([250, 250, 250]))), 0.0);
        assert_eq!(estimate_skew(&RgbImage::new(4, 4)), 0.0);
        let mut noise = RgbImage::new(200, 200);
        let mut s = 12345u32;
        for p in noise.pixels_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let v = (s >> 24) as u8;
            *p = Rgb([v, v, v]);
        }
        assert!(estimate_skew(&noise).abs() <= MAX_SLANT_DEGREES);
    }

    #[test]
    fn crop_rect_maths() {
        let r = CropRect { x: 0.25, y: 0.5, w: 0.5, h: 0.25 };
        assert_eq!(r.to_pixels(200, 100), (50, 50, 100, 25));
        assert!(CropRect::FULL.is_full() && !r.is_full());
        // Out-of-range rectangles are pulled back inside the image.
        let wild = CropRect { x: 0.9, y: -1.0, w: 0.5, h: 2.0 }.clamped(0.01);
        assert_eq!((wild.x, wild.y, wild.w, wild.h), (0.5, 0.0, 0.5, 1.0));
        let (x, y, w, h) = CropRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }.to_pixels(10, 10);
        assert!(w >= 1 && h >= 1 && x + w <= 10 && y + h <= 10);
        let img = RgbImage::from_pixel(40, 20, Rgb([1, 2, 3]));
        assert_eq!(crop(&img, CropRect { x: 0.0, y: 0.0, w: 0.5, h: 1.0 }).dimensions(), (20, 20));
    }

    #[test]
    fn aspect_presets() {
        let ratio = |r: CropRect, iw: u32, ih: u32| (r.w * iw as f32) / (r.h * ih as f32);
        // Landscape image 300x200 (3:2) cropped to 4:3.
        let r = fit_aspect(300, 200, AspectPreset::R4x3, None);
        assert!((ratio(r, 300, 200) - 4.0 / 3.0).abs() < 1e-3);
        assert!((r.x + r.w / 2.0 - 0.5).abs() < 1e-4, "centred");
        assert!((r.h - 1.0).abs() < 1e-4 && r.w < 1.0, "the longer side was trimmed");
        // Portrait image: the long side is vertical.
        let r = fit_aspect(200, 300, AspectPreset::R16x9, None);
        assert!((ratio(r, 200, 300) - 9.0 / 16.0).abs() < 1e-3);
        // Square and Original.
        let r = fit_aspect(300, 200, AspectPreset::Square, None);
        assert!((ratio(r, 300, 200) - 1.0).abs() < 1e-3);
        let sub = CropRect { x: 0.1, y: 0.1, w: 0.5, h: 0.6 };
        let r = fit_aspect(300, 200, AspectPreset::Original, Some(sub));
        assert!((ratio(r, 300, 200) - 1.5).abs() < 1e-3);
        assert!(
            r.x >= sub.x - 1e-4 && r.x + r.w <= sub.x + sub.w + 1e-4,
            "stays inside the old box"
        );
        // Custom keeps what it is given.
        assert_eq!(fit_aspect(300, 200, AspectPreset::Custom, Some(sub)), sub);
        assert_eq!(fit_aspect(300, 200, AspectPreset::Custom, None), CropRect::FULL);
        for p in AspectPreset::ALL {
            assert!(AspectPreset::parse(p.label().split(' ').next().unwrap()).is_some(), "{p:?}");
        }
        assert_eq!(AspectPreset::parse("10:8"), Some(AspectPreset::R10x8));
        assert_eq!(AspectPreset::parse("9:7"), None);
    }
}
