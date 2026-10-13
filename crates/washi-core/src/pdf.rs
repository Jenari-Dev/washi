// SPDX-License-Identifier: GPL-2.0-or-later
//! A minimal PDF writer for scanned pages: one image per page, JPEG streams embedded as they
//! are (`DCTDecode`, no recompression) and 1-bit black-and-white pages as Flate-compressed bit
//! maps. No fonts, no text; searchable PDFs come from `tesseract` (see `scan::ocr`).

use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;

enum PageImage {
    Jpeg {
        data: Vec<u8>,
        gray: bool,
    },
    /// Packed 1-bit rows (MSB first, rows padded to whole bytes), `1` = white.
    Bilevel {
        packed: Vec<u8>,
    },
}

struct PdfPage {
    width_px: u32,
    height_px: u32,
    width_pt: f64,
    height_pt: f64,
    image: PageImage,
}

/// Collects pages, then [`PdfBuilder::finish`] serialises the document.
#[derive(Default)]
pub struct PdfBuilder {
    pages: Vec<PdfPage>,
    title: Option<String>,
}

fn pt(px: u32, dpi: f64) -> f64 {
    f64::from(px) * 72.0 / dpi.max(1.0)
}

/// PDF literal string with the characters that need escaping.
fn pdf_string(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push('?'),
        }
    }
    out.push(')');
    out
}

impl PdfBuilder {
    pub fn new() -> PdfBuilder {
        PdfBuilder::default()
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Add a page that shows this baseline JPEG (`gray`: single component) at `dpi`.
    pub fn add_jpeg_page(&mut self, jpeg: Vec<u8>, width: u32, height: u32, gray: bool, dpi: f64) {
        self.pages.push(PdfPage {
            width_px: width,
            height_px: height,
            width_pt: pt(width, dpi),
            height_pt: pt(height, dpi),
            image: PageImage::Jpeg { data: jpeg, gray },
        });
    }

    /// Add a black-and-white page from packed bits (MSB first, rows padded to bytes, 1 = white).
    pub fn add_bilevel_page(&mut self, packed: Vec<u8>, width: u32, height: u32, dpi: f64) {
        self.pages.push(PdfPage {
            width_px: width,
            height_px: height,
            width_pt: pt(width, dpi),
            height_pt: pt(height, dpi),
            image: PageImage::Bilevel { packed },
        });
    }

    /// Serialise the document.
    pub fn finish(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets: Vec<usize> = Vec::new(); // offsets[i] = offset of object i + 1

        // Object numbers: 1 catalog, 2 pages, 3 info, then (page, content, image) per page.
        let page_obj = |i: usize| 4 + i * 3;
        let kids: Vec<String> =
            (0..self.pages.len()).map(|i| format!("{} 0 R", page_obj(i))).collect();

        let begin = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, num: usize| {
            debug_assert_eq!(offsets.len() + 1, num);
            offsets.push(out.len());
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        };

        begin(&mut out, &mut offsets, 1);
        out.extend_from_slice(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        begin(&mut out, &mut offsets, 2);
        out.extend_from_slice(
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
                kids.join(" "),
                self.pages.len()
            )
            .as_bytes(),
        );
        begin(&mut out, &mut offsets, 3);
        let title =
            self.title.as_deref().map(|t| format!(" /Title {}", pdf_string(t))).unwrap_or_default();
        out.extend_from_slice(format!("<< /Producer (Washi){title} >>\nendobj\n").as_bytes());

        for (i, p) in self.pages.iter().enumerate() {
            let (pn, cn, imn) = (page_obj(i), page_obj(i) + 1, page_obj(i) + 2);
            begin(&mut out, &mut offsets, pn);
            out.extend_from_slice(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] /Resources << /XObject << /Im0 {imn} 0 R >> >> /Contents {cn} 0 R >>\nendobj\n",
                    w = p.width_pt,
                    h = p.height_pt
                )
                .as_bytes(),
            );
            begin(&mut out, &mut offsets, cn);
            let content = format!("q {:.2} 0 0 {:.2} 0 0 cm /Im0 Do Q\n", p.width_pt, p.height_pt);
            out.extend_from_slice(
                format!("<< /Length {} >>\nstream\n{content}endstream\nendobj\n", content.len())
                    .as_bytes(),
            );
            begin(&mut out, &mut offsets, imn);
            let (dict, data): (String, Vec<u8>) = match &p.image {
                PageImage::Jpeg { data, gray } => (
                    format!(
                        "/ColorSpace {} /BitsPerComponent 8 /Filter /DCTDecode",
                        if *gray { "/DeviceGray" } else { "/DeviceRGB" }
                    ),
                    data.clone(),
                ),
                PageImage::Bilevel { packed } => {
                    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
                    // Writing to a Vec cannot fail.
                    let _ = z.write_all(packed);
                    (
                        "/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /FlateDecode"
                            .to_string(),
                        z.finish().unwrap_or_default(),
                    )
                }
            };
            out.extend_from_slice(
                format!(
                    "<< /Type /XObject /Subtype /Image /Width {} /Height {} {dict} /Length {} >>\nstream\n",
                    p.width_px,
                    p.height_px,
                    data.len()
                )
                .as_bytes(),
            );
            out.extend_from_slice(&data);
            out.extend_from_slice(b"\nendstream\nendobj\n");
        }

