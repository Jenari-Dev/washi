// SPDX-License-Identifier: GPL-2.0-or-later
//! Photo Print: load (JPEG / PNG / BMP), make it sRGB, lay it out on the paper (borderless
//! "fill", bordered "fit", or n photos per sheet) and encode the JPEG the printer takes.

use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageReader, Rgb, RgbImage};

use super::Orientation;
use super::layout::{self, Rect};
use super::paper::PaperSize;
use crate::error::{Error, Result};

/// The printer takes JPEGs up to 16 MB (`jpeg-k-octets-supported` 0..16384) and 16..19200 px.
pub const MAX_JPEG_BYTES: usize = 15 * 1024 * 1024;
pub const MAX_JPEG_EDGE: u32 = 19200;

const WHITE: Rgb<u8> = Rgb([255, 255, 255]);

/// Convert pixels tagged with an ICC profile to sRGB. Profiles that already say sRGB, and
/// anything that cannot be handled, leave the pixels alone.
pub fn icc_to_srgb(img: RgbImage, icc: Option<&[u8]>) -> RgbImage {
    let Some(icc) = icc else { return img };
    if icc.windows(4).any(|w| w == b"sRGB") {
        return img;
    }
    let convert = || -> Option<RgbImage> {
        let src = moxcms::ColorProfile::new_from_slice(icc).ok()?;
        let dst = moxcms::ColorProfile::new_srgb();
        let t = src
            .create_transform_8bit(
                moxcms::Layout::Rgb,
                &dst,
                moxcms::Layout::Rgb,
                moxcms::TransformOptions::default(),
            )
            .ok()?;
        let mut out = vec![0u8; img.as_raw().len()];
        let stride = img.width() as usize * 3;
        for (s, d) in img.as_raw().chunks_exact(stride).zip(out.chunks_exact_mut(stride)) {
            t.transform(s, d).ok()?;
        }
        RgbImage::from_raw(img.width(), img.height(), out)
    };
    convert().unwrap_or(img)
}

/// Decode an image file: EXIF orientation applied, pixels converted to sRGB.
pub fn load_photo(path: &Path) -> Result<RgbImage> {
    let ctx = |what: &str, e: &dyn std::fmt::Display| {
        Error::Image(format!("{}: {what}: {e}", path.display()))
    };
    let reader = ImageReader::open(path)
        .map_err(|e| Error::io(format!("cannot open {}", path.display()), e))?
        .with_guessed_format()
        .map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
    let mut decoder = reader.into_decoder().map_err(|e| ctx("not a supported image", &e))?;
    let orientation = decoder.orientation().ok();
    let icc = decoder.icc_profile().ok().flatten();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| ctx("cannot decode", &e))?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(icc_to_srgb(img.to_rgb8(), icc.as_deref()))
}

/// Turn the photo when its orientation differs from the target's, so it fills the page.
fn orient_to(img: RgbImage, target_portrait: bool) -> RgbImage {
    let portrait = img.height() >= img.width();
    if portrait == target_portrait || img.width() == img.height() {
        img
    } else {
        imageops::rotate90(&img)
    }
}

/// Place one photo on a white canvas of the paper's shape.
///
/// * `fill` (borderless): the photo covers the whole sheet, the overhang is cut off.
/// * otherwise (`fit`): the whole photo is visible on a white sheet.
///
/// The canvas has the paper's aspect ratio. It is never larger than the photo needs: if the
/// photo has fewer pixels than the paper has at 600 dpi, nothing is up-sampled (the printer
/// scales), so a 12 MP photo does not become a 35 MP file. The photo is turned to suit the
/// paper ([`Orientation::Auto`]).
pub fn compose_single(photo: &RgbImage, paper: &PaperSize, fill: bool) -> RgbImage {
    compose_single_oriented(photo, paper, fill, Orientation::Auto)
}

/// [`compose_single`] with an explicit sheet orientation. `Portrait` and `Landscape` leave the
/// photo upright on the sheet; a landscape sheet is composed sideways and turned a quarter
/// clockwise onto the (portrait) paper.
pub fn compose_single_oriented(
    photo: &RgbImage,
    paper: &PaperSize,
    fill: bool,
    orientation: Orientation,
) -> RgbImage {
    let (pw, ph) = paper.pixels(600);
    let photo = match orientation {
        Orientation::Auto => orient_to(photo.clone(), paper.is_portrait()),
        _ => photo.clone(),
    };
    if orientation.is_landscape() {
        imageops::rotate90(&compose_on(&photo, ph, pw, fill))
    } else {
        compose_on(&photo, pw, ph, fill)
    }
}

