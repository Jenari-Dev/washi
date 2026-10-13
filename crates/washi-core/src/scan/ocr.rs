// SPDX-License-Identifier: GPL-2.0-or-later
//! OCR through the `tesseract` command-line program, when it is installed ([`Tools::can_ocr`]).

use std::process::Command;
use std::time::Duration;

use image::DynamicImage;

use super::save::encode_jpeg;
use crate::error::{Error, Result};
use crate::tools::{Tools, run_capture, stderr_hint};

const OCR_TIMEOUT: Duration = Duration::from_secs(300);

fn tesseract(tools: &Tools) -> Result<&std::path::Path> {
    tools.tesseract.as_deref().ok_or_else(|| {
        Error::tool(
            "tesseract",
            "not installed (install the `tesseract` package and its language data)",
        )
    })
}

/// `eng`, `jpn`, `eng+jpn`: letters, digits, `_` and `+` only (the value goes on a command line).
pub fn check_language(lang: &str) -> Result<()> {
    if lang.is_empty() || !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+') {
        return Err(Error::Invalid(format!(
            "{lang:?} is not a tesseract language code (e.g. eng, jpn, eng+jpn)"
        )));
    }
    Ok(())
}

/// Languages tesseract has data for.
pub fn languages(tools: &Tools) -> Result<Vec<String>> {
    let mut c = Command::new(tesseract(tools)?);
    c.arg("--list-langs");
    let o = run_capture(c, Duration::from_secs(20), "tesseract")?;
    if !o.status.success() {
        return Err(Error::tool("tesseract", stderr_hint(&o)));
    }
    Ok(String::from_utf8_lossy(&o.stdout)
        .lines()
        .skip(1) // "List of available languages in ..."
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Recognise the text on a page.
pub fn ocr_text(img: &DynamicImage, lang: &str, dpi: u32, tools: &Tools) -> Result<String> {
    check_language(lang)?;
    let exe = tesseract(tools)?;
    let dir =
        tempfile::tempdir().map_err(|e| Error::io("cannot create a temporary directory", e))?;
    let input = dir.path().join("page.jpg");
    std::fs::write(&input, encode_jpeg(img, 95, dpi)?)
        .map_err(|e| Error::io("cannot write the page", e))?;
    let mut c = Command::new(exe);
    c.arg(&input).arg("stdout").args(["-l", lang, "--dpi", &dpi.to_string()]);
    let o = run_capture(c, OCR_TIMEOUT, "tesseract")?;
    if !o.status.success() {
        return Err(Error::tool("tesseract", stderr_hint(&o)));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim_end().to_string())
}

/// A PDF with an invisible text layer over the page images.
pub fn searchable_pdf(
    pages: &[DynamicImage],
    lang: &str,
    dpi: u32,
    tools: &Tools,
) -> Result<Vec<u8>> {
    check_language(lang)?;
    let exe = tesseract(tools)?;
    if pages.is_empty() {
        return Err(Error::Invalid("there are no pages to recognise".into()));
    }
    let dir =
        tempfile::tempdir().map_err(|e| Error::io("cannot create a temporary directory", e))?;
    let mut list = String::new();
    for (i, p) in pages.iter().enumerate() {
        let path = dir.path().join(format!("p{:03}.jpg", i + 1));
        std::fs::write(&path, encode_jpeg(p, 92, dpi)?)
            .map_err(|e| Error::io("cannot write a page", e))?;
        list.push_str(&format!("{}\n", path.display()));
    }
    let list_path = dir.path().join("pages.txt");
    std::fs::write(&list_path, list).map_err(|e| Error::io("cannot write the page list", e))?;
    let base = dir.path().join("out");
    let mut c = Command::new(exe);
    c.arg(&list_path).arg(&base).args(["-l", lang, "--dpi", &dpi.to_string(), "pdf"]);
    let o = run_capture(c, OCR_TIMEOUT * 2, "tesseract")?;
    if !o.status.success() {
        return Err(Error::tool("tesseract", stderr_hint(&o)));
    }
    std::fs::read(base.with_extension("pdf")).map_err(|e| Error::io("tesseract wrote no PDF", e))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use image::RgbImage;

    use super::*;

    /// A stand-in `tesseract`: `--list-langs`, text to stdout, or a PDF named after the base.
    fn fake_tesseract(dir: &std::path::Path) -> Tools {
        let p = dir.join("tesseract");
        std::fs::write(
            &p,
            "#!/bin/sh\n\
             if [ \"$1\" = \"--list-langs\" ]; then echo 'List of available languages in \"/x\" (2):'; echo eng; echo jpn; exit 0; fi\n\
             if [ \"$2\" = \"stdout\" ]; then echo \"recognised $4\"; exit 0; fi\n\
             for last; do :; done\n\
             if [ \"$last\" = \"pdf\" ]; then printf '%%PDF-fake from %s' \"$1\" > \"$2.pdf\"; exit 0; fi\n\
             echo 'boom' >&2; exit 2\n",
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        Tools { tesseract: Some(p), ..Tools::default() }
    }

    #[test]
    fn language_codes_are_checked_before_use() {
        assert!(check_language("eng+jpn").is_ok());
        assert!(check_language("").is_err());
        assert!(check_language("eng; rm -rf /").is_err());
        assert!(check_language("-l").is_err());
    }

    #[test]
    fn missing_tesseract_is_a_clear_error() {
        let e =
            ocr_text(&DynamicImage::ImageRgb8(RgbImage::new(4, 4)), "eng", 300, &Tools::default())
                .unwrap_err();
        assert!(e.to_string().contains("not installed"), "{e}");
        assert!(languages(&Tools::default()).is_err());
    }

    #[test]
    fn text_languages_and_pdf_through_a_stand_in() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake_tesseract(dir.path());
        assert_eq!(languages(&tools).unwrap(), ["eng", "jpn"]);
        let img =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(20, 20, image::Rgb([255, 255, 255])));
        assert_eq!(ocr_text(&img, "jpn", 300, &tools).unwrap(), "recognised jpn");
        let pdf = searchable_pdf(&[img.clone(), img], "eng", 300, &tools).unwrap();
        assert!(pdf.starts_with(b"%PDF-fake from "), "{}", String::from_utf8_lossy(&pdf));
        assert!(searchable_pdf(&[], "eng", 300, &tools).is_err());
    }
}
