// SPDX-License-Identifier: GPL-2.0-or-later
//! Scanning: the eSCL flow ([`flow`]) and the host-side image pipeline behind the Scan screens.
//!
//! * [`ScanSession`]: the pages of one scan job, in order. "+ Next scan" is
//!   [`ScanSession::add_page`]; delete, reorder and rotate are methods too.
//! * [`Page`] keeps the scanner's image untouched plus a [`PageEdits`] recipe (rotation, slant,
//!   crop, image correction); [`Page::render`] applies the recipe, "Revert to original" resets
//!   it. Edits never modify the original.
//! * [`ops`] (rotate, slant, trim, aspect presets), [`correct`] (colour correction presets,
//!   show-through, colour mode, unsharp mask), [`detect`] (auto crop, multi-photo, document or
//!   photo), [`stitch`], [`ocr`] and [`save`] (JPEG / PDF) are plain functions the GUI can call.

use std::sync::Arc;

use image::{DynamicImage, RgbImage};
use serde::{Deserialize, Serialize};

pub mod correct;
pub mod detect;
pub mod flow;
pub mod ocr;
pub mod ops;
pub mod save;
pub mod sizes;
pub mod stitch;

pub use correct::{ColorCorrection, ImageCorrection, OutputMode};
pub use ops::{AspectPreset, CropRect, MAX_SLANT_DEGREES, Rotation};

use crate::error::{Error, Result};

/// The recipe applied to a page's original image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageEdits {
    pub rotation: Rotation,
    /// Slant correction in degrees (clockwise positive), within +-[`MAX_SLANT_DEGREES`].
    pub slant_degrees: f32,
    /// Trimming box as fractions of the rotated, de-slanted image.
    pub crop: Option<CropRect>,
    pub correction: ImageCorrection,
}

impl Default for PageEdits {
    fn default() -> Self {
        PageEdits {
            rotation: Rotation::None,
            slant_degrees: 0.0,
            crop: None,
            correction: ImageCorrection::default(),
        }
    }
}

impl PageEdits {
    pub fn is_identity(&self) -> bool {
        *self == PageEdits::default()
    }
}

/// One scanned page.
#[derive(Debug, Clone)]
pub struct Page {
    original: Arc<DynamicImage>,
    /// Resolution the page was scanned at (sizes the PDF page; scales filter radii).
    pub dpi: u32,
    pub edits: PageEdits,
}

const WHITE: image::Rgb<u8> = image::Rgb([255, 255, 255]);

impl Page {
    pub fn new(image: DynamicImage, dpi: u32) -> Page {
        Page { original: Arc::new(image), dpi: dpi.max(1), edits: PageEdits::default() }
    }

