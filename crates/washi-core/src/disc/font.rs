// SPDX-License-Identifier: GPL-2.0-or-later
//! Fonts of disc labels: which font file a text uses, and how a line of it is drawn.
//!
//! A text names a font **family** (what the app's picker lists) with **bold** and **italic**
//! switches. The file is the one fontconfig picks for that family, weight and slant
//! (`fc-match`), so the printed image and the app's canvas, which asks the same fontconfig through
//! Qt, show the same face. Three details make that hold up:
//!
//! * A family that fontconfig keeps in a **collection** (`NotoSansCJK-Bold.ttc` holds the JP, KR,
//!   SC, TC and HK cuts) is opened at the face index it reports, not at the first face.
//! * A **variable** font (`Cantarell-VF.otf`) is a named instance of the file: its weight (and
//!   width) are set on the outlines.
//! * When the family has no bold or no italic cut, fontconfig says the style is **synthetic**
//!   (`embolden`, a slant `matrix`); the canvas then draws it that way, and so does the printed
//!   image: thickened outlines and a sheared line.
//!
//! A text whose letters the chosen font does not have (Japanese in a Latin family, say) is set
//! with further fonts from fontconfig for those letters, so it never prints as empty boxes when
//! some font on the computer has the letter.
//!
//! Everything that asks fontconfig goes through a function value, so the tests can answer for it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ab_glyph::{
    Font, FontRef, Glyph, GlyphId, PxScale, PxScaleFont, ScaleFont, VariableFont, point,
};
use image::{GrayImage, Luma};

use super::TextItem;
use crate::error::{Error, Result};
use crate::tools::run_capture;

/// What `fc-match` prints for each font: the file, the face inside it, and what is needed to draw
/// the style that was asked for.
const FORMAT: &str =
    "%{file}|%{index}|%{weight}|%{slant}|%{variable}|%{embolden}|%{matrix}|%{family}\n";

/// Fonts to try when fontconfig is missing or answers nothing usable.
const WELL_KNOWN: [&str; 6] = [
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
];

/// The slant fontconfig gives a synthetic italic (its `90-synthetic.conf`).
const SYNTHETIC_SHEAR: f32 = 0.2;
/// Thickness added by a synthetic bold, as a fraction of the em (FreeType's `em / 24`).
const SYNTHETIC_BOLD_EM: f32 = 1.0 / 24.0;

/// A font as the label draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFace {
    pub path: PathBuf,
    /// The face inside a font collection (`.ttc`); 0 for an ordinary font file.
    pub index: u32,
    /// Variable-font axes to set so that the file's default outlines become the cut fontconfig
    /// picked: `wght` and `wdth`.
    pub variations: Vec<([u8; 4], f32)>,
    /// The family has no bold cut: thicken the outlines.
    pub embolden: bool,
    /// The family has no italic cut: slant the letters by this much (0 = upright).
    pub shear: f32,
}

impl FontFace {
    /// A font file used as it is.
    pub fn file(path: impl Into<PathBuf>, index: u32) -> FontFace {
        FontFace { path: path.into(), index, variations: Vec::new(), embolden: false, shear: 0.0 }
    }
}

/// How fontconfig is asked: the pattern, and whether the whole sorted list is wanted (`-s`).
/// Returns what `fc-match` printed, `None` when it cannot be run.
pub type Matcher<'a> = &'a dyn Fn(&str, bool) -> Option<String>;

