// SPDX-License-Identifier: GPL-2.0-or-later
//! Paper sizes, trays and media types as the XK140's IPP interface names them.

use serde::Serialize;

use crate::error::{Error, Result};

/// A paper size. Dimensions are in 1/100 mm, the unit of IPP `media-size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PaperSize {
    pub key: &'static str,
    pub label: &'static str,
    /// The PWG self-describing media name (also the `PageSizeName` of the raster).
    pub ipp_name: &'static str,
    pub width: i32,
    pub height: i32,
}

macro_rules! paper {
    ($key:expr, $label:expr, $name:expr, $w:expr, $h:expr) => {
        PaperSize { key: $key, label: $label, ipp_name: $name, width: $w, height: $h }
    };
}

/// Every size the XK140's `media-col-database` offers (read from the real printer).
pub const PAPER: &[PaperSize] = &[
    paper!("a4", "A4", "iso_a4_210x297mm", 21000, 29700),
    paper!("a5", "A5", "iso_a5_148x210mm", 14800, 21000),
    paper!("b5", "B5 (JIS)", "jis_b5_182x257mm", 18200, 25700),
    paper!("letter", "Letter", "na_letter_8.5x11in", 21590, 27940),
    paper!("legal", "Legal", "na_legal_8.5x14in", 21590, 35560),
    paper!("l", "L photo", "oe_photo-l_3.5x5in", 8890, 12700),
    paper!("2l", "2L / 5x7 in", "na_5x7_5x7in", 12700, 17780),
    paper!("4x6", "4x6 in", "na_index-4x6_4x6in", 10160, 15240),
    paper!("postcard", "Postcard (Hagaki)", "jpn_hagaki_100x148mm", 10000, 14800),
    paper!("8x10", "8x10 in", "na_govt-letter_8x10in", 20320, 25400),
    paper!("7x10", "7x10 in", "oe_7x10_7x10in", 17780, 25400),
    paper!("square", "Square 5x5 in", "oe_square-photo_5x5in", 12700, 12700),
    paper!("square-89", "Square 89 mm", "om_square-photo_89x89mm", 8900, 8900),
    paper!("card", "Business card", "om_business-card_55x91mm", 5500, 9100),
    paper!("dl", "Envelope DL", "iso_dl_110x220mm", 11000, 22000),
    paper!("env10", "Envelope #10", "na_number-10_4.125x9.5in", 10477, 24130),
    paper!("you4", "Envelope You 4", "jpn_you4_105x235mm", 10500, 23500),
    paper!("you6", "Envelope You 6", "jpn_you6_98x190mm", 9800, 19000),
    paper!("chou3", "Envelope Chou 3", "jpn_chou3_120x235mm", 12000, 23500),
    paper!("chou4", "Envelope Chou 4", "jpn_chou4_90x205mm", 9000, 20500),
];

/// The disc tray's size: `om_square_120x120mm`, 120 x 120 mm (read from the printer's
/// `media-col-database`, `media-ready` and `media-supported`).
pub const DISC_PAPER: PaperSize =
    paper!("disc", "Disc 120 mm", "om_square_120x120mm", 12000, 12000);

/// The margins (1/100 mm) of the disc entries in the media database: 0.1 mm on every side.
pub const DISC_MARGIN: i32 = 10;

/// Margins of the most common bordered entry of the database (A4 plain paper and most others);
/// the real values are taken from the database at print time.
pub const DEFAULT_MARGINS: (i32, i32, i32, i32) = (500, 500, 340, 340); // top, bottom, left, right

impl PaperSize {
    pub fn parse(s: &str) -> Result<&'static PaperSize> {
        let k = s.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        let k = match k.as_str() {
            "hagaki" => "postcard",
            "5x7" | "127x178" => "2l",
            "89x127" | "3.5x5" => "l",
            "4x6in" | "10x15" => "4x6",
            "business-card" => "card",
            "5x5" => "square",
            "jis-b5" => "b5",
            "us-letter" => "letter",
            "envelope-dl" => "dl",
            other => other,
        };
        PAPER.iter().find(|p| p.key == k).ok_or_else(|| {
            Error::Invalid(format!(
                "unknown paper size {s:?} (choose one of: {})",
                PAPER.iter().map(|p| p.key).collect::<Vec<_>>().join(", ")
            ))
        })
    }

    /// Look a size up by its dimensions (1/100 mm), either orientation.
    pub fn by_dimensions(w: i32, h: i32) -> Option<&'static PaperSize> {
        PAPER.iter().find(|p| (p.width == w && p.height == h) || (p.width == h && p.height == w))
    }

    pub fn is_portrait(&self) -> bool {
        self.height >= self.width
    }

    /// Pixels at `dpi`.
    pub fn pixels(&self, dpi: u32) -> (u32, u32) {
        (px(self.width, dpi), px(self.height, dpi))
    }

    /// Points (1/72 inch).
    pub fn points(&self) -> (u32, u32) {
        let pt = |v: i32| (f64::from(v) / 2540.0 * 72.0).round() as u32;
        (pt(self.width), pt(self.height))
    }
}

/// 1/100 mm to pixels.
pub fn px(hundredths_mm: i32, dpi: u32) -> u32 {
    (f64::from(hundredths_mm) / 2540.0 * f64::from(dpi)).round().max(1.0) as u32
}

