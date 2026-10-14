// SPDX-License-Identifier: GPL-2.0-or-later
//! Document Print: PDF (and, through LibreOffice, Office files) to PWG raster.
//!
//! The XK140 takes no PDF, so every page is rendered on the host by poppler's `pdftoppm` at
//! 600 dpi, composed onto a sheet (1, 2, 4, 6, 9 or 16 pages per sheet, duplex back sides
//! turned as the printer wants) and written as one PWG raster page per sheet side. One sheet
//! is in memory at a time; the stream goes to a spool file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use image::{GrayImage, RgbImage, imageops};
use washi_net::ipp::Sides;

use super::Orientation;
use super::layout::{self, Rect};
use super::paper::PaperSize;
use crate::error::{Error, Result};
use crate::pwg::{ColorSpace, PageHeader, PwgWriter};
use crate::tools::{Tools, run_capture, stderr_hint};

const POPPLER_TIMEOUT: Duration = Duration::from_secs(120);
const OFFICE_TIMEOUT: Duration = Duration::from_secs(240);

/// Page count and page sizes of a PDF.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfInfo {
    pub pages: u32,
    /// Size of every page in points; may be shorter than `pages` when `pdfinfo` stopped early.
    pub sizes_pt: Vec<(f64, f64)>,
}

impl PdfInfo {
    /// Size of 1-based page `n` (A4 when unknown).
    pub fn size_of(&self, n: u32) -> (f64, f64) {
        self.sizes_pt
            .get(n as usize - 1)
            .copied()
            .or_else(|| self.sizes_pt.first().copied())
            .unwrap_or((595.0, 842.0))
    }
}

fn parse_pages(text: &str) -> Option<u32> {
    text.lines().find_map(|l| l.strip_prefix("Pages:")).and_then(|v| v.trim().parse().ok())
}

fn parse_sizes(text: &str) -> Vec<(f64, f64)> {
    let mut out: Vec<(usize, (f64, f64))> = Vec::new();
    for l in text.lines() {
        // "Page    2 size:  842 x 595 pts (A4)"
        let Some(rest) = l.strip_prefix("Page ") else { continue };
        let Some((n, size)) = rest.split_once("size:") else { continue };
        let Ok(n) = n.trim().parse::<usize>() else { continue };
        let mut it = size.split_whitespace();
        let (w, x, h) = (it.next(), it.next(), it.next());
        if let (Some(w), Some("x"), Some(h)) =
            (w.and_then(|v| v.parse().ok()), x, h.and_then(|v| v.parse().ok()))
        {
            out.push((n, (w, h)));
        }
    }
    out.sort_by_key(|(n, _)| *n);
    out.into_iter().map(|(_, s)| s).collect()
}

