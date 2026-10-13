// SPDX-License-Identifier: GPL-2.0-or-later
//! IJ Scan Utility's "Stitch": join two scans of an original that is bigger than the platen.

use image::RgbImage;
use image::imageops::{self, FilterType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchDirection {
    /// The second scan continues to the right of the first.
    LeftRight,
    /// The second scan continues below the first.
    TopBottom,
}

#[derive(Debug, Clone, Copy)]
pub struct StitchOptions {
    /// Look for the strip both scans have in common and join there (the scans overlap when
    /// the original was moved by less than the platen size).
    pub auto_overlap: bool,
    /// Largest overlap searched, as a fraction of the shorter scan's length.
    pub max_overlap_fraction: f32,
    /// Largest average pixel difference (0..=255) still accepted as "the same strip".
    pub accept_cost: f32,
}

impl Default for StitchOptions {
    fn default() -> Self {
        StitchOptions { auto_overlap: true, max_overlap_fraction: 0.25, accept_cost: 10.0 }
    }
}

fn luma(p: &image::Rgb<u8>) -> i32 {
    (i32::from(p.0[0]) * 299 + i32::from(p.0[1]) * 587 + i32::from(p.0[2]) * 114) / 1000
}

/// Width in pixels of the strip at the right edge of `a` that equals the left edge of `b`
/// (same height assumed); 0 when none is convincing.
pub fn find_overlap(a: &RgbImage, b: &RgbImage, opts: &StitchOptions) -> u32 {
    let h = a.height().min(b.height());
    let (wa, wb) = (a.width(), b.width());
    let max_k = ((wa.min(wb) as f32 * opts.max_overlap_fraction) as u32).min(wa.min(wb) / 2);
    if h < 4 || max_k < 8 {
        return 0;
    }
    let row_step = (h / 160).max(1);
    let mut best: Option<(f32, u32)> = None;
    for k in 8..=max_k {
        let col_step = (k / 48).max(1);
        let (mut sum, mut n) = (0i64, 0i64);
        let mut y = 0;
        while y < h {
            let mut i = 0;
            while i < k {
                sum +=
                    i64::from((luma(a.get_pixel(wa - k + i, y)) - luma(b.get_pixel(i, y))).abs());
                n += 1;
                i += col_step;
            }
            y += row_step;
        }
        let cost = sum as f32 / n.max(1) as f32;
        if best.is_none_or(|(c, _)| cost < c) {
            best = Some((cost, k));
        }
    }
    let Some((cost, k)) = best else { return 0 };
    if cost > opts.accept_cost {
        return 0;
    }
    // A uniform strip matches itself at any width: demand some contrast in it.
    let (mut sum, mut sq, mut n) = (0f64, 0f64, 0f64);
    for y in (0..h).step_by(row_step as usize) {
        for i in (0..k).step_by(((k / 48).max(1)) as usize) {
            let v = f64::from(luma(a.get_pixel(wa - k + i, y)));
            sum += v;
            sq += v * v;
            n += 1.0;
        }
    }
    let var = sq / n - (sum / n) * (sum / n);
    if var.sqrt() < 8.0 { 0 } else { k }
}

fn stitch_left_right(a: &RgbImage, b: &RgbImage, opts: &StitchOptions) -> RgbImage {
    let b = if b.height() == a.height() {
        b.clone()
    } else {
        let w = ((b.width() as f32 * a.height() as f32 / b.height().max(1) as f32).round() as u32)
            .max(1);
        imageops::resize(b, w, a.height(), FilterType::CatmullRom)
    };
    let k = if opts.auto_overlap { find_overlap(a, &b, opts) } else { 0 };
    let (wa, wb, h) = (a.width(), b.width(), a.height());
    let mut out = RgbImage::new(wa + wb - k, h);
    imageops::replace(&mut out, a, 0, 0);
    imageops::replace(&mut out, &b, i64::from(wa - k), 0);
    // Feather the seam.
    for i in 0..k {
        let t = (i as f32 + 0.5) / k as f32;
        for y in 0..h {
            let pa = a.get_pixel(wa - k + i, y).0;
            let pb = b.get_pixel(i, y).0;
            let mut px = [0u8; 3];
            for c in 0..3 {
                px[c] = (f32::from(pa[c]) * (1.0 - t) + f32::from(pb[c]) * t).round() as u8;
            }
            out.put_pixel(wa - k + i, y, image::Rgb(px));
        }
    }
    out
}

/// Join two scans into one image.
pub fn stitch(a: &RgbImage, b: &RgbImage, dir: StitchDirection, opts: &StitchOptions) -> RgbImage {
    match dir {
        StitchDirection::LeftRight => stitch_left_right(a, b, opts),
        StitchDirection::TopBottom => {
            // Turn both scans so that "below" becomes "right", join, turn back.
            let (ra, rb) = (imageops::rotate270(a), imageops::rotate270(b));
            imageops::rotate90(&stitch_left_right(&ra, &rb, opts))
        }
    }
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;

    /// A reproducible, non-repeating texture.
    fn texture(w: u32, h: u32) -> RgbImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let mut s =
                (x / 3).wrapping_mul(73856093) ^ (y / 3).wrapping_mul(19349663) ^ 0x9E37_79B9;
            s = s.wrapping_mul(2654435761);
            let v = (s >> 24) as u8;
            *p = Rgb([v, v.wrapping_mul(3), 255 - v]);
        }
        img
    }

    fn max_diff(a: &RgbImage, b: &RgbImage) -> u8 {
        a.pixels()
            .zip(b.pixels())
            .map(|(p, q)| (0..3).map(|c| p.0[c].abs_diff(q.0[c])).max().unwrap())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn overlapping_halves_are_joined_exactly() {
        let whole = texture(300, 120);
        let a = imageops::crop_imm(&whole, 0, 0, 180, 120).to_image();
        let b = imageops::crop_imm(&whole, 140, 0, 160, 120).to_image(); // 40 px overlap
        assert_eq!(
            find_overlap(
                &a,
                &b,
                &StitchOptions { max_overlap_fraction: 0.4, ..Default::default() }
            ),
            40
        );
        let s = stitch(
            &a,
            &b,
            StitchDirection::LeftRight,
            &StitchOptions { max_overlap_fraction: 0.4, ..Default::default() },
        );
        assert_eq!(s.dimensions(), (300, 120));
        assert!(max_diff(&s, &whole) <= 1, "stitched image equals the original");
    }

    #[test]
    fn top_bottom_uses_the_same_logic() {
        let whole = texture(120, 300);
        let a = imageops::crop_imm(&whole, 0, 0, 120, 180).to_image();
        let b = imageops::crop_imm(&whole, 0, 140, 120, 160).to_image();
        let o = StitchOptions { max_overlap_fraction: 0.4, ..Default::default() };
        let s = stitch(&a, &b, StitchDirection::TopBottom, &o);
        assert_eq!(s.dimensions(), (120, 300));
        assert!(max_diff(&s, &whole) <= 1);
    }

    #[test]
    fn scans_without_a_common_strip_are_placed_side_by_side() {
        let a = texture(100, 60);
        let b = {
            let mut t = texture(80, 60);
            for p in t.pixels_mut() {
                p.0 = [255 - p.0[0], p.0[2], p.0[1]];
            }
            t
        };
        let s = stitch(&a, &b, StitchDirection::LeftRight, &StitchOptions::default());
        assert_eq!(s.dimensions(), (180, 60));
        assert_eq!(s.get_pixel(99, 10), a.get_pixel(99, 10));
        assert_eq!(s.get_pixel(100, 10), b.get_pixel(0, 10));
        let off = StitchOptions { auto_overlap: false, ..StitchOptions::default() };
        assert_eq!(stitch(&a, &a, StitchDirection::LeftRight, &off).width(), 200);
    }

    #[test]
    fn blank_margins_are_not_mistaken_for_an_overlap() {
        let white = |w| RgbImage::from_pixel(w, 80, Rgb([250, 250, 250]));
        assert_eq!(find_overlap(&white(100), &white(100), &StitchOptions::default()), 0);
    }

    #[test]
    fn different_heights_are_scaled_to_match() {
        let a = texture(100, 80);
        let b = texture(50, 40); // half the height
        let s = stitch(
            &a,
            &b,
            StitchDirection::LeftRight,
            &StitchOptions { auto_overlap: false, ..Default::default() },
        );
        assert_eq!(s.dimensions(), (200, 80));
    }

    #[test]
    fn tiny_inputs_do_not_panic() {
        let a = RgbImage::new(3, 2);
        assert_eq!(find_overlap(&a, &a, &StitchOptions::default()), 0);
        assert_eq!(
            stitch(&a, &a, StitchDirection::LeftRight, &StitchOptions::default()).width(),
            6
        );
    }
}
