// SPDX-License-Identifier: GPL-2.0-or-later
//! The printer's own words and picture, read from the printer and kept between runs.
//!
//! * the **strings**: its display names for the keywords it uses (`media-type.<keyword>`, ...), as
//!   a `.strings` file per language, named by IPP `printer-strings-uri`;
//! * the **picture**: a product image of the printer, named by IPP `printer-icons`.
//!
//! Both come from the user's own printer at run time (read-only IPP and HTTP reads) and are kept
//! in the user's cache directory, `~/.cache/washi/`. They are not part of Washi and are not
//! redistributed.
//!
//! A cached copy is used as long as it is [current](is_current): younger than 30 days and read
//! from the same firmware version the printer reports now. An older copy is still shown while
//! the caller reads a new one, so nothing waits for the printer.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use washi_net::web::{PrinterIcon, WebOptions, fetch_icon, fetch_strings};
use washi_net::{ipp::normalize_language, web::png_size};

use crate::cache::printer_key;
use crate::error::{Error, Result};
use crate::printer::Printer;

/// A copy older than this is read again (30 days).
pub const MAX_AGE_SECS: u64 = 30 * 24 * 3600;

const CACHE_VERSION: u32 = 1;

/// Is a copy read at `fetched_at` from firmware `cached_firmware` still good at `now`, with the
/// printer reporting `current_firmware` (`None` when that is not known yet)? A copy from the
/// future (the clock was set back) is not.
pub fn is_current(
    fetched_at: u64,
    cached_firmware: Option<&str>,
    current_firmware: Option<&str>,
    now: u64,
) -> bool {
    if fetched_at > now.saturating_add(24 * 3600) || now.saturating_sub(fetched_at) >= MAX_AGE_SECS
    {
        return false;
    }
    match (cached_firmware, current_firmware) {
        (Some(a), Some(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
        _ => true,
    }
}

// ---- what is read from the printer ------------------------------------------------------------

/// The printer's strings in one language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchedStrings {
    /// The language they are in (normalised).
    pub language: String,
    /// Empty when the printer has no strings file for the language.
    pub strings: BTreeMap<String, String>,
    /// The firmware version the printer reported with them.
    pub firmware: Option<String>,
}

/// The printer's picture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchedIcon {
    /// PNG bytes, the transparent margin cut off (`None`: the printer offers no picture).
    pub png: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub firmware: Option<String>,
}

impl Printer {
    /// Read the printer's strings for `language` (read-only: IPP `Get-Printer-Attributes`, then
    /// `GET` of the strings file). A printer that has none in that language is not an error; the
    /// result is empty.
    pub fn fetch_strings(&self, language: &str) -> Result<FetchedStrings> {
        let lang = normalize_language(language)
            .ok_or_else(|| Error::Invalid(format!("{language:?} is not a language tag")))?;
        let loc = self.ipp().localization(&lang)?;
        let mut out = FetchedStrings {
            language: lang.clone(),
            strings: BTreeMap::new(),
            firmware: loc.firmware_version.clone(),
        };
        if let Some(path) = loc.strings_path(&lang) {
            let opts = WebOptions { accept_language: lang, ..WebOptions::default() };
            out.strings = fetch_strings(self.host(), self.chmp_port(), &path, &opts)?;
        }
        Ok(out)
    }

    /// Read the printer's picture (read-only). The largest the printer lists is taken and its
    /// transparent margin cut off, so it fills the space it is shown in. A printer without a
    /// picture is not an error; `png` is then `None`.
    pub fn fetch_icon(&self) -> Result<FetchedIcon> {
        let loc = self.ipp().localization("en")?;
        let mut out = FetchedIcon { firmware: loc.firmware_version.clone(), ..Default::default() };
        let paths = loc.icon_paths();
        if paths.is_empty() {
            return Ok(out);
        }
        let icon = fetch_icon(self.host(), self.chmp_port(), &paths, &WebOptions::default())?;
        let (png, width, height) = match trim_margin(&icon) {
            Some(t) => t,
            None => (icon.png, icon.width, icon.height),
        };
        out.png = Some(png);
        out.width = width;
        out.height = height;
        Ok(out)
    }
}