/// The real thing: runs `fc-match`.
pub fn run_fc_match(pattern: &str, sorted: bool) -> Option<String> {
    let mut c = std::process::Command::new("fc-match");
    c.args(["-f", FORMAT]);
    if sorted {
        c.arg("-s");
    }
    c.arg("--").arg(pattern);
    let o = run_capture(c, Duration::from_secs(10), "fc-match").ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

pub(super) fn has_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

// ---------------------------------------------------------------------------------------------
// Asking fontconfig
// ---------------------------------------------------------------------------------------------

fn usable_family(f: &str) -> bool {
    !f.is_empty() && f.len() <= 120 && !f.chars().any(char::is_control)
}

/// The characters that separate the parts of a fontconfig pattern.
fn escape_family(f: &str) -> String {
    let mut out = String::with_capacity(f.len() + 4);
    for c in f.chars() {
        if matches!(c, '\\' | '-' | ':' | ',' | '=') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn style_suffix(bold: bool, italic: bool) -> String {
    let mut s = String::new();
    if bold {
        s.push_str(":weight=bold");
    }
    if italic {
        s.push_str(":slant=italic");
    }
    s
}

/// The fontconfig pattern for a family with a style. No family (or one that cannot be a family
/// name) means the default sans-serif font, one with Japanese letters when `japanese`.
pub fn match_pattern(family: Option<&str>, bold: bool, italic: bool, japanese: bool) -> String {
    let base = match family.map(str::trim).filter(|f| usable_family(f)) {
        Some(f) => escape_family(f),
        None if japanese => "sans:lang=ja".to_string(),
        None => "sans".to_string(),
    };
    base + &style_suffix(bold, italic)
}

/// fontconfig's weight scale (`FC_WEIGHT_*`) as OpenType's 100..=1000, by the same table
/// fontconfig itself uses (`FcWeightToOpenType`), interpolated between the entries.
pub fn fc_weight_to_opentype(fc: f32) -> f32 {
    const TABLE: [(f32, f32); 12] = [
        (0.0, 100.0),
        (40.0, 200.0),
        (50.0, 300.0),
        (55.0, 350.0),
        (75.0, 380.0),
        (80.0, 400.0),
        (100.0, 500.0),
        (180.0, 600.0),
        (200.0, 700.0),
        (205.0, 800.0),
        (210.0, 900.0),
        (215.0, 1000.0),
    ];
    if fc <= TABLE[0].0 {
        return TABLE[0].1;
    }
    for w in TABLE.windows(2) {
        let ((a, oa), (b, ob)) = (w[0], w[1]);
        if fc <= b {
            return oa + (ob - oa) * (fc - a) / (b - a);
        }
    }
    TABLE[TABLE.len() - 1].1
}

/// A family name that says "Japanese" (Noto Sans CJK JP, Source Han Sans JP, ...), used to prefer
/// the Japanese cut of a CJK collection when the text is Japanese.
fn is_japanese_name(family: &str) -> bool {
    let lower = family.to_lowercase();
    lower.split(|c: char| !c.is_alphanumeric()).any(|w| matches!(w, "jp" | "jpn" | "japanese"))
        || lower.contains("ゴシック")
        || lower.contains("明朝")
        || lower.contains("日本")
}

/// One line of `FORMAT` as a font and its family names. The file must exist.
fn parse_line(line: &str) -> Option<(FontFace, String)> {
    let mut f = line.trim_end().rsplitn(8, '|');
    let family = f.next()?.to_string();
    let matrix = f.next()?;
    let embolden = f.next()?;
    let variable = f.next()?;
    let _slant = f.next()?;
    let weight = f.next()?;
    let index: u32 = f.next()?.trim().parse().ok()?;
    let path = PathBuf::from(f.next()?);
    if !path.is_file() {
        return None;
    }
    // fontconfig packs a named instance of a variable font into the upper half of the index.
    let (face, instance) = (index & 0xFFFF, index >> 16);
    let mut variations = Vec::new();
    if instance != 0 || variable.trim() == "True" {
        if let Ok(w) = weight.trim().parse::<f32>() {
            variations.push((*b"wght", fc_weight_to_opentype(w)));
        }
    }
    // "1 0.2 0 1": the matrix of a synthetic italic; its second number is the shear.
    let shear = matrix
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|s| s.abs() > 0.01 && s.abs() < 0.6)
        .unwrap_or(0.0);
    Some((
        FontFace { path, index: face, variations, embolden: embolden.trim() == "True", shear },
        family,
    ))
}

/// The font for a family and style. `None` when fontconfig is missing or names no readable file.
pub fn resolve_face(
    matcher: Matcher<'_>,
    family: Option<&str>,
    bold: bool,
    italic: bool,
    japanese: bool,
) -> Option<FontFace> {
    let pattern = match_pattern(family, bold, italic, japanese);
    // Japanese text in the default font: look through the best matches for the Japanese cut
    // (fontconfig ranks the cuts of a CJK collection alike and may put the Korean one first).
    let prefer_japanese = japanese && family.map(str::trim).is_none_or(|f| !usable_family(f));
    let out = matcher(&pattern, prefer_japanese)?;
    let found: Vec<(FontFace, String)> = out.lines().filter_map(parse_line).collect();
    if prefer_japanese {
        if let Some(hit) = found.iter().take(16).find(|(_, fam)| is_japanese_name(fam)) {
            return Some(hit.0.clone());
        }
    }
    found.into_iter().next().map(|(f, _)| f)
}

/// A readable regular font from the well-known places, with the style made up when asked for.
fn well_known(bold: bool, italic: bool) -> Option<FontFace> {
    let path = WELL_KNOWN.iter().map(PathBuf::from).find(|p| p.is_file())?;
    let mut f = FontFace::file(path, 0);
    f.embolden = bold;
    f.shear = if italic { SYNTHETIC_SHEAR } else { 0.0 };
    Some(f)
}

/// A font file for a sample of text, with its face index: `WASHI_FONT`, else fontconfig's default
/// sans-serif (a Japanese one for Japanese text), else a few well-known paths.
pub fn find_font(sample: &str) -> Option<(PathBuf, u32)> {
    if let Some(p) = std::env::var_os("WASHI_FONT").map(PathBuf::from).filter(|p| p.exists()) {
        return Some((p, 0));
    }
    let face = resolve_face(&run_fc_match, None, false, false, has_cjk(sample))
        .or_else(|| well_known(false, false))?;
    Some((face.path, face.index))
}

// ---------------------------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------------------------

/// Font files and fontconfig answers kept for one rendering, so that a label with five texts in
/// one family reads the file (a CJK collection is 19 MB) and asks fontconfig once.
#[derive(Default)]
pub(super) struct FontCache {
    files: HashMap<PathBuf, Arc<Vec<u8>>>,
    answers: HashMap<String, Option<FontFace>>,
}

impl FontCache {
    fn resolve(
        &mut self,
        matcher: Matcher<'_>,
        family: Option<&str>,
        bold: bool,
        italic: bool,
        japanese: bool,
    ) -> Option<FontFace> {
        let key = format!("{}|{}", match_pattern(family, bold, italic, japanese), japanese);
        self.answers
            .entry(key)
            .or_insert_with(|| resolve_face(matcher, family, bold, italic, japanese))
            .clone()
    }

    fn read(&mut self, path: &Path) -> Result<Arc<Vec<u8>>> {
        if let Some(d) = self.files.get(path) {
            return Ok(d.clone());
        }
        let data = Arc::new(
            std::fs::read(path)
                .map_err(|e| Error::io(format!("cannot read the font {}", path.display()), e))?,
        );
        self.files.insert(path.to_path_buf(), data.clone());
        Ok(data)
    }
}

struct Loaded {
    face: FontFace,
    data: Arc<Vec<u8>>,
}

impl Loaded {
    fn open(face: FontFace, data: Arc<Vec<u8>>) -> Result<Loaded> {
        FontRef::try_from_slice_and_index(&data, face.index).map_err(|_| {
            Error::Invalid(format!("{} is not a usable font file", face.path.display()))
        })?;
        Ok(Loaded { face, data })
    }

    fn font(&self) -> FontRef<'_> {
        // Checked in `open`.
        let mut f = FontRef::try_from_slice_and_index(&self.data, self.face.index)
            .expect("font was validated");
        for (tag, value) in &self.face.variations {
            f.set_variation(tag, *value);
        }
        f
    }

    fn covers(&self, c: char) -> bool {
        self.font().glyph_id(c) != GlyphId(0)
    }
}

/// The fonts of one text: the one it asked for, then those that supply letters it lacks.
pub(super) struct FontSet {
    faces: Vec<Loaded>,
}

/// The letters to look at when judging whether a font has what the text needs.
fn needed(text: &str) -> Vec<char> {
    let mut v: Vec<char> = text.chars().filter(|c| !c.is_whitespace() && !c.is_control()).collect();
    v.sort_unstable();
    v.dedup();
    v
}

impl FontSet {
    /// Fonts for `item`. A font file named on the item is used as it is; otherwise the family and
    /// style go to fontconfig, and what it cannot give falls back to the default sans-serif and,
    /// last, to a few well-known files. An error only when no font at all can be read.
    pub(super) fn load(
        item: &TextItem,
        matcher: Matcher<'_>,
        cache: &mut FontCache,
    ) -> Result<FontSet> {
        let japanese = has_cjk(&item.text);
        let family = item.family.as_deref().map(str::trim).filter(|f| usable_family(f));
        let primary = if let Some(p) = &item.font {
            FontFace::file(p, 0)
        } else if let Some(p) = std::env::var_os("WASHI_FONT")
            .map(PathBuf::from)
            .filter(|p| family.is_none() && p.exists())
        {
            FontFace::file(p, 0)
        } else {
            cache
                .resolve(matcher, family, item.bold, item.italic, japanese)
                .or_else(|| {
                    // The family is unknown to this fontconfig, or fontconfig is not there.
                    family.and_then(|_| {
                        cache.resolve(matcher, None, item.bold, item.italic, japanese)
                    })
                })
                .or_else(|| well_known(item.bold, item.italic))
                .ok_or_else(|| {
                    Error::Invalid("no font found for the label text; pass a font file".into())
                })?
        };
        let data = cache.read(&primary.path)?;
        let mut set = FontSet { faces: vec![Loaded::open(primary, data)?] };

        // Letters the font lacks come from other fonts, one at a time, a few rounds at most.
        let mut missing: Vec<char> = needed(&item.text);
        for _ in 0..3 {
            missing.retain(|c| !set.faces.iter().any(|f| f.covers(*c)));
            let Some(&first) = missing.first() else { break };
            let face = if is_cjk(first) {
                cache.resolve(matcher, None, item.bold, item.italic, true)
            } else {
                let key = format!("sans:charset={:x}", first as u32)
                    + &style_suffix(item.bold, item.italic);
                let out = matcher(&key, false);
                out.and_then(|o| o.lines().find_map(parse_line)).map(|(f, _)| f)
            };
            let loaded = face
                .and_then(|f| cache.read(&f.path).ok().and_then(|d| Loaded::open(f, d).ok()))
                .filter(|l| l.covers(first));
            match loaded {
                Some(l) => set.faces.push(l),
                // Nothing on this computer has that letter: it stays a box, the others go on.
                None => missing.retain(|c| *c != first),
            }
        }
        Ok(set)
    }

    pub(super) fn styled(&self) -> Vec<StyledFont<'_>> {
        self.faces
            .iter()
            .map(|l| StyledFont { font: l.font(), embolden: l.face.embolden, shear: l.face.shear })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

/// A font ready to draw with, and the style that has to be made up for it.
pub(super) struct StyledFont<'a> {
    pub font: FontRef<'a>,
    pub embolden: bool,
    pub shear: f32,
}

impl StyledFont<'_> {
    /// The scale for a font whose em is `px_size` pixels.
    fn scale(&self, px_size: f32) -> PxScale {
        self.font.pt_to_px_scale(px_size).unwrap_or(PxScale::from(px_size))
    }

    fn scaled(&self, px_size: f32) -> PxScaleFont<&FontRef<'_>> {
        self.font.as_scaled(self.scale(px_size))
    }

    /// Extra advance of a made-up bold.
    fn extra_advance(&self, px_size: f32) -> f32 {
        if self.embolden { px_size * SYNTHETIC_BOLD_EM } else { 0.0 }
    }
}