/// The photo on a white canvas shaped `pw` x `ph` (600 dpi pixels), see [`compose_single`].
fn compose_on(photo: &RgbImage, pw: u32, ph: u32, fill: bool) -> RgbImage {
    let (sw, sh) = (f64::from(photo.width()), f64::from(photo.height()));
    // Scale from photo pixels to 600-dpi sheet pixels that makes it cover / contain the sheet.
    let need = if fill {
        (f64::from(pw) / sw).max(f64::from(ph) / sh)
    } else {
        (f64::from(pw) / sw).min(f64::from(ph) / sh)
    };
    // Do not magnify: shrink the whole canvas instead.
    let eff = if need > 1.0 { 1.0 / need } else { 1.0 };
    let (cw, ch) = (
        ((f64::from(pw) * eff).round() as u32).max(16),
        ((f64::from(ph) * eff).round() as u32).max(16),
    );
    let scale = need * eff;
    let (nw, nh) = (((sw * scale).round() as u32).max(1), ((sh * scale).round() as u32).max(1));
    let resized = if (nw, nh) == photo.dimensions() {
        photo.clone()
    } else {
        imageops::resize(photo, nw, nh, FilterType::Lanczos3)
    };
    let mut canvas = RgbImage::from_pixel(cw, ch, WHITE);
    let ox = (i64::from(cw) - i64::from(nw)) / 2;
    let oy = (i64::from(ch) - i64::from(nh)) / 2;
    imageops::replace(&mut canvas, &resized, ox, oy);
    canvas
}

/// `n` photos on one sheet (2, 4, 6, 9 or 16), reading order, each fitted into its cell and
/// turned when that makes it bigger. The canvas is the paper at 300 dpi.
pub fn compose_sheet(photos: &[RgbImage], paper: &PaperSize, n: u8) -> RgbImage {
    compose_sheet_oriented(photos, paper, n, Orientation::Auto)
}

/// [`compose_sheet`] with an explicit orientation: `Portrait` and `Landscape` keep every photo
/// upright in its cell (they are not turned to fit); a landscape sheet is composed sideways and
/// turned a quarter clockwise.
pub fn compose_sheet_oriented(
    photos: &[RgbImage],
    paper: &PaperSize,
    n: u8,
    orientation: Orientation,
) -> RgbImage {
    compose_sheet_at(photos, paper, n, orientation, 300)
}

/// [`compose_sheet_oriented`] on a canvas of `dpi` (previews use a low value; printing 300).
pub fn compose_sheet_at(
    photos: &[RgbImage],
    paper: &PaperSize,
    n: u8,
    orientation: Orientation,
    dpi: u32,
) -> RgbImage {
    let (px, py) = paper.pixels(dpi);
    let landscape = orientation.is_landscape();
    let (pw, ph) = if landscape { (py, px) } else { (px, py) };
    let mut canvas = RgbImage::from_pixel(pw, ph, WHITE);
    let (cols, rows) = layout::grid(n, ph >= pw);
    let gutter = (pw.min(ph) / 60).max(2);
    let turn = orientation == Orientation::Auto;
    for (photo, cell) in photos.iter().zip(layout::cells(pw, ph, cols, rows, gutter)) {
        let f = layout::fit(f64::from(photo.width()), f64::from(photo.height()), cell, turn);
        let src = if f.rotate { imageops::rotate90(photo) } else { photo.clone() };
        let placed = imageops::resize(&src, f.w, f.h, FilterType::Lanczos3);
        imageops::replace(&mut canvas, &placed, i64::from(f.x), i64::from(f.y));
    }
    if landscape { imageops::rotate90(&canvas) } else { canvas }
}

