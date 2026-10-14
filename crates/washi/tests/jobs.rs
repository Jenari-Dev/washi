// SPDX-License-Identifier: GPL-2.0-or-later
//! `washi scan`, `print`, `disc`, `identify`, `media`, `scanner`: dry runs open no connection,
//! confirmed runs work end to end against the in-process fake printer. No real device.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use washi_fake::FakePrinter;

fn washi(fake: &FakePrinter, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_washi"))
        .args(["--host", &fake.chmp_target(), "--ipp-port", &fake.ipp.port().to_string()])
        .args(args)
        .env("XDG_CONFIG_HOME", "/nonexistent/washi-test")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// Run against a host that listens but must never be contacted.
fn dry(args: &[&str]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let host = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let out = Command::new(env!("CARGO_BIN_EXE_washi"))
        .args(["--host", &host])
        .args(args)
        .env("XDG_CONFIG_HOME", "/nonexistent/washi-test")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{args:?}: {stderr}\n{stdout}");
    assert!(stdout.starts_with("DRY RUN"), "{stdout}");
    assert!(
        matches!(listener.accept(), Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "{args:?} opened a connection"
    );
    stdout
}

fn photo(dir: &Path, name: &str) -> String {
    let p = dir.join(name);
    image::RgbImage::from_fn(300, 200, |x, y| {
        image::Rgb([(x * 255 / 300) as u8, (y * 255 / 200) as u8, 80])
    })
    .save(&p)
    .unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn scan_dry_run_shows_the_exact_request_and_connects_to_nothing() {
    let s = dry(&[
        "scan",
        "--size",
        "a4",
        "--res",
        "300",
        "--mode",
        "color",
        "--type",
        "document",
        "--format",
        "pdf",
        "-o",
        "/tmp/out.pdf",
    ]);
    assert!(s.contains("POST http://127.0.0.1:"), "{s}");
    assert!(s.contains("/eSCL/ScanJobs HTTP/1.1") && s.contains("Content-Type: text/xml"), "{s}");
    for want in [
        "<pwg:Version>2.1</pwg:Version>",
        "<scan:Intent>Document</scan:Intent>",
        "<pwg:Width>2480</pwg:Width>",
        "<pwg:Height>3508</pwg:Height>",
        "<pwg:InputSource>Platen</pwg:InputSource>",
        "<scan:ColorMode>RGB24</scan:ColorMode>",
        "<scan:XResolution>300</scan:XResolution>",
        "<scan:DocumentFormatExt>image/jpeg</scan:DocumentFormatExt>",
    ] {
        assert!(s.contains(want), "missing {want}\n{s}");
    }
    assert!(s.contains("NextDocument") && s.contains("Run again with --confirm to scan."), "{s}");
    // Variants: auto size, black and white, photo, several pages, stitching, OCR note.
    let s = dry(&[
        "scan", "--size", "auto", "--mode", "bw", "--type", "photo", "--res", "600", "-o", "x.jpg",
        "--format", "jpeg", "--pages", "3", "--deskew", "--ocr",
    ]);
    assert!(
        s.contains("<pwg:Width>2550</pwg:Width>")
            && s.contains("Grayscale8")
            && s.contains("<scan:Intent>Photo</scan:Intent>"),
        "{s}"
    );
    assert!(
        s.contains("3 scans")
            && s.contains("auto-crop")
            && s.contains("black and white on the host")
            && s.contains("tesseract"),
        "{s}"
    );
    dry(&["scan", "-o", "x.pdf", "--auto"]);
    dry(&["scan", "-o", "x.pdf", "--stitch", "left-right", "--no-intent"]);
}

#[test]
fn scan_arguments_are_checked_in_the_dry_run_too() {
    for args in [
        &["scan", "--size", "a3", "-o", "x"][..],
        &["scan", "--res", "250", "-o", "x"],
        &["scan", "--mode", "sepia", "-o", "x"],
        &["scan"],
        &["scan", "-o", "x", "--auto", "--crop-photos"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_washi"))
            .args(["--host", "127.0.0.1:9"])
            .args(args)
            .env("XDG_CONFIG_HOME", "/nonexistent")
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?} should fail");
    }
}

#[test]
fn print_dry_run_lists_the_ipp_request() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = photo(dir.path(), "beach.png");
    let s = dry(&[
        "print",
        &jpg,
        "--size",
        "l",
        "--media-type",
        "glossy",
        "--source",
        "rear",
        "--borderless",
        "--quality",
        "high",
        "--copies",
        "2",
    ]);
    for want in [
        "Print-Job (IPP 2.0",
        "document-format (mimeMediaType) = image/jpeg",
        "copies (integer) = 2",
        "print-quality (enum) = 5",
        "print-scaling (keyword) = fill",
        "media-source=rear",
        "media-type=com.canon-mtglossy",
        "x-dimension=8890 y-dimension=12700",
        "media-top-margin=0",
        "Validate-Job",
        "ipp://127.0.0.1:",
        "Run again with --confirm to print.",
    ] {
        assert!(s.contains(want), "missing {want}\n{s}");
    }
}

#[test]
fn print_dry_run_for_a_pdf_names_the_raster_format_and_the_pages() {
    let tools = washi_core::Tools::detect();
    if !tools.can_print_pdf() {
        eprintln!("no poppler: skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("doc.pdf");
    let mut b = washi_core::pdf::PdfBuilder::new();
    for _ in 0..4 {
        b.add_bilevel_page(vec![0xFF; 8 * 80], 64, 80, 8.0);
    }
    std::fs::write(&pdf, b.finish()).unwrap();
    let s = dry(&[
        "print",
        &pdf.to_string_lossy(),
        "--pages",
        "1-3",
        "--layout",
        "2",
        "--duplex",
        "long",
        "--mono",
        "--source",
        "cassette",
        "--media-type",
        "plain",
        "--page-order",
        "reverse",
    ]);
    assert!(s.contains("document-format (mimeMediaType) = image/pwg-raster"), "{s}");
    assert!(s.contains("3 of 4 page(s) -> 1 sheet(s), two-sided, last sheet first"), "{s}");
    assert!(
        s.contains("sides (keyword) = two-sided-long-edge")
            && s.contains("print-color-mode (keyword) = monochrome"),
        "{s}"
    );
    assert!(s.contains("media-source=main") && s.contains("media-type=stationery"), "{s}");
    assert!(!s.contains("print-scaling"), "raster jobs have no print-scaling");
}

#[test]
fn disc_dry_run_and_preview_need_no_printer() {
    let s =
        dry(&["disc", "--text", "My Mix", "--inner", "24", "--outer", "115", "--media-type", "b"]);
    assert!(s.contains("com.canon-mtdiscb") && s.contains("media-source=disc"), "{s}");
    assert!(
        s.contains("x-dimension=12000 y-dimension=12000") && s.contains("media-top-margin=10"),
        "{s}"
    );
    assert!(s.contains("2834 x 2834") && s.contains("24 mm to 115 mm"), "{s}");

    let dir = tempfile::tempdir().unwrap();
    let bg = photo(dir.path(), "art.png");
    let png = dir.path().join("preview.png");
    let out = Command::new(env!("CARGO_BIN_EXE_washi"))
        .args([
            "--host",
            "127.0.0.1:9",
            "disc",
            "--image",
            &bg,
            "--preview",
            &png.to_string_lossy(),
        ])
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out).1);
    let img = image::open(&png).unwrap().to_rgb8();
    assert_eq!(img.dimensions(), (1200, 1200));
    assert_eq!(img.get_pixel(600, 600).0, [255, 255, 255], "hub");
    assert_ne!(img.get_pixel(600, 200).0, [255, 255, 255], "ring");
    // Bad numbers and an empty label are refused.
    for args in [
        &["disc", "--text", "x", "--inner", "10"][..],
        &["disc", "--text", "x", "--outer", "130"],
        &["disc"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_washi"))
            .args(["--host", "127.0.0.1:9"])
            .args(args)
            .env("XDG_CONFIG_HOME", "/nonexistent")
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?}");
    }
}

#[test]
fn disc_text_takes_a_font_family_with_bold_and_italic() {
    if washi_core::disc::find_font("Washi").is_none() {
        eprintln!("no font on this system; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let render = |extra: &[&str], name: &str| {
        let png = dir.path().join(name);
        let out = Command::new(env!("CARGO_BIN_EXE_washi"))
            .args(["--host", "127.0.0.1:9", "disc", "--text", "Road trip", "--text-size", "30"])
            .args(["--text-color", "#ff0000"])
            .args(extra)
            .args(["--preview", &png.to_string_lossy()])
            .env("XDG_CONFIG_HOME", "/nonexistent")
            .output()
            .unwrap();
        assert!(out.status.success(), "{extra:?}: {}", text(&out).1);
        image::open(&png)
            .unwrap()
            .to_rgb8()
            .pixels()
            .filter(|p| p.0[0] > 200 && p.0[1] < 90)
            .count()
    };
    let plain = render(&[], "plain.png");
    // A family fontconfig does not know still prints (its best match), and bold adds ink.
    let bold = render(&["--font-family", "No Such Family 4242", "--bold"], "bold.png");
    assert!(plain > 200 && bold > plain, "{plain} / {bold}");
    render(&["--italic"], "italic.png");
    // A font file and a family are two answers to one question.
    let out = Command::new(env!("CARGO_BIN_EXE_washi"))
        .args(["--host", "127.0.0.1:9", "disc", "--text", "x", "--font", "/x.ttf", "--bold"])
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn identify_and_cancel_are_dry_without_confirm() {
    let s = dry(&["identify", "--action", "sound"]);
    assert!(
        s.contains("Identify-Printer") && s.contains("identify-actions (1setOf keyword) = sound")
            || s.contains("identify-actions (keyword) = sound"),
        "{s}"
    );
    let s = dry(&["cancel", "42"]);
    assert!(s.contains("Cancel-Job") && s.contains("job-id (integer) = 42"), "{s}");
}

#[test]
fn identify_confirmed_flashes_once() {
    let fake = FakePrinter::start();
    let o = washi(&fake, &["identify", "--confirm"]);
    assert!(o.status.success(), "{}", text(&o).1);
    assert_eq!(fake.identify_requests(), ["flash"]);
}

#[test]
fn reads_never_write() {
    let fake = FakePrinter::start();
    let o = washi(&fake, &["scanner"]);
    let (out, err) = text(&o);
    assert!(o.status.success(), "{err}");
    assert!(out.contains("Canon XK140 series (eSCL 2.9)") && out.contains("Platen"), "{out}");
    let o = washi(&fake, &["scanner", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["capabilities"]["version"], "2.9");
    assert!(!String::from_utf8_lossy(&o.stdout).contains("000000000000"));

    let o = washi(&fake, &["media"]);
    let (out, err) = text(&o);
    assert!(o.status.success(), "{err}");
    assert!(
        out.contains("Source disc tray:")
            && out.contains("media-source=disc  media-type=com.canon-mtdisca"),
        "{out}"
    );
    assert!(out.contains("120.0 x 120.0 mm") || out.contains("120 x 120 mm"), "{out}");
    let (out, _) = text(&washi(&fake, &["media", "--source", "rear", "--size", "a4"]));
    assert!(out.contains("borderless") && out.contains("com.canon-mtglossy"), "{out}");
    let (out, _) = text(&washi(&fake, &["jobs"]));
    assert!(out.contains("No jobs"), "{out}");
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
    let (out, _) = text(&washi(&fake, &["tools"]));
    assert!(out.contains("pdftoppm"), "{out}");
}

#[test]
fn a_confirmed_scan_writes_a_pdf() {
    let fake = FakePrinter::start();
    fake.set_scan_max_edge(400);
    let dir = tempfile::tempdir().unwrap();
    let out_pdf = dir.path().join("scans").join("page.pdf");
    let o = washi(
        &fake,
        &[
            "scan",
            "--res",
            "150",
            "--mode",
            "gray",
            "--correct",
            "document",
            "--sharpen",
            "30",
            "-o",
            &out_pdf.to_string_lossy(),
            "--confirm",
        ],
    );
    let (out, err) = text(&o);
    assert!(o.status.success(), "{err}\n{out}");
    assert!(out.contains("Saved ") && out.contains("page.pdf"), "{out}");
    let pdf = std::fs::read(&out_pdf).unwrap();
    assert!(pdf.starts_with(b"%PDF-1.4") && String::from_utf8_lossy(&pdf).contains("/DeviceGray"));
    let xml = &fake.scan_requests()[0];
    assert!(
        xml.contains("Grayscale8") && xml.contains("<scan:XResolution>150</scan:XResolution>"),
        "{xml}"
    );
    assert_eq!(fake.writes(), ["eSCL/POST-ScanJobs", "eSCL/NextDocument", "eSCL/NextDocument"]);

    // JPEG with the multi-photo crop on a platen scan that holds two photos.
    let mut platen = image::RgbImage::from_pixel(800, 1000, image::Rgb([250, 250, 250]));
    for (ox, oy) in [(60u32, 60u32), (420, 500)] {
        for y in 0..260u32 {
            for x in 0..300u32 {
                platen.put_pixel(
                    ox + x,
                    oy + y,
                    image::Rgb([(40 + x / 2) as u8, (60 + y / 2) as u8, 160]),
                );
            }
        }
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92)
        .encode(platen.as_raw(), 800, 1000, image::ExtendedColorType::Rgb8)
        .unwrap();
    fake.set_scan_image(jpeg);
    let base = dir.path().join("photos").join("p");
    let o = washi(
        &fake,
        &[
            "scan",
            "--size",
            "auto",
            "--type",
            "photo",
            "--format",
            "jpeg",
            "--crop-photos",
            "-o",
            &base.to_string_lossy(),
            "--confirm",
        ],
    );
    let (out, err) = text(&o);
    assert!(o.status.success(), "{err}\n{out}");
    assert!(out.contains("found 2 photo(s)"), "{out}");
    assert!(
        dir.path().join("photos/p_001.jpg").exists()
            && dir.path().join("photos/p_002.jpg").exists()
    );
}

#[test]
fn a_confirmed_print_and_a_disc_label_reach_the_printer() {
    let fake = FakePrinter::start();
    let dir = tempfile::tempdir().unwrap();
    let jpg = photo(dir.path(), "beach.png");
    let o = washi(
        &fake,
        &[
            "print",
            &jpg,
            "--size",
            "l",
            "--media-type",
            "glossy",
            "--source",
            "rear",
            "--borderless",
            "--confirm",
        ],
    );
    let (out, err) = text(&o);
    assert!(o.status.success(), "{err}\n{out}");
    assert!(
        out.contains("the printer took the job: id 101")
            && out.contains("beach.png: job 101 completed"),
        "{out}"
    );
    assert_eq!(fake.printed_jobs().len(), 1);

    // The printer's own media database vetoes borderless plain paper before anything is sent.
    fake.clear_log();
    let o = washi(&fake, &["print", &jpg, "--media-type", "plain", "--borderless", "--confirm"]);
    let (_, err) = text(&o);
    assert!(!o.status.success() && err.contains("no borderless entry"), "{err}");
    assert!(fake.writes().is_empty());

    let o = washi(
        &fake,
        &["disc", "--text", "Road trip", "--text-size", "24", "--confirm", "--no-wait"],
    );
    let (out, err) = text(&o);
    assert!(o.status.success(), "{err}\n{out}");
    let sent = fake.printed_jobs();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].data.starts_with(b"RaS2"));
}

fn washi_with_input(fake: &FakePrinter, args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_washi"))
        .args(["--host", &fake.chmp_target(), "--ipp-port", &fake.ipp.port().to_string()])
        .args(args)
        .env("XDG_CONFIG_HOME", "/nonexistent/washi-test")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn several_scans_become_one_pdf_and_stdin_ending_stops_early() {
    let fake = FakePrinter::start();
    fake.set_scan_max_edge(200);
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("multi.pdf");
    let o = washi_with_input(
        &fake,
        &["scan", "--pages", "3", "--res", "100", "-o", &out.to_string_lossy(), "--confirm"],
        "\n\n",
    );
    let (stdout, stderr) = text(&o);
    assert!(o.status.success(), "{stderr}\n{stdout}");
    assert!(
        stdout.contains("Put page 2 on the glass") && stdout.contains("Put page 3 on the glass"),
        "{stdout}"
    );
    assert!(String::from_utf8_lossy(&std::fs::read(&out).unwrap()).contains("/Count 3"));
    assert_eq!(fake.scan_requests().len(), 3);

    // Typing q (or closing stdin) after the first page keeps what was scanned.
    let out2 = dir.path().join("short.pdf");
    let o = washi_with_input(
        &fake,
        &["scan", "--pages", "5", "--res", "100", "-o", &out2.to_string_lossy(), "--confirm"],
        "q\n",
    );
    assert!(o.status.success(), "{}", text(&o).1);
    assert!(String::from_utf8_lossy(&std::fs::read(&out2).unwrap()).contains("/Count 1"));
}

#[test]
fn stitching_joins_two_scans_into_one_page() {
    let fake = FakePrinter::start();
    // Two halves with a 40 px common strip.
    let whole = image::RgbImage::from_fn(300, 100, |x, y| {
        let v = ((x.wrapping_mul(2654435761) >> 7) ^ (y.wrapping_mul(40503) >> 3)) as u8;
        image::Rgb([v, v.wrapping_mul(3), 255 - v])
    });
    let encode = |img: &image::RgbImage| {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 98)
            .encode(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)
            .unwrap();
        out
    };
    let left = image::imageops::crop_imm(&whole, 0, 0, 180, 100).to_image();
    fake.set_scan_image(encode(&left));
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("joined.jpg");
    let o = washi_with_input(
        &fake,
        &[
            "scan",
            "--stitch",
            "left-right",
            "--format",
            "jpeg",
            "-o",
            &out.to_string_lossy(),
            "--confirm",
        ],
        "\n",
    );
    let (stdout, stderr) = text(&o);
    assert!(o.status.success(), "{stderr}\n{stdout}");
    // The fake delivers the same image twice; with no overlap found the halves sit side by side.
    assert!(stdout.contains("joined the two scans into"), "{stdout}");
    assert!(image::open(&out).unwrap().width() >= 180 + 100);
}

#[test]
fn auto_scan_treats_a_text_page_as_a_document() {
    let fake = FakePrinter::start();
    // The fake's default test pattern is a page with a dark block and a coloured bar: a photo-ish
    // page. Give it a text-like page instead.
    let mut page = image::RgbImage::from_pixel(600, 800, image::Rgb([245, 245, 245]));
    let mut y = 60u32;
    while y < 740 {
        let mut x = 60u32;
        while x < 520 {
            let len = 10 + (x * 7 + y * 3) % 20;
            for yy in y..y + 5 {
                for xx in x..(x + len).min(540) {
                    page.put_pixel(xx, yy, image::Rgb([25, 25, 25]));
                }
            }
            x += len + 6 + (x + y) % 7;
        }
        y += 14;
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92)
        .encode(page.as_raw(), 600, 800, image::ExtendedColorType::Rgb8)
        .unwrap();
    fake.set_scan_image(jpeg);
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("auto.pdf");
    let o = washi(&fake, &["scan", "--auto", "-o", &out.to_string_lossy(), "--confirm"]);
    let (stdout, stderr) = text(&o);
    assert!(o.status.success(), "{stderr}\n{stdout}");
    assert!(stdout.contains("looks like a document"), "{stdout}");
    let xml = &fake.scan_requests()[0];
    assert!(
        xml.contains("<pwg:Width>2550</pwg:Width>")
            && xml.contains("<scan:XResolution>300</scan:XResolution>"),
        "{xml}"
    );
    assert!(String::from_utf8_lossy(&std::fs::read(&out).unwrap()).contains("/Count 1"));
}

#[test]
fn printing_a_picture_to_the_disc_tray_makes_a_disc_label() {
    let dir = tempfile::tempdir().unwrap();
    let art = photo(dir.path(), "art.png");
    let s = dry(&["print", &art, "--source", "disc", "--media-type", "b"]);
    assert!(s.contains("com.canon-mtdiscb") && s.contains("media-source=disc"), "{s}");
    assert!(s.contains("x-dimension=12000 y-dimension=12000"), "{s}");

    let fake = FakePrinter::start();
    let o = washi(&fake, &["print", &art, "--source", "disc", "--confirm"]);
    let (out, err) = text(&o);
    assert!(o.status.success(), "{err}\n{out}");
    let sent = fake.printed_jobs();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].data.starts_with(b"RaS2"));
    // Documents and several files are not disc material.
    let o = washi(&fake, &["print", &art, &art, "--source", "disc"]);
    assert!(!o.status.success());
}

#[test]
fn firmware_status_and_links_are_reads() {
    let fake = FakePrinter::start();
    let (out, err) = text(&washi(&fake, &["firmware"]));
    assert!(out.contains("Firmware  1.060") && out.contains("up to date"), "{out}{err}");
    fake.offer_firmware_update("1.061", 100);
    let o = washi(&fake, &["firmware", "status", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!((v["version"].as_str(), v["update"].as_str()), (Some("1.060"), Some("available")));

    let (out, err) = text(&washi(&fake, &["links", "--code", "1300"]));
    assert!(o.status.success(), "{err}");
    assert!(out.contains("ink") && out.contains("maker") && out.contains("&ERR=1300"), "{out}");
    assert!(out.contains("maintenance") && out.contains("page=PAGE_UTIL"), "{out}");
    let o = washi(&fake, &["links", "--lang", "ja", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(v["ink"]["url"].as_str().unwrap().contains("&LNG=JP"));
    assert!(v["error"].is_null(), "no code, no error link");
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
}

#[test]
fn a_confirmed_firmware_update_waits_for_the_restart() {
    let fake = FakePrinter::start();
    // No update offered: refused, nothing sent.
    let o = washi(&fake, &["firmware", "update", "--confirm"]);
    assert!(!o.status.success());
    assert!(text(&o).1.contains("up to date"), "{}", text(&o).1);
    assert!(fake.writes().is_empty());

    fake.offer_firmware_update("1.061", 700);
    let o = Command::new(env!("CARGO_BIN_EXE_washi"))
        .args(["--host", &fake.chmp_target(), "firmware", "update", "--confirm"])
        .env("XDG_CONFIG_HOME", "/nonexistent/washi-test")
        .env("WASHI_FIRMWARE_POLL_MS", "40")
        .output()
        .unwrap();
    let (out, err) = text(&o);
    assert!(o.status.success(), "{out}{err}");
    assert!(out.contains("Firmware updated from 1.060 to 1.061"), "{out}");
    assert_eq!(fake.writes(), ["VendorCmd:UpdateFRom/device"]);
}
