// SPDX-License-Identifier: GPL-2.0-or-later
//! The printer's `media-col-database`: every (size, source, type, margins) combination the
//! printer prints. The XK140 lists ~1300 entries (370 KB) and only serves them when the attribute
//! is requested on its own.

use std::collections::BTreeSet;

use serde::Serialize;

use super::job::Margins;
use super::{IppError, IppMessage, IppValue, status_name, tag};

/// One entry of `media-col-database`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaEntry {
    /// `media-size` in 1/100 mm.
    pub width: i32,
    pub height: i32,
    pub top: Option<i32>,
    pub bottom: Option<i32>,
    pub left: Option<i32>,
    pub right: Option<i32>,
    /// `auto`, `main`, `rear` or `disc`.
    pub source: Option<String>,
    pub media_type: Option<String>,
}

impl MediaEntry {
    /// All four margins, when the entry states them.
    pub fn margins(&self) -> Option<Margins> {
        Some(Margins { top: self.top?, bottom: self.bottom?, left: self.left?, right: self.right? })
    }

    /// Margins are stated and all zero (borderless printing).
    pub fn is_borderless(&self) -> bool {
        self.margins().is_some_and(|m| m.is_zero())
    }
}

/// The parsed database.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MediaDatabase {
    pub entries: Vec<MediaEntry>,
}

fn int_member(members: &[(String, Vec<IppValue>)], name: &str) -> Option<i32> {
    members.iter().find(|(k, _)| k == name).and_then(|(_, v)| v.first()).and_then(IppValue::as_i32)
}

fn str_member(members: &[(String, Vec<IppValue>)], name: &str) -> Option<String> {
    members
        .iter()
        .find(|(k, _)| k == name)
        .and_then(|(_, v)| v.first())
        .and_then(IppValue::as_str)
        .map(str::to_string)
}

fn parse_entry(members: &[(String, Vec<IppValue>)]) -> Option<MediaEntry> {
    let size = members.iter().find(|(k, _)| k == "media-size")?.1.first()?;
    let IppValue::Collection(size) = size else { return None };
    // Range-valued (custom size) entries have no single width / height and are skipped.
    let width = int_member(size, "x-dimension")?;
    let height = int_member(size, "y-dimension")?;
    Some(MediaEntry {
        width,
        height,
        top: int_member(members, "media-top-margin"),
        bottom: int_member(members, "media-bottom-margin"),
        left: int_member(members, "media-left-margin"),
        right: int_member(members, "media-right-margin"),
        source: str_member(members, "media-source"),
        media_type: str_member(members, "media-type"),
    })
}

/// `main` is the cassette; accept the words users and the CLI use.
fn canonical_source(s: &str) -> &str {
    match s {
        "cassette" | "tray" => "main",
        other => other,
    }
}

