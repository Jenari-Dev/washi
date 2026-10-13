// SPDX-License-Identifier: GPL-2.0-or-later
//! PWG Raster (PWG 5102.4, `image/pwg-raster`): encoder and decoder.
//!
//! A stream is the sync word `RaS2` followed by pages. Each page is a 1796-byte header (all
//! integers big-endian; the `cups_page_header2_t` layout, with the fields PWG defines) and the
//! pixel lines, run-length coded:
//!
//! * every line starts with a *line repeat* byte `r`: the line appears `r + 1` times (1..=256);
//! * then chunks that cover exactly `width` pixels: a byte `c` in `0..=127` repeats the one pixel
//!   that follows `c + 1` times; a byte `c` in `129..=255` is followed by `257 - c` (2..=128)
//!   literal pixels; `128` fills the rest of the line with white (read, never written).
//!
//! Washi writes 8-bit `sRGB` (3 bytes per pixel) or `sGray` (1 byte per pixel) at 600 dpi: the
//! XK140 advertises `srgb_8` and `sgray_8` at 600 dpi.

use std::io::{self, Write};

use thiserror::Error;

pub const SYNC_WORD: &[u8; 4] = b"RaS2";
pub const HEADER_LEN: usize = 1796;

/// Header field offsets (bytes).
pub mod offset {
    pub const MEDIA_CLASS: usize = 0;
    pub const MEDIA_COLOR: usize = 64;
    pub const MEDIA_TYPE: usize = 128;
    pub const PRINT_CONTENT_TYPE: usize = 192;
    pub const CUT_MEDIA: usize = 268;
    pub const DUPLEX: usize = 272;
    pub const HW_RESOLUTION: usize = 276;
    pub const NUM_COPIES: usize = 340;
    pub const ORIENTATION: usize = 344;
    pub const PAGE_SIZE: usize = 352;
    pub const TUMBLE: usize = 368;
    pub const WIDTH: usize = 372;
    pub const HEIGHT: usize = 376;
    pub const BITS_PER_COLOR: usize = 384;
    pub const BITS_PER_PIXEL: usize = 388;
    pub const BYTES_PER_LINE: usize = 392;
    pub const COLOR_ORDER: usize = 396;
    pub const COLOR_SPACE: usize = 400;
    pub const NUM_COLORS: usize = 420;
    pub const TOTAL_PAGE_COUNT: usize = 452;
    pub const CROSS_FEED_TRANSFORM: usize = 456;
    pub const FEED_TRANSFORM: usize = 460;
    pub const PRINT_QUALITY: usize = 484;
    pub const RENDERING_INTENT: usize = 1668;
    pub const PAGE_SIZE_NAME: usize = 1732;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PwgError {
    #[error("not a PWG raster stream (missing RaS2 sync word)")]
    BadSync,
    #[error("truncated PWG raster data")]
    Truncated,
    #[error("unsupported PWG raster page: {0}")]
    Unsupported(String),
    #[error("corrupt PWG raster line data: {0}")]
    Corrupt(&'static str),
}

/// `cupsColorSpace` values PWG raster uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    /// `sgray_8`
    SGray,
    /// `srgb_8`
    SRgb,
}

impl ColorSpace {
    pub fn cups_value(self) -> u32 {
        match self {
            ColorSpace::SGray => 18,
            ColorSpace::SRgb => 19,
        }
    }

    pub fn from_cups(v: u32) -> Option<ColorSpace> {
        match v {
            18 => Some(ColorSpace::SGray),
            19 => Some(ColorSpace::SRgb),
            _ => None,
        }
    }

    pub fn bytes_per_pixel(self) -> usize {
        match self {
            ColorSpace::SGray => 1,
            ColorSpace::SRgb => 3,
        }
    }

    /// The IPP `pwg-raster-document-type-supported` keyword.
    pub fn ipp_type(self) -> &'static str {
        match self {
            ColorSpace::SGray => "sgray_8",
            ColorSpace::SRgb => "srgb_8",
        }
    }
}

