// SPDX-License-Identifier: GPL-2.0-or-later
//! Finding things on the platen: the document or photo(s) on the glass (auto crop, multi-photo
//! crop) and whether a scan is a document or a photo (IJ Scan Utility's "Auto Scan").
//!
//! Detection works on a reduced copy (<= 800 px): the background colour is read from the
//! border of the scan, pixels that differ from it are the foreground, small gaps are closed,
//! connected areas become candidate regions and each one is described by its minimum-area
//! rotated rectangle, so a crooked photo is cut out and straightened in one step.

use image::imageops::{self, FilterType};
use image::{Rgb, RgbImage};
use serde::Serialize;

/// A rectangle with a rotation, in pixels of the image it was found in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RotatedRect {
    pub cx: f32,
    pub cy: f32,
    /// Extent along the rectangle's own x axis / y axis.
    pub w: f32,
    pub h: f32,
    /// Clockwise rotation of the rectangle's x axis, in degrees, within (-45, 45].
    pub angle: f32,
}

impl RotatedRect {
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    pub fn corners(&self) -> [(f32, f32); 4] {
        let (s, c) = self.angle.to_radians().sin_cos();
        let (hw, hh) = (self.w / 2.0, self.h / 2.0);
        let p = |lx: f32, ly: f32| (self.cx + lx * c - ly * s, self.cy + lx * s + ly * c);
        [p(-hw, -hh), p(hw, -hh), p(hw, hh), p(-hw, hh)]
    }