/// Count pages and read their sizes with `pdfinfo`.
pub fn pdf_info(tools: &Tools, pdf: &Path) -> Result<PdfInfo> {
    let exe = tools.pdfinfo.as_deref().ok_or_else(|| {
        Error::tool(
            "pdfinfo",
            "not installed (install poppler: the `poppler-utils` or `poppler` package)",
        )
    })?;
    let mut c = Command::new(exe);
    c.arg(pdf);
    let o = run_capture(c, POPPLER_TIMEOUT, "pdfinfo")?;
    if !o.status.success() {
        return Err(Error::tool("pdfinfo", format!("{}: {}", pdf.display(), stderr_hint(&o))));
    }
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    if text.lines().any(|l| l.starts_with("Encrypted:") && l.contains("yes")) {
        return Err(Error::Invalid(format!(
            "{} is an encrypted PDF; remove the password first",
            pdf.display()
        )));
    }
    let pages = parse_pages(&text)
        .filter(|n| *n > 0)
        .ok_or_else(|| Error::tool("pdfinfo", format!("{} has no pages", pdf.display())))?;
    let mut c = Command::new(exe);
    c.args(["-f", "1", "-l", &pages.min(5000).to_string()]).arg(pdf);
    let sizes = run_capture(c, POPPLER_TIMEOUT, "pdfinfo")
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_sizes(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    Ok(PdfInfo { pages, sizes_pt: sizes })
}

/// Convert an Office document to PDF with `soffice --headless`. The PDF lands in `workdir`.
pub fn convert_office(tools: &Tools, file: &Path, workdir: &Path) -> Result<PathBuf> {
    let exe = tools.soffice.as_deref().ok_or_else(|| {
        Error::tool("soffice", "LibreOffice is not installed, so Office documents cannot be converted; export a PDF instead")
    })?;
    let mut c = Command::new(exe);
    c.arg("--headless")
        .arg("--norestore")
        .arg(format!("-env:UserInstallation=file://{}", workdir.join("lo-profile").display()))
        .args(["--convert-to", "pdf", "--outdir"])
        .arg(workdir)
        .arg(file);
    let o = run_capture(c, OFFICE_TIMEOUT, "soffice")?;
    let pdf = workdir.join(file.file_stem().unwrap_or_default()).with_extension("pdf");
    if !o.status.success() || !pdf.exists() {
        return Err(Error::tool(
            "soffice",
            format!("conversion of {} failed: {}", file.display(), stderr_hint(&o)),
        ));
    }
    Ok(pdf)
}

// ---------------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------------

enum Raster {
    Rgb(RgbImage),
    Gray(GrayImage),
}

impl Raster {
    fn dims(&self) -> (u32, u32) {
        match self {
            Raster::Rgb(i) => i.dimensions(),
            Raster::Gray(i) => i.dimensions(),
        }
    }

    fn rotated(self) -> Raster {
        match self {
            Raster::Rgb(i) => Raster::Rgb(imageops::rotate90(&i)),
            Raster::Gray(i) => Raster::Gray(imageops::rotate90(&i)),
        }
    }
}

/// Parse the `P6` / `P5` file `pdftoppm` writes.
fn read_pnm(bytes: &[u8]) -> Result<Raster> {
    let bad = || Error::tool("pdftoppm", "unreadable image data");
    let mut pos = 0usize;
    let token = |pos: &mut usize| -> Option<String> {
        while bytes.get(*pos).is_some_and(|b| b.is_ascii_whitespace()) {
            *pos += 1;
        }
        let start = *pos;
        while bytes.get(*pos).is_some_and(|b| !b.is_ascii_whitespace()) {
            *pos += 1;
        }
        (*pos > start).then(|| String::from_utf8_lossy(&bytes[start..*pos]).into_owned())
    };
    let magic = token(&mut pos).ok_or_else(bad)?;
    let w: u32 = token(&mut pos).and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let h: u32 = token(&mut pos).and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let max: u32 = token(&mut pos).and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    if max != 255 {
        return Err(bad());
    }
    pos += 1; // the single whitespace byte after the header
    let body = bytes.get(pos..).ok_or_else(bad)?;
    let channels = match magic.as_str() {
        "P6" => 3usize,
        "P5" => 1,
        _ => return Err(bad()),
    };
    let need = w as usize * h as usize * channels;
    if body.len() < need || w == 0 || h == 0 {
        return Err(bad());
    }
    let data = body[..need].to_vec();
    Ok(if channels == 3 {
        Raster::Rgb(RgbImage::from_raw(w, h, data).ok_or_else(bad)?)
    } else {
        Raster::Gray(GrayImage::from_raw(w, h, data).ok_or_else(bad)?)
    })
}

fn render_page(
    tools: &Tools,
    pdf: &Path,
    page: u32,
    dpi: f64,
    gray: bool,
    tmp: &Path,
) -> Result<Raster> {
    let exe = tools.pdftoppm.as_deref().ok_or_else(|| {
        Error::tool(
            "pdftoppm",
            "not installed (install poppler: the `poppler-utils` or `poppler` package)",
        )
    })?;
    let base = tmp.join("page");
    let mut c = Command::new(exe);
    c.args([
        "-r",
        &format!("{dpi:.3}"),
        "-f",
        &page.to_string(),
        "-l",
        &page.to_string(),
        "-singlefile",
    ]);
    if gray {
        c.arg("-gray");
    }
    c.arg(pdf).arg(&base);
    let o = run_capture(c, POPPLER_TIMEOUT, "pdftoppm")?;
    let file = base.with_extension(if gray { "pgm" } else { "ppm" });
    if !o.status.success() || !file.exists() {
        return Err(Error::tool("pdftoppm", format!("page {page}: {}", stderr_hint(&o))));
    }
    let bytes = std::fs::read(&file).map_err(|e| Error::io("cannot read the rendered page", e))?;
    let _ = std::fs::remove_file(&file);
    read_pnm(&bytes)
}

/// The sheet being composed: 8-bit RGB or grey, white.
struct Canvas {
    w: u32,
    h: u32,
    bpp: usize,
    data: Vec<u8>,
}

impl Canvas {
    fn white(w: u32, h: u32, gray: bool) -> Canvas {
        let bpp = if gray { 1 } else { 3 };
        Canvas { w, h, bpp, data: vec![0xFF; w as usize * h as usize * bpp] }
    }

    /// Copy `r` with its top-left at `(x, y)`, clipped to the sheet.
    fn blit(&mut self, r: &Raster, x: i64, y: i64) {
        let (rw, rh) = r.dims();
        let src: &[u8] = match r {
            Raster::Rgb(i) => i.as_raw(),
            Raster::Gray(i) => i.as_raw(),
        };
        let sbpp = if matches!(r, Raster::Rgb(_)) { 3 } else { 1 };
        for row in 0..i64::from(rh) {
            let dy = y + row;
            if dy < 0 || dy >= i64::from(self.h) {
                continue;
            }
            let x0 = x.max(0);
            let x1 = (x + i64::from(rw)).min(i64::from(self.w));
            if x0 >= x1 {
                continue;
            }
            for dx in x0..x1 {
                let s = (row as usize * rw as usize + (dx - x) as usize) * sbpp;
                let d = (dy as usize * self.w as usize + dx as usize) * self.bpp;
                if sbpp == self.bpp {
                    self.data[d..d + sbpp].copy_from_slice(&src[s..s + sbpp]);
                } else if sbpp == 3 {
                    // RGB source on a grey sheet (should not happen): luma.
                    let l = (u32::from(src[s]) * 299
                        + u32::from(src[s + 1]) * 587
                        + u32::from(src[s + 2]) * 114)
                        / 1000;
                    self.data[d] = l as u8;
                } else {
                    self.data[d..d + 3].fill(src[s]);
                }
            }
        }
    }

    /// A quarter turn clockwise (`w` x `h` becomes `h` x `w`).
    fn rotated_clockwise(&self) -> Canvas {
        let (w, h, bpp) = (self.w as usize, self.h as usize, self.bpp);
        let mut data = vec![0xFF; self.data.len()];
        for y in 0..h {
            for x in 0..w {
                // (x, y) lands at (h - 1 - y, x) of the new h x w canvas.
                let d = (x * h + (h - 1 - y)) * bpp;
                let s = (y * w + x) * bpp;
                data[d..d + bpp].copy_from_slice(&self.data[s..s + bpp]);
            }
        }
        Canvas { w: self.h, h: self.w, bpp, data }
    }

    fn row(&self, y: u32) -> &[u8] {
        let n = self.w as usize * self.bpp;
        &self.data[y as usize * n..(y as usize + 1) * n]
    }

    /// Back-side adjustments for duplex (see [`BackTurn`]).
    fn apply(&mut self, t: BackTurn) {
        let bpp = self.bpp;
        let n = self.w as usize * bpp;
        match t {
            BackTurn::None => {}
            BackTurn::Rotate180 => {
                let px = self.data.len() / bpp;
                for i in 0..px / 2 {
                    let j = px - 1 - i;
                    for k in 0..bpp {
                        self.data.swap(i * bpp + k, j * bpp + k);
                    }
                }
            }
            BackTurn::FlipHorizontal => {
                for row in self.data.chunks_exact_mut(n) {
                    let px = n / bpp;
                    for i in 0..px / 2 {
                        let j = px - 1 - i;
                        for k in 0..bpp {
                            row.swap(i * bpp + k, j * bpp + k);
                        }
                    }
                }
            }
            BackTurn::FlipVertical => {
                let h = self.h as usize;
                for y in 0..h / 2 {
                    let (a, b) = self.data.split_at_mut((h - 1 - y) * n);
                    a[y * n..(y + 1) * n].swap_with_slice(&mut b[..n]);
                }
            }
        }
    }
}

/// How the back side of a duplex sheet must be turned before it is rasterised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackTurn {
    None,
    Rotate180,
    FlipHorizontal,
    FlipVertical,
}