/// The header fields Washi sets (everything else is zero, as PWG requires).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHeader {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
    pub colorspace: ColorSpace,
    /// Two-sided job (`Duplex`).
    pub duplex: bool,
    /// Short-edge binding (`Tumble`).
    pub tumble: bool,
    /// `MediaType` (the IPP media-type keyword), at most 63 bytes.
    pub media_type: String,
    /// `PageSizeName`, e.g. `iso_a4_210x297mm`.
    pub page_size_name: String,
    /// `PageSize` in points.
    pub page_width_pt: u32,
    pub page_height_pt: u32,
    /// `PrintQuality` (3 draft, 4 normal, 5 high; 0 = not set).
    pub quality: u32,
    /// `TotalPageCount` (0 = unknown).
    pub total_pages: u32,
    pub copies: u32,
    pub cross_feed_transform: i32,
    pub feed_transform: i32,
    /// `PrintContentType`, usually empty.
    pub content_type: String,
}

impl PageHeader {
    /// A header for a `width` x `height` px page at `dpi`, page size derived from the pixels.
    pub fn new(width: u32, height: u32, dpi: u32, colorspace: ColorSpace) -> PageHeader {
        let pt = |px: u32| ((f64::from(px) * 72.0 / f64::from(dpi.max(1))).round() as u32).max(1);
        PageHeader {
            width,
            height,
            dpi,
            colorspace,
            duplex: false,
            tumble: false,
            media_type: String::new(),
            page_size_name: String::new(),
            page_width_pt: pt(width),
            page_height_pt: pt(height),
            quality: 0,
            total_pages: 0,
            copies: 1,
            cross_feed_transform: 1,
            feed_transform: 1,
            content_type: String::new(),
        }
    }

    pub fn bytes_per_pixel(&self) -> usize {
        self.colorspace.bytes_per_pixel()
    }

    pub fn bytes_per_line(&self) -> usize {
        self.width as usize * self.bytes_per_pixel()
    }