    /// Shrink every side by `fraction` of the shorter side.
    pub fn inset(&self, fraction: f32) -> RotatedRect {
        let d = self.w.min(self.h) * fraction;
        RotatedRect { w: (self.w - 2.0 * d).max(1.0), h: (self.h - 2.0 * d).max(1.0), ..*self }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DetectOptions {
    /// Smallest region, as a fraction of the whole scan.
    pub min_area_fraction: f32,
    pub max_regions: usize,
    /// Shrink each region by this fraction of its shorter side (drops shadow and border).
    pub inset_fraction: f32,
    /// Difference from the background that counts as foreground (0..=255); `None` = automatic.
    pub threshold: Option<u8>,
}

impl Default for DetectOptions {
    fn default() -> Self {
        DetectOptions {
            min_area_fraction: 0.02,
            max_regions: 16,
            inset_fraction: 0.01,
            threshold: None,
        }
    }
}

const WORK_EDGE: f32 = 800.0;

fn median(mut v: Vec<u8>) -> u8 {
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(255)
}

/// Median colour of the outer 3 px ring.
fn border_colour(img: &RgbImage) -> [u8; 3] {
    let (w, h) = img.dimensions();
    let mut ch: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for y in 0..h {
        for x in 0..w {
            if x < 3 || y < 3 || x + 3 >= w || y + 3 >= h {
                let p = img.get_pixel(x, y).0;
                for c in 0..3 {
                    ch[c].push(p[c]);
                }
            }
        }
    }
    [
        median(std::mem::take(&mut ch[0])),
        median(std::mem::take(&mut ch[1])),
        median(std::mem::take(&mut ch[2])),
    ]
}

fn otsu_u8(values: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for v in values {
        hist[usize::from(*v)] += 1;
    }
    let total: u64 = hist.iter().sum();
    let sum_all: f64 = hist.iter().enumerate().map(|(i, c)| i as f64 * *c as f64).sum();
    let (mut w_b, mut sum_b, mut best, mut thr) = (0u64, 0f64, -1f64, 20u8);
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

/// Max over a (2r+1) window, separable; `grow` = dilate, otherwise erode.
fn morph(mask: &[bool], w: usize, h: usize, r: usize, grow: bool) -> Vec<bool> {
    let pass = |src: &[bool], horizontal: bool| -> Vec<bool> {
        let mut out = vec![!grow; w * h];
        for y in 0..h {
            for x in 0..w {
                let (lo, hi) = if horizontal {
                    (x.saturating_sub(r), (x + r).min(w - 1))
                } else {
                    (y.saturating_sub(r), (y + r).min(h - 1))
                };
                let mut v = !grow;
                for k in lo..=hi {
                    let s = if horizontal { src[y * w + k] } else { src[k * w + x] };
                    if grow && s {
                        v = true;
                        break;
                    }
                    if !grow && !s {
                        v = false;
                        break;
                    }
                }
                out[y * w + x] = v;
            }
        }
        out
    };
    let a = pass(mask, true);
    pass(&a, false)
}

struct Component {
    pixels: usize,
    /// Extreme points: leftmost and rightmost of each row, topmost and bottom-most of each column.
    extremes: Vec<(f32, f32)>,
}

fn components(mask: &[bool], w: usize, h: usize) -> Vec<Component> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..w * h {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut pixels = 0usize;
        let mut row_min: std::collections::BTreeMap<usize, (usize, usize)> = Default::default();
        let mut col_min: std::collections::BTreeMap<usize, (usize, usize)> = Default::default();
        while let Some(i) = stack.pop() {
            pixels += 1;
            let (x, y) = (i % w, i / w);
            let r = row_min.entry(y).or_insert((x, x));
            r.0 = r.0.min(x);
            r.1 = r.1.max(x);
            let c = col_min.entry(x).or_insert((y, y));
            c.0 = c.0.min(y);
            c.1 = c.1.max(y);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    if mask[n] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        let mut extremes = Vec::with_capacity((row_min.len() + col_min.len()) * 2);
        for (y, (a, b)) in &row_min {
            extremes.push((*a as f32, *y as f32));
            extremes.push((*b as f32 + 1.0, *y as f32 + 1.0));
            extremes.push((*a as f32, *y as f32 + 1.0));
            extremes.push((*b as f32 + 1.0, *y as f32));
        }
        for (x, (a, b)) in &col_min {
            extremes.push((*x as f32, *a as f32));
            extremes.push((*x as f32 + 1.0, *b as f32 + 1.0));
            extremes.push((*x as f32 + 1.0, *a as f32));
            extremes.push((*x as f32, *b as f32 + 1.0));
        }
        out.push(Component { pixels, extremes });
    }
    out
}

fn cross(o: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

/// Andrew's monotone chain.
fn convex_hull(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut p: Vec<(f32, f32)> = points.to_vec();
    p.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let mut hull: Vec<(f32, f32)> = Vec::new();
    for &pt in &p {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], pt) <= 0.0 {
            hull.pop();
        }
        hull.push(pt);
    }
    let lower = hull.len() + 1;
    for &pt in p.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], pt) <= 0.0 {
            hull.pop();
        }
        hull.push(pt);
    }
    hull.pop();
    hull
}

fn polygon_area(h: &[(f32, f32)]) -> f32 {
    let mut a = 0.0;
    for i in 0..h.len() {
        let (p, q) = (h[i], h[(i + 1) % h.len()]);
        a += p.0 * q.1 - q.0 * p.1;
    }
    a.abs() / 2.0
}

