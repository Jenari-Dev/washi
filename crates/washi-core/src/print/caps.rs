// SPDX-License-Identifier: GPL-2.0-or-later
//! What the print screens may offer: a compact summary of the printer's IPP attributes and its
//! `media-col-database`, built with read-only queries.
//!
//! The database has over a thousand entries (source x media type x size x bordered / borderless).
//! [`PrintSummary`] boils it down to "which media types exist, and for each source, type and
//! known paper size, can it be printed with margins and / or borderless", which is all a
//! settings panel needs. Sizes the program has no name for (custom sizes) are left out.

use std::collections::BTreeMap;

use serde::Serialize;
use washi_net::ipp::{MediaDatabase, PrintCapabilities};

use super::paper::{PAPER, PaperSize};
use crate::error::Result;
use crate::printer::Printer;

/// Flag in [`PrintSummary::media`]: printable with margins.
pub const BORDERED: u8 = 1;
/// Flag in [`PrintSummary::media`]: printable borderless.
pub const BORDERLESS: u8 = 2;

/// A paper size the printer offers (known to Washi by key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SizeInfo {
    pub key: String,
    pub label: String,
    pub ipp_name: String,
    /// 1/100 mm, portrait.
    pub width: i32,
    pub height: i32,
}

impl SizeInfo {
    fn of(p: &PaperSize) -> SizeInfo {
        SizeInfo {
            key: p.key.to_string(),
            label: p.label.to_string(),
            ipp_name: p.ipp_name.to_string(),
            width: p.width,
            height: p.height,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PrintSummary {
    pub accepting_jobs: Option<bool>,
    /// IPP `printer-state` (`idle`, `processing`, `stopped`).
    pub state: Option<String>,
    /// Tray keywords of the database, without the disc tray: `auto`, `main` (cassette), `rear`.
    pub sources: Vec<String>,
    /// `media-type-supported` without the disc media.
    pub media_types: Vec<String>,
    /// Known sizes that appear in the database (not the disc tray), in Washi's order.
    pub sizes: Vec<SizeInfo>,
    /// source -> media type -> size key -> [`BORDERED`] | [`BORDERLESS`] flags.
    pub media: BTreeMap<String, BTreeMap<String, BTreeMap<String, u8>>>,
    /// source -> media type -> size key -> the margins of the bordered entry in 1/100 mm as
    /// `[top, bottom, left, right]` for the portrait sheet: the edge the printer cannot print on.
    /// Absent for a combination with no bordered entry (borderless only), where nothing is lost.
    pub margins: BTreeMap<String, BTreeMap<String, BTreeMap<String, [i32; 4]>>>,
    /// `draft`, `normal`, `high`.
    pub qualities: Vec<String>,
    /// Two-sided printing is offered.
    pub duplex: bool,
    pub copies_max: Option<i32>,
    pub document_formats: Vec<String>,
}

impl PrintSummary {
    /// Build the summary from what the printer said. Pure.
    pub fn build(caps: &PrintCapabilities, db: &MediaDatabase) -> PrintSummary {
        let mut media: BTreeMap<String, BTreeMap<String, BTreeMap<String, u8>>> = BTreeMap::new();
        let mut margins: BTreeMap<String, BTreeMap<String, BTreeMap<String, [i32; 4]>>> =
            BTreeMap::new();
        let mut size_keys: Vec<&'static str> = Vec::new();
        for e in &db.entries {
            let (Some(source), Some(ty)) = (e.source.as_deref(), e.media_type.as_deref()) else {
                continue;
            };
            if source == "disc" {
                continue;
            }
            let Some(size) = PaperSize::by_dimensions(e.width, e.height) else { continue };
            // The database lists sizes upright; skip a sideways duplicate.
            if (size.width, size.height) != (e.width, e.height) {
                continue;
            }
            let flag = if e.is_borderless() { BORDERLESS } else { BORDERED };
            *media
                .entry(source.to_string())
                .or_default()
                .entry(ty.to_string())
                .or_default()
                .entry(size.key.to_string())
                .or_insert(0) |= flag;
            if let (BORDERED, Some(g)) = (flag, e.margins()) {
                // Entries of one combination agree; should they not, the wider edge is kept.
                let m = margins
                    .entry(source.to_string())
                    .or_default()
                    .entry(ty.to_string())
                    .or_default()
                    .entry(size.key.to_string())
                    .or_insert([0; 4]);
                for (cur, new) in m.iter_mut().zip([g.top, g.bottom, g.left, g.right]) {
                    *cur = (*cur).max(new);
                }
            }
            if !size_keys.contains(&size.key) {
                size_keys.push(size.key);
            }
        }
        let sizes = PAPER.iter().filter(|p| size_keys.contains(&p.key)).map(SizeInfo::of).collect();
        let sources = media.keys().cloned().collect();
        let media_types = caps
            .media_types
            .iter()
            .filter(|t| !t.starts_with("com.canon-mtdisc"))
            .cloned()
            .collect();
        PrintSummary {
            accepting_jobs: caps.accepting_jobs,
            state: caps.state.clone(),
            sources,
            media_types,
            sizes,
            media,
            margins,
            qualities: vec!["draft".into(), "normal".into(), "high".into()],
            duplex: caps.sides.iter().any(|s| s.starts_with("two-sided")),
            copies_max: caps.copies_max,
            document_formats: caps.document_formats.clone(),
        }
    }

    /// The margins of a bordered combination, `[top, bottom, left, right]` in 1/100 mm.
    pub fn margins_of(&self, source: &str, media_type: &str, size_key: &str) -> Option<[i32; 4]> {
        self.margins.get(source)?.get(media_type)?.get(size_key).copied()
    }

    /// Flags for one combination (0 = not offered).
    pub fn flags(&self, source: &str, media_type: &str, size_key: &str) -> u8 {
        self.media
            .get(source)
            .and_then(|t| t.get(media_type))
            .and_then(|s| s.get(size_key))
            .copied()
            .unwrap_or(0)
    }
}

impl Printer {
    /// Read-only: the printer's IPP print attributes and `media-col-database`, summarised.
    pub fn print_summary(&self) -> Result<PrintSummary> {
        let ipp = self.ipp();
        let caps = ipp.print_capabilities()?;
        let db = ipp.media_database()?;
        Ok(PrintSummary::build(&caps, &db))
    }
}

#[cfg(test)]
mod tests {
    use washi_net::ipp::MediaEntry;

    use super::*;

    fn entry(w: i32, h: i32, margin: i32, source: &str, ty: &str) -> MediaEntry {
        MediaEntry {
            width: w,
            height: h,
            top: Some(margin),
            bottom: Some(margin),
            left: Some(margin),
            right: Some(margin),
            source: Some(source.into()),
            media_type: Some(ty.into()),
        }
    }

    #[test]
    fn the_database_boils_down_to_flags_per_source_type_and_size() {
        let db = MediaDatabase {
            entries: vec![
                entry(21000, 29700, 340, "auto", "stationery"),
                entry(21000, 29700, 0, "auto", "stationery"),
                entry(21000, 29700, 340, "rear", "stationery"),
                entry(8890, 12700, 0, "rear", "com.canon-mtglossy"),
                entry(21000, 29700, 340, "main", "stationery"),
                entry(12000, 12000, 10, "disc", "com.canon-mtdisca"),
                entry(12345, 23456, 0, "rear", "stationery"), // not a size Washi names
            ],
        };
        let caps = PrintCapabilities {
            media_types: vec![
                "stationery".into(),
                "com.canon-mtglossy".into(),
                "com.canon-mtdisca".into(),
            ],
            sides: vec!["one-sided".into(), "two-sided-long-edge".into()],
            copies_max: Some(99),
            ..PrintCapabilities::default()
        };
        let s = PrintSummary::build(&caps, &db);
        assert_eq!(s.sources, ["auto", "main", "rear"], "no disc tray");
        assert_eq!(s.media_types, ["stationery", "com.canon-mtglossy"]);
        assert_eq!(s.flags("auto", "stationery", "a4"), BORDERED | BORDERLESS);
        assert_eq!(s.flags("rear", "stationery", "a4"), BORDERED);
        assert_eq!(s.flags("rear", "com.canon-mtglossy", "l"), BORDERLESS);
        assert_eq!(s.flags("main", "stationery", "a4"), BORDERED);
        assert_eq!(s.flags("main", "com.canon-mtglossy", "a4"), 0);
        assert_eq!(s.flags("nope", "x", "a4"), 0);
        assert_eq!(s.sizes.iter().map(|z| z.key.as_str()).collect::<Vec<_>>(), ["a4", "l"]);
        assert!(s.duplex);
        assert_eq!(s.copies_max, Some(99));
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["media"]["auto"]["stationery"]["a4"], 3);
    }

    #[test]
    fn the_summary_keeps_the_margins_of_the_bordered_entries() {
        let mut uneven = entry(8890, 12700, 0, "rear", "stationery");
        (uneven.top, uneven.bottom, uneven.left, uneven.right) =
            (Some(300), Some(1200), Some(340), Some(340));
        let db = MediaDatabase {
            entries: vec![
                entry(21000, 29700, 340, "auto", "stationery"),
                entry(21000, 29700, 0, "auto", "stationery"), // the borderless twin
                entry(21000, 29700, 0, "auto", "photographic"), // borderless only
                uneven,
                // A second bordered entry of one combination: the wider edge wins.
                MediaEntry { top: Some(500), ..entry(21000, 29700, 340, "rear", "stationery") },
                entry(21000, 29700, 340, "rear", "stationery"),
                MediaEntry { top: None, ..entry(8890, 12700, 340, "main", "stationery") },
            ],
        };
        let caps = PrintCapabilities::default();
        let s = PrintSummary::build(&caps, &db);
        assert_eq!(s.margins_of("auto", "stationery", "a4"), Some([340; 4]), "the bordered one");
        assert_eq!(s.margins_of("auto", "photographic", "a4"), None, "borderless only: no edge");
        assert_eq!(s.margins_of("rear", "stationery", "l"), Some([300, 1200, 340, 340]));
        assert_eq!(s.margins_of("rear", "stationery", "a4"), Some([500, 340, 340, 340]));
        assert_eq!(
            s.margins_of("main", "stationery", "l"),
            None,
            "a margin the printer leaves out"
        );
        assert_eq!(s.margins_of("nope", "x", "a4"), None);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json["margins"]["rear"]["stationery"]["l"],
            serde_json::json!([300, 1200, 340, 340])
        );
    }

    #[test]
    fn an_empty_database_gives_an_empty_summary() {
        let s = PrintSummary::build(&PrintCapabilities::default(), &MediaDatabase::default());
        assert!(s.sources.is_empty() && s.sizes.is_empty() && !s.duplex);
        assert_eq!(s.qualities, ["draft", "normal", "high"]);
    }
}