impl MediaDatabase {
    /// Parse a `Get-Printer-Attributes` reply for `media-col-database`. A reply without the
    /// attribute (or with an out-of-band value, as for a broad request) is an empty database.
    pub fn from_message(msg: &IppMessage) -> Result<MediaDatabase, IppError> {
        if msg.code >= 0x0100 {
            return Err(IppError::Status { code: msg.code, name: status_name(msg.code) });
        }
        let g = msg.group(tag::PRINTER_ATTRIBUTES).ok_or(IppError::NoPrinterAttributes)?;
        let entries = g
            .get("media-col-database")
            .map(|a| {
                a.values
                    .iter()
                    .filter_map(|v| match v {
                        IppValue::Collection(m) => parse_entry(m),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(MediaDatabase { entries })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries for a media source.
    pub fn for_source<'a>(&'a self, source: &'a str) -> impl Iterator<Item = &'a MediaEntry> + 'a {
        let source = canonical_source(source);
        self.entries.iter().filter(move |e| e.source.as_deref() == Some(source))
    }

    /// The disc-tray entries (`media-source` = `disc`).
    pub fn disc(&self) -> Vec<&MediaEntry> {
        self.for_source("disc").collect()
    }

    /// Best entry for a request. `source` / `media_type` of `None` match anything; a source of
    /// `disc` ignores `borderless` (the disc tray lists a tiny non-zero margin instead).
    pub fn find(
        &self,
        width: i32,
        height: i32,
        source: Option<&str>,
        media_type: Option<&str>,
        borderless: bool,
    ) -> Option<&MediaEntry> {
        let source = source.map(canonical_source);
        self.entries.iter().find(|e| {
            e.width == width
                && e.height == height
                && source.is_none_or(|s| e.source.as_deref() == Some(s))
                && media_type.is_none_or(|t| e.media_type.as_deref() == Some(t))
                && (source == Some("disc") || e.is_borderless() == borderless)
        })
    }

    /// Distinct sources, sorted.
    pub fn sources(&self) -> Vec<String> {
        let s: BTreeSet<&str> = self.entries.iter().filter_map(|e| e.source.as_deref()).collect();
        s.into_iter().map(str::to_string).collect()
    }

    /// Distinct sizes (1/100 mm) offered from a source (`None` = any), sorted.
    pub fn sizes(&self, source: Option<&str>) -> Vec<(i32, i32)> {
        let source = source.map(canonical_source);
        let s: BTreeSet<(i32, i32)> = self
            .entries
            .iter()
            .filter(|e| source.is_none_or(|s| e.source.as_deref() == Some(s)))
            .map(|e| (e.width, e.height))
            .collect();
        s.into_iter().collect()
    }

    /// Distinct media types offered for a size and source, sorted.
    pub fn types_for(&self, width: i32, height: i32, source: Option<&str>) -> Vec<String> {
        let source = source.map(canonical_source);
        let s: BTreeSet<&str> = self
            .entries
            .iter()
            .filter(|e| {
                e.width == width
                    && e.height == height
                    && source.is_none_or(|s| e.source.as_deref() == Some(s))
            })
            .filter_map(|e| e.media_type.as_deref())
            .collect();
        s.into_iter().map(str::to_string).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{IppAttribute, IppGroup};
    use super::*;

    fn entry(w: i32, h: i32, m: [i32; 4], source: &str, ty: &str) -> IppValue {
        let int = |v: i32| vec![IppValue::Integer(v)];
        let kw = |s: &str| vec![IppValue::Keyword(s.into())];
        IppValue::Collection(vec![
            (
                "media-size".into(),
                vec![IppValue::Collection(vec![
                    ("x-dimension".into(), int(w)),
                    ("y-dimension".into(), int(h)),
                ])],
            ),
            ("media-top-margin".into(), int(m[0])),
            ("media-bottom-margin".into(), int(m[1])),
            ("media-left-margin".into(), int(m[2])),
            ("media-right-margin".into(), int(m[3])),
            ("media-source".into(), kw(source)),
            ("media-type".into(), kw(ty)),
        ])
    }

    fn database_message(values: Vec<IppValue>) -> IppMessage {
        IppMessage {
            version: (1, 1),
            code: 0,
            request_id: 1,
            groups: vec![IppGroup::new(
                tag::PRINTER_ATTRIBUTES,
                vec![IppAttribute::new("media-col-database", values)],
            )],
        }
    }

    fn sample() -> MediaDatabase {
        MediaDatabase::from_message(&database_message(vec![
            entry(21000, 29700, [500, 500, 340, 340], "auto", "stationery"),
            entry(21000, 29700, [0, 0, 0, 0], "rear", "com.canon-mtglossy"),
            entry(21000, 29700, [500, 500, 340, 340], "main", "stationery"),
            entry(12000, 12000, [10, 10, 10, 10], "disc", "com.canon-mtdisca"),
            entry(12000, 12000, [10, 10, 10, 10], "disc", "com.canon-mtdiscb"),
        ]))
        .unwrap()
    }

    #[test]
    fn disc_entries_are_found() {
        let db = sample();
        let disc = db.disc();
        assert_eq!(disc.len(), 2);
        assert_eq!((disc[0].width, disc[0].height), (12000, 12000));
        assert_eq!(disc[0].margins(), Some(Margins { top: 10, bottom: 10, left: 10, right: 10 }));
        assert_eq!(disc[0].media_type.as_deref(), Some("com.canon-mtdisca"));
        // The disc tray ignores the borderless flag.
        let e = db.find(12000, 12000, Some("disc"), Some("com.canon-mtdiscb"), true).unwrap();
        assert_eq!(e.media_type.as_deref(), Some("com.canon-mtdiscb"));
        assert!(db.find(12000, 12000, Some("rear"), None, false).is_none());
    }

    #[test]
    fn borderless_and_sources() {
        let db = sample();
        assert!(db.find(21000, 29700, Some("rear"), Some("com.canon-mtglossy"), true).is_some());
        assert!(db.find(21000, 29700, Some("rear"), Some("com.canon-mtglossy"), false).is_none());
        assert!(db.find(21000, 29700, Some("auto"), Some("stationery"), true).is_none());
        // "cassette" is the printer's `main` source.
        let e = db.find(21000, 29700, Some("cassette"), None, false).unwrap();
        assert_eq!(e.source.as_deref(), Some("main"));
        assert_eq!(db.sources(), ["auto", "disc", "main", "rear"]);
        assert_eq!(db.sizes(Some("disc")), [(12000, 12000)]);
        assert_eq!(db.sizes(None).len(), 2);
        assert_eq!(db.types_for(21000, 29700, Some("rear")), ["com.canon-mtglossy"]);
        assert_eq!(db.for_source("cassette").count(), 1);
    }

    #[test]
    fn broad_request_gives_an_empty_database() {
        let msg = IppMessage {
            version: (1, 1),
            code: 0,
            request_id: 1,
            groups: vec![IppGroup::new(
                tag::PRINTER_ATTRIBUTES,
                vec![IppAttribute::single(
                    "media-col-database",
                    IppValue::OutOfBand(super::super::OutOfBand::NoValue),
                )],
            )],
        };
        assert!(MediaDatabase::from_message(&msg).unwrap().is_empty());
        let mut none = msg.clone();
        none.groups[0].attributes.clear();
        assert!(MediaDatabase::from_message(&none).unwrap().is_empty());
        let mut err = msg;
        err.code = 0x0400;
        assert!(MediaDatabase::from_message(&err).is_err());
    }

    #[test]
    fn malformed_entries_are_skipped() {
        let no_size = IppValue::Collection(vec![(
            "media-source".into(),
            vec![IppValue::Keyword("auto".into())],
        )]);
        let ranged = IppValue::Collection(vec![(
            "media-size".into(),
            vec![IppValue::Collection(vec![(
                "x-dimension".into(),
                vec![IppValue::Range { lower: 5500, upper: 21590 }],
            )])],
        )]);
        let db = MediaDatabase::from_message(&database_message(vec![
            no_size,
            ranged,
            IppValue::Integer(3),
            entry(100, 200, [0, 0, 0, 0], "auto", "x"),
        ]))
        .unwrap();
        assert_eq!(db.entries.len(), 1);
        assert!(db.entries[0].is_borderless());
    }
}