/// Minimum-area bounding rectangle of a convex polygon.
fn min_area_rect(hull: &[(f32, f32)]) -> Option<RotatedRect> {
    if hull.len() < 3 {
        return None;
    }
    let mut best: Option<(f32, RotatedRect)> = None;
    for i in 0..hull.len() {
        let (p, q) = (hull[i], hull[(i + 1) % hull.len()]);
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-3 {
            continue;
        }
        let (ux, uy) = (dx / len, dy / len); // along the edge
        let (vx, vy) = (-uy, ux); // perpendicular
        let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &(x, y) in hull {
            let (u, v) = (x * ux + y * uy, x * vx + y * vy);
            u0 = u0.min(u);
            u1 = u1.max(u);
            v0 = v0.min(v);
            v1 = v1.max(v);
        }
        let (w, h) = (u1 - u0, v1 - v0);
        let area = w * h;
        if best.as_ref().is_none_or(|(a, _)| area < *a) {
            let (cu, cv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
            let (cx, cy) = (cu * ux + cv * vx, cu * uy + cv * vy);
            let mut angle = uy.atan2(ux).to_degrees();
            let (mut rw, mut rh) = (w, h);
            // Normalise the angle into (-45, 45] by swapping the axes.
            while angle > 45.0 {
                angle -= 90.0;
                std::mem::swap(&mut rw, &mut rh);
            }
            while angle <= -45.0 {
                angle += 90.0;
                std::mem::swap(&mut rw, &mut rh);
            }
            best = Some((area, RotatedRect { cx, cy, w: rw, h: rh, angle }));
        }
    }
    best.map(|(_, r)| r)
}

/// Find the regions that stand out from the scan's background, biggest first by position in
/// reading order. Coordinates are in pixels of `img`.
pub fn detect_regions(img: &RgbImage, opts: &DetectOptions) -> Vec<RotatedRect> {
    let (iw, ih) = img.dimensions();
    if iw < 8 || ih < 8 {
        return Vec::new();
    }
    let scale = (WORK_EDGE / iw.max(ih) as f32).min(1.0);
    let small = if scale < 1.0 {
        imageops::resize(
            img,
            ((iw as f32 * scale).round() as u32).max(8),
            ((ih as f32 * scale).round() as u32).max(8),
            FilterType::Triangle,
        )
    } else {
        img.clone()
    };
    let small = imageops::blur(&small, 1.0);
    let (w, h) = (small.width() as usize, small.height() as usize);
    let bg = border_colour(&small);
    let diff: Vec<u8> =
        small.pixels().map(|p| (0..3).map(|c| p.0[c].abs_diff(bg[c])).max().unwrap_or(0)).collect();
    let thr = opts.threshold.unwrap_or_else(|| otsu_u8(&diff).clamp(14, 70));
    let mask: Vec<bool> = diff.iter().map(|d| *d > thr).collect();
    let close_r = (w.max(h) / 150).max(2);
    let mask = morph(&morph(&mask, w, h, close_r, true), w, h, close_r, false);

    let total = (w * h) as f32;
    let mut found: Vec<(usize, RotatedRect)> = Vec::new();
    for comp in components(&mask, w, h) {
        if (comp.pixels as f32) < opts.min_area_fraction * total {
            continue;
        }
        let hull = convex_hull(&comp.extremes);
        // Thin or ragged shapes (noise, a long streak) are not photos.
        if polygon_area(&hull) < 0.5 * 1.0 || (comp.pixels as f32) < 0.45 * polygon_area(&hull) {
            continue;
        }
        let Some(r) = min_area_rect(&hull) else { continue };
        let r = RotatedRect {
            cx: r.cx / scale,
            cy: r.cy / scale,
            w: r.w / scale,
            h: r.h / scale,
            angle: r.angle,
        }
        .inset(opts.inset_fraction);
        found.push((comp.pixels, r));
    }
    found.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    found.truncate(opts.max_regions);
    let mut regions: Vec<RotatedRect> = found.into_iter().map(|(_, r)| r).collect();
    // Reading order: rows of regions top to bottom, each row left to right.
    regions.sort_by(|a, b| a.cy.partial_cmp(&b.cy).unwrap_or(std::cmp::Ordering::Equal));
    let mut ordered: Vec<RotatedRect> = Vec::new();
    while !regions.is_empty() {
        let first = regions.remove(0);
        let band = first.h.min(first.w) * 0.5;
        let mut row = vec![first];
        let mut i = 0;
        while i < regions.len() {
            if (regions[i].cy - first.cy).abs() < band {
                row.push(regions.remove(i));
            } else {
                i += 1;
            }
        }
        row.sort_by(|a, b| a.cx.partial_cmp(&b.cx).unwrap_or(std::cmp::Ordering::Equal));
        ordered.extend(row);
    }
    ordered
}

fn sample_clamped(img: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
    let (w, h) = img.dimensions();
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (a, b, c, d) = (
        img.get_pixel(x0, y0).0,
        img.get_pixel(x1, y0).0,
        img.get_pixel(x0, y1).0,
        img.get_pixel(x1, y1).0,
    );
    let mut out = [0u8; 3];
    for i in 0..3 {
        let top = f32::from(a[i]) * (1.0 - fx) + f32::from(b[i]) * fx;
        let bot = f32::from(c[i]) * (1.0 - fx) + f32::from(d[i]) * fx;
        out[i] = (top * (1.0 - fy) + bot * fy).round() as u8;
    }
    Rgb(out)
}

/// Cut a (possibly rotated) rectangle out of `img` and return it upright.
pub fn extract_region(img: &RgbImage, r: &RotatedRect) -> RgbImage {
    let (ow, oh) = ((r.w.round() as u32).max(1), (r.h.round() as u32).max(1));
    if r.angle.abs() < 0.25 {
        let x = (r.cx - r.w / 2.0).round().max(0.0) as u32;
        let y = (r.cy - r.h / 2.0).round().max(0.0) as u32;
        let w = ow.min(img.width().saturating_sub(x)).max(1);
        let h = oh.min(img.height().saturating_sub(y)).max(1);
        return imageops::crop_imm(img, x.min(img.width() - 1), y.min(img.height() - 1), w, h)
            .to_image();
    }
    let (s, c) = r.angle.to_radians().sin_cos();
    let mut out = RgbImage::new(ow, oh);
    for v in 0..oh {
        let ly = v as f32 + 0.5 - oh as f32 / 2.0;
        for u in 0..ow {
            let lx = u as f32 + 0.5 - ow as f32 / 2.0;
            let x = r.cx + lx * c - ly * s - 0.5;
            let y = r.cy + lx * s + ly * c - 0.5;
            out.put_pixel(u, v, sample_clamped(img, x, y));
        }
    }
    out
}

/// The most prominent object on the platen, cut out and straightened ("Auto" original size).
/// `None` when nothing stands out from the background.
pub fn auto_crop(img: &RgbImage) -> Option<RgbImage> {
    let regions =
        detect_regions(img, &DetectOptions { max_regions: 1, ..DetectOptions::default() });
    let best = regions
        .into_iter()
        .max_by(|a, b| a.area().partial_cmp(&b.area()).unwrap_or(std::cmp::Ordering::Equal))?;
    Some(extract_region(img, &best))
}

/// Every photo on the platen, each cut out and straightened, in reading order.
pub fn crop_photos(img: &RgbImage) -> Vec<RgbImage> {
    detect_regions(img, &DetectOptions::default()).iter().map(|r| extract_region(img, r)).collect()
}

/// What a scan shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ScanKind {
    Blank,
    Document,
    Photo,
}