        let xref_at = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
        );
        for off in &offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                offsets.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn structure_and_xref_offsets_are_consistent() {
        let mut b = PdfBuilder::new();
        b.set_title("Scan (1) \\ test");
        b.add_jpeg_page(vec![0xFF, 0xD8, 1, 2, 3, 0xFF, 0xD9], 300, 150, false, 300.0);
        b.add_bilevel_page(vec![0xFF; 4 * 8], 32, 8, 150.0);
        b.add_jpeg_page(vec![0xFF, 0xD8, 0xFF, 0xD9], 10, 10, true, 72.0);
        let pdf = b.finish();
        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF\n"));

        // startxref points at the xref table, whose entries point at "N 0 obj".
        let text = String::from_utf8_lossy(&pdf).into_owned();
        let sx: usize =
            text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert_eq!(&pdf[sx..sx + 4], b"xref");
        let table = String::from_utf8_lossy(&pdf[sx..]).into_owned();
        let entries: Vec<&str> = table.lines().skip(3).take_while(|l| l.ends_with(" n ")).collect();
        assert_eq!(entries.len(), 3 + 3 * 3, "catalog, pages, info + 3 objects per page");
        for (i, e) in entries.iter().enumerate() {
            let off: usize = e[..10].parse().unwrap();
            assert!(
                pdf[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()),
                "object {}",
                i + 1
            );
        }
        assert!(text.contains("/Count 3"));
        assert!(text.contains("/Title (Scan \\(1\\) \\\\ test)"));
        assert!(text.contains("/Size 13"));
    }

    #[test]
    fn page_size_follows_dpi_and_jpeg_is_embedded_verbatim() {
        let jpeg = vec![0xFF, 0xD8, 9, 8, 7, 6, 0xFF, 0xD9];
        let mut b = PdfBuilder::new();
        b.add_jpeg_page(jpeg.clone(), 2480, 3508, false, 300.0);
        let pdf = b.finish();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/MediaBox [0 0 595.20 841.92]"), "{text}");
        assert!(text.contains("/Filter /DCTDecode"));
        assert!(text.contains("/ColorSpace /DeviceRGB"));
        assert!(find(&pdf, &jpeg).is_some());
        let len_at = find(&pdf, b"/Length 8 >>").expect("image stream length");
        assert!(len_at > 0);
    }

    #[test]
    fn bilevel_pages_are_flate_compressed() {
        let mut b = PdfBuilder::new();
        b.add_bilevel_page(vec![0xFF; 1000], 400, 20, 300.0);
        let pdf = b.finish();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/BitsPerComponent 1 /Filter /FlateDecode"));
        assert!(pdf.len() < 1000, "1000 white bytes compress away");
    }

    #[test]
    fn empty_document_is_still_valid_syntax() {
        let pdf = PdfBuilder::new().finish();
        assert!(String::from_utf8_lossy(&pdf).contains("/Count 0"));
    }
}