impl BackTurn {
    /// From the printer's `pwg-raster-document-sheet-back` and the binding edge (PWG 5102.4).
    /// **Not verified on the XK140** (it reports `rotated`); the first duplex print will show.
    pub fn for_sheet_back(sheet_back: &str, short_edge: bool) -> BackTurn {
        match (sheet_back, short_edge) {
            ("rotated", false) | ("manual-tumble", true) => BackTurn::Rotate180,
            ("flipped", false) => BackTurn::FlipHorizontal,
            ("flipped", true) => BackTurn::FlipVertical,
            _ => BackTurn::None,
        }
    }

    /// `(CrossFeedTransform, FeedTransform)` for the raster header.
    pub fn transforms(self) -> (i32, i32) {
        match self {
            BackTurn::None => (1, 1),
            BackTurn::Rotate180 => (-1, -1),
            BackTurn::FlipHorizontal => (-1, 1),
            BackTurn::FlipVertical => (1, -1),
        }
    }
}

/// What to rasterise and how.
pub struct RasterSpec<'a> {
    pub pdf: &'a Path,
    pub info: &'a PdfInfo,
    /// 1-based pages to print, in order.
    pub pages: &'a [u32],
    pub paper: PaperSize,
    pub layout: u8,
    pub mono: bool,
    pub sides: Sides,
    /// `pwg-raster-document-sheet-back` of the printer.
    pub sheet_back: &'a str,
    /// `MediaType` of the raster header.
    pub media_type: &'a str,
    /// 3 draft, 4 normal, 5 high.
    pub quality: u32,
    pub dpi: u32,
    /// `Auto` turns pages to suit their cell; `Portrait` / `Landscape` do not, and a landscape
    /// sheet is composed sideways and turned a quarter clockwise onto the paper.
    pub orientation: Orientation,
    /// Page Order "Reverse": the last sheet comes out first (see [`side_order`]).
    pub reverse: bool,
}

/// What was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterStats {
    /// Raster pages written (sheet sides, including a blank back side added to an odd count).
    pub sides: u32,
    /// Sheets of paper.
    pub sheets: u32,
}

/// Sheet sides needed for `pages` pages at `layout` per sheet, and the paper sheets.
pub fn count_sides(pages: usize, layout: u8, duplex: bool) -> (u32, u32) {
    let mut sides = pages.div_ceil(usize::from(layout.max(1))) as u32;
    let sheets;
    if duplex {
        sheets = sides.div_ceil(2);
        sides = sheets * 2;
    } else {
        sheets = sides;
    }
    (sides, sheets)
}

