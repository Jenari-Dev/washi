// SPDX-License-Identifier: GPL-2.0-or-later
//! Saving a [`ScanSession`]: JPEG per page, or PDF (one multi-page file or one file per page).

use std::path::{Path, PathBuf};

use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::{DynamicImage, ExtendedColorType};
use serde::{Deserialize, Serialize};

use super::correct::{OutputMode, pack_bilevel};
use super::{Page, ScanSession};
use crate::error::{Error, Result};
use crate::pdf::PdfBuilder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveFormat {
    Jpeg,
    Pdf,
}

impl SaveFormat {
    pub fn extension(self) -> &'static str {
        match self {
            SaveFormat::Jpeg => "jpg",
            SaveFormat::Pdf => "pdf",
        }
    }

    pub fn parse(s: &str) -> Option<SaveFormat> {
        match s.trim().to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => Some(SaveFormat::Jpeg),
            "pdf" => Some(SaveFormat::Pdf),
            _ => None,
        }
    }
}

/// "Range": save all pages or only the one on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveRange {
    All,
    /// A zero-based page index.
    Current(usize),
    /// Several zero-based indices, saved in this order.
    Pages(Vec<usize>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveOptions {
    pub format: SaveFormat,
    pub range: SaveRange,
    /// "Combine all pages into one file" (PDF only; JPEG is always one file per page).
    pub single_file: bool,
    pub jpeg_quality: u8,
}

impl Default for SaveOptions {
    fn default() -> Self {
        SaveOptions {
            format: SaveFormat::Pdf,
            range: SaveRange::All,
            single_file: true,
            jpeg_quality: 90,
        }
    }
}

/// Encode a rendered page as JPEG (`Rgb8` or `Luma8`), writing `dpi` into the JFIF header.
pub fn encode_jpeg(img: &DynamicImage, quality: u8, dpi: u32) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    enc.set_pixel_density(PixelDensity::dpi(dpi.clamp(1, u32::from(u16::MAX)) as u16));
    let (w, h) = (img.width(), img.height());
    let res = match img {
        DynamicImage::ImageLuma8(g) => enc.encode(g.as_raw(), w, h, ExtendedColorType::L8),
        other => enc.encode(other.to_rgb8().as_raw(), w, h, ExtendedColorType::Rgb8),
    };
    res.map_err(|e| Error::Image(format!("cannot encode JPEG: {e}")))?;
    Ok(buf)
}

fn add_pdf_page(pdf: &mut PdfBuilder, page: &Page, quality: u8) -> Result<()> {
    let img = page.render();
    let dpi = f64::from(page.dpi);
    if page.edits.correction.color_mode == OutputMode::BlackWhite {
        if let DynamicImage::ImageLuma8(g) = &img {
            pdf.add_bilevel_page(pack_bilevel(g), g.width(), g.height(), dpi);
            return Ok(());
        }
    }
    let gray = matches!(img, DynamicImage::ImageLuma8(_));
    let jpeg = encode_jpeg(&img, quality, page.dpi)?;
    pdf.add_jpeg_page(jpeg, img.width(), img.height(), gray, dpi);
    Ok(())
}

/// The file name for page `n` (1-based) of `count`: `base` itself for one page, otherwise
/// `<stem>_001.<ext>`. A missing or foreign extension is replaced by the format's.
pub fn page_path(base: &Path, format: SaveFormat, n: usize, count: usize) -> PathBuf {
    let ext = format.extension();
    let has_ext = base.extension().and_then(|e| e.to_str()).is_some_and(|e| match format {
        SaveFormat::Jpeg => e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"),
        SaveFormat::Pdf => e.eq_ignore_ascii_case("pdf"),
    });
    let stem = if has_ext { base.with_extension("") } else { base.to_path_buf() };
    let keep_ext =
        if has_ext { base.extension().and_then(|e| e.to_str()).unwrap_or(ext) } else { ext };
    if count <= 1 {
        let mut p = stem.into_os_string();
        p.push(".");
        p.push(keep_ext);
        return PathBuf::from(p);
    }
    let mut p = stem.into_os_string();
    p.push(format!("_{n:03}.{keep_ext}"));
    PathBuf::from(p)
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::io(format!("cannot create {}", dir.display()), e))?;
    }
    std::fs::write(path, bytes)
        .map_err(|e| Error::io(format!("cannot write {}", path.display()), e))
}