/// The index of the first font that has the letter (the first font when none has).
pub(super) fn pick(fonts: &[StyledFont<'_>], ch: char) -> usize {
    fonts.iter().position(|f| f.font.glyph_id(ch) != GlyphId(0)).unwrap_or(0)
}

/// How far the pen moves after a letter, in pixels.
pub(super) fn advance_px(fonts: &[StyledFont<'_>], px_size: f32, ch: char) -> f32 {
    let f = &fonts[pick(fonts, ch)];
    let s = f.scaled(px_size);
    s.h_advance(s.glyph_id(ch)) + f.extra_advance(px_size)
}

/// A drawn line: its coverage, where the pen starts (`left`) and where the baseline is.
pub(super) struct Line {
    pub cov: GrayImage,
    pub left: f32,
    pub baseline: f32,
}

fn deposit(img: &mut GrayImage, x: i64, y: i64, c: f32, add: bool) {
    if x < 0 || y < 0 || x >= i64::from(img.width()) || y >= i64::from(img.height()) {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let cur = f32::from(p.0[0]);
    let v = if add { cur + c * 255.0 } else { cur.max(c * 255.0) };
    *p = Luma([v.min(255.0) as u8]);
}

/// Draw one glyph into `img`. `shear` leans the letter right above the baseline; with `add`
/// false the coverage is merged by taking the larger value (for the copies a made-up bold is
/// built from).
fn draw_glyph(
    img: &mut GrayImage,
    font: &FontRef<'_>,
    g: Glyph,
    shear: f32,
    baseline: f32,
    add: bool,
) {
    let Some(o) = font.outline_glyph(g) else { return };
    let b = o.px_bounds();
    o.draw(|gx, gy, c| {
        let py = b.min.y as i64 + i64::from(gy);
        if shear == 0.0 {
            deposit(img, b.min.x as i64 + i64::from(gx), py, c, add);
            return;
        }
        // A whole row moves by the same amount, split between two pixels: no gaps.
        let fx = b.min.x.floor() + gx as f32 + shear * (baseline - (py as f32 + 0.5));
        let x0 = fx.floor();
        let t = fx - x0;
        deposit(img, x0 as i64, py, c * (1.0 - t), add);
        if t > 0.001 {
            deposit(img, x0 as i64 + 1, py, c * t, add);
        }
    });
}

/// Coverage mask of one line of text, set with the fonts that have its letters.
pub(super) fn render_line(fonts: &[StyledFont<'_>], px_size: f32, line: &str) -> Line {
    let chars: Vec<char> = line.chars().collect();
    let which: Vec<usize> = chars.iter().map(|c| pick(fonts, *c)).collect();
    // The lines' metrics are those of the fonts actually used.
    let mut used = which.clone();
    used.sort_unstable();
    used.dedup();
    if used.is_empty() {
        used.push(0);
    }
    let (mut ascent, mut descent, mut shear, mut bold_r) = (0f32, 0f32, 0f32, 0f32);
    for &i in &used {
        let s = fonts[i].scaled(px_size);
        ascent = ascent.max(s.ascent());
        descent = descent.max(-s.descent());
        shear = shear.max(fonts[i].shear);
        if fonts[i].embolden {
            bold_r = bold_r.max(px_size * SYNTHETIC_BOLD_EM / 2.0);
        }
    }
    let pad = 1.0 + bold_r.ceil();
    let left = pad + (shear * descent).ceil();
    let baseline = ascent + pad;

    let mut x = 0f32;
    let mut prev: Option<(usize, GlyphId)> = None;
    let mut glyphs: Vec<(usize, Glyph)> = Vec::with_capacity(chars.len());
    for (ch, &i) in chars.iter().zip(&which) {
        let s = fonts[i].scaled(px_size);
        let id = s.glyph_id(*ch);
        if let Some((pi, pid)) = prev {
            if pi == i {
                x += s.kern(pid, id);
            }
        }
        glyphs.push((
            i,
            id.with_scale_and_position(fonts[i].scale(px_size), point(left + x, baseline)),
        ));
        x += s.h_advance(id) + fonts[i].extra_advance(px_size);
        prev = Some((i, id));
    }
    let w = (left + x + pad + shear * ascent).ceil().max(1.0) as u32 + 2;
    let h = (ascent + descent + 2.0 * pad).ceil().max(1.0) as u32 + 1;
    let mut cov = GrayImage::new(w, h);
    for (i, g) in glyphs {
        let f = &fonts[i];
        if !f.embolden {
            draw_glyph(&mut cov, &f.font, g, f.shear, baseline, true);
            continue;
        }
        // A made-up bold: the glyph drawn again around its place, the copies merged.
        let r = px_size * SYNTHETIC_BOLD_EM / 2.0;
        let n = if r > 4.0 { 16 } else { 8 };
        draw_glyph(&mut cov, &f.font, g.clone(), f.shear, baseline, false);
        for k in 0..n {
            let a = std::f32::consts::TAU * k as f32 / n as f32;
            let mut copy = g.clone();
            copy.position = point(g.position.x + r * a.cos(), g.position.y + r * a.sin());
            draw_glyph(&mut cov, &f.font, copy, f.shear, baseline, false);
        }
    }
    Line { cov, left, baseline }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake `fc-match`: answers every pattern with `reply` (once per font for `-s`).
    fn answering(reply: String) -> impl Fn(&str, bool) -> Option<String> {
        move |_, _| Some(reply.clone())
    }

    fn item(text: &str) -> TextItem {
        TextItem::new(text)
    }

    fn line_for(path: &Path, index: u32, weight: &str, extra: &str, family: &str) -> String {
        // file | index | weight | slant | variable | embolden | matrix | family
        let (variable, embolden, matrix) = match extra {
            "variable" => ("True", "", ""),
            "bold" => ("False", "True", ""),
            "italic" => ("False", "", "1 0.2 0 1"),
            _ => ("False", "", ""),
        };
        format!("{}|{index}|{weight}|0|{variable}|{embolden}|{matrix}|{family}\n", path.display())
    }

    #[test]
    fn the_pattern_carries_family_and_style_and_escapes_the_family() {
        assert_eq!(match_pattern(Some("Noto Sans"), false, false, false), "Noto Sans");
        assert_eq!(
            match_pattern(Some("Noto Sans CJK JP"), true, true, true),
            "Noto Sans CJK JP:weight=bold:slant=italic"
        );
        assert_eq!(match_pattern(Some("  Hack  "), true, false, false), "Hack:weight=bold");
        // Characters that mean something in a pattern lose it.
        assert_eq!(
            match_pattern(Some("A-B:C,D=E\\F"), false, false, false),
            "A\\-B\\:C\\,D\\=E\\\\F"
        );
        // No usable family: the default sans-serif; Japanese text asks for a Japanese one.
        assert_eq!(match_pattern(None, true, false, false), "sans:weight=bold");
        assert_eq!(match_pattern(None, false, true, true), "sans:lang=ja:slant=italic");
        assert_eq!(match_pattern(Some(""), false, false, false), "sans");
        assert_eq!(match_pattern(Some("bad\nname"), false, false, false), "sans");
        assert_eq!(match_pattern(Some(&"x".repeat(500)), false, false, false), "sans");
        // A family that starts with a dash cannot pass for a program option.
        assert!(match_pattern(Some("-evil"), false, false, false).starts_with('\\'));
    }

    #[test]
    fn weights_map_to_opentype_like_fontconfig() {
        for (fc, ot) in [
            (0.0, 100.0),
            (80.0, 400.0),
            (100.0, 500.0),
            (180.0, 600.0),
            (200.0, 700.0),
            (210.0, 900.0),
            (300.0, 1000.0),
        ] {
            assert!((fc_weight_to_opentype(fc) - ot).abs() < 0.01, "{fc}");
        }
        // Between two entries it is interpolated: 140 is half way from 100 to 180.
        assert!((fc_weight_to_opentype(140.0) - 550.0).abs() < 0.01);
        assert!(fc_weight_to_opentype(-5.0) <= 100.0);
    }

    #[test]
    fn what_fc_match_prints_becomes_a_face() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Some.ttc");
        std::fs::write(&file, b"x").unwrap();
        let (f, fam) =
            parse_line(&line_for(&file, 2, "80", "", "Some Sans,Some Sans Regular")).unwrap();
        assert_eq!((f.index, f.embolden, f.shear), (2, false, 0.0), "a face of a collection");
        assert!(f.variations.is_empty());
        assert_eq!(fam, "Some Sans,Some Sans Regular");
        // A named instance of a variable font: the instance is in the upper half of the index.
        let (f, _) = parse_line(&line_for(&file, 0x4_0000, "200", "", "Var")).unwrap();
        assert_eq!(f.index, 0, "only the face is the index");
        assert_eq!(f.variations, vec![(*b"wght", 700.0)]);
        // The whole variable font.
        let (f, _) = parse_line(&line_for(&file, 0, "80", "variable", "Var")).unwrap();
        assert_eq!(f.variations, vec![(*b"wght", 400.0)]);
        // Made-up styles.
        let (f, _) = parse_line(&line_for(&file, 0, "80", "bold", "Plain")).unwrap();
        assert!(f.embolden && f.shear == 0.0);
        let (f, _) = parse_line(&line_for(&file, 0, "80", "italic", "Plain")).unwrap();
        assert!(!f.embolden && (f.shear - 0.2).abs() < 1e-6);
        // Not a font file, or not our format.
        assert!(parse_line(&line_for(&dir.path().join("missing.ttf"), 0, "80", "", "x")).is_none());
        assert!(parse_line("garbage").is_none());
        assert!(parse_line("").is_none());
        // A path with a bar in it survives.
        let odd = dir.path().join("a|b.ttf");
        std::fs::write(&odd, b"x").unwrap();
        assert_eq!(parse_line(&line_for(&odd, 0, "80", "", "x")).unwrap().0.path, odd);
    }

    #[test]
    fn japanese_names_are_told_apart() {
        for yes in [
            "Noto Sans CJK JP",
            "Noto Sans CJK JP,Noto Sans CJK JP Regular",
            "Source Han Sans JP",
            "IPAex ゴシック",
            "Japanese Sans",
        ] {
            assert!(is_japanese_name(yes), "{yes}");
        }
        for no in ["Noto Sans CJK KR", "Noto Sans CJK SC", "DejaVu Sans", "Jpeg Sans Mono", "Hack"]
        {
            assert!(!is_japanese_name(no), "{no}");
        }
    }

    #[test]
    fn the_japanese_cut_wins_among_equal_matches_for_japanese_text() {
        let dir = tempfile::tempdir().unwrap();
        let ttc = dir.path().join("Cjk.ttc");
        std::fs::write(&ttc, b"x").unwrap();
        // fontconfig ranks the Korean cut first; the Japanese one comes a few lines later.
        let reply = format!(
            "{}{}{}",
            line_for(&ttc, 1, "80", "", "Noto Sans CJK KR"),
            line_for(&ttc, 2, "80", "", "Noto Sans CJK SC"),
            line_for(&ttc, 0, "80", "", "Noto Sans CJK JP")
        );
        let m = answering(reply);
        let f = resolve_face(&m, None, false, false, true).unwrap();
        assert_eq!(f.index, 0, "the JP face");
        // Latin text takes the best match as it is.
        let f = resolve_face(&m, None, false, false, false).unwrap();
        assert_eq!(f.index, 1);
        // So does a family the user picked: that is their choice.
        let f = resolve_face(&m, Some("Noto Sans CJK KR"), false, false, true).unwrap();
        assert_eq!(f.index, 1);
    }

    #[test]
    fn without_fontconfig_a_well_known_font_stands_in_with_the_style_made_up() {
        let nothing = |_: &str, _: bool| None;
        let mut cache = FontCache::default();
        let mut it = item("Disc");
        it.family = Some("No Such Family".into());
        it.bold = true;
        it.italic = true;
        match FontSet::load(&it, &nothing, &mut cache) {
            Ok(set) => {
                let styled = set.styled();
                assert!(styled[0].embolden && styled[0].shear > 0.1, "regular file, style made up");
            }
            Err(e) => {
                // No well-known font on this computer: the error says what to do.
                assert!(e.to_string().contains("pass a font file"), "{e}");
            }
        }
    }

    #[test]
    fn an_answer_that_is_not_a_readable_font_falls_through() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.ttf");
        std::fs::write(&junk, b"this is not a font").unwrap();
        let m = answering(line_for(&junk, 0, "80", "", "Junk"));
        let mut cache = FontCache::default();
        let mut it = item("Disc");
        it.family = Some("Junk".into());
        // fontconfig names a file that is no font: neither it nor the well-known list is
        // pretended to work; the user gets the error about the file, not a crash.
        match FontSet::load(&it, &m, &mut cache) {
            Err(e) => assert!(e.to_string().contains("not a usable font"), "{e}"),
            Ok(_) => panic!("junk was taken for a font"),
        }
        // A missing file named on the item is an error too.
        it.font = Some(dir.path().join("nope.ttf"));
        assert!(FontSet::load(&it, &m, &mut cache).is_err());
    }

    fn real_font() -> Option<PathBuf> {
        find_font("Washi").map(|(p, _)| p)
    }

    fn ink(l: &Line) -> u64 {
        l.cov.pixels().map(|p| u64::from(p.0[0])).sum()
    }

    /// Mean x of the ink in rows `y0..y1`.
    fn centre_x(l: &Line, y0: u32, y1: u32) -> f64 {
        let (mut sum, mut n) = (0f64, 0f64);
        for y in y0..y1.min(l.cov.height()) {
            for x in 0..l.cov.width() {
                let v = f64::from(l.cov.get_pixel(x, y).0[0]);
                sum += v * f64::from(x);
                n += v;
            }
        }
        sum / n.max(1.0)
    }

    #[test]
    fn a_made_up_bold_thickens_and_a_made_up_italic_leans() {
        let Some(path) = real_font() else {
            eprintln!("no font on this system; skipping");
            return;
        };
        let data = std::fs::read(&path).unwrap();
        let (_, index) = find_font("Washi").unwrap();
        let mk = |embolden: bool, shear: f32| {
            vec![StyledFont {
                font: FontRef::try_from_slice_and_index(&data, index).unwrap(),
                embolden,
                shear,
            }]
        };
        let plain = render_line(&mk(false, 0.0), 80.0, "Hill");
        let bold = render_line(&mk(true, 0.0), 80.0, "Hill");
        assert!(ink(&bold) as f64 > ink(&plain) as f64 * 1.12, "{} vs {}", ink(&bold), ink(&plain));
        assert!(bold.cov.width() > plain.cov.width(), "a bold line is wider");
        // Vertical bars only: upright ones have the same centre at every height.
        let plain = render_line(&mk(false, 0.0), 80.0, "IIII");
        let lean = render_line(&mk(false, 0.2), 80.0, "IIII");
        // The top of an italic line sits further right than its bottom; an upright one does not.
        let (h, base) = (lean.cov.height(), lean.baseline as u32);
        let drift = |l: &Line| centre_x(l, 0, base / 2) - centre_x(l, base - base / 4, base);
        assert!(drift(&lean) > 4.0, "leans: {}", drift(&lean));
        assert!(drift(&plain).abs() < 1.5, "upright: {}", drift(&plain));
        assert!(h > 0 && ink(&lean) > 0);
        // The ink stays inside the picture of the line (nothing cut at the edges).
        let edge =
            |l: &Line| (0..l.cov.height()).any(|y| l.cov.get_pixel(l.cov.width() - 1, y).0[0] > 40);
        assert!(!edge(&lean) && !edge(&bold));
    }

    #[test]
    fn a_variable_font_is_set_to_the_weight_fontconfig_picked() {
        let Some(path) = [
            "/usr/share/fonts/cantarell/Cantarell-VF.otf",
            "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file()) else {
            eprintln!("no variable font here; skipping");
            return;
        };
        let data = Arc::new(std::fs::read(&path).unwrap());
        let load = |w: f32| {
            let mut f = FontFace::file(&path, 0);
            f.variations.push((*b"wght", w));
            Loaded::open(f, data.clone()).unwrap()
        };
        let (light, heavy) = (load(300.0), load(800.0));
        let draw = |l: &Loaded| {
            let fonts = vec![StyledFont { font: l.font(), embolden: false, shear: 0.0 }];
            ink(&render_line(&fonts, 60.0, "Hill"))
        };
        assert!(
            draw(&heavy) as f64 > draw(&light) as f64 * 1.3,
            "{} vs {}",
            draw(&heavy),
            draw(&light)
        );
    }

    fn label_with(t: TextItem) -> super::super::DiscLabel {
        super::super::DiscLabel { texts: vec![t], ..super::super::DiscLabel::default() }
    }

    #[test]
    fn a_named_family_renders_in_that_family_with_its_bold_cut() {
        // Needs fontconfig and two families with a bold cut.
        let Some(reg) = resolve_face(&run_fc_match, Some("DejaVu Sans"), false, false, false)
        else {
            eprintln!("no fontconfig; skipping");
            return;
        };
        let bold = resolve_face(&run_fc_match, Some("DejaVu Sans"), true, false, false).unwrap();
        if !reg.path.to_string_lossy().contains("DejaVuSans.ttf") || bold.path == reg.path {
            eprintln!("DejaVu Sans is not installed; skipping");
            return;
        }
        let mut t = item("Disc label");
        t.family = Some("DejaVu Sans".into());
        t.color = [255, 0, 0];
        t.size_pt = 30.0;
        let reds =
            |im: &image::RgbImage| im.pixels().filter(|p| p.0[0] > 200 && p.0[1] < 90).count();
        let regular = reds(&label_with(t.clone()).render_sized(600).unwrap());
        t.bold = true;
        let heavy = reds(&label_with(t).render_sized(600).unwrap());
        assert!(heavy as f64 > regular as f64 * 1.1, "{heavy} vs {regular}");
    }

    #[test]
    fn japanese_families_render_japanese_text_from_the_right_face() {
        // Noto Sans CJK JP is face 0 of a collection; KR is face 1. Needs the font.
        let jp = resolve_face(&run_fc_match, Some("Noto Sans CJK JP"), false, false, true);
        let kr = resolve_face(&run_fc_match, Some("Noto Sans CJK KR"), false, false, true);
        let (Some(jp), Some(kr)) = (jp, kr) else {
            eprintln!("no Noto Sans CJK; skipping");
            return;
        };
        if jp.path != kr.path || jp.index == kr.index {
            eprintln!("the CJK cuts are not in one collection here; skipping");
            return;
        }
        let mut t = item("日本語 ありがとう");
        t.family = Some("Noto Sans CJK JP".into());
        t.color = [255, 0, 0];
        t.size_pt = 30.0;
        let mut cache = FontCache::default();
        let set = FontSet::load(&t, &run_fc_match, &mut cache).unwrap();
        assert_eq!(set.faces.len(), 1, "the family has every letter itself");
        assert_eq!(set.faces[0].face.index, jp.index);
        assert!(set.faces[0].covers('日'));
        let img = label_with(t.clone()).render_sized(900).unwrap();
        let n = img.pixels().filter(|p| p.0[0] > 200 && p.0[1] < 90).count();
        assert!(n > 800, "Japanese ink: {n}");
        // Bold uses the bold collection.
        t.bold = true;
        let set = FontSet::load(&t, &run_fc_match, &mut cache).unwrap();
        assert!(
            set.faces[0].face.path.to_string_lossy().contains("Bold"),
            "{:?}",
            set.faces[0].face.path
        );
        assert!(set.faces[0].covers('日'));
    }

    #[test]
    fn letters_a_family_lacks_come_from_another_font() {
        let Some(latin) = resolve_face(&run_fc_match, Some("DejaVu Sans"), false, false, false)
        else {
            eprintln!("no fontconfig; skipping");
            return;
        };
        let cjk = resolve_face(&run_fc_match, None, false, false, true);
        if cjk.is_none_or(|c| c.path == latin.path) {
            eprintln!("no second font with Japanese letters; skipping");
            return;
        }
        if !latin.path.to_string_lossy().contains("DejaVuSans") {
            eprintln!("DejaVu Sans is not installed; skipping");
            return;
        }
        let mut t = item("Mix 夏");
        t.family = Some("DejaVu Sans".into());
        t.color = [255, 0, 0];
        t.size_pt = 30.0;
        let mut cache = FontCache::default();
        let set = FontSet::load(&t, &run_fc_match, &mut cache).unwrap();
        if !set.faces[0].covers('夏') && set.faces.len() == 1 {
            eprintln!("no font here has that letter; skipping");
            return;
        }
        assert_eq!(set.faces.len(), 2, "the Latin font plus one for 夏");
        let styled = set.styled();
        assert_eq!(pick(&styled, 'M'), 0);
        assert_eq!(pick(&styled, '夏'), 1);
        // Both parts are inked.
        let l = render_line(&styled, 60.0, "Mix 夏");
        let w = l.cov.width();
        let right: u64 = (w * 3 / 4..w)
            .map(|x| {
                (0..l.cov.height()).map(|y| u64::from(l.cov.get_pixel(x, y).0[0])).sum::<u64>()
            })
            .sum();
        assert!(right > 0, "the last letter is drawn");
        // A letter nothing has stays a box and does not stop the label.
        let mut t = item("A\u{10FFFD}");
        t.family = Some("DejaVu Sans".into());
        assert!(FontSet::load(&t, &run_fc_match, &mut cache).is_ok());
    }
}
