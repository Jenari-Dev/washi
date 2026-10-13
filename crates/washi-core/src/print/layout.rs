// SPDX-License-Identifier: GPL-2.0-or-later
//! Page layout: n pages (or photos) on one sheet, and fitting one page into a cell.

use crate::error::{Error, Result};

/// "Page Layout": pages per sheet.
pub const LAYOUTS: [u8; 6] = [1, 2, 4, 6, 9, 16];

pub fn check_layout(n: u8) -> Result<u8> {
    if LAYOUTS.contains(&n) {
        Ok(n)
    } else {
        Err(Error::Invalid(format!(
            "{n} pages per sheet is not offered (choose 1, 2, 4, 6, 9 or 16)"
        )))
    }
}

/// Columns and rows for `n` per sheet. Two and six per sheet follow the sheet's orientation
/// (portrait sheets stack the pages, landscape sheets put them side by side).
pub fn grid(n: u8, sheet_portrait: bool) -> (u32, u32) {
    match (n, sheet_portrait) {
        (2, true) => (1, 2),
        (2, false) => (2, 1),
        (4, _) => (2, 2),
        (6, true) => (2, 3),
        (6, false) => (3, 2),
        (9, _) => (3, 3),
        (16, _) => (4, 4),
        _ => (1, 1),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// The cells of a `cols` x `rows` grid on a sheet, in reading order (left to right, then down).
/// `gutter` is the gap between cells and around the grid, in pixels.
pub fn cells(sheet_w: u32, sheet_h: u32, cols: u32, rows: u32, gutter: u32) -> Vec<Rect> {
    let cw = sheet_w.saturating_sub(gutter * (cols + 1)) / cols.max(1);
    let ch = sheet_h.saturating_sub(gutter * (rows + 1)) / rows.max(1);
    // Centre the grid in what is left over.
    let used_w = cw * cols + gutter * (cols + 1);
    let used_h = ch * rows + gutter * (rows + 1);
    let (ox, oy) = ((sheet_w - used_w.min(sheet_w)) / 2, (sheet_h - used_h.min(sheet_h)) / 2);
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            out.push(Rect {
                x: ox + gutter + c * (cw + gutter),
                y: oy + gutter + r * (ch + gutter),
                w: cw.max(1),
                h: ch.max(1),
            });
        }
    }
    out
}

/// How to put a `src_w` x `src_h` source into a cell, as large as fits and centred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// Turn the source by 90 degrees clockwise first.
    pub rotate: bool,
    /// Scale applied to the (rotated) source.
    pub scale: f64,
    /// Size of the placed source in cell pixels.
    pub w: u32,
    pub h: u32,
    /// Top-left corner on the sheet.
    pub x: u32,
    pub y: u32,
}

/// Fit with the source turned if that makes it bigger (`allow_rotate`).
pub fn fit(src_w: f64, src_h: f64, cell: Rect, allow_rotate: bool) -> Fit {
    let s0 = (f64::from(cell.w) / src_w.max(1e-9)).min(f64::from(cell.h) / src_h.max(1e-9));
    let s1 = (f64::from(cell.w) / src_h.max(1e-9)).min(f64::from(cell.h) / src_w.max(1e-9));
    // Keep the upright orientation unless turning wins clearly.
    let rotate = allow_rotate && s1 > s0 * 1.01;
    let (sw, sh, scale) = if rotate { (src_h, src_w, s1) } else { (src_w, src_h, s0) };
    let (w, h) = (
        ((sw * scale).round() as u32).clamp(1, cell.w),
        ((sh * scale).round() as u32).clamp(1, cell.h),
    );
    Fit { rotate, scale, w, h, x: cell.x + (cell.w - w) / 2, y: cell.y + (cell.h - h) / 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_the_canon_print_choices() {
        for n in LAYOUTS {
            assert!(check_layout(n).is_ok());
            let (c, r) = grid(n, true);
            assert_eq!((c * r) as u8, n, "{n} up");
            let (c, r) = grid(n, false);
            assert_eq!((c * r) as u8, n);
        }
        assert!(check_layout(3).is_err() && check_layout(0).is_err() && check_layout(8).is_err());
        assert_eq!(grid(2, true), (1, 2));
        assert_eq!(grid(2, false), (2, 1));
        assert_eq!(grid(6, true), (2, 3));
    }

    #[test]
    fn cells_tile_the_sheet_in_reading_order() {
        let c = cells(1000, 1400, 2, 2, 20);
        assert_eq!(c.len(), 4);
        assert!(c[0].x < c[1].x && c[0].y == c[1].y && c[2].y > c[0].y);
        for r in &c {
            assert!(r.x + r.w <= 1000 && r.y + r.h <= 1400);
        }
        // No overlaps.
        for (i, a) in c.iter().enumerate() {
            for b in c.iter().skip(i + 1) {
                let disjoint =
                    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(disjoint, "{a:?} overlaps {b:?}");
            }
        }
        assert_eq!(cells(100, 100, 1, 1, 0), [Rect { x: 0, y: 0, w: 100, h: 100 }]);
    }

    #[test]
    fn fitting_centres_and_rotates_when_it_helps() {
        let cell = Rect { x: 0, y: 0, w: 1000, h: 1400 };
        // A page of the same aspect fills the cell.
        let f = fit(500.0, 700.0, cell, true);
        assert!(!f.rotate && (f.w, f.h) == (1000, 1400) && (f.x, f.y) == (0, 0));
        // A landscape page on a portrait cell is turned.
        let f = fit(700.0, 500.0, cell, true);
        assert!(f.rotate && (f.w, f.h) == (1000, 1400), "{f:?}");
        let f = fit(700.0, 500.0, cell, false);
        assert!(!f.rotate && f.w == 1000 && f.h == 714 && f.y == (1400 - 714) / 2, "{f:?}");
        // A square source in a portrait cell keeps its orientation (rotating gains nothing).
        assert!(!fit(100.0, 100.0, cell, true).rotate);
        // Offsets include the cell origin.
        let f = fit(100.0, 100.0, Rect { x: 50, y: 60, w: 200, h: 100 }, false);
        assert_eq!((f.x, f.y, f.w, f.h), (100, 60, 100, 100));
    }
}