/// Baseline JPEG under the printer's size limit (quality steps down if needed).
pub fn encode_for_printer(img: &RgbImage) -> Result<Vec<u8>> {
    if img.width() > MAX_JPEG_EDGE || img.height() > MAX_JPEG_EDGE {
        return Err(Error::Invalid(format!(
            "{}x{} px is more than the printer's {MAX_JPEG_EDGE} px limit",
            img.width(),
            img.height()
        )));
    }
    for quality in [92u8, 85, 75, 65, 50] {
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, quality)
            .encode(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgb8)
            .map_err(|e| Error::Image(format!("cannot encode JPEG: {e}")))?;
        if buf.len() <= MAX_JPEG_BYTES {
            return Ok(buf);
        }
    }
    Err(Error::Image("the photo is too large for the printer even at low JPEG quality".into()))
}

/// The cell of a photo on a sheet (exposed for previews).
pub fn cells_for(paper: &PaperSize, n: u8) -> Vec<Rect> {
    let (pw, ph) = paper.pixels(300);
    let (cols, rows) = layout::grid(n, paper.is_portrait());
    layout::cells(pw, ph, cols, rows, (pw.min(ph) / 60).max(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, base: u8) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            Rgb([base, (x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8])
        })
    }

    #[test]
    fn borderless_fill_covers_the_sheet_and_keeps_the_aspect() {
        let paper = PaperSize::parse("card").unwrap(); // 55 x 91 mm -> 1299 x 2150 px @ 600 dpi
        // A big landscape photo is turned to portrait and cropped to fill.
        let c = compose_single(&gradient(2400, 1600, 200), paper, true);
        assert_eq!(c.dimensions(), paper.pixels(600), "downscaled onto the full 600 dpi canvas");
        for (x, y) in
            [(0, 0), (c.width() - 1, 0), (0, c.height() - 1), (c.width() - 1, c.height() - 1)]
        {
            assert_ne!(*c.get_pixel(x, y), WHITE, "photo reaches the corner ({x},{y})");
        }
    }

    #[test]
    fn bordered_fit_letterboxes_with_white() {
        let paper = PaperSize::parse("card").unwrap();
        // A square photo on a portrait card: white bands above and below. 1000 px is less than
        // the 1299 px of the card at 600 dpi, so the canvas is not blown up: 1000 x 1655.
        let c = compose_single(&gradient(1000, 1000, 10), paper, false);
        assert!(
            (c.width() as i32 - 1000).abs() <= 1 && (c.height() as i32 - 1655).abs() <= 1,
            "{:?}",
            c.dimensions()
        );
        assert_eq!(*c.get_pixel(c.width() / 2, 5), WHITE);
        assert_eq!(*c.get_pixel(c.width() / 2, c.height() - 5), WHITE);
        assert_ne!(*c.get_pixel(c.width() / 2, c.height() / 2), WHITE);
        assert_ne!(*c.get_pixel(3, c.height() / 2), WHITE, "full width used");
        // A photo big enough for 600 dpi gets the full-size canvas.
        let full = compose_single(&gradient(2600, 2600, 10), paper, false);
        assert_eq!(full.dimensions(), paper.pixels(600));
    }

    #[test]
    fn small_photos_are_not_magnified() {
        let paper = PaperSize::parse("a4").unwrap();
        let c = compose_single(&gradient(1000, 1400, 50), paper, true);
        // 1000x1400 px covers A4 (aspect 0.707 vs 0.714) at scale ~1: the canvas shrinks.
        assert!(c.width() <= 1010 && c.height() <= 1420, "{:?}", c.dimensions());
        let ratio = c.width() as f64 / c.height() as f64;
        assert!((ratio - 21000.0 / 29700.0).abs() < 0.01, "paper aspect kept: {ratio}");
    }

    #[test]
    fn n_up_sheets_hold_every_photo_in_its_cell() {
        let paper = PaperSize::parse("card").unwrap();
        let photos: Vec<RgbImage> = (0..4).map(|i| gradient(80, 60, 40 * i as u8 + 20)).collect();
        let sheet = compose_sheet(&photos, paper, 4);
        assert_eq!(sheet.dimensions(), paper.pixels(300));
        for (i, cell) in cells_for(paper, 4).iter().enumerate() {
            let centre = sheet.get_pixel(cell.x + cell.w / 2, cell.y + cell.h / 2);
            assert_eq!(centre.0[0], 40 * i as u8 + 20, "cell {i} shows photo {i}");
        }
        // Fewer photos than cells leaves the rest white.
        let two = compose_sheet(&photos[..2], paper, 4);
        let last = &cells_for(paper, 4)[3];
        assert_eq!(*two.get_pixel(last.x + last.w / 2, last.y + last.h / 2), WHITE);
    }

    #[test]
    fn explicit_orientation_keeps_the_photo_upright_and_turns_the_sheet() {
        let paper = PaperSize::parse("card").unwrap(); // 55 x 91 mm portrait
        let wide = gradient(1600, 1000, 120);
        // Auto turns the landscape photo to fill the portrait card.
        let auto = compose_single_oriented(&wide, paper, true, Orientation::Auto);
        assert!(auto.height() > auto.width());
        // Portrait keeps it upright: borderless fill still covers the card, but crops the sides.
        let portrait = compose_single_oriented(&wide, paper, true, Orientation::Portrait);
        assert!(portrait.height() > portrait.width());
        assert_ne!(auto, portrait, "turned versus cropped");
        // Landscape: the sheet is composed sideways and turned onto the portrait paper, so the
        // photo (a gradient from left to right) now runs top to bottom.
        let land = compose_single_oriented(&wide, paper, false, Orientation::Landscape);
        assert!(land.height() > land.width());
        let top = land.get_pixel(land.width() / 2, land.height() / 10 * 3);
        let bottom = land.get_pixel(land.width() / 2, land.height() / 10 * 7);
        assert!(bottom.0[1] > top.0[1], "green rises toward the bottom: {top:?} {bottom:?}");
    }

    #[test]
    fn sheets_follow_the_orientation() {
        let paper = PaperSize::parse("card").unwrap();
        let photos: Vec<RgbImage> = (0..2).map(|i| gradient(80, 60, 40 * i as u8 + 20)).collect();
        let land = compose_sheet_oriented(&photos, paper, 2, Orientation::Landscape);
        assert_eq!(land.dimensions(), paper.pixels(300), "turned back onto the portrait paper");
        let auto = compose_sheet_oriented(&photos, paper, 2, Orientation::Auto);
        assert_ne!(land, auto);
    }

    #[test]
    fn jpeg_encoding_and_limits() {
        let img = gradient(64, 48, 100);
        let jpeg = encode_for_printer(&img).unwrap();
        assert_eq!(&jpeg[..3], &[0xFF, 0xD8, 0xFF]);
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (64, 48));
        assert!(encode_for_printer(&RgbImage::new(MAX_JPEG_EDGE + 1, 4)).is_err());
    }

    #[test]
    fn files_load_with_exif_orientation_and_bad_files_fail_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let img = gradient(40, 20, 90);
        let png = dir.path().join("p.png");
        img.save(&png).unwrap();
        assert_eq!(load_photo(&png).unwrap().dimensions(), (40, 20));
        let bmp = dir.path().join("b.bmp");
        img.save(&bmp).unwrap();
        assert_eq!(load_photo(&bmp).unwrap().dimensions(), (40, 20));
        let jpg = dir.path().join("j.jpg");
        std::fs::write(&jpg, encode_for_printer(&img).unwrap()).unwrap();
        assert_eq!(load_photo(&jpg).unwrap().dimensions(), (40, 20));
        let junk = dir.path().join("junk.jpg");
        std::fs::write(&junk, b"not an image at all").unwrap();
        assert!(matches!(load_photo(&junk), Err(Error::Image(_))));
        assert!(matches!(load_photo(&dir.path().join("missing.jpg")), Err(Error::Io { .. })));
    }

    #[test]
    fn icc_handling_is_safe() {
        let img = gradient(8, 8, 30);
        assert_eq!(icc_to_srgb(img.clone(), None), img);
        assert_eq!(icc_to_srgb(img.clone(), Some(b"....sRGB IEC61966....")), img);
        assert_eq!(icc_to_srgb(img.clone(), Some(b"garbage that is no profile")), img);
    }

    #[test]
    fn orientation_follows_the_target() {
        let wide = RgbImage::new(30, 20);
        assert_eq!(orient_to(wide.clone(), true).dimensions(), (20, 30));
        assert_eq!(orient_to(wide.clone(), false).dimensions(), (30, 20));
        assert_eq!(orient_to(RgbImage::new(10, 10), true).dimensions(), (10, 10));
    }
}