/// `media-source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Source {
    Auto,
    Rear,
    Cassette,
    Disc,
}

impl Source {
    pub fn parse(s: &str) -> Result<Source> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Source::Auto),
            "rear" | "rear-tray" | "back" => Ok(Source::Rear),
            "cassette" | "main" | "tray" => Ok(Source::Cassette),
            "disc" | "cd" | "dvd" => Ok(Source::Disc),
            other => Err(Error::Invalid(format!(
                "unknown paper source {other:?} (choose auto, rear, cassette or disc)"
            ))),
        }
    }

    /// The IPP keyword: the printer calls the cassette `main`.
    pub fn ipp(self) -> &'static str {
        match self {
            Source::Auto => "auto",
            Source::Rear => "rear",
            Source::Cassette => "main",
            Source::Disc => "disc",
        }
    }
}

/// Friendly names for the media types the printer lists. Everything else must already be a
/// keyword the printer knows (`stationery`, `photographic`, `envelope`, `com.canon-*`).
const MEDIA_TYPE_ALIASES: &[(&str, &str)] = &[
    ("plain", "stationery"),
    ("stationery", "stationery"),
    ("photo", "photographic"),
    ("photographic", "photographic"),
    ("envelope", "envelope"),
    ("auto", "auto"),
    ("glossy", "com.canon-mtglossy"),
    ("matte", "com.canon-mtmat"),
    ("luster", "com.canon-mtluster"),
    ("platinum", "com.canon-mtplatinum"),
    ("semi-gloss", "com.canon-mtsemisuper"),
    ("fine-art", "com.canon-mtfineart"),
    ("card-stock", "com.canon-mtcardstock"),
    ("high-resolution", "com.canon-mthires"),
    ("tshirt", "com.canon-mttshirt"),
    ("hagaki", "com.canon-mthagaki"),
    ("disc", "com.canon-mtdisca"),
    ("disc-a", "com.canon-mtdisca"),
    ("disc-b", "com.canon-mtdiscb"),
];

/// Turn a friendly media type into the IPP keyword.
pub fn media_type(s: &str) -> Result<String> {
    let k = s.trim().to_ascii_lowercase();
    if let Some((_, v)) = MEDIA_TYPE_ALIASES.iter().find(|(a, _)| *a == k) {
        return Ok((*v).to_string());
    }
    if !k.is_empty()
        && k.len() <= 63
        && k.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
    {
        return Ok(k);
    }
    Err(Error::Invalid(format!("{s:?} is not a media type keyword")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_match_the_printers_database() {
        let a4 = PaperSize::parse("A4").unwrap();
        assert_eq!((a4.width, a4.height, a4.ipp_name), (21000, 29700, "iso_a4_210x297mm"));
        assert_eq!(a4.pixels(600), (4961, 7016));
        assert_eq!(a4.points(), (595, 842));
        assert_eq!(PaperSize::parse("Letter").unwrap().pixels(600), (5100, 6600));
        assert_eq!(PaperSize::parse("hagaki").unwrap().key, "postcard");
        assert_eq!(PaperSize::parse("5x7").unwrap().key, "2l");
        assert_eq!(PaperSize::parse("business card").unwrap().key, "card");
        assert_eq!(PaperSize::by_dimensions(29700, 21000).unwrap().key, "a4");
        assert!(PaperSize::by_dimensions(1, 2).is_none());
        let e = PaperSize::parse("a3").unwrap_err().to_string();
        assert!(e.contains("a4") && e.contains("chou4"), "{e}");
        assert!(a4.is_portrait());
    }

    #[test]
    fn keys_are_unique_and_dimensions_sane() {
        for (i, p) in PAPER.iter().enumerate() {
            assert!(PAPER.iter().skip(i + 1).all(|q| q.key != p.key), "duplicate key {}", p.key);
            assert!(p.width >= 5500 && p.height >= 8000 && p.height <= 67600, "{}", p.key);
        }
        // The disc tray: 120 x 120 mm = 2835 px at 600 dpi, 340 pt.
        assert_eq!(DISC_PAPER.pixels(600), (2835, 2835));
        assert_eq!(DISC_PAPER.points(), (340, 340));
    }

    #[test]
    fn sources_use_the_printers_keywords() {
        assert_eq!(Source::parse("cassette").unwrap().ipp(), "main");
        assert_eq!(Source::parse("Rear").unwrap().ipp(), "rear");
        assert_eq!(Source::parse("auto").unwrap().ipp(), "auto");
        assert_eq!(Source::parse("disc").unwrap().ipp(), "disc");
        assert!(Source::parse("manual").is_err());
    }

    #[test]
    fn media_types_accept_aliases_and_raw_keywords() {
        assert_eq!(media_type("plain").unwrap(), "stationery");
        assert_eq!(media_type("Photo").unwrap(), "photographic");
        assert_eq!(media_type("disc-b").unwrap(), "com.canon-mtdiscb");
        assert_eq!(media_type("com.canon-mtluster").unwrap(), "com.canon-mtluster");
        assert!(media_type("two words").is_err());
        assert!(media_type("").is_err());
        assert!(media_type("x\"y").is_err());
    }
}