/// Cut the transparent margin off a PNG, leaving a small border. `None` when there is nothing to
/// cut (no alpha channel, no transparent margin, nothing visible at all) or the image cannot be
/// decoded; the picture is then used as it is.
pub fn trim_margin(icon: &PrinterIcon) -> Option<(Vec<u8>, u32, u32)> {
    use image::{ImageFormat, ImageReader};
    let img =
        ImageReader::with_format(io::Cursor::new(&icon.png), ImageFormat::Png).decode().ok()?;
    if !img.color().has_alpha() {
        return None;
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (x, y, p) in rgba.enumerate_pixels() {
        if p.0[3] > 8 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 < x0 || y1 < y0 {
        return None;
    }
    // A border of about 2 % keeps a soft edge or shadow from being cut.
    let pad = (w.max(h) / 50).max(2);
    let (x0, y0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
    let (x1, y1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
    // Cut only when it matters.
    if u64::from(cw) * u64::from(ch) * 100 > u64::from(w) * u64::from(h) * 90 {
        return None;
    }
    let cropped = image::imageops::crop_imm(&rgba, x0, y0, cw, ch).to_image();
    let mut out = Vec::new();
    cropped.write_to(&mut io::Cursor::new(&mut out), ImageFormat::Png).ok()?;
    Some((out, cw, ch))
}

// ---- the cache ----------------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct StringsFile {
    version: u32,
    language: String,
    firmware: Option<String>,
    fetched_at: u64,
    strings: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct IconMeta {
    version: u32,
    firmware: Option<String>,
    fetched_at: u64,
    /// The printer has a picture (the PNG file exists).
    has_icon: bool,
    width: u32,
    height: u32,
}

/// Strings read earlier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedStrings {
    pub strings: BTreeMap<String, String>,
    pub firmware: Option<String>,
    pub fetched_at: u64,
}

impl CachedStrings {
    pub fn is_current(&self, current_firmware: Option<&str>, now: u64) -> bool {
        is_current(self.fetched_at, self.firmware.as_deref(), current_firmware, now)
    }
}

/// A picture read earlier (or the note that the printer has none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedIcon {
    /// The PNG file, `None` when the printer has no picture.
    pub path: Option<PathBuf>,
    pub firmware: Option<String>,
    pub fetched_at: u64,
}

impl CachedIcon {
    pub fn is_current(&self, current_firmware: Option<&str>, now: u64) -> bool {
        is_current(self.fetched_at, self.firmware.as_deref(), current_firmware, now)
    }
}

/// Where one printer's strings and picture are kept: `<dir>/<printer-key>-strings-<lang>.json`,
/// `<printer-key>-icon.json` and `<printer-key>-icon.png`. The key is a hash of the host name
/// (see [`printer_key`]), so no file name carries a host name or an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalizationCache {
    dir: PathBuf,
    key: String,
}

impl LocalizationCache {
    pub fn in_dir(dir: &Path, host: &str, port: u16) -> Self {
        LocalizationCache { dir: dir.to_path_buf(), key: printer_key(host, port) }
    }

    /// In `~/.cache/washi` (`XDG_CACHE_HOME` is honoured); `None` without a cache directory.
    pub fn for_printer(host: &str, port: u16) -> Option<Self> {
        Some(Self::in_dir(&crate::cache::SnapshotCache::default_dir()?, host, port))
    }

    fn strings_path(&self, language: &str) -> Option<PathBuf> {
        let lang = normalize_language(language)?;
        Some(self.dir.join(format!("{}-strings-{lang}.json", self.key)))
    }

    fn icon_meta_path(&self) -> PathBuf {
        self.dir.join(format!("{}-icon.json", self.key))
    }

    pub fn icon_path(&self) -> PathBuf {
        self.dir.join(format!("{}-icon.png", self.key))
    }

    fn make_dir(&self) -> io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.dir)
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.make_dir()?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }

    /// The strings kept for a language. A missing, unreadable, corrupt or other-version file is
    /// `None`.
    pub fn strings(&self, language: &str) -> Option<CachedStrings> {
        let text = std::fs::read_to_string(self.strings_path(language)?).ok()?;
        let f: StringsFile = serde_json::from_str(&text).ok()?;
        (f.version == CACHE_VERSION
            && Some(f.language.as_str()) == normalize_language(language).as_deref())
        .then_some(CachedStrings {
            strings: f.strings,
            firmware: f.firmware,
            fetched_at: f.fetched_at,
        })
    }

    pub fn store_strings(&self, fetched: &FetchedStrings, now: u64) -> io::Result<()> {
        let path = self
            .strings_path(&fetched.language)
            .ok_or_else(|| io::Error::other("not a language tag"))?;
        let text = serde_json::to_string(&StringsFile {
            version: CACHE_VERSION,
            language: fetched.language.clone(),
            firmware: fetched.firmware.clone(),
            fetched_at: now,
            strings: fetched.strings.clone(),
        })
        .map_err(io::Error::other)?;
        self.write_atomic(&path, text.as_bytes())
    }

    /// The picture kept (or the note that there is none). A missing or corrupt note, or a note
    /// that promises a file that is gone, is `None`.
    pub fn icon(&self) -> Option<CachedIcon> {
        let text = std::fs::read_to_string(self.icon_meta_path()).ok()?;
        let m: IconMeta = serde_json::from_str(&text).ok()?;
        if m.version != CACHE_VERSION {
            return None;
        }
        let path = if m.has_icon {
            let p = self.icon_path();
            // The file must still be the PNG that was written.
            std::fs::read(&p).ok().filter(|b| png_size(b).is_some())?;
            Some(p)
        } else {
            None
        };
        Some(CachedIcon { path, firmware: m.firmware, fetched_at: m.fetched_at })
    }

    /// Keep the picture (the file first, then the note that points to it), or the fact that the
    /// printer has none.
    pub fn store_icon(&self, fetched: &FetchedIcon, now: u64) -> io::Result<()> {
        if let Some(png) = &fetched.png {
            self.write_atomic(&self.icon_path(), png)?;
        } else {
            match std::fs::remove_file(self.icon_path()) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        let text = serde_json::to_string(&IconMeta {
            version: CACHE_VERSION,
            firmware: fetched.firmware.clone(),
            fetched_at: now,
            has_icon: fetched.png.is_some(),
            width: fetched.width,
            height: fetched.height,
        })
        .map_err(io::Error::other)?;
        self.write_atomic(&self.icon_meta_path(), text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG drawn for the tests: `w` x `h`, an opaque rectangle at `rect` (x0, y0, x1, y1
    /// exclusive) on a transparent ground.
    fn png(w: u32, h: u32, rect: Option<(u32, u32, u32, u32)>) -> PrinterIcon {
        let mut img = image::RgbaImage::new(w, h);
        if let Some((x0, y0, x1, y1)) = rect {
            for y in y0..y1 {
                for x in x0..x1 {
                    img.put_pixel(x, y, image::Rgba([200, 30, 30, 255]));
                }
            }
        }
        let mut out = Vec::new();
        img.write_to(&mut io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        PrinterIcon { png: out, width: w, height: h }
    }

    fn size_of(png: &[u8]) -> (u32, u32) {
        png_size(png).unwrap()
    }

    #[test]
    fn a_copy_is_current_for_30_days_and_the_same_firmware() {
        let day = 24 * 3600;
        let t0 = 1_000_000_000;
        assert!(is_current(t0, Some("1.060"), Some("1.060"), t0 + 29 * day));
        assert!(!is_current(t0, Some("1.060"), Some("1.060"), t0 + 30 * day));
        assert!(!is_current(t0, Some("1.060"), Some("1.061"), t0 + day), "new firmware");
        assert!(is_current(t0, Some(" 1.060 "), Some("1.060"), t0), "spaces do not matter");
        // Not knowing the firmware (either side) does not make a copy stale.
        assert!(is_current(t0, None, Some("1.060"), t0 + day));
        assert!(is_current(t0, Some("1.060"), None, t0 + day));
        assert!(is_current(t0, None, None, t0));
        // A copy dated after now (the clock went back) is read again.
        assert!(is_current(t0 + day / 2, None, None, t0), "a few hours of drift is fine");
        assert!(!is_current(t0 + 2 * day, None, None, t0));
    }

    #[test]
    fn the_transparent_margin_is_cut_off_with_a_small_border() {
        let icon = png(200, 200, Some((40, 80, 160, 140)));
        let (bytes, w, h) = trim_margin(&icon).unwrap();
        assert_eq!(size_of(&bytes), (w, h));
        // 120 x 60 visible, a border of 4 pixels (2 % of 200) each side.
        assert_eq!((w, h), (128, 68));
        let back = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(back.get_pixel(0, 0).0[3], 0, "the border is transparent");
        assert_eq!(back.get_pixel(w / 2, h / 2).0, [200, 30, 30, 255]);
    }

    #[test]
    fn pictures_that_need_no_cutting_are_left_alone() {
        // Nothing transparent around it, nothing visible at all, no alpha channel, not a PNG.
        assert!(trim_margin(&png(100, 100, Some((0, 0, 100, 100)))).is_none());
        assert!(
            trim_margin(&png(100, 100, Some((2, 2, 98, 98)))).is_none(),
            "less than 10 % to gain"
        );
        assert!(trim_margin(&png(100, 100, None)).is_none());
        let rgb = image::RgbImage::from_pixel(50, 50, image::Rgb([1, 2, 3]));
        let mut out = Vec::new();
        rgb.write_to(&mut io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        assert!(trim_margin(&PrinterIcon { png: out, width: 50, height: 50 }).is_none());
        assert!(
            trim_margin(&PrinterIcon { png: b"not a png".to_vec(), width: 1, height: 1 }).is_none()
        );
    }

    fn fetched(lang: &str, fw: &str, pairs: &[(&str, &str)]) -> FetchedStrings {
        FetchedStrings {
            language: lang.into(),
            strings: pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(),
            firmware: Some(fw.into()),
        }
    }

    #[test]
    fn strings_round_trip_per_language_under_a_hashed_name() {
        let dir = tempfile::tempdir().unwrap();
        let c = LocalizationCache::in_dir(&dir.path().join("washi"), "printer.local", 80);
        assert_eq!(c.strings("en"), None);
        c.store_strings(&fetched("en", "1.060", &[("media-type.a", "Paper A")]), 100).unwrap();
        c.store_strings(&fetched("ja", "1.060", &[("media-type.a", "\u{7d19}A")]), 101).unwrap();
        let en = c.strings("en").unwrap();
        assert_eq!(en.strings["media-type.a"], "Paper A");
        assert_eq!((en.firmware.as_deref(), en.fetched_at), (Some("1.060"), 100));
        assert_eq!(c.strings("JA").unwrap().strings["media-type.a"], "\u{7d19}A");
        assert_eq!(c.strings("fr"), None);
        assert_eq!(c.strings("not a tag"), None);
        let names: Vec<String> = std::fs::read_dir(dir.path().join("washi"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?} (no temp files left)");
        assert!(names.iter().all(|n| !n.contains("printer") && !n.contains("local")), "{names:?}");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("washi")).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "private directory: {mode:o}");
        // Another printer has its own files.
        let other = LocalizationCache::in_dir(&dir.path().join("washi"), "other.local", 80);
        assert_eq!(other.strings("en"), None);
        assert!(en.is_current(Some("1.060"), 200));
        assert!(!en.is_current(Some("1.061"), 200));
    }

    #[test]
    fn corrupt_foreign_and_mislabelled_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let c = LocalizationCache::in_dir(dir.path(), "p.local", 80);
        c.store_strings(&fetched("en", "1", &[("k", "v")]), 1).unwrap();
        let path = c.strings_path("en").unwrap();
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(c.strings("en"), None);
        std::fs::write(
            &path,
            r#"{"version": 9, "language": "en", "fetched_at": 1, "strings": {}}"#,
        )
        .unwrap();
        assert_eq!(c.strings("en"), None);
        // An English file copied over the Japanese name is not taken for Japanese.
        c.store_strings(&fetched("en", "1", &[("k", "v")]), 1).unwrap();
        std::fs::copy(&path, c.strings_path("ja").unwrap()).unwrap();
        assert_eq!(c.strings("ja"), None);
    }

    #[test]
    fn the_picture_is_kept_with_its_note_and_a_missing_picture_is_a_note_too() {
        let dir = tempfile::tempdir().unwrap();
        let c = LocalizationCache::in_dir(dir.path(), "p.local", 80);
        assert_eq!(c.icon(), None);
        let (png, w, h) = trim_margin(&png(100, 100, Some((20, 30, 80, 60)))).unwrap();
        let icon = FetchedIcon {
            png: Some(png.clone()),
            width: w,
            height: h,
            firmware: Some("1.060".into()),
        };
        c.store_icon(&icon, 50).unwrap();
        let got = c.icon().unwrap();
        assert_eq!(got.path.as_deref(), Some(c.icon_path().as_path()));
        assert_eq!(std::fs::read(got.path.unwrap()).unwrap(), png);
        assert_eq!((got.firmware.as_deref(), got.fetched_at), (Some("1.060"), 50));
        // The file is gone or damaged although the note is there: read again.
        std::fs::write(c.icon_path(), b"junk").unwrap();
        assert_eq!(c.icon(), None);
        // A printer without a picture: the note says so and no file is left from an earlier one.
        c.store_icon(&icon, 60).unwrap();
        c.store_icon(&FetchedIcon { firmware: Some("1.061".into()), ..Default::default() }, 70)
            .unwrap();
        let none = c.icon().unwrap();
        assert_eq!((none.path.as_ref(), none.fetched_at), (None, 70));
        assert!(!c.icon_path().exists());
        assert!(none.is_current(Some("1.061"), 80) && !none.is_current(Some("1.062"), 80));
    }
}
