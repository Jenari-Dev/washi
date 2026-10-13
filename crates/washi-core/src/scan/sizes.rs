// SPDX-License-Identifier: GPL-2.0-or-later
//! "Original Size" choices of the Scan screen and their scan regions.

use serde::Serialize;
use washi_net::escl::{ScanRegion, SourceCaps, UNITS_PER_INCH, mm_to_units};

use crate::error::{Error, Result};

/// A fixed original size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct OriginalSize {
    pub key: &'static str,
    pub label: &'static str,
    pub width_mm: f64,
    pub height_mm: f64,
}

pub const SIZES: [OriginalSize; 8] = [
    OriginalSize { key: "a4", label: "A4", width_mm: 210.0, height_mm: 297.0 },
    OriginalSize { key: "letter", label: "Letter", width_mm: 215.9, height_mm: 279.4 },
    OriginalSize { key: "a5", label: "A5", width_mm: 148.0, height_mm: 210.0 },
    OriginalSize { key: "b5", label: "B5", width_mm: 182.0, height_mm: 257.0 },
    OriginalSize { key: "l", label: "L (89 x 127 mm)", width_mm: 89.0, height_mm: 127.0 },
    OriginalSize { key: "2l", label: "2L (127 x 178 mm)", width_mm: 127.0, height_mm: 178.0 },
    OriginalSize { key: "postcard", label: "Postcard", width_mm: 100.0, height_mm: 148.0 },
    OriginalSize { key: "card", label: "Business card", width_mm: 55.0, height_mm: 91.0 },
];

/// What to scan: a fixed original or "Auto" (scan the whole platen, then crop on the host).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum SizeChoice {
    Fixed(OriginalSize),
    Auto,
}

impl SizeChoice {
    /// `a4`, `letter`, `a5`, `b5`, `l`, `2l`, `postcard`/`hagaki`, `card`/`business-card`,
    /// `auto` (case-insensitive).
    pub fn parse(s: &str) -> Result<SizeChoice> {
        let k = s.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        let k = match k.as_str() {
            "hagaki" => "postcard",
            "business-card" | "businesscard" => "card",
            "photo-l" => "l",
            other => other,
        };
        if k == "auto" {
            return Ok(SizeChoice::Auto);
        }
        SIZES.iter().find(|z| z.key == k).map(|z| SizeChoice::Fixed(*z)).ok_or_else(|| {
            Error::Invalid(format!(
                "unknown original size {s:?} (choose one of: {}, auto)",
                SIZES.iter().map(|z| z.key).collect::<Vec<_>>().join(", ")
            ))
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            SizeChoice::Fixed(z) => z.label,
            SizeChoice::Auto => "Auto",
        }
    }

    /// The region to request: the original's size at the platen's reference corner, cut to
    /// the scan area; `Auto` is the whole scan area. Without capabilities the XK140's area
    /// (2550 x 3508 units) is assumed.
    pub fn region(&self, platen: Option<&SourceCaps>) -> ScanRegion {
        let (max_w, max_h) = match platen {
            Some(p) if p.max_width > 0 && p.max_height > 0 => (p.max_width, p.max_height),
            _ => (2550, 3508),
        };
        match self {
            SizeChoice::Auto => {
                ScanRegion { x_offset: 0, y_offset: 0, width: max_w, height: max_h }
            }
            SizeChoice::Fixed(z) => {
                let mut r = ScanRegion::from_mm(z.width_mm, z.height_mm);
                r.width = r.width.min(max_w);
                r.height = r.height.min(max_h);
                r
            }
        }
    }
}

/// Inches covered by `units` of scan region.
pub fn units_to_inches(units: u32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_INCH)
}

/// Pixel size a region will have at `dpi`.
pub fn region_pixels(r: &ScanRegion, dpi: u32) -> (u32, u32) {
    let px = |u: u32| (f64::from(u) * f64::from(dpi) / f64::from(UNITS_PER_INCH)).round() as u32;
    (px(r.width), px(r.height))
}

/// A fixed size in region units (for tests and UIs).
pub fn size_units(z: &OriginalSize) -> (u32, u32) {
    (mm_to_units(z.width_mm), mm_to_units(z.height_mm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_is_forgiving_and_lists_the_choices() {
        assert_eq!(SizeChoice::parse("A4").unwrap().label(), "A4");
        assert_eq!(SizeChoice::parse(" Hagaki ").unwrap().label(), "Postcard");
        assert_eq!(SizeChoice::parse("business card").unwrap().label(), "Business card");
        assert_eq!(SizeChoice::parse("2L").unwrap().label(), "2L (127 x 178 mm)");
        assert_eq!(SizeChoice::parse("AUTO").unwrap(), SizeChoice::Auto);
        let e = SizeChoice::parse("a3").unwrap_err().to_string();
        assert!(e.contains("a4") && e.contains("auto"), "{e}");
    }

    #[test]
    fn regions_in_300ths_of_an_inch() {
        let r = SizeChoice::parse("a4").unwrap().region(None);
        assert_eq!((r.x_offset, r.y_offset, r.width, r.height), (0, 0, 2480, 3508));
        let r = SizeChoice::parse("letter").unwrap().region(None);
        assert_eq!((r.width, r.height), (2550, 3300));
        let r = SizeChoice::parse("l").unwrap().region(None);
        assert_eq!((r.width, r.height), (1051, 1500));
        let auto = SizeChoice::Auto.region(None);
        assert_eq!((auto.width, auto.height), (2550, 3508));
        assert_eq!(
            region_pixels(&SizeChoice::parse("a4").unwrap().region(None), 300),
            (2480, 3508)
        );
        assert_eq!(region_pixels(&auto, 600), (5100, 7016));
        assert!((units_to_inches(300) - 1.0).abs() < 1e-9);
        assert_eq!(size_units(&SIZES[0]), (2480, 3508));
    }

    #[test]
    fn regions_are_cut_to_the_scan_area() {
        use washi_net::escl::ScannerCapabilities;
        let caps = ScannerCapabilities::parse(include_str!(
            "../../../washi-net/tests/fixtures/escl-capabilities-xk140.xml"
        ))
        .unwrap();
        let p = caps.platen();
        // A hypothetical A3 would be cut; every real choice fits the XK140.
        let big = SizeChoice::Fixed(OriginalSize {
            key: "a3",
            label: "A3",
            width_mm: 297.0,
            height_mm: 420.0,
        });
        let r = big.region(p);
        assert_eq!((r.width, r.height), (2550, 3508));
        for z in SIZES {
            let r = SizeChoice::Fixed(z).region(p);
            assert_eq!((r.width, r.height), size_units(&z), "{} fits", z.key);
        }
    }
}