    /// Decode an image file's bytes (the scanner's JPEG).
    pub fn from_bytes(bytes: &[u8], dpi: u32) -> Result<Page> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| Error::Image(format!("cannot decode the scanned image: {e}")))?;
        Ok(Page::new(img, dpi))
    }

    pub fn original(&self) -> &DynamicImage {
        &self.original
    }

    /// Size of the untouched original.
    pub fn original_size(&self) -> (u32, u32) {
        (self.original.width(), self.original.height())
    }

    /// Size of the image after rotation (before slant and crop): what the crop box refers to.
    pub fn rotated_size(&self) -> (u32, u32) {
        let (w, h) = self.original_size();
        if self.edits.rotation.swaps_axes() { (h, w) } else { (w, h) }
    }

    pub fn is_edited(&self) -> bool {
        !self.edits.is_identity()
    }

    /// "Revert to original".
    pub fn revert(&mut self) {
        self.edits = PageEdits::default();
    }

    pub fn rotate_left(&mut self) {
        self.rotate(Rotation::Left);
    }

    pub fn rotate_right(&mut self) {
        self.rotate(Rotation::Right);
    }

    /// Turn the page; a crop box is turned with it so it keeps framing the same content.
    pub fn rotate(&mut self, by: Rotation) {
        if by == Rotation::None {
            return;
        }
        if let Some(c) = self.edits.crop {
            self.edits.crop = Some(match by {
                Rotation::Right => CropRect { x: 1.0 - c.y - c.h, y: c.x, w: c.h, h: c.w },
                Rotation::Left => CropRect { x: c.y, y: 1.0 - c.x - c.w, w: c.h, h: c.w },
                _ => CropRect { x: 1.0 - c.x - c.w, y: 1.0 - c.y - c.h, w: c.w, h: c.h },
            });
        }
        self.edits.rotation = self.edits.rotation.then(by);
    }

    pub fn set_slant(&mut self, degrees: f32) {
        self.edits.slant_degrees = degrees.clamp(-MAX_SLANT_DEGREES, MAX_SLANT_DEGREES);
    }

    /// Detect the tilt of the (rotated) page and set the slant correction; returns it.
    pub fn auto_deskew(&mut self) -> f32 {
        let base = ops::rotate_quarters(&self.original.to_rgb8(), self.edits.rotation);
        let angle = ops::estimate_skew(&base);
        self.set_slant(angle);
        self.edits.slant_degrees
    }

    pub fn set_crop(&mut self, crop: Option<CropRect>) {
        self.edits.crop = crop.map(|c| c.clamped(0.005)).filter(|c| !c.is_full());
    }

    /// Apply a "Trimming Aspect Ratio" preset to the current crop (or the whole page).
    pub fn set_aspect(&mut self, preset: AspectPreset) {
        let (w, h) = self.rotated_size();
        let r = ops::fit_aspect(w, h, preset, self.edits.crop);
        self.set_crop(Some(r));
    }

    pub fn set_correction(&mut self, c: ImageCorrection) {
        self.edits.correction = c;
    }

    /// Apply the recipe at full resolution. The result is `Rgb8` (colour) or `Luma8`
    /// (grayscale and black-and-white).
    pub fn render(&self) -> DynamicImage {
        self.render_scaled(None)
    }

    /// Like [`Page::render`], but the original is first shrunk so that its longest edge is at
    /// most `max_edge` (fast previews and thumbnails).
    pub fn render_preview(&self, max_edge: u32) -> DynamicImage {
        self.render_scaled(Some(max_edge.max(1)))
    }

    fn render_scaled(&self, max_edge: Option<u32>) -> DynamicImage {
        let mut img: RgbImage = self.original.to_rgb8();
        let mut dpi = self.dpi;
        if let Some(max) = max_edge {
            let long = img.width().max(img.height());
            if long > max {
                let f = max as f32 / long as f32;
                img = image::imageops::resize(
                    &img,
                    ((img.width() as f32 * f).round() as u32).max(1),
                    ((img.height() as f32 * f).round() as u32).max(1),
                    image::imageops::FilterType::Triangle,
                );
                dpi = ((dpi as f32 * f).round() as u32).max(1);
            }
        }
        let e = &self.edits;
        if e.rotation != Rotation::None {
            img = ops::rotate_quarters(&img, e.rotation);
        }
        if e.slant_degrees.abs() > 1e-3 {
            img = ops::rotate_degrees(&img, e.slant_degrees, WHITE);
        }
        if let Some(c) = e.crop {
            img = ops::crop(&img, c);
        }
        let c = &e.correction;
        correct::apply_color_correction(&mut img, c.color_correction, dpi);
        if c.reduce_show_through {
            correct::reduce_show_through(&mut img, c.show_through_threshold);
        }
        correct::unsharp(&mut img, c.unsharp_amount, dpi);
        match c.color_mode {
            OutputMode::Color => DynamicImage::ImageRgb8(img),
            OutputMode::Grayscale => DynamicImage::ImageLuma8(image::imageops::grayscale(&img)),
            OutputMode::BlackWhite => {
                let gray = image::imageops::grayscale(&img);
                DynamicImage::ImageLuma8(correct::threshold_bw(&gray, c.bw_threshold))
            }
        }
    }
}

/// The pages of one scan, in order.
#[derive(Debug, Clone, Default)]
pub struct ScanSession {
    pages: Vec<Page>,
}