/// Document, photo or blank?
///
/// A text page is mostly paper with thin strokes: nearly every ink pixel has paper within two
/// pixels of it. A photo (alone or on the glass) is made of big solid areas whose pixels are
/// far from any paper. So: of the pixels that are not paper, how many are "interior"? Few
/// (under 40 %) and little saturated colour means a document.
pub fn classify(img: &RgbImage) -> ScanKind {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return ScanKind::Blank;
    }
    // Detail must survive the reduction: 1600 px on the long edge keeps 300 dpi text apart.
    let scale = (1600.0 / w.max(h) as f32).min(1.0);
    let small = if scale < 1.0 {
        imageops::resize(
            img,
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
            FilterType::Triangle,
        )
    } else {
        img.clone()
    };
    let (sw, sh) = (small.width() as usize, small.height() as usize);
    let mut paper = vec![false; sw * sh];
    let (mut nonwhite, mut colour) = (0u64, 0u64);
    for (i, p) in small.pixels().enumerate() {
        let [r, g, b] = p.0;
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        let luma = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
        if luma > 195 && mx - mn < 40 {
            paper[i] = true;
        } else {
            nonwhite += 1;
            if mx - mn > 60 {
                colour += 1;
            }
        }
    }
    if (nonwhite as f64) < 0.005 * (sw * sh) as f64 {
        return ScanKind::Blank;
    }
    // Pixels with no paper within two pixels (the dilated paper does not reach them).
    let near_paper = morph(&paper, sw, sh, 2, true);
    let interior =
        near_paper.iter().zip(&paper).filter(|(near, is_paper)| !**near && !**is_paper).count();
    let nw = nonwhite as f64;
    if (interior as f64 / nw) < 0.4 && (colour as f64 / nw) < 0.3 {
        ScanKind::Document
    } else {
        ScanKind::Photo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::ops::{rotate_degrees, tests_support::text_page};

    fn white(w: u32, h: u32) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb([250, 250, 250]))
    }

    /// A "photo": a smooth colour gradient (so it has mid-tones and colour).
    fn photo(w: u32, h: u32) -> RgbImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([
                (40 + x * 150 / w) as u8,
                (60 + y * 120 / h) as u8,
                (180 - x * 100 / w) as u8,
            ]);
        }
        img
    }

    fn paste(dst: &mut RgbImage, src: &RgbImage, x: u32, y: u32) {
        imageops::overlay(dst, src, i64::from(x), i64::from(y));
    }

    #[test]
    fn finds_one_upright_photo() {
        let mut scan = white(800, 1000);
        paste(&mut scan, &photo(300, 200), 100, 150);
        let regions = detect_regions(
            &scan,
            &DetectOptions { inset_fraction: 0.0, ..DetectOptions::default() },
        );
        assert_eq!(regions.len(), 1);
        let r = regions[0];
        assert!((r.cx - 250.0).abs() < 6.0 && (r.cy - 250.0).abs() < 6.0, "{r:?}");
        assert!((r.w - 300.0).abs() < 10.0 && (r.h - 200.0).abs() < 10.0, "{r:?}");
        assert!(r.angle.abs() < 1.5);
        let out = auto_crop(&scan).unwrap();
        // 1 % inset on each side.
        assert!(
            (out.width() as i32 - 294).abs() < 12 && (out.height() as i32 - 196).abs() < 12,
            "{:?}",
            out.dimensions()
        );
    }

    #[test]
    fn finds_several_photos_in_reading_order_and_straightens_a_crooked_one() {
        let mut scan = white(1000, 800);
        let a = photo(260, 180);
        let b = rotate_degrees(
            &{
                let mut c = white(360, 300);
                paste(&mut c, &photo(240, 160), 60, 70);
                c
            },
            12.0,
            Rgb([250, 250, 250]),
        );
        let c = photo(200, 200);
        paste(&mut scan, &c, 600, 450); // bottom right
        paste(&mut scan, &a, 60, 60); // top left
        paste(&mut scan, &b, 480, 20); // top right, crooked
        let regions = detect_regions(&scan, &DetectOptions::default());
        assert_eq!(regions.len(), 3, "{regions:?}");
        assert!(
            regions[0].cx < regions[1].cx && regions[1].cy < regions[2].cy,
            "reading order: {regions:?}"
        );
        let crooked = regions[1];
        assert!((crooked.angle.abs() - 12.0).abs() < 3.0, "angle {}", crooked.angle);
        assert!((crooked.w.max(crooked.h) - 240.0).abs() < 25.0, "{crooked:?}");
        let photos = crop_photos(&scan);
        assert_eq!(photos.len(), 3);
        // The straightened crop of the crooked photo is upright: its left column is the left
        // colour of the gradient everywhere (no white wedge in the corners).
        let up = &photos[1];
        let corner = up.get_pixel(3, 3).0;
        assert!(corner.iter().any(|c| *c < 200), "corner is photo, not background: {corner:?}");
    }

    #[test]
    fn works_on_a_dark_lid_too() {
        let mut scan = RgbImage::from_pixel(600, 600, Rgb([12, 12, 14]));
        paste(&mut scan, &photo(250, 300), 200, 150);
        let regions = detect_regions(&scan, &DetectOptions::default());
        assert_eq!(regions.len(), 1);
        assert!((regions[0].w.min(regions[0].h) - 250.0).abs() < 15.0, "{:?}", regions[0]);
    }

    #[test]
    fn empty_scans_and_specks_give_nothing() {
        assert!(detect_regions(&white(500, 500), &DetectOptions::default()).is_empty());
        let mut speck = white(500, 500);
        for x in 100..104 {
            for y in 100..104 {
                speck.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        assert!(detect_regions(&speck, &DetectOptions::default()).is_empty());
        assert!(auto_crop(&white(300, 300)).is_none());
        assert!(detect_regions(&RgbImage::new(2, 2), &DetectOptions::default()).is_empty());
    }

    #[test]
    fn rotated_rectangle_geometry() {
        let r = RotatedRect { cx: 10.0, cy: 10.0, w: 8.0, h: 4.0, angle: 0.0 };
        assert_eq!(r.corners()[0], (6.0, 8.0));
        assert_eq!(r.area(), 32.0);
        let i = r.inset(0.25);
        assert_eq!((i.w, i.h), (6.0, 2.0));
        let q = RotatedRect { cx: 0.0, cy: 0.0, w: 2.0, h: 2.0, angle: 90.0 };
        let c = q.corners();
        assert!((c[0].0 - 1.0).abs() < 1e-5 && (c[0].1 + 1.0).abs() < 1e-5);
    }

    #[test]
    fn min_area_rect_of_a_tilted_square() {
        let (s, c) = 20f32.to_radians().sin_cos();
        let pts: Vec<(f32, f32)> = [(-50.0, -50.0), (50.0, -50.0), (50.0, 50.0), (-50.0, 50.0)]
            .iter()
            .map(|(x, y): &(f32, f32)| (200.0 + x * c - y * s, 150.0 + x * s + y * c))
            .collect();
        let r = min_area_rect(&convex_hull(&pts)).unwrap();
        assert!((r.w - 100.0).abs() < 0.5 && (r.h - 100.0).abs() < 0.5, "{r:?}");
        assert!((r.angle - 20.0).abs() < 0.5, "{r:?}");
        assert!((r.cx - 200.0).abs() < 0.5 && (r.cy - 150.0).abs() < 0.5);
        assert!(min_area_rect(&[(0.0, 0.0), (1.0, 1.0)]).is_none());
        assert_eq!(
            convex_hull(&[(0.0, 0.0), (1.0, 0.0), (0.5, 0.2), (0.5, 0.0), (1.0, 1.0), (0.0, 1.0)])
                .len(),
            4
        );
    }

    #[test]
    fn extraction_of_an_upright_and_a_rotated_region() {
        let mut img = white(200, 200);
        paste(&mut img, &photo(100, 60), 40, 70);
        let up = extract_region(
            &img,
            &RotatedRect { cx: 90.0, cy: 100.0, w: 100.0, h: 60.0, angle: 0.0 },
        );
        assert_eq!(up.dimensions(), (100, 60));
        assert_eq!(up.get_pixel(0, 0), img.get_pixel(40, 70));
        let rot = extract_region(
            &img,
            &RotatedRect { cx: 90.0, cy: 100.0, w: 100.0, h: 60.0, angle: 5.0 },
        );
        assert_eq!(rot.dimensions(), (100, 60));
    }

    #[test]
    fn classification() {
        assert_eq!(classify(&text_page(500, 700)), ScanKind::Document);
        assert_eq!(classify(&white(400, 400)), ScanKind::Blank);
        assert_eq!(classify(&photo(400, 300)), ScanKind::Photo);
        let mut platen = white(600, 800);
        paste(&mut platen, &photo(300, 400), 100, 100);
        assert_eq!(classify(&platen), ScanKind::Photo, "a photo on a white lid is still a photo");
        assert_eq!(classify(&RgbImage::new(0, 0)), ScanKind::Blank);
    }
}