/// Pages chosen by `range`, validated.
pub fn select_pages(session: &ScanSession, range: &SaveRange) -> Result<Vec<usize>> {
    let idx: Vec<usize> = match range {
        SaveRange::All => (0..session.len()).collect(),
        SaveRange::Current(i) => vec![*i],
        SaveRange::Pages(v) => v.clone(),
    };
    if idx.is_empty() {
        return Err(Error::Invalid("there are no pages to save".into()));
    }
    if let Some(bad) = idx.iter().find(|i| **i >= session.len()) {
        return Err(Error::Invalid(format!(
            "page {} does not exist (the scan has {} page(s))",
            bad + 1,
            session.len()
        )));
    }
    Ok(idx)
}

/// Save pages and return the files written, in page order.
///
/// `base` is the file name asked for. One output file is written exactly there (an extension is
/// added when missing); several files get `_001`, `_002`, ... before the extension.
pub fn save(session: &ScanSession, base: &Path, opts: &SaveOptions) -> Result<Vec<PathBuf>> {
    let idx = select_pages(session, &opts.range)?;
    let pages: Vec<&Page> = idx.iter().map(|i| &session.pages()[*i]).collect();
    let mut written = Vec::new();
    match (opts.format, opts.single_file) {
        (SaveFormat::Pdf, true) => {
            let mut pdf = PdfBuilder::new();
            if let Some(name) = base.file_stem().and_then(|s| s.to_str()) {
                pdf.set_title(name);
            }
            for p in &pages {
                add_pdf_page(&mut pdf, p, opts.jpeg_quality)?;
            }
            let path = page_path(base, SaveFormat::Pdf, 1, 1);
            write_file(&path, &pdf.finish())?;
            written.push(path);
        }
        (SaveFormat::Pdf, false) => {
            for (n, p) in pages.iter().enumerate() {
                let mut pdf = PdfBuilder::new();
                add_pdf_page(&mut pdf, p, opts.jpeg_quality)?;
                let path = page_path(base, SaveFormat::Pdf, n + 1, pages.len());
                write_file(&path, &pdf.finish())?;
                written.push(path);
            }
        }
        (SaveFormat::Jpeg, _) => {
            for (n, p) in pages.iter().enumerate() {
                let jpeg = encode_jpeg(&p.render(), opts.jpeg_quality, p.dpi)?;
                let path = page_path(base, SaveFormat::Jpeg, n + 1, pages.len());
                write_file(&path, &jpeg)?;
                written.push(path);
            }
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;
    use crate::scan::{ImageCorrection, Page};

    fn session(n: usize) -> ScanSession {
        let mut s = ScanSession::new();
        for i in 0..n {
            let mut img = image::RgbImage::from_pixel(60, 40, Rgb([220, 220, 220]));
            for x in 0..(10 + 10 * i as u32) {
                img.put_pixel(x, 5, Rgb([0, 0, 0]));
            }
            s.add_page(Page::new(DynamicImage::ImageRgb8(img), 150));
        }
        s
    }

    #[test]
    fn page_paths() {
        let b = Path::new("/tmp/out/scan");
        assert_eq!(page_path(b, SaveFormat::Pdf, 1, 1), Path::new("/tmp/out/scan.pdf"));
        assert_eq!(page_path(b, SaveFormat::Jpeg, 2, 3), Path::new("/tmp/out/scan_002.jpg"));
        let withext = Path::new("x/report.PDF");
        assert_eq!(page_path(withext, SaveFormat::Pdf, 1, 1), Path::new("x/report.PDF"));
        assert_eq!(page_path(withext, SaveFormat::Pdf, 12, 20), Path::new("x/report_012.PDF"));
        // A foreign extension is kept as part of the name and the right one appended.
        assert_eq!(page_path(Path::new("a.b.c"), SaveFormat::Jpeg, 1, 1), Path::new("a.b.c.jpg"));
        assert_eq!(page_path(Path::new("p.jpeg"), SaveFormat::Jpeg, 1, 2), Path::new("p_001.jpeg"));
    }

    #[test]
    fn jpegs_are_one_file_per_page_with_density() {
        let dir = tempfile::tempdir().unwrap();
        let s = session(3);
        let opts = SaveOptions { format: SaveFormat::Jpeg, ..SaveOptions::default() };
        let files = save(&s, &dir.path().join("scan"), &opts).unwrap();
        assert_eq!(files.len(), 3);
        assert!(files[0].ends_with("scan_001.jpg") && files[2].ends_with("scan_003.jpg"));
        let bytes = std::fs::read(&files[1]).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (60, 40));
        // JFIF density is 150 dpi (units = 1, x = y = 150).
        let jfif = bytes.windows(5).position(|w| w == b"JFIF\0").expect("JFIF header");
        assert_eq!(bytes[jfif + 7], 1);
        assert_eq!(u16::from_be_bytes([bytes[jfif + 8], bytes[jfif + 9]]), 150);

        // A single page lands exactly where it was asked for.
        let one = save(
            &s,
            &dir.path().join("one.jpg"),
            &SaveOptions { range: SaveRange::Current(2), ..opts },
        )
        .unwrap();
        assert_eq!(one, [dir.path().join("one.jpg")]);
    }

    #[test]
    fn pdf_single_and_split_and_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let s = session(3);
        let single = save(&s, &dir.path().join("all"), &SaveOptions::default()).unwrap();
        assert_eq!(single, [dir.path().join("all.pdf")]);
        let pdf = std::fs::read(&single[0]).unwrap();
        assert!(String::from_utf8_lossy(&pdf).contains("/Count 3"));
        assert!(pdf.starts_with(b"%PDF-1.4"));
        // 60 px at 150 dpi = 28.8 pt.
        assert!(String::from_utf8_lossy(&pdf).contains("/MediaBox [0 0 28.80 19.20]"));

        let split = save(
            &s,
            &dir.path().join("split"),
            &SaveOptions {
                single_file: false,
                range: SaveRange::Pages(vec![2, 0]),
                ..SaveOptions::default()
            },
        )
        .unwrap();
        assert_eq!(split.len(), 2);
        assert!(split[0].ends_with("split_001.pdf"));
        assert!(String::from_utf8_lossy(&std::fs::read(&split[1]).unwrap()).contains("/Count 1"));

        let cur = save(
            &s,
            &dir.path().join("cur.pdf"),
            &SaveOptions { range: SaveRange::Current(1), ..SaveOptions::default() },
        )
        .unwrap();
        assert_eq!(cur.len(), 1);
    }

    #[test]
    fn black_and_white_pages_become_bilevel_pdf_images() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = session(1);
        s.page_mut(0).unwrap().set_correction(ImageCorrection {
            color_mode: OutputMode::BlackWhite,
            ..ImageCorrection::default()
        });
        let f = save(&s, &dir.path().join("bw.pdf"), &SaveOptions::default()).unwrap();
        let pdf = String::from_utf8_lossy(&std::fs::read(&f[0]).unwrap()).into_owned();
        assert!(pdf.contains("/BitsPerComponent 1 /Filter /FlateDecode"));
        // Grayscale stays JPEG, DeviceGray.
        s.page_mut(0).unwrap().set_correction(ImageCorrection {
            color_mode: OutputMode::Grayscale,
            ..ImageCorrection::default()
        });
        let f = save(&s, &dir.path().join("g.pdf"), &SaveOptions::default()).unwrap();
        let pdf = String::from_utf8_lossy(&std::fs::read(&f[0]).unwrap()).into_owned();
        assert!(pdf.contains("/DeviceGray /BitsPerComponent 8 /Filter /DCTDecode"));
    }

    #[test]
    fn bad_ranges_are_errors_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let s = session(2);
        let bad = SaveOptions { range: SaveRange::Current(5), ..SaveOptions::default() };
        assert!(save(&s, &dir.path().join("x"), &bad).is_err());
        let none = SaveOptions { range: SaveRange::Pages(vec![]), ..SaveOptions::default() };
        assert!(save(&s, &dir.path().join("x"), &none).is_err());
        assert!(save(&ScanSession::new(), &dir.path().join("x"), &SaveOptions::default()).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn missing_directories_are_created() {
        let dir = tempfile::tempdir().unwrap();
        let f = save(&session(1), &dir.path().join("a/b/c.pdf"), &SaveOptions::default()).unwrap();
        assert!(f[0].exists());
        assert!(
            SaveFormat::parse("JPG") == Some(SaveFormat::Jpeg)
                && SaveFormat::parse("png").is_none()
        );
    }
}