/// Which chunk of pages (one chunk = what one sheet side carries, `layout` pages) each side of
/// the output holds, in printing order; `None` is the blank back of a duplex sheet that has no
/// second chunk.
///
/// Reversing turns round the *sheets*, not the sides: a duplex sheet keeps its front before its
/// back, and a sheet keeps the pages it carries in their order. So 5 pages two-sided come out as
/// `[Some(4), None, Some(2), Some(3), Some(0), Some(1)]`: the sheet with the odd page and a blank
/// back first, and pages 3 and 4 still on the same sheet as 1 and 2 were. A plain reversal of the
/// chunk list would have put page 4 on the front of a sheet and page 3 on its back.
pub fn side_order(chunks: usize, duplex: bool, reverse: bool) -> Vec<Option<usize>> {
    let per_sheet = if duplex { 2 } else { 1 };
    let sheets = chunks.div_ceil(per_sheet);
    let order: Vec<usize> =
        if reverse { (0..sheets).rev().collect() } else { (0..sheets).collect() };
    order
        .into_iter()
        .flat_map(|sheet| {
            (0..per_sheet).map(move |j| Some(sheet * per_sheet + j).filter(|i| *i < chunks))
        })
        .collect()
}

/// Render the selected pages and write a PWG raster stream.
pub fn rasterize<W: Write>(
    tools: &Tools,
    spec: &RasterSpec<'_>,
    out: W,
    progress: &mut dyn FnMut(u32, u32),
    cancelled: &dyn Fn() -> bool,
) -> Result<RasterStats> {
    let layout = layout::check_layout(spec.layout)?;
    if spec.pages.is_empty() {
        return Err(Error::Invalid("no pages selected".into()));
    }
    if let Some(bad) = spec.pages.iter().find(|p| **p == 0 || **p > spec.info.pages) {
        return Err(Error::Invalid(format!(
            "page {bad} does not exist (the document has {} page(s))",
            spec.info.pages
        )));
    }
    let (paper_w, paper_h) = spec.paper.pixels(spec.dpi);
    let landscape = spec.orientation.is_landscape();
    // The sheet as the pages see it: on its side for Landscape.
    let (sheet_w, sheet_h) = if landscape { (paper_h, paper_w) } else { (paper_w, paper_h) };
    let turn_pages = spec.orientation == Orientation::Auto;
    let (cols, rows) = layout::grid(layout, sheet_h >= sheet_w);
    let gutter = if layout == 1 { 0 } else { (sheet_w.min(sheet_h) / 60).max(2) };
    let cells: Vec<Rect> = layout::cells(sheet_w, sheet_h, cols, rows, gutter);
    let duplex = spec.sides.is_duplex();
    let short_edge = spec.sides == Sides::TwoSidedShortEdge;
    let (total_sides, sheets) = count_sides(spec.pages.len(), layout, duplex);
    let back_turn = BackTurn::for_sheet_back(spec.sheet_back, short_edge);
    let colorspace = if spec.mono { ColorSpace::SGray } else { ColorSpace::SRgb };
    let tmp =
        tempfile::tempdir().map_err(|e| Error::io("cannot create a temporary directory", e))?;

    let mut writer = PwgWriter::new(out);
    let chunks: Vec<&[u32]> = spec.pages.chunks(usize::from(layout)).collect();
    let order = side_order(chunks.len(), duplex, spec.reverse);
    debug_assert_eq!(order.len() as u32, total_sides);
    for side in 0..total_sides {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        progress(side, total_sides);
        let mut canvas = Canvas::white(sheet_w, sheet_h, spec.mono);
        if let Some(chunk) = order.get(side as usize).copied().flatten().map(|i| &chunks[i]) {
            for (page, cell) in chunk.iter().zip(&cells) {
                let (pw_pt, ph_pt) = spec.info.size_of(*page);
                let (pw, ph) =
                    (pw_pt / 72.0 * f64::from(spec.dpi), ph_pt / 72.0 * f64::from(spec.dpi));
                // Fit the page into its cell, turned when that makes it bigger; layout 1
                // prints the page at its natural size when the sheet is the same size.
                let f = layout::fit(pw, ph, *cell, turn_pages);
                let scale = f.scale.min(8.0);
                let mut raster = render_page(
                    tools,
                    spec.pdf,
                    *page,
                    f64::from(spec.dpi) * scale,
                    spec.mono,
                    tmp.path(),
                )?;
                if f.rotate {
                    raster = raster.rotated();
                }
                let (rw, rh) = raster.dims();
                let x = i64::from(cell.x) + (i64::from(cell.w) - i64::from(rw)) / 2;
                let y = i64::from(cell.y) + (i64::from(cell.h) - i64::from(rh)) / 2;
                canvas.blit(&raster, x, y);
            }
        }
        if landscape {
            canvas = canvas.rotated_clockwise();
        }
        let is_back = duplex && side % 2 == 1;
        if is_back {
            canvas.apply(back_turn);
        }
        let mut header = PageHeader::new(paper_w, paper_h, spec.dpi, colorspace);
        header.media_type = spec.media_type.to_string();
        header.page_size_name = spec.paper.ipp_name.to_string();
        header.quality = spec.quality;
        header.total_pages = total_sides;
        header.duplex = duplex;
        header.tumble = short_edge;
        if is_back {
            (header.cross_feed_transform, header.feed_transform) = back_turn.transforms();
        }
        let rows_iter = (0..canvas.h).map(|y| canvas.row(y));
        writer
            .write_rows(&header, rows_iter)
            .map_err(|e| Error::io("cannot write the raster spool file", e))?;
    }
    progress(total_sides, total_sides);
    writer.finish().map_err(|e| Error::io("cannot finish the raster spool file", e))?;
    Ok(RasterStats { sides: total_sides, sheets })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::pwg;

    #[test]
    fn pdfinfo_text_is_parsed() {
        assert_eq!(parse_pages("Title: x\nPages:           12\nEncrypted: no\n"), Some(12));
        assert_eq!(parse_pages("nothing"), None);
        let sizes = parse_sizes(
            "Page    1 size:  595 x 842 pts (A4)\nPage    2 size:  841.89 x 595.276 pts (A4)\nPage 3 rot: 0\nPage    x size: 1 x 2 pts\n",
        );
        assert_eq!(sizes, [(595.0, 842.0), (841.89, 595.276)]);
        let info = PdfInfo { pages: 3, sizes_pt: sizes };
        assert_eq!(info.size_of(2), (841.89, 595.276));
        assert_eq!(info.size_of(3), (595.0, 842.0), "unknown pages use the first size");
        assert_eq!(PdfInfo { pages: 1, sizes_pt: vec![] }.size_of(1), (595.0, 842.0));
    }

    #[test]
    fn pnm_files_parse() {
        let mut p6 = b"P6\n2 1\n255\n".to_vec();
        p6.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let Raster::Rgb(i) = read_pnm(&p6).unwrap() else { panic!("not rgb") };
        assert_eq!(i.dimensions(), (2, 1));
        assert_eq!(i.get_pixel(1, 0).0, [4, 5, 6]);
        let mut p5 = b"P5\n3 1\n255\n".to_vec();
        p5.extend_from_slice(&[7, 8, 9, 99]);
        let Raster::Gray(g) = read_pnm(&p5).unwrap() else { panic!("not gray") };
        assert_eq!(g.as_raw(), &[7, 8, 9]);
        assert!(read_pnm(b"P6\n2 2\n255\n\x01").is_err(), "truncated");
        assert!(read_pnm(b"P3\n1 1\n255\n0 0 0").is_err());
        assert!(read_pnm(b"").is_err());
    }

    #[test]
    fn side_counting() {
        assert_eq!(count_sides(5, 1, false), (5, 5));
        assert_eq!(count_sides(5, 2, false), (3, 3));
        assert_eq!(count_sides(5, 1, true), (6, 3), "odd count gets a blank back");
        assert_eq!(count_sides(9, 4, true), (4, 2));
        assert_eq!(count_sides(1, 16, true), (2, 1));
    }

    #[test]
    fn back_side_turns() {
        assert_eq!(BackTurn::for_sheet_back("normal", false), BackTurn::None);
        assert_eq!(BackTurn::for_sheet_back("rotated", false), BackTurn::Rotate180);
        assert_eq!(BackTurn::for_sheet_back("rotated", true), BackTurn::None);
        assert_eq!(BackTurn::for_sheet_back("manual-tumble", true), BackTurn::Rotate180);
        assert_eq!(BackTurn::for_sheet_back("flipped", false), BackTurn::FlipHorizontal);
        assert_eq!(BackTurn::for_sheet_back("flipped", true), BackTurn::FlipVertical);
        assert_eq!(BackTurn::for_sheet_back("", true), BackTurn::None);
        assert_eq!(BackTurn::Rotate180.transforms(), (-1, -1));
        // The pixel operations: a 3x2 RGB canvas with distinct pixels.
        let mut c = Canvas { w: 3, h: 2, bpp: 1, data: vec![1, 2, 3, 4, 5, 6] };
        c.apply(BackTurn::Rotate180);
        assert_eq!(c.data, [6, 5, 4, 3, 2, 1]);
        let mut c = Canvas { w: 3, h: 2, bpp: 1, data: vec![1, 2, 3, 4, 5, 6] };
        c.apply(BackTurn::FlipHorizontal);
        assert_eq!(c.data, [3, 2, 1, 6, 5, 4]);
        let mut c = Canvas { w: 3, h: 2, bpp: 1, data: vec![1, 2, 3, 4, 5, 6] };
        c.apply(BackTurn::FlipVertical);
        assert_eq!(c.data, [4, 5, 6, 1, 2, 3]);
        let mut c = Canvas { w: 2, h: 1, bpp: 3, data: vec![1, 2, 3, 4, 5, 6] };
        c.apply(BackTurn::Rotate180);
        assert_eq!(c.data, [4, 5, 6, 1, 2, 3], "pixels move whole");
        let mut c = Canvas { w: 2, h: 1, bpp: 3, data: vec![1, 2, 3, 4, 5, 6] };
        c.apply(BackTurn::FlipHorizontal);
        assert_eq!(c.data, [4, 5, 6, 1, 2, 3]);
    }

    #[test]
    fn a_quarter_turn_moves_pixels_clockwise() {
        // 3 x 2:  1 2 3   ->  4 1
        //         4 5 6       5 2
        //                     6 3
        let c = Canvas { w: 3, h: 2, bpp: 1, data: vec![1, 2, 3, 4, 5, 6] };
        let r = c.rotated_clockwise();
        assert_eq!((r.w, r.h), (2, 3));
        assert_eq!(r.data, [4, 1, 5, 2, 6, 3]);
        let rgb = Canvas { w: 2, h: 1, bpp: 3, data: vec![1, 2, 3, 4, 5, 6] };
        assert_eq!(rgb.rotated_clockwise().data, [1, 2, 3, 4, 5, 6][..].to_vec());
        let two = Canvas { w: 1, h: 2, bpp: 3, data: vec![1, 2, 3, 4, 5, 6] };
        assert_eq!(two.rotated_clockwise().data, vec![4, 5, 6, 1, 2, 3]);
    }

    #[test]
    fn blitting_clips_to_the_sheet() {
        let mut c = Canvas::white(4, 4, false);
        let r = Raster::Rgb(RgbImage::from_pixel(3, 3, image::Rgb([9, 9, 9])));
        c.blit(&r, 2, 2);
        assert_eq!(&c.data[(2 * 4 + 2) * 3..(2 * 4 + 2) * 3 + 3], &[9, 9, 9]);
        assert_eq!(&c.data[(3 * 4 + 3) * 3..(3 * 4 + 3) * 3 + 3], &[9, 9, 9]);
        assert_eq!(&c.data[0..3], &[255, 255, 255]);
        c.blit(&r, -2, -2); // only the bottom-right pixel of the source lands at (0,0)
        assert_eq!(&c.data[0..3], &[9, 9, 9]);
        c.blit(&r, 10, 10); // fully outside: nothing happens
        let mut g = Canvas::white(2, 2, true);
        g.blit(&Raster::Gray(GrayImage::from_pixel(1, 1, image::Luma([5]))), 1, 1);
        assert_eq!(g.data, [255, 255, 255, 5]);
    }

    /// Stand-ins for poppler: `pdfinfo` reports 3 pages of 100x140 pt and `pdftoppm` writes a
    /// 100x140 px image whose colour encodes the page number (R = 40 * page).
    fn fake_poppler(dir: &Path) -> Tools {
        let pdfinfo = dir.join("pdfinfo");
        std::fs::write(
            &pdfinfo,
            "#!/bin/sh\necho 'Pages: 3'\necho 'Encrypted: no'\nif [ \"$1\" = \"-f\" ]; then for i in 1 2 3; do echo \"Page    $i size:  100 x 140 pts\"; done; fi\n",
        )
        .unwrap();
        let pdftoppm = dir.join("pdftoppm");
        std::fs::write(
            &pdftoppm,
            "#!/bin/sh\n\
             gray=0; page=1\n\
             while [ $# -gt 0 ]; do case \"$1\" in -f) page=$2; shift;; -gray) gray=1;; -r|-l) shift;; -singlefile) ;; *) last=$1;; esac; shift; done\n\
             v=$((40 * page))\n\
             if [ $gray = 1 ]; then printf 'P5\\n100 140\\n255\\n' > \"$last.pgm\"; head -c 14000 /dev/zero | tr '\\0' \"\\\\$(printf '%03o' $v)\" >> \"$last.pgm\";\n\
             else printf 'P6\\n100 140\\n255\\n' > \"$last.ppm\"; head -c 42000 /dev/zero | tr '\\0' \"\\\\$(printf '%03o' $v)\" >> \"$last.ppm\"; fi\n",
        )
        .unwrap();
        for p in [&pdfinfo, &pdftoppm] {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Tools { pdfinfo: Some(pdfinfo), pdftoppm: Some(pdftoppm), ..Tools::default() }
    }

    fn spec<'a>(info: &'a PdfInfo, pages: &'a [u32], layout: u8, sides: Sides) -> RasterSpec<'a> {
        RasterSpec {
            pdf: Path::new("/dev/null"),
            info,
            pages,
            paper: *PaperSize::parse("a5").unwrap(),
            layout,
            mono: false,
            sides,
            sheet_back: "rotated",
            media_type: "stationery",
            quality: 4,
            dpi: 100, // small sheets keep the test quick: A5 = 583 x 827 px
            orientation: Orientation::Auto,
            reverse: false,
        }
    }

    #[test]
    fn pages_are_composed_and_written_as_pwg_raster() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake_poppler(dir.path());
        let info = pdf_info(&tools, Path::new("x.pdf")).unwrap();
        assert_eq!(info.pages, 3);
        assert_eq!(info.sizes_pt, [(100.0, 140.0); 3]);

        // Two pages on one A5 sheet.
        let mut out = Vec::new();
        let mut steps = Vec::new();
        let stats = rasterize(
            &tools,
            &spec(&info, &[1, 3], 2, Sides::OneSided),
            &mut out,
            &mut |d, t| steps.push((d, t)),
            &|| false,
        )
        .unwrap();
        assert_eq!(stats, RasterStats { sides: 1, sheets: 1 });
        assert_eq!(steps.first(), Some(&(0, 1)));
        assert_eq!(steps.last(), Some(&(1, 1)));
        let pages = pwg::decode(&out).unwrap();
        assert_eq!(pages.len(), 1);
        let p = &pages[0];
        assert_eq!((p.header.width, p.header.height, p.header.dpi), (583, 827, 100));
        assert_eq!(p.header.page_size_name, "iso_a5_148x210mm");
        assert_eq!(p.header.media_type, "stationery");
        assert_eq!((p.header.quality, p.header.total_pages), (4, 1));
        assert!(!p.header.duplex);
        let at = |x: usize, y: usize| p.pixels[(y * 583 + x) * 3];
        // Portrait A5 grid for 2-up: pages stacked; page 1 (R=40) on top, page 3 (R=120) below.
        assert_eq!(at(291, 200), 40);
        assert_eq!(at(291, 620), 120);
        assert_eq!(at(2, 2), 255, "margins stay white");
    }

    #[test]
    fn duplex_pads_odd_page_counts_and_turns_the_back() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake_poppler(dir.path());
        let info = pdf_info(&tools, Path::new("x.pdf")).unwrap();
        let mut out = Vec::new();
        let stats = rasterize(
            &tools,
            &spec(&info, &[1, 2, 3], 1, Sides::TwoSidedLongEdge),
            &mut out,
            &mut |_, _| {},
            &|| false,
        )
        .unwrap();
        assert_eq!(stats, RasterStats { sides: 4, sheets: 2 });
        let pages = pwg::decode(&out).unwrap();
        assert_eq!(pages.len(), 4);
        assert!(
            pages.iter().all(|p| p.header.duplex && !p.header.tumble && p.header.total_pages == 4)
        );
        // Front sides keep (1,1); back sides carry the printer's `rotated` transform.
        assert_eq!((pages[0].header.cross_feed_transform, pages[0].header.feed_transform), (1, 1));
        assert_eq!(
            (pages[1].header.cross_feed_transform, pages[1].header.feed_transform),
            (-1, -1)
        );
        // Page 2 is printed on the back of sheet 1: turned 180 degrees, so its content (a
        // portrait page centred on the sheet) is still centred but the sheet is rotated.
        let last = &pages[3];
        assert!(last.pixels.iter().all(|b| *b == 255), "the padded back side is blank");
        // Short-edge binding sets Tumble and does not turn a `rotated` back.
        let mut out = Vec::new();
        rasterize(
            &tools,
            &spec(&info, &[1, 2], 1, Sides::TwoSidedShortEdge),
            &mut out,
            &mut |_, _| {},
            &|| false,
        )
        .unwrap();
        let pages = pwg::decode(&out).unwrap();
        assert!(pages[1].header.tumble);
        assert_eq!((pages[1].header.cross_feed_transform, pages[1].header.feed_transform), (1, 1));
    }

    #[test]
    fn reversing_turns_the_sheets_round_and_keeps_the_sides_of_a_sheet_together() {
        let some = |v: &[usize]| v.iter().map(|i| Some(*i)).collect::<Vec<_>>();
        // One-sided: the sheets simply run backwards.
        assert_eq!(side_order(4, false, false), some(&[0, 1, 2, 3]));
        assert_eq!(side_order(4, false, true), some(&[3, 2, 1, 0]));
        // Two-sided, an even count: sheet 2 (chunks 2 and 3) first, but its front is still
        // chunk 2 and its back chunk 3. A plain reversal would give 3, 2, 1, 0.
        assert_eq!(side_order(4, true, false), some(&[0, 1, 2, 3]));
        assert_eq!(side_order(4, true, true), some(&[2, 3, 0, 1]));
        // An odd count: the last sheet carries one chunk and a blank back, and comes out first.
        assert_eq!(
            side_order(5, true, false),
            vec![Some(0), Some(1), Some(2), Some(3), Some(4), None]
        );
        assert_eq!(
            side_order(5, true, true),
            vec![Some(4), None, Some(2), Some(3), Some(0), Some(1)]
        );
        // The number of sides is what `count_sides` says, reversed or not.
        for chunks in 1..12usize {
            for duplex in [false, true] {
                for reverse in [false, true] {
                    let o = side_order(chunks, duplex, reverse);
                    assert_eq!(
                        o.len() as u32,
                        count_sides(chunks, 1, duplex).0,
                        "{chunks} {duplex}"
                    );
                    let mut seen: Vec<usize> = o.iter().flatten().copied().collect();
                    seen.sort_unstable();
                    assert_eq!(seen, (0..chunks).collect::<Vec<_>>(), "every chunk exactly once");
                }
            }
        }
        assert!(side_order(0, true, true).is_empty());
    }

    /// The centre pixel (red channel) of every raster page of a stream.
    fn centres(out: &[u8], w: usize, h: usize) -> Vec<u8> {
        pwg::decode(out)
            .unwrap()
            .iter()
            .map(|p| {
                assert_eq!((p.header.width as usize, p.header.height as usize), (w, h));
                p.pixels[(h / 2 * w + w / 2) * 3]
            })
            .collect()
    }

    #[test]
    fn page_order_reverse_prints_the_last_sheet_first() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake_poppler(dir.path());
        let info = pdf_info(&tools, Path::new("x.pdf")).unwrap();
        let run = |pages: &[u32], layout: u8, sides: Sides, reverse: bool| {
            let mut s = spec(&info, pages, layout, sides);
            s.reverse = reverse;
            let mut out = Vec::new();
            let stats = rasterize(&tools, &s, &mut out, &mut |_, _| {}, &|| false).unwrap();
            (out, stats)
        };
        // One-sided, one page per sheet: page 3 (R = 120), 2 (80), 1 (40).
        let (out, stats) = run(&[1, 2, 3], 1, Sides::OneSided, true);
        assert_eq!(stats, RasterStats { sides: 3, sheets: 3 });
        assert_eq!(centres(&out, 583, 827), [120, 80, 40]);
        let (out, _) = run(&[1, 2, 3], 1, Sides::OneSided, false);
        assert_eq!(centres(&out, 583, 827), [40, 80, 120], "normal order is untouched");

        // Two-sided, 3 pages: sheet 2 (page 3 and a blank back) first, then sheet 1 (pages 1
        // front, 2 back). Not 3, 2, 1 with 3 and 2 sharing a sheet.
        let (out, stats) = run(&[1, 2, 3], 1, Sides::TwoSidedLongEdge, true);
        assert_eq!(stats, RasterStats { sides: 4, sheets: 2 });
        assert_eq!(centres(&out, 583, 827), [120, 255, 40, 80]);
        let pages = pwg::decode(&out).unwrap();
        let duplex_flags: Vec<(bool, i32)> =
            pages.iter().map(|p| (p.header.duplex, p.header.cross_feed_transform)).collect();
        // The backs are still the odd-numbered sides of the stream, and carry the back turn.
        assert_eq!(duplex_flags, [(true, 1), (true, -1), (true, 1), (true, -1)]);

        // Two pages per sheet: pages 1 and 2 on sheet 1, page 3 alone on sheet 2, which now
        // comes first; the pages keep their cells (top, bottom) within a sheet.
        let (out, stats) = run(&[1, 2, 3], 2, Sides::OneSided, true);
        assert_eq!(stats, RasterStats { sides: 2, sheets: 2 });
        let pages = pwg::decode(&out).unwrap();
        let at = |p: usize, y: usize| pages[p].pixels[(y * 583 + 291) * 3];
        assert_eq!((at(0, 200), at(0, 620)), (120, 255), "page 3 on top, the bottom cell empty");
        assert_eq!((at(1, 200), at(1, 620)), (40, 80), "pages 1 and 2 in their order");

        // Both: 3 pages, 2-up, two-sided: 2 sides of content = one duplex sheet; reversing a
        // single sheet changes nothing.
        let (out, _) = run(&[1, 2, 3], 2, Sides::TwoSidedLongEdge, true);
        let (plain, _) = run(&[1, 2, 3], 2, Sides::TwoSidedLongEdge, false);
        assert_eq!(out, plain);
    }

    #[test]
    fn monochrome_jobs_use_the_grey_colour_space() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake_poppler(dir.path());
        let info = pdf_info(&tools, Path::new("x.pdf")).unwrap();
        let mut s = spec(&info, &[2], 1, Sides::OneSided);
        s.mono = true;
        let mut out = Vec::new();
        rasterize(&tools, &s, &mut out, &mut |_, _| {}, &|| false).unwrap();
        let p = &pwg::decode(&out).unwrap()[0];
        assert_eq!(p.header.colorspace, ColorSpace::SGray);
        assert_eq!(p.pixels.len(), 583 * 827);
        assert_eq!(p.pixels[413 * 583 + 291], 80);
    }

    #[test]
    fn bad_selections_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake_poppler(dir.path());
        let info = pdf_info(&tools, Path::new("x.pdf")).unwrap();
        let mut sink = Vec::new();
        let e = rasterize(
            &tools,
            &spec(&info, &[4], 1, Sides::OneSided),
            &mut sink,
            &mut |_, _| {},
            &|| false,
        )
        .unwrap_err();
        assert!(e.to_string().contains("page 4 does not exist"), "{e}");
        assert!(
            rasterize(
                &tools,
                &spec(&info, &[], 1, Sides::OneSided),
                &mut sink,
                &mut |_, _| {},
                &|| false
            )
            .is_err()
        );
        assert!(
            rasterize(
                &tools,
                &spec(&info, &[1], 3, Sides::OneSided),
                &mut sink,
                &mut |_, _| {},
                &|| false
            )
            .is_err()
        );
        let e = rasterize(
            &tools,
            &spec(&info, &[1, 2], 1, Sides::OneSided),
            &mut sink,
            &mut |_, _| {},
            &|| true,
        )
        .unwrap_err();
        assert!(matches!(e, Error::Cancelled));
        let none = Tools::default();
        assert!(pdf_info(&none, Path::new("x.pdf")).unwrap_err().to_string().contains("poppler"));
        assert!(convert_office(&none, Path::new("a.docx"), dir.path()).is_err());
    }

    #[test]
    fn office_conversion_through_a_stand_in() {
        let dir = tempfile::tempdir().unwrap();
        let soffice = dir.path().join("soffice");
        // Writes <outdir>/<stem>.pdf like `soffice --convert-to pdf --outdir DIR FILE`.
        std::fs::write(
            &soffice,
            "#!/bin/sh\nfor a; do last=$a; done\nwhile [ $# -gt 0 ]; do if [ \"$1\" = \"--outdir\" ]; then out=$2; fi; shift; done\nname=$(basename \"$last\")\nprintf '%%PDF-1.4 converted' > \"$out/${name%.*}.pdf\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&soffice, std::fs::Permissions::from_mode(0o755)).unwrap();
        let tools = Tools { soffice: Some(soffice), ..Tools::default() };
        let work = tempfile::tempdir().unwrap();
        let pdf =
            convert_office(&tools, Path::new("/some/where/report.docx"), work.path()).unwrap();
        assert_eq!(pdf, work.path().join("report.pdf"));
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
    }
}
