// SPDX-License-Identifier: GPL-2.0-or-later
//! Image Correction: colour correction presets, show-through reduction, colour mode and unsharp
//! mask. All functions work on 8-bit pixels and are deterministic.
//!
//! What each preset does (the text the GUI shows as a tooltip):
//!
//! * **For document**: finds the paper white per colour channel (95th percentile), scales each
//!   channel so the paper becomes white (this removes a yellow or grey cast), pushes everything
//!   close to white fully to white (whitens the background) and adds a little contrast.
//! * **For photo**: automatic levels. The darkest 0.5 % and the lightest 0.5 % of the luminance
//!   are clipped and the rest is stretched over the full range (the same for all channels, so
//!   colours keep their balance); a gamma between 0.8 and 1.25 then moves the average brightness
//!   toward mid-grey.
//! * **For magazine**: removes the halftone dot pattern of printed pictures (descreening) with a
//!   Gaussian blur that scales with the scan resolution, then applies the photo levels and
//!   restores some edge contrast with a light unsharp mask.

use image::imageops;
use image::{GrayImage, RgbImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorCorrection {
    #[default]
    None,
    Document,
    Photo,
    Magazine,
}

/// `Color Mode` of the Image Correction screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OutputMode {
    #[default]
    Color,
    Grayscale,
    BlackWhite,
}

impl OutputMode {
    pub fn parse(s: &str) -> Option<OutputMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "color" | "colour" => Some(OutputMode::Color),
            "gray" | "grey" | "grayscale" | "greyscale" => Some(OutputMode::Grayscale),
            "bw" | "b&w" | "blackwhite" | "black-white" | "black-and-white" | "mono" => {
                Some(OutputMode::BlackWhite)
            }
            _ => None,
        }
    }
}

/// Everything on the Image Correction screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ImageCorrection {
    pub color_correction: ColorCorrection,
    /// "Reduce Show-through" (text from the back of a thin page).
    pub reduce_show_through: bool,
    /// Brightness (0..=255) from which a pixel is lifted to white when show-through reduction
    /// is on. Lower values are more aggressive.
    pub show_through_threshold: u8,
    pub color_mode: OutputMode,
    /// Black-and-white threshold; `None` picks it automatically (Otsu).
    pub bw_threshold: Option<u8>,
    /// Unsharp mask strength, 0.0 (off) to 1.0 (strong).
    pub unsharp_amount: f32,
}

impl Default for ImageCorrection {
    fn default() -> Self {
        ImageCorrection {
            color_correction: ColorCorrection::None,
            reduce_show_through: false,
            show_through_threshold: 200,
            color_mode: OutputMode::Color,
            bw_threshold: None,
            unsharp_amount: 0.0,
        }
    }
}

impl ImageCorrection {
    /// Nothing to apply.
    pub fn is_identity(&self) -> bool {
        *self == ImageCorrection::default()
    }
}

fn luma(p: &[u8]) -> u8 {
    // Rec. 601 integer approximation.
    ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8
}

/// Value below which `fraction` of the samples lie.
fn percentile(hist: &[u64; 256], total: u64, fraction: f64) -> u8 {
    let target = (total as f64 * fraction).ceil() as u64;
    let mut acc = 0u64;
    for (v, c) in hist.iter().enumerate() {
        acc += c;
        if acc >= target.max(1) {
            return v as u8;
        }
    }
    255
}