    /// Size of the uncompressed pixel data.
    pub fn pixel_bytes(&self) -> usize {
        self.bytes_per_line() * self.height as usize
    }

    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut h = [0u8; HEADER_LEN];
        let put_str = |h: &mut [u8; HEADER_LEN], at: usize, s: &str| {
            let b = s.as_bytes();
            let n = b.len().min(63);
            h[at..at + n].copy_from_slice(&b[..n]);
        };
        let put_u32 = |h: &mut [u8; HEADER_LEN], at: usize, v: u32| {
            h[at..at + 4].copy_from_slice(&v.to_be_bytes());
        };
        put_str(&mut h, offset::MEDIA_CLASS, "PwgRaster");
        put_str(&mut h, offset::MEDIA_TYPE, &self.media_type);
        put_str(&mut h, offset::PRINT_CONTENT_TYPE, &self.content_type);
        put_u32(&mut h, offset::DUPLEX, u32::from(self.duplex));
        put_u32(&mut h, offset::HW_RESOLUTION, self.dpi);
        put_u32(&mut h, offset::HW_RESOLUTION + 4, self.dpi);
        put_u32(&mut h, offset::NUM_COPIES, self.copies);
        put_u32(&mut h, offset::PAGE_SIZE, self.page_width_pt);
        put_u32(&mut h, offset::PAGE_SIZE + 4, self.page_height_pt);
        put_u32(&mut h, offset::TUMBLE, u32::from(self.tumble));
        put_u32(&mut h, offset::WIDTH, self.width);
        put_u32(&mut h, offset::HEIGHT, self.height);
        put_u32(&mut h, offset::BITS_PER_COLOR, 8);
        put_u32(&mut h, offset::BITS_PER_PIXEL, 8 * self.bytes_per_pixel() as u32);
        put_u32(&mut h, offset::BYTES_PER_LINE, self.bytes_per_line() as u32);
        put_u32(&mut h, offset::COLOR_ORDER, 0); // chunky
        put_u32(&mut h, offset::COLOR_SPACE, self.colorspace.cups_value());
        put_u32(&mut h, offset::NUM_COLORS, self.bytes_per_pixel() as u32);
        put_u32(&mut h, offset::TOTAL_PAGE_COUNT, self.total_pages);
        put_u32(&mut h, offset::CROSS_FEED_TRANSFORM, self.cross_feed_transform as u32);
        put_u32(&mut h, offset::FEED_TRANSFORM, self.feed_transform as u32);
        put_u32(&mut h, offset::PRINT_QUALITY, self.quality);
        put_str(&mut h, offset::PAGE_SIZE_NAME, &self.page_size_name);
        h
    }

    pub fn parse(buf: &[u8]) -> Result<PageHeader, PwgError> {
        if buf.len() < HEADER_LEN {
            return Err(PwgError::Truncated);
        }
        let u32_at =
            |at: usize| u32::from_be_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]);
        let str_at = |at: usize| {
            let raw = &buf[at..at + 64];
            let end = raw.iter().position(|b| *b == 0).unwrap_or(64);
            String::from_utf8_lossy(&raw[..end]).into_owned()
        };
        if str_at(offset::MEDIA_CLASS) != "PwgRaster" {
            return Err(PwgError::Unsupported("MediaClass is not PwgRaster".into()));
        }
        let bpc = u32_at(offset::BITS_PER_COLOR);
        let space = ColorSpace::from_cups(u32_at(offset::COLOR_SPACE)).ok_or_else(|| {
            PwgError::Unsupported(format!("colour space {}", u32_at(offset::COLOR_SPACE)))
        })?;
        if bpc != 8 || u32_at(offset::BITS_PER_PIXEL) != 8 * space.bytes_per_pixel() as u32 {
            return Err(PwgError::Unsupported(format!(
                "{} bits per colour / {} per pixel",
                bpc,
                u32_at(offset::BITS_PER_PIXEL)
            )));
        }
        let width = u32_at(offset::WIDTH);
        if u32_at(offset::BYTES_PER_LINE) as usize != width as usize * space.bytes_per_pixel() {
            return Err(PwgError::Corrupt("BytesPerLine does not match width"));
        }
        Ok(PageHeader {
            width,
            height: u32_at(offset::HEIGHT),
            dpi: u32_at(offset::HW_RESOLUTION),
            colorspace: space,
            duplex: u32_at(offset::DUPLEX) != 0,
            tumble: u32_at(offset::TUMBLE) != 0,
            media_type: str_at(offset::MEDIA_TYPE),
            page_size_name: str_at(offset::PAGE_SIZE_NAME),
            page_width_pt: u32_at(offset::PAGE_SIZE),
            page_height_pt: u32_at(offset::PAGE_SIZE + 4),
            quality: u32_at(offset::PRINT_QUALITY),
            total_pages: u32_at(offset::TOTAL_PAGE_COUNT),
            copies: u32_at(offset::NUM_COPIES),
            cross_feed_transform: u32_at(offset::CROSS_FEED_TRANSFORM) as i32,
            feed_transform: u32_at(offset::FEED_TRANSFORM) as i32,
            content_type: str_at(offset::PRINT_CONTENT_TYPE),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Line coding
// ---------------------------------------------------------------------------------------------

/// Append the chunks for one line of `bpp`-byte pixels.
pub fn encode_line(line: &[u8], bpp: usize, out: &mut Vec<u8>) {
    debug_assert!(bpp > 0 && line.len() % bpp == 0);
    let n = line.len() / bpp;
    let px = |i: usize| &line[i * bpp..(i + 1) * bpp];
    let mut i = 0;
    while i < n {
        // How many identical pixels start here (capped at 128)?
        let mut run = 1;
        while i + run < n && run < 128 && px(i + run) == px(i) {
            run += 1;
        }
        if run >= 2 {
            out.push((run - 1) as u8);
            out.extend_from_slice(px(i));
            i += run;
            continue;
        }
        // A literal run: pixels that do not start a repeat, up to 128.
        let mut lit = 1;
        while i + lit < n && lit < 128 && (i + lit + 1 >= n || px(i + lit) != px(i + lit + 1)) {
            lit += 1;
        }
        if lit == 1 {
            // A single pixel is a run of one.
            out.push(0);
            out.extend_from_slice(px(i));
        } else {
            out.push((257 - lit) as u8);
            out.extend_from_slice(&line[i * bpp..(i + lit) * bpp]);
        }
        i += lit;
    }
}

/// Decode the chunks of one line into `out` (exactly `width` pixels). Returns the bytes used.
fn decode_line(
    data: &[u8],
    width: usize,
    bpp: usize,
    out: &mut Vec<u8>,
) -> Result<usize, PwgError> {
    let mut pos = 0usize;
    let mut px = 0usize;
    let take = |pos: &mut usize, n: usize| -> Result<&[u8], PwgError> {
        let end = pos.checked_add(n).ok_or(PwgError::Truncated)?;
        let s = data.get(*pos..end).ok_or(PwgError::Truncated)?;
        *pos = end;
        Ok(s)
    };
    while px < width {
        let c = take(&mut pos, 1)?[0];
        match c {
            0..=127 => {
                let count = usize::from(c) + 1;
                if px + count > width {
                    return Err(PwgError::Corrupt("repeat run crosses the end of the line"));
                }
                let pixel = take(&mut pos, bpp)?.to_vec();
                for _ in 0..count {
                    out.extend_from_slice(&pixel);
                }
                px += count;
            }
            128 => {
                out.resize(out.len() + (width - px) * bpp, 0xFF);
                px = width;
            }
            _ => {
                let count = 257 - usize::from(c);
                if px + count > width {
                    return Err(PwgError::Corrupt("literal run crosses the end of the line"));
                }
                out.extend_from_slice(take(&mut pos, count * bpp)?);
                px += count;
            }
        }
    }
    Ok(pos)
}

// ---------------------------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------------------------

/// Writes a PWG raster stream page by page.
pub struct PwgWriter<W: Write> {
    out: W,
    started: bool,
    pages: u32,
}

impl<W: Write> PwgWriter<W> {
    pub fn new(out: W) -> Self {
        PwgWriter { out, started: false, pages: 0 }
    }

    pub fn pages_written(&self) -> u32 {
        self.pages
    }

    fn start(&mut self) -> io::Result<()> {
        if !self.started {
            self.out.write_all(SYNC_WORD)?;
            self.started = true;
        }
        Ok(())
    }

    /// Write one page from a full pixel buffer (`header.pixel_bytes()` bytes, top line first).
    pub fn write_page(&mut self, header: &PageHeader, pixels: &[u8]) -> io::Result<()> {
        let bpl = header.bytes_per_line();
        if pixels.len() != header.pixel_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "page {}x{} needs {} bytes, got {}",
                    header.width,
                    header.height,
                    header.pixel_bytes(),
                    pixels.len()
                ),
            ));
        }
        self.write_rows(header, pixels.chunks_exact(bpl.max(1)))
    }

    /// Write one page from an iterator of lines (each `header.bytes_per_line()` long).
    pub fn write_rows<'a>(
        &mut self,
        header: &PageHeader,
        rows: impl Iterator<Item = &'a [u8]>,
    ) -> io::Result<()> {
        self.start()?;
        self.out.write_all(&header.encode())?;
        let bpp = header.bytes_per_pixel();
        let mut buf: Vec<u8> = Vec::with_capacity(1 << 20);
        let mut prev: Option<&[u8]> = None;
        let mut repeat = 0u32; // additional copies of `prev`
        let mut encoded_lines = 0u32;
        let flush_line = |prev: &[u8], repeat: u32, buf: &mut Vec<u8>, w: &mut W| {
            buf.push(repeat as u8);
            encode_line(prev, bpp, buf);
            if buf.len() >= (1 << 20) - 4096 {
                w.write_all(buf)?;
                buf.clear();
            }
            Ok::<(), io::Error>(())
        };
        for row in rows {
            if row.len() != header.bytes_per_line() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "row has the wrong length",
                ));
            }
            match prev {
                Some(p) if p == row && repeat < 255 => repeat += 1,
                Some(p) => {
                    flush_line(p, repeat, &mut buf, &mut self.out)?;
                    encoded_lines += repeat + 1;
                    prev = Some(row);
                    repeat = 0;
                }
                None => prev = Some(row),
            }
        }
        if let Some(p) = prev {
            flush_line(p, repeat, &mut buf, &mut self.out)?;
            encoded_lines += repeat + 1;
        }
        if encoded_lines != header.height {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{encoded_lines} lines written for a page of height {}", header.height),
            ));
        }
        self.out.write_all(&buf)?;
        self.pages += 1;
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.start()?; // an empty document is still a valid stream
        self.out.flush()?;
        Ok(self.out)
    }
}