impl ScanSession {
    pub fn new() -> ScanSession {
        ScanSession::default()
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub fn page(&self, i: usize) -> Option<&Page> {
        self.pages.get(i)
    }

    pub fn page_mut(&mut self, i: usize) -> Option<&mut Page> {
        self.pages.get_mut(i)
    }

    fn check(&self, i: usize) -> Result<()> {
        if i < self.pages.len() {
            Ok(())
        } else {
            Err(Error::Invalid(format!(
                "page {} does not exist (the scan has {} page(s))",
                i + 1,
                self.pages.len()
            )))
        }
    }

    /// "+ Next scan": append a page; returns its index.
    pub fn add_page(&mut self, page: Page) -> usize {
        self.pages.push(page);
        self.pages.len() - 1
    }

    pub fn delete_page(&mut self, i: usize) -> Result<Page> {
        self.check(i)?;
        Ok(self.pages.remove(i))
    }

    /// Move page `from` so that it ends up at index `to`.
    pub fn move_page(&mut self, from: usize, to: usize) -> Result<()> {
        self.check(from)?;
        self.check(to)?;
        let p = self.pages.remove(from);
        self.pages.insert(to, p);
        Ok(())
    }

    pub fn rotate_page_left(&mut self, i: usize) -> Result<()> {
        self.check(i)?;
        self.pages[i].rotate_left();
        Ok(())
    }

    pub fn rotate_page_right(&mut self, i: usize) -> Result<()> {
        self.check(i)?;
        self.pages[i].rotate_right();
        Ok(())
    }

    pub fn revert_page(&mut self, i: usize) -> Result<()> {
        self.check(i)?;
        self.pages[i].revert();
        Ok(())
    }

    pub fn render_page(&self, i: usize) -> Result<DynamicImage> {
        self.check(i)?;
        Ok(self.pages[i].render())
    }
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;

    /// A 40x20 image: red left half, blue right half, a green marker at the top-left corner.
    fn sample() -> DynamicImage {
        let mut img = RgbImage::new(40, 20);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 20 { Rgb([200, 0, 0]) } else { Rgb([0, 0, 200]) };
        }
        img.put_pixel(0, 0, Rgb([0, 255, 0]));
        DynamicImage::ImageRgb8(img)
    }

    fn session(n: usize) -> ScanSession {
        let mut s = ScanSession::new();
        for i in 0..n {
            let mut p = Page::new(sample(), 300);
            p.dpi = 100 + i as u32; // a marker to tell pages apart
            s.add_page(p);
        }
        s
    }

    #[test]
    fn pages_are_added_deleted_and_reordered() {
        let mut s = session(3);
        assert_eq!(s.len(), 3);
        assert_eq!(s.add_page(Page::new(sample(), 150)), 3);
        s.move_page(0, 2).unwrap();
        assert_eq!(s.pages().iter().map(|p| p.dpi).collect::<Vec<_>>(), [101, 102, 100, 150]);
        s.move_page(3, 0).unwrap();
        assert_eq!(s.page(0).unwrap().dpi, 150);
        let gone = s.delete_page(1).unwrap();
        assert_eq!(gone.dpi, 101);
        assert_eq!(s.len(), 3);
        assert!(s.delete_page(3).is_err());
        assert!(s.move_page(0, 9).is_err());
        assert!(s.rotate_page_left(7).is_err());
        assert!(s.render_page(7).is_err());
        assert!(!s.is_empty());
    }

    #[test]
    fn rotation_swaps_the_axes_and_revert_restores() {
        let mut s = session(1);
        s.rotate_page_right(0).unwrap();
        assert_eq!(s.render_page(0).unwrap().to_rgb8().dimensions(), (20, 40));
        assert_eq!(s.page(0).unwrap().rotated_size(), (20, 40));
        // The marker at the top-left went to the top-right.
        assert_eq!(s.render_page(0).unwrap().to_rgb8().get_pixel(19, 0).0, [0, 255, 0]);
        s.rotate_page_left(0).unwrap();
        assert_eq!(s.page(0).unwrap().edits.rotation, Rotation::None);
        s.rotate_page_left(0).unwrap();
        s.rotate_page_left(0).unwrap();
        assert_eq!(s.page(0).unwrap().edits.rotation, Rotation::Half);
        assert!(s.page(0).unwrap().is_edited());
        s.revert_page(0).unwrap();
        assert!(!s.page(0).unwrap().is_edited());
        assert_eq!(s.render_page(0).unwrap().to_rgb8(), sample().to_rgb8());
    }

    #[test]
    fn a_crop_box_follows_the_content_when_the_page_is_turned() {
        let mut p = Page::new(sample(), 300);
        // The right (blue) half.
        p.set_crop(Some(CropRect { x: 0.5, y: 0.0, w: 0.5, h: 1.0 }));
        let blue = |img: &DynamicImage| {
            let rgb = img.to_rgb8();
            rgb.pixels().all(|px| px.0 == [0, 0, 200])
        };
        assert!(blue(&p.render()));
        for turn in [Rotation::Right, Rotation::Left, Rotation::Half] {
            p.rotate(turn);
            assert!(blue(&p.render()), "still framing the blue half after {turn:?}");
        }
        // Full-size crops are dropped.
        p.set_crop(Some(CropRect::FULL));
        assert!(p.edits.crop.is_none());
    }