fn apply_lut(img: &mut RgbImage, lut: &[[u8; 256]; 3]) {
    for p in img.pixels_mut() {
        p.0[0] = lut[0][usize::from(p.0[0])];
        p.0[1] = lut[1][usize::from(p.0[1])];
        p.0[2] = lut[2][usize::from(p.0[2])];
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// "For document".
pub fn correct_document(img: &mut RgbImage) {
    let n = u64::from(img.width()) * u64::from(img.height());
    if n == 0 {
        return;
    }
    let mut hist = [[0u64; 256]; 3];
    for p in img.pixels() {
        for c in 0..3 {
            hist[c][usize::from(p.0[c])] += 1;
        }
    }
    let mut lut = [[0u8; 256]; 3];
    for c in 0..3 {
        let white = f32::from(percentile(&hist[c], n, 0.95)).max(96.0);
        let gain = (255.0 / white).clamp(1.0, 1.6);
        for (v, out) in lut[c].iter_mut().enumerate() {
            let scaled = (v as f32 * gain).min(255.0);
            // Whiten: from 86 % of white upward the value ramps to pure white.
            let lift = smoothstep(0.86 * 255.0, 0.97 * 255.0, scaled);
            let whitened = scaled + (255.0 - scaled) * lift;
            // A little contrast around mid-grey.
            let contrasted = 128.0 + (whitened - 128.0) * 1.1;
            *out = contrasted.clamp(0.0, 255.0).round() as u8;
        }
    }
    apply_lut(img, &lut);
}

/// "For photo": automatic levels.
pub fn correct_photo(img: &mut RgbImage) {
    let n = u64::from(img.width()) * u64::from(img.height());
    if n == 0 {
        return;
    }
    let mut hist = [0u64; 256];
    let mut sum = 0u64;
    for p in img.pixels() {
        let l = luma(&p.0);
        hist[usize::from(l)] += 1;
        sum += u64::from(l);
    }
    let lo = f32::from(percentile(&hist, n, 0.005));
    let hi = f32::from(percentile(&hist, n, 0.995));
    if hi - lo < 24.0 {
        return; // flat image: stretching would only amplify noise
    }
    let mean = (sum as f32 / n as f32 - lo) / (hi - lo);
    let gamma =
        if mean > 0.02 && mean < 0.98 { (mean.ln() / 0.46f32.ln()).clamp(0.8, 1.25) } else { 1.0 };
    let mut row = [0u8; 256];
    for (v, out) in row.iter_mut().enumerate() {
        let t = ((v as f32 - lo) / (hi - lo)).clamp(0.0, 1.0);
        *out = (t.powf(1.0 / gamma) * 255.0).round() as u8;
    }
    apply_lut(img, &[row, row, row]);
}

/// Gaussian sigma that matches a feature size of "1 pixel at 300 dpi".
fn dpi_sigma(base: f32, dpi: u32) -> f32 {
    (base * dpi.max(1) as f32 / 300.0).clamp(0.5, 6.0)
}

/// "For magazine": descreen, levels, restore edges.
pub fn correct_magazine(img: &mut RgbImage, dpi: u32) {
    *img = imageops::blur(img, dpi_sigma(1.2, dpi));
    correct_photo(img);
    unsharp(img, 0.3, dpi);
}

pub fn apply_color_correction(img: &mut RgbImage, kind: ColorCorrection, dpi: u32) {
    match kind {
        ColorCorrection::None => {}
        ColorCorrection::Document => correct_document(img),
        ColorCorrection::Photo => correct_photo(img),
        ColorCorrection::Magazine => correct_magazine(img, dpi),
    }
}

/// "Reduce Show-through": pixels brighter than `threshold - 32` are lifted toward white and
/// everything from `threshold` up becomes white, so faint ink from the page behind disappears
/// while real text (dark) is untouched.
pub fn reduce_show_through(img: &mut RgbImage, threshold: u8) {
    let t = f32::from(threshold);
    for p in img.pixels_mut() {
        let l = f32::from(luma(&p.0));
        let lift = smoothstep((t - 32.0).max(0.0), t, l);
        if lift > 0.0 {
            for c in &mut p.0 {
                *c = (f32::from(*c) + (255.0 - f32::from(*c)) * lift).round() as u8;
            }
        }
    }
}

/// "Unsharp Mask": `out = in + strength * (in - blurred)`. `amount` 0..=1 maps to a strength of
/// 0..=1.5; the blur radius scales with the resolution.
pub fn unsharp(img: &mut RgbImage, amount: f32, dpi: u32) {
    let amount = amount.clamp(0.0, 1.0);
    if amount < 0.005 {
        return;
    }
    let strength = amount * 1.5;
    let blurred = imageops::blur(img, dpi_sigma(1.0, dpi));
    for (p, b) in img.pixels_mut().zip(blurred.pixels()) {
        for c in 0..3 {
            let (o, bl) = (f32::from(p.0[c]), f32::from(b.0[c]));
            p.0[c] = (o + strength * (o - bl)).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Otsu's threshold of a grey image.
pub fn otsu(gray: &GrayImage) -> u8 {
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

/// Black (0) / white (255): pixels above `threshold` are white. `None` = Otsu.
pub fn threshold_bw(gray: &GrayImage, threshold: Option<u8>) -> GrayImage {
    let t = threshold.unwrap_or_else(|| otsu(gray));
    let mut out = gray.clone();
    for p in out.pixels_mut() {
        p.0[0] = if p.0[0] > t { 255 } else { 0 };
    }
    out
}

/// Pack a black/white image into PDF bits (MSB first, rows padded to bytes, 1 = white).
pub fn pack_bilevel(bw: &GrayImage) -> Vec<u8> {
    let (w, h) = bw.dimensions();
    let stride = (w as usize).div_ceil(8);
    let mut out = vec![0u8; stride * h as usize];
    for (x, y, p) in bw.enumerate_pixels() {
        if p.0[0] > 127 {
            out[y as usize * stride + x as usize / 8] |= 0x80 >> (x % 8);
        }
    }
    // Padding bits in the last byte of each row are white too.
    if w % 8 != 0 {
        let pad = (0xFFu8) >> (w % 8);
        for y in 0..h as usize {
            out[y * stride + stride - 1] |= pad;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;

    fn paper_with_text() -> RgbImage {
        // Greyish yellow paper (225, 215, 190) with dark "text" columns.
        let mut img = RgbImage::from_pixel(100, 100, Rgb([225, 215, 190]));
        for y in 0..100 {
            for x in (5..95).step_by(10) {
                img.put_pixel(x, y, Rgb([40, 35, 30]));
            }
        }
        img
    }

    #[test]
    fn document_correction_whitens_the_paper_and_keeps_text() {
        let mut img = paper_with_text();
        correct_document(&mut img);
        let bg = img.get_pixel(0, 0).0;
        assert!(bg.iter().all(|c| *c >= 250), "paper white, got {bg:?}");
        let text = img.get_pixel(5, 50).0;
        assert!(text.iter().all(|c| *c < 80), "text stays dark, got {text:?}");
        // Colour cast removed: the three channels of the paper are (near) equal.
        let spread = bg.iter().max().unwrap() - bg.iter().min().unwrap();
        assert!(spread <= 5);
    }

    #[test]
    fn photo_levels_stretch_a_dull_image() {
        let mut img = RgbImage::new(256, 4);
        for (x, _, p) in img.enumerate_pixels_mut() {
            let v = 60 + (x as u8) / 2; // 60..187
            *p = Rgb([v, v, v]);
        }
        correct_photo(&mut img);
        let lo = img.get_pixel(1, 0).0[0];
        let hi = img.get_pixel(254, 0).0[0];
        assert!(lo < 10 && hi > 245, "stretched to the full range, got {lo}..{hi}");
        // Monotonic: levels never reorder tones.
        let mut last = 0;
        for x in 0..256 {
            let v = img.get_pixel(x, 0).0[0];
            assert!(v >= last);
            last = v;
        }
    }

    #[test]
    fn flat_images_are_left_alone_by_photo_levels() {
        let mut img = RgbImage::from_pixel(50, 50, Rgb([120, 120, 120]));
        correct_photo(&mut img);
        assert_eq!(img.get_pixel(3, 3).0, [120, 120, 120]);
        let mut empty = RgbImage::new(0, 0);
        correct_document(&mut empty);
        correct_photo(&mut empty);
    }

    #[test]
    fn magazine_descreen_flattens_a_halftone_pattern() {
        // A 2 px checkerboard of 90/210 (a fine halftone) averages out.
        let mut img = RgbImage::new(64, 64);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let v = if (x / 2 + y / 2) % 2 == 0 { 90 } else { 210 };
            *p = Rgb([v, v, v]);
        }
        let swing = |i: &RgbImage| {
            let vals: Vec<u8> = (20..44).map(|x| i.get_pixel(x, 32).0[0]).collect();
            vals.iter().max().unwrap() - vals.iter().min().unwrap()
        };
        let before = swing(&img);
        let mut out = img.clone();
        correct_magazine(&mut out, 300);
        assert!(swing(&out) < before / 2, "dots {before} -> {}", swing(&out));
    }

    #[test]
    fn show_through_lifts_faint_ink_but_not_text() {
        let mut img = RgbImage::from_pixel(10, 10, Rgb([235, 235, 235]));
        img.put_pixel(1, 1, Rgb([205, 205, 205])); // ghost of the back page
        img.put_pixel(2, 2, Rgb([60, 60, 60])); // real text
        let mut out = img.clone();
        reduce_show_through(&mut out, 200);
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(out.get_pixel(1, 1).0, [255, 255, 255], "ghost removed");
        assert_eq!(out.get_pixel(2, 2).0, [60, 60, 60], "text untouched");
        // A lower threshold is more aggressive.
        let mut mid = RgbImage::from_pixel(2, 2, Rgb([150, 150, 150]));
        reduce_show_through(&mut mid, 160);
        assert!(mid.get_pixel(0, 0).0[0] > 150);
        let mut keep = RgbImage::from_pixel(2, 2, Rgb([150, 150, 150]));
        reduce_show_through(&mut keep, 240);
        assert_eq!(keep.get_pixel(0, 0).0[0], 150);
    }

    #[test]
    fn unsharp_sharpens_edges_and_zero_is_a_no_op() {
        let mut img = RgbImage::new(40, 10);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 20 { Rgb([100, 100, 100]) } else { Rgb([150, 150, 150]) };
        }
        let before = img.clone();
        unsharp(&mut img, 0.0, 300);
        assert_eq!(img, before);
        unsharp(&mut img, 1.0, 300);
        // Overshoot on both sides of the edge, flat areas unchanged.
        assert!(img.get_pixel(19, 5).0[0] < 100);
        assert!(img.get_pixel(20, 5).0[0] > 150);
        assert_eq!(img.get_pixel(2, 5).0[0], 100);
        assert_eq!(img.get_pixel(37, 5).0[0], 150);
    }

    #[test]
    fn black_white_threshold_and_packing() {
        let mut g = GrayImage::new(10, 2);
        for (x, _, p) in g.enumerate_pixels_mut() {
            p.0[0] = if x < 5 { 40 } else { 200 };
        }
        let fixed = threshold_bw(&g, Some(100));
        assert_eq!(fixed.get_pixel(4, 0).0[0], 0);
        assert_eq!(fixed.get_pixel(5, 0).0[0], 255);
        // Otsu finds the gap between 40 and 200.
        let auto = threshold_bw(&g, None);
        assert_eq!(auto, fixed);
        let t = otsu(&g);
        assert!((40..200).contains(&t));
        // Packed: 10 pixels per row -> 2 bytes; first five black, rest white, padding white.
        let packed = pack_bilevel(&fixed);
        assert_eq!(packed.len(), 4);
        assert_eq!(&packed[..2], &[0b0000_0111, 0b1111_1111]);
        // An all-white 8-wide image packs to 0xFF.
        assert_eq!(pack_bilevel(&GrayImage::from_pixel(8, 1, image::Luma([255]))), [0xFF]);
    }

    #[test]
    fn mode_parsing_and_identity() {
        assert_eq!(OutputMode::parse("BW"), Some(OutputMode::BlackWhite));
        assert_eq!(OutputMode::parse("grey"), Some(OutputMode::Grayscale));
        assert_eq!(OutputMode::parse("colour"), Some(OutputMode::Color));
        assert_eq!(OutputMode::parse("sepia"), None);
        assert!(ImageCorrection::default().is_identity());
        let c = ImageCorrection { unsharp_amount: 0.2, ..ImageCorrection::default() };
        assert!(!c.is_identity());
    }
}