// ---------------------------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------------------------

/// A decoded page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub header: PageHeader,
    pub pixels: Vec<u8>,
}

/// Decode a whole PWG raster stream (used by tests and for previews).
pub fn decode(bytes: &[u8]) -> Result<Vec<Page>, PwgError> {
    if bytes.len() < 4 || &bytes[..4] != SYNC_WORD {
        return Err(PwgError::BadSync);
    }
    let mut pos = 4usize;
    let mut pages = Vec::new();
    while pos < bytes.len() {
        let header = PageHeader::parse(bytes.get(pos..).ok_or(PwgError::Truncated)?)?;
        pos += HEADER_LEN;
        let (w, h, bpp) = (header.width as usize, header.height as usize, header.bytes_per_pixel());
        let mut pixels: Vec<u8> = Vec::with_capacity(header.pixel_bytes());
        let mut lines = 0usize;
        let mut line: Vec<u8> = Vec::with_capacity(w * bpp);
        while lines < h {
            let repeat = usize::from(*bytes.get(pos).ok_or(PwgError::Truncated)?) + 1;
            pos += 1;
            line.clear();
            pos += decode_line(bytes.get(pos..).ok_or(PwgError::Truncated)?, w, bpp, &mut line)?;
            if lines + repeat > h {
                return Err(PwgError::Corrupt("line repeat crosses the end of the page"));
            }
            for _ in 0..repeat {
                pixels.extend_from_slice(&line);
            }
            lines += repeat;
        }
        pages.push(Page { header, pixels });
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb_header(w: u32, h: u32) -> PageHeader {
        let mut hd = PageHeader::new(w, h, 600, ColorSpace::SRgb);
        hd.media_type = "stationery".into();
        hd.page_size_name = "iso_a4_210x297mm".into();
        hd.quality = 4;
        hd.total_pages = 3;
        hd
    }

    fn be(buf: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
    }

    #[test]
    fn header_bytes_are_exact() {
        let mut hd = PageHeader::new(4961, 7016, 600, ColorSpace::SRgb);
        hd.media_type = "stationery".into();
        hd.page_size_name = "iso_a4_210x297mm".into();
        hd.quality = 5;
        hd.total_pages = 7;
        hd.duplex = true;
        hd.tumble = true;
        hd.cross_feed_transform = -1;
        hd.feed_transform = -1;
        let b = hd.encode();
        assert_eq!(b.len(), 1796);
        assert_eq!(&b[0..10], b"PwgRaster\0");
        assert_eq!(&b[128..139], b"stationery\0");
        assert_eq!(be(&b, 272), 1, "Duplex");
        assert_eq!((be(&b, 276), be(&b, 280)), (600, 600), "HWResolution");
        assert_eq!((be(&b, 352), be(&b, 356)), (595, 842), "PageSize in points");
        assert_eq!(be(&b, 368), 1, "Tumble");
        assert_eq!((be(&b, 372), be(&b, 376)), (4961, 7016));
        assert_eq!((be(&b, 384), be(&b, 388), be(&b, 392)), (8, 24, 4961 * 3));
        assert_eq!((be(&b, 396), be(&b, 400), be(&b, 420)), (0, 19, 3));
        assert_eq!(be(&b, 452), 7, "TotalPageCount");
        assert_eq!(be(&b, 456) as i32, -1, "CrossFeedTransform");
        assert_eq!(be(&b, 460) as i32, -1, "FeedTransform");
        assert_eq!(be(&b, 484), 5, "PrintQuality");
        assert_eq!(&b[1732..1748], b"iso_a4_210x297mm");
        assert_eq!(b[1748], 0);
        // Fields PWG reserves stay zero (ImagingBoundingBox, Margins, cupsReal, vendor data...).
        assert!(b[284..300].iter().all(|x| *x == 0));
        assert!(b[516..1604].iter().all(|x| *x == 0));
        // Grayscale: 1 byte per pixel, colour space 18.
        let g = PageHeader::new(10, 4, 600, ColorSpace::SGray).encode();
        assert_eq!((be(&g, 388), be(&g, 392), be(&g, 400), be(&g, 420)), (8, 10, 18, 1));
        // Overlong strings are cut to 63 bytes + NUL.
        let mut long = PageHeader::new(1, 1, 600, ColorSpace::SGray);
        long.media_type = "x".repeat(200);
        assert_eq!(long.encode()[128 + 63], 0);
    }

    #[test]
    fn header_round_trips() {
        let hd = rgb_header(100, 50);
        assert_eq!(PageHeader::parse(&hd.encode()).unwrap(), hd);
        assert!(matches!(PageHeader::parse(&[0; 100]), Err(PwgError::Truncated)));
        let mut bad = hd.encode();
        bad[0] = b'X';
        assert!(matches!(PageHeader::parse(&bad), Err(PwgError::Unsupported(_))));
        let mut bad = hd.encode();
        bad[offset::COLOR_SPACE + 3] = 6;
        assert!(PageHeader::parse(&bad).is_err());
    }

    #[test]
    fn packbits_chunks_are_exact() {
        let px = |v: u8| [v, v, v];
        // Four identical pixels: one repeat chunk (count - 1 = 3).
        let mut out = Vec::new();
        encode_line(&[px(9), px(9), px(9), px(9)].concat(), 3, &mut out);
        assert_eq!(out, [3, 9, 9, 9]);
        // Three different pixels: a literal chunk, 257 - 3 = 254.
        out.clear();
        encode_line(&[px(1), px(2), px(3)].concat(), 3, &mut out);
        assert_eq!(out, [254, 1, 1, 1, 2, 2, 2, 3, 3, 3]);
        // A single pixel is a run of one: count byte 0.
        out.clear();
        encode_line(&px(7), 3, &mut out);
        assert_eq!(out, [0, 7, 7, 7]);
        // Literal, then a repeat that starts mid-line: 1 2 | 5 5 5
        out.clear();
        encode_line(&[1u8, 2, 5, 5, 5], 1, &mut out);
        assert_eq!(out, [255, 1, 2, 2, 5]);
        // 128 equal pixels is the longest repeat (127); 130 -> 128 + 2.
        out.clear();
        encode_line(&[4u8; 130], 1, &mut out);
        assert_eq!(out, [127, 4, 1, 4]);
        // 130 distinct-neighbour pixels: literal of 128 (byte 129) then a literal of 2 (255).
        out.clear();
        let line: Vec<u8> = (0..130u32).map(|i| (i % 2) as u8).collect();
        encode_line(&line, 1, &mut out);
        assert_eq!(out[0], 129);
        assert_eq!(out[1 + 128], 255);
        assert_eq!(out.len(), 1 + 128 + 1 + 2);
    }

    #[test]
    fn single_page_stream_is_exact() {
        let hd = PageHeader::new(3, 2, 600, ColorSpace::SGray);
        let mut w = PwgWriter::new(Vec::new());
        w.write_page(&hd, &[1, 1, 1, 1, 1, 1]).unwrap();
        let bytes = w.finish().unwrap();
        assert_eq!(&bytes[..4], b"RaS2");
        // Two identical lines: repeat byte 1 (= 2 lines), one repeat chunk of 3 pixels.
        assert_eq!(&bytes[4 + 1796..], &[1, 2, 1]);
        assert_eq!(bytes.len(), 4 + 1796 + 3);
        assert_eq!(decode(&bytes).unwrap()[0].pixels, [1; 6]);
    }

    fn pattern(w: usize, h: usize, bpp: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                // Bands, a ramp, some repeated lines and some noise.
                let base = if y < h / 4 {
                    255
                } else if y % 7 == 0 {
                    (x * 3 % 256) as u8
                } else {
                    ((x / 5 + y / 3) % 256) as u8
                };
                for c in 0..bpp {
                    v.push(base.wrapping_add(c as u8 * 17));
                }
            }
        }
        v
    }

    #[test]
    fn round_trip_rgb_gray_and_multiple_pages() {
        let (w, h) = (300, 90);
        let rgb = pattern(w, h, 3);
        let gray = pattern(w, h, 1);
        let mut hw = PwgWriter::new(Vec::new());
        let mut first = rgb_header(w as u32, h as u32);
        first.duplex = true;
        hw.write_page(&first, &rgb).unwrap();
        let mut second = PageHeader::new(w as u32, h as u32, 600, ColorSpace::SGray);
        second.duplex = true;
        second.tumble = false;
        hw.write_page(&second, &gray).unwrap();
        assert_eq!(hw.pages_written(), 2);
        let bytes = hw.finish().unwrap();
        let pages = decode(&bytes).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].header, first);
        assert_eq!(pages[0].pixels, rgb);
        assert_eq!(pages[1].header, second);
        assert_eq!(pages[1].pixels, gray);
        assert!(bytes.len() < rgb.len() + gray.len(), "the pattern compresses");
    }

    #[test]
    fn more_than_256_identical_lines_split_the_repeat() {
        let hd = PageHeader::new(8, 600, 600, ColorSpace::SGray);
        let px = vec![200u8; 8 * 600];
        let mut w = PwgWriter::new(Vec::new());
        w.write_page(&hd, &px).unwrap();
        let bytes = w.finish().unwrap();
        // 600 = 256 + 256 + 88 identical lines: three line records of 3 bytes each.
        assert_eq!(bytes.len(), 4 + 1796 + 3 * 3);
        assert_eq!(&bytes[4 + 1796..], &[255, 7, 200, 255, 7, 200, 87, 7, 200]);
        assert_eq!(decode(&bytes).unwrap()[0].pixels, px);
    }

    #[test]
    fn writer_rejects_wrong_sizes() {
        let hd = PageHeader::new(4, 4, 600, ColorSpace::SGray);
        let mut w = PwgWriter::new(Vec::new());
        assert!(w.write_page(&hd, &[0; 15]).is_err());
        let rows: Vec<&[u8]> = vec![&[0, 0, 0, 0]; 3];
        assert!(w.write_rows(&hd, rows.into_iter()).is_err(), "too few lines");
        assert!(w.write_rows(&hd, std::iter::once(&[0u8; 3][..])).is_err(), "short row");
    }

    #[test]
    fn decoder_handles_the_white_fill_code_and_rejects_garbage() {
        let hd = PageHeader::new(4, 1, 600, ColorSpace::SGray);
        let mut stream = SYNC_WORD.to_vec();
        stream.extend_from_slice(&hd.encode());
        stream.extend_from_slice(&[0, 0, 9, 128]); // 1 line: one pixel 9, then white fill
        assert_eq!(decode(&stream).unwrap()[0].pixels, [9, 255, 255, 255]);

        assert_eq!(decode(b"nope"), Err(PwgError::BadSync));
        assert_eq!(decode(b""), Err(PwgError::BadSync));
        // Truncated everywhere: never a panic, always an error (or a clean shorter parse).
        let mut ok = PwgWriter::new(Vec::new());
        ok.write_page(&rgb_header(20, 5), &pattern(20, 5, 3)).unwrap();
        let ok = ok.finish().unwrap();
        for cut in 5..ok.len() {
            assert!(decode(&ok[..cut]).is_err(), "cut at {cut} decoded");
        }
        // A run that overshoots the line is corrupt, not a panic.
        let mut bad = SYNC_WORD.to_vec();
        bad.extend_from_slice(&hd.encode());
        bad.extend_from_slice(&[0, 127, 1]);
        assert!(matches!(decode(&bad), Err(PwgError::Corrupt(_))));
        // Bit flips never panic.
        for i in (4..ok.len()).step_by(7) {
            let mut f = ok.clone();
            f[i] ^= 0xA5;
            let _ = decode(&f);
        }
    }
}