    #[test]
    fn aspect_presets_set_the_crop() {
        let mut p = Page::new(sample(), 300); // 2:1 landscape
        p.set_aspect(AspectPreset::Square);
        let img = p.render().to_rgb8();
        assert_eq!(img.dimensions(), (20, 20));
        p.revert();
        p.set_aspect(AspectPreset::R16x9);
        let (w, h) = p.render().to_rgb8().dimensions();
        assert!((w as f32 / h as f32 - 16.0 / 9.0).abs() < 0.1, "{w}x{h}");
        p.revert();
        p.set_aspect(AspectPreset::Custom);
        assert!(p.edits.crop.is_none(), "Custom on an uncropped page changes nothing");
    }

    #[test]
    fn slant_is_clamped_and_changes_the_pixels() {
        let mut p = Page::new(sample(), 300);
        p.set_slant(45.0);
        assert_eq!(p.edits.slant_degrees, MAX_SLANT_DEGREES);
        p.set_slant(-99.0);
        assert_eq!(p.edits.slant_degrees, -MAX_SLANT_DEGREES);
        p.set_slant(5.0);
        let out = p.render().to_rgb8();
        assert_eq!(out.dimensions(), (40, 20));
        assert_ne!(out, sample().to_rgb8());
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255], "the corner is filled with white");
    }

    #[test]
    fn colour_modes_change_the_output_type() {
        let mut p = Page::new(sample(), 300);
        let mut c = ImageCorrection::default();
        assert!(matches!(p.render(), DynamicImage::ImageRgb8(_)));
        c.color_mode = OutputMode::Grayscale;
        p.set_correction(c);
        assert!(matches!(p.render(), DynamicImage::ImageLuma8(_)));
        c.color_mode = OutputMode::BlackWhite;
        c.bw_threshold = Some(60);
        p.set_correction(c);
        let bw = p.render().to_luma8();
        assert!(bw.pixels().all(|px| px.0[0] == 0 || px.0[0] == 255));
        assert!(p.is_edited());
    }

    #[test]
    fn preview_is_smaller_but_follows_the_same_recipe() {
        let mut big = RgbImage::from_pixel(400, 200, Rgb([10, 10, 10]));
        big.put_pixel(0, 0, Rgb([255, 255, 255]));
        let mut p = Page::new(DynamicImage::ImageRgb8(big), 300);
        p.rotate_right();
        let prev = p.render_preview(100).to_rgb8();
        assert_eq!(prev.dimensions(), (50, 100));
        assert_eq!(p.render().to_rgb8().dimensions(), (200, 400));
        // A preview request larger than the image changes nothing.
        assert_eq!(p.render_preview(5000).to_rgb8().dimensions(), (200, 400));
    }

    #[test]
    fn auto_deskew_straightens_a_tilted_text_page() {
        let page = ops_tests_text_page();
        let tilted = ops::rotate_degrees(&page, 3.0, Rgb([240, 240, 240]));
        let mut p = Page::new(DynamicImage::ImageRgb8(tilted), 300);
        let a = p.auto_deskew();
        assert!((a + 3.0).abs() < 0.4, "{a}");
        assert_eq!(p.edits.slant_degrees, a);
    }

    fn ops_tests_text_page() -> RgbImage {
        let (w, h) = (600u32, 800u32);
        let mut img = RgbImage::from_pixel(w, h, Rgb([240, 240, 240]));
        let mut y = h / 12;
        while y + 6 < h * 11 / 12 {
            let mut x = w / 10;
            while x + 12 < w * 9 / 10 {
                let len = 8 + (x * 7 + y * 3) % 18;
                for yy in y..y + 5 {
                    for xx in x..(x + len).min(w * 9 / 10) {
                        img.put_pixel(xx, yy, Rgb([20, 20, 20]));
                    }
                }
                x += len + 5 + (x + y) % 7;
            }
            y += 14;
        }
        img
    }

    #[test]
    fn undecodable_bytes_are_an_error() {
        assert!(matches!(Page::from_bytes(b"not an image", 300), Err(Error::Image(_))));
    }
}
