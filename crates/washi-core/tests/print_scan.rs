// SPDX-License-Identifier: GPL-2.0-or-later
//! Scanning, printing and disc labels end to end against the in-process fake printer
//! (`washi-fake`). Nothing here touches a real device.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use image::{DynamicImage, Rgb, RgbImage};
use washi_core::disc::{self, DiscLabel};
use washi_core::net::ipp::{IppValue, JobState, Quality, tag};
use washi_core::net::{ClientOptions, EsclOptions, IppOptions};
use washi_core::print::{
    self, PageSelection, PrintOptions, PrintProgress, RunOptions, Source, plan_print, run_print,
};
use washi_core::scan::flow::{OriginalKind, ScanProgress, ScanRequest, plan_scan, run_scan};
use washi_core::scan::save::{SaveFormat, SaveOptions, save};
use washi_core::scan::sizes::SizeChoice;
use washi_core::scan::{OutputMode, ScanSession};
use washi_core::{Confirm, Error, Printer, Tools, pwg};
use washi_fake::FakePrinter;

fn printer_for(fake: &FakePrinter) -> Printer {
    let mut p = Printer::with_options(
        "127.0.0.1",
        ClientOptions {
            port: fake.chmp.port(),
            connect_timeout: Duration::from_secs(2),
            io_timeout: Duration::from_secs(5),
            ..ClientOptions::default()
        },
    );
    p.set_ipp_options(IppOptions {
        port: fake.ipp.port(),
        connect_timeout: Duration::from_secs(2),
        io_timeout: Duration::from_secs(5),
        job_timeout: Duration::from_secs(30),
        ..IppOptions::default()
    });
    p.set_escl_options(EsclOptions {
        port: fake.chmp.port(),
        connect_timeout: Duration::from_secs(2),
        io_timeout: Duration::from_secs(5),
        page_timeout: Duration::from_secs(30),
        busy_timeout: Duration::from_secs(2),
        busy_retry: Duration::from_millis(20),
        ..EsclOptions::default()
    });
    p
}

fn quick() -> RunOptions {
    RunOptions {
        poll_interval: Duration::from_millis(5),
        timeout: Duration::from_secs(20),
        ..RunOptions::default()
    }
}

fn ipp_attr(msg: &washi_core::net::ipp::IppMessage, group: u8, name: &str) -> Vec<IppValue> {
    msg.group(group).and_then(|g| g.get(name)).map(|a| a.values.clone()).unwrap_or_default()
}

fn jpeg_of(img: &RgbImage) -> Vec<u8> {
    washi_core::scan::save::encode_jpeg(&DynamicImage::ImageRgb8(img.clone()), 92, 300).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------------------------

#[test]
fn a_dry_run_plan_opens_no_connection() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let plan = plan_scan(&p, &ScanRequest::default()).unwrap();
    assert!(plan.request.contains("POST http://127.0.0.1:"));
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("p.jpg");
    std::fs::write(&jpg, jpeg_of(&RgbImage::from_pixel(40, 30, Rgb([9, 9, 9])))).unwrap();
    let plan = plan_print(&p, &[jpg], &PrintOptions::default(), &Tools::default()).unwrap();
    assert!(plan.describe(&p.ipp()).contains("Print-Job"));
    let label = disc::plan_disc(&p, &DiscLabel::default(), Quality::Normal).unwrap();
    assert!(label.request.contains("disc"));
    assert_eq!(
        (fake.chmp_connections(), fake.ipp_connections()),
        (0, 0),
        "planning is purely local"
    );
    assert!(fake.log().is_empty());
}

#[test]
fn a_color_a4_scan_sends_the_documented_request_and_decodes_the_page() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let mut seen = Vec::new();
    let pages =
        run_scan(&p, &ScanRequest::default(), Confirm::explicit(), &mut |e| seen.push(e)).unwrap();
    assert_eq!(pages.len(), 1);
    let (w, h) = pages[0].page.original().to_rgb8().dimensions();
    assert!(w <= 1200 && h <= 1200 && h > w, "{w}x{h}: portrait test pattern (capped by the fake)");
    assert_eq!(pages[0].page.dpi, 300);
    assert_eq!(&pages[0].jpeg[..2], &[0xFF, 0xD8]);
    assert_eq!(seen.first(), Some(&ScanProgress::Checked));
    assert!(seen.contains(&ScanProgress::Started));
    assert!(matches!(
        seen.iter().find(|e| matches!(e, ScanProgress::PageReceived { .. })),
        Some(ScanProgress::PageReceived { index: 0, .. })
    ));
    assert_eq!(seen.last(), Some(&ScanProgress::Done { pages: 1 }));

    let xml = &fake.scan_requests()[0];
    for want in [
        "<pwg:Width>2480</pwg:Width>",
        "<pwg:Height>3508</pwg:Height>",
        "<pwg:XOffset>0</pwg:XOffset>",
        "<scan:XResolution>300</scan:XResolution>",
        "<scan:ColorMode>RGB24</scan:ColorMode>",
        "<scan:Intent>Document</scan:Intent>",
        "<pwg:InputSource>Platen</pwg:InputSource>",
        "escl:ThreeHundredthsOfInches",
        "<scan:DocumentFormatExt>image/jpeg</scan:DocumentFormatExt>",
    ] {
        assert!(xml.contains(want), "missing {want}\n{xml}");
    }
    // Capabilities and status (reads), then the three writes: POST, page, 404.
    assert_eq!(
        fake.log(),
        [
            "eSCL/ScannerCapabilities",
            "eSCL/ScannerStatus",
            "eSCL/POST-ScanJobs",
            "eSCL/NextDocument",
            "eSCL/NextDocument"
        ]
    );
}

#[test]
fn grayscale_black_and_white_and_photo_requests() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let req = ScanRequest {
        size: SizeChoice::parse("l").unwrap(),
        resolution: 600,
        mode: OutputMode::BlackWhite,
        kind: OriginalKind::Photo,
        use_intent: true,
        auto_crop: true,
    };
    let pages = run_scan(&p, &req, Confirm::explicit(), &mut |_| {}).unwrap();
    let xml = &fake.scan_requests()[0];
    assert!(
        xml.contains("<scan:ColorMode>Grayscale8</scan:ColorMode>"),
        "B&W is a grey scan on the XK140"
    );
    assert!(
        xml.contains("<scan:Intent>Photo</scan:Intent>")
            && xml.contains("<scan:XResolution>600</scan:XResolution>")
    );
    assert!(
        xml.contains("<pwg:Width>1051</pwg:Width>")
            && xml.contains("<pwg:Height>1500</pwg:Height>"),
        "{xml}"
    );
    assert!(
        matches!(pages[0].page.original(), DynamicImage::ImageLuma8(_)),
        "the scanner sent a grey JPEG"
    );
    // Rendering applies the host-side threshold.
    let out = pages[0].page.render().to_luma8();
    assert!(out.pixels().all(|px| px.0[0] == 0 || px.0[0] == 255));
}

#[test]
fn auto_size_scans_the_platen_and_crops_the_object() {
    // A 1000x1400 "platen" with a tilted-free photo on it.
    let mut platen = RgbImage::from_pixel(1000, 1400, Rgb([250, 250, 250]));
    for (x, y, p) in
        RgbImage::from_fn(400, 300, |x, y| Rgb([(30 + x / 3) as u8, (60 + y / 3) as u8, 150]))
            .enumerate_pixels()
    {
        platen.put_pixel(200 + x, 300 + y, *p);
    }
    let fake = FakePrinter::start();
    fake.set_scan_image(jpeg_of(&platen));
    let p = printer_for(&fake);
    let req = ScanRequest { size: SizeChoice::Auto, ..ScanRequest::default() };
    let pages = run_scan(&p, &req, Confirm::explicit(), &mut |_| {}).unwrap();
    let xml = &fake.scan_requests()[0];
    assert!(
        xml.contains("<pwg:Width>2550</pwg:Width>")
            && xml.contains("<pwg:Height>3508</pwg:Height>"),
        "{xml}"
    );
    let (w, h) = pages[0].page.original_size();
    assert!(
        (w as i32 - 392).abs() < 24 && (h as i32 - 294).abs() < 24,
        "cropped to the photo: {w}x{h}"
    );
    assert_eq!(&pages[0].jpeg[..2], &[0xFF, 0xD8], "the cropped page is re-encoded");
}

#[test]
fn a_session_of_scans_is_saved_as_one_pdf() {
    let fake = FakePrinter::start();
    fake.set_scan_max_edge(300);
    let p = printer_for(&fake);
    let mut session = ScanSession::new();
    for _ in 0..3 {
        for sp in run_scan(&p, &ScanRequest::default(), Confirm::explicit(), &mut |_| {}).unwrap() {
            session.add_page(sp.page);
        }
    }
    assert_eq!(session.len(), 3);
    session.rotate_page_right(1).unwrap();
    session.move_page(2, 0).unwrap();
    session.delete_page(1).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let files = save(&session, &dir.path().join("out"), &SaveOptions::default()).unwrap();
    assert_eq!(files, [dir.path().join("out.pdf")]);
    let pdf = std::fs::read(&files[0]).unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/Count 2"));
    let jpgs = save(
        &session,
        &dir.path().join("pic"),
        &SaveOptions { format: SaveFormat::Jpeg, ..SaveOptions::default() },
    )
    .unwrap();
    assert_eq!(jpgs.len(), 2);
    // poppler (when installed) reads the PDF we wrote.
    let tools = Tools::detect();
    if let Some(info) = tools.pdfinfo.as_ref() {
        let o = std::process::Command::new(info).arg(&files[0]).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(
            o.status.success() && text.contains("Pages:           2"),
            "{text}{}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

#[test]
fn scanner_refusals_are_explained_and_a_busy_scanner_is_retried() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    fake.set_scan_reject(Some(409));
    let e = run_scan(&p, &ScanRequest::default(), Confirm::explicit(), &mut |_| {}).unwrap_err();
    assert!(e.to_string().contains("409"), "{e}");
    assert!(e.hint().is_some_and(|h| h.contains("cover")), "{:?}", e.hint());
    fake.set_scan_reject(None);
    fake.set_scanner_busy_for(2);
    assert_eq!(
        run_scan(&p, &ScanRequest::default(), Confirm::explicit(), &mut |_| {}).unwrap().len(),
        1
    );
    // A scanner that is not idle (cover open, busy) is not asked to start a job.
    fake.set_scanner_state("Stopped");
    fake.clear_log();
    let e = run_scan(&p, &ScanRequest::default(), Confirm::explicit(), &mut |_| {}).unwrap_err();
    assert!(e.to_string().contains("stopped") && e.to_string().contains("cover"), "{e}");
    assert!(fake.writes().is_empty());
    fake.set_scanner_state("Idle");
    // A request the scanner cannot do never reaches the POST.
    fake.clear_log();
    let bad = ScanRequest { resolution: 250, ..ScanRequest::default() };
    assert!(matches!(run_scan(&p, &bad, Confirm::explicit(), &mut |_| {}), Err(Error::Invalid(_))));
    assert!(fake.writes().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------------------------

fn photo_file(dir: &std::path::Path, name: &str, w: u32, h: u32) -> PathBuf {
    let p = dir.join(name);
    RgbImage::from_fn(w, h, |x, y| Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 90]))
        .save(&p)
        .unwrap();
    p
}

#[test]
fn a_photo_is_validated_printed_and_followed_to_completion() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let file = photo_file(dir.path(), "beach.png", 900, 600);
    let opts = PrintOptions {
        paper: *print::PaperSize::parse("l").unwrap(),
        media_type: Some("com.canon-mtglossy".into()),
        source: Source::Rear,
        borderless: true,
        quality: Quality::High,
        copies: 2,
        ..PrintOptions::default()
    };
    let plan = plan_print(&p, &[file], &opts, &Tools::default()).unwrap();
    let mut events = Vec::new();
    let report = run_print(&p, &plan, &quick(), &Tools::default(), Confirm::explicit(), &mut |e| {
        events.push(e)
    })
    .unwrap();
    assert_eq!(report.jobs.len(), 1);
    assert_eq!(report.jobs[0].state, JobState::Completed);
    assert_eq!(report.jobs[0].label, "beach.png");

    let sent = fake.printed_jobs();
    assert_eq!(sent.len(), 1);
    let j = &sent[0];
    assert_eq!(j.format, "image/jpeg");
    let img = image::load_from_memory(&j.data).expect("a JPEG was sent");
    // 900x600 turned to portrait L paper (89x127 mm): aspect of the paper, nothing blown up.
    let ratio = f64::from(img.width()) / f64::from(img.height());
    assert!((ratio - 8890.0 / 12700.0).abs() < 0.01, "{}x{}", img.width(), img.height());
    let job = &j.request.group(tag::JOB_ATTRIBUTES).unwrap();
    let get = |n: &str| job.get(n).map(|a| a.values.clone()).unwrap_or_default();
    assert_eq!(get("copies"), [IppValue::Integer(2)]);
    assert_eq!(get("print-quality"), [IppValue::Enum(5)]);
    assert_eq!(get("print-scaling"), [IppValue::Keyword("fill".into())]);
    let IppValue::Collection(mc) = &get("media-col")[0] else { panic!("no media-col") };
    let member = |n: &str| mc.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(member("media-source"), [IppValue::Keyword("rear".into())]);
    assert_eq!(member("media-type"), [IppValue::Keyword("com.canon-mtglossy".into())]);
    assert_eq!(member("media-top-margin"), [IppValue::Integer(0)], "borderless");

    // The sequence on the wire: reads first, Validate-Job before Print-Job, then polling.
    let log = fake.log();
    let pos = |what: &str| {
        log.iter().position(|l| l == what).unwrap_or_else(|| panic!("{what} missing in {log:?}"))
    };
    assert!(pos("IPP/Get-Printer-Attributes") < pos("IPP/Validate-Job"));
    assert!(
        pos("IPP/Validate-Job") < pos("IPP/Print-Job")
            && pos("IPP/Print-Job") < pos("IPP/Get-Job-Attributes")
    );
    assert_eq!(log.iter().filter(|l| *l == "IPP/Print-Job").count(), 1);
    assert!(matches!(events.first(), Some(PrintProgress::Preflight)));
    assert!(events.iter().any(|e| matches!(e, PrintProgress::Submitted { id: 101, .. })));
    assert!(matches!(
        events.last(),
        Some(PrintProgress::Finished { state: JobState::Completed, .. })
    ));
}

/// Print `file` on L paper with `orientation` and return the JPEG the printer received.
fn printed_photo(fake: &FakePrinter, file: PathBuf, orientation: print::Orientation) -> RgbImage {
    let p = printer_for(fake);
    let opts = PrintOptions {
        paper: *print::PaperSize::parse("l").unwrap(),
        borderless: true,
        orientation,
        ..PrintOptions::default()
    };
    let plan = plan_print(&p, &[file], &opts, &Tools::default()).unwrap();
    run_print(&p, &plan, &quick(), &Tools::default(), Confirm::explicit(), &mut |_| {}).unwrap();
    let sent = fake.printed_jobs();
    image::load_from_memory(&sent.last().unwrap().data).unwrap().to_rgb8()
}

#[test]
fn the_orientation_choice_decides_whether_the_photo_is_turned() {
    let spec = |j: &str| print::PrintSpec::from_json(j).and_then(|s| s.to_options());
    assert_eq!(
        spec(r#"{"orientation":"landscape"}"#).unwrap().orientation,
        print::Orientation::Landscape
    );
    assert_eq!(spec("{}").unwrap().orientation, print::Orientation::Auto);
    assert!(spec(r#"{"orientation":"sideways"}"#).is_err());

    let fake = FakePrinter::start();
    let dir = tempfile::tempdir().unwrap();
    // Red grows to the right, green downwards; 900 x 600 is a landscape photo.
    let file = photo_file(dir.path(), "wide.png", 900, 600);
    let red = |img: &RgbImage, x: u32, y: u32| img.get_pixel(x, y).0[0];

    // Landscape: the photo is upright on a sideways sheet, which is turned a quarter clockwise
    // onto the portrait paper, so what grew to the right now grows downwards.
    let land = printed_photo(&fake, file.clone(), print::Orientation::Landscape);
    assert!(land.height() > land.width());
    let (cx, h) = (land.width() / 2, land.height());
    assert!(red(&land, cx, h * 4 / 5) > red(&land, cx, h / 5) + 60);

    // Auto turns the photo itself (the same quarter turn), so both agree.
    let auto = printed_photo(&fake, file.clone(), print::Orientation::Auto);
    assert!(
        red(&auto, cx.min(auto.width() - 1), auto.height() * 4 / 5)
            > red(&auto, cx.min(auto.width() - 1), auto.height() / 5) + 60
    );

    // Portrait: the photo stays upright and is cropped to fill, so red still grows to the right.
    let port = printed_photo(&fake, file, print::Orientation::Portrait);
    let (w, cy) = (port.width(), port.height() / 2);
    assert!(red(&port, w * 4 / 5, cy) > red(&port, w / 5, cy) + 20);
}

#[test]
fn photo_layout_puts_several_photos_on_one_sheet() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> =
        (0..5).map(|i| photo_file(dir.path(), &format!("p{i}.png"), 200, 150)).collect();
    let opts = PrintOptions {
        layout: 4,
        paper: *print::PaperSize::parse("card").unwrap(),
        ..PrintOptions::default()
    };
    let plan = plan_print(&p, &files, &opts, &Tools::default()).unwrap();
    assert_eq!(plan.jobs.len(), 2, "4 + 1 photos");
    run_print(&p, &plan, &quick(), &Tools::default(), Confirm::explicit(), &mut |_| {}).unwrap();
    let jobs = fake.printed_jobs();
    assert_eq!(jobs.len(), 2);
    for j in &jobs {
        assert!(image::load_from_memory(&j.data).is_ok());
    }
}

/// A tiny PDF made with our own writer: one page, a solid black block on white.
fn block_pdf(dir: &std::path::Path, name: &str, pages: usize) -> PathBuf {
    let mut pdf = washi_core::pdf::PdfBuilder::new();
    for i in 0..pages {
        let mut img = RgbImage::from_pixel(200, 280, Rgb([255, 255, 255]));
        for y in 100..180 {
            for x in 40..(40 + 20 * (i as u32 + 1)).min(180) {
                img.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        pdf.add_jpeg_page(jpeg_of(&img), 200, 280, false, 24.0); // 600 x 840 pt
    }
    let p = dir.join(name);
    std::fs::write(&p, pdf.finish()).unwrap();
    p
}

#[test]
fn a_pdf_is_rasterised_to_pwg_and_printed() {
    let tools = Tools::detect();
    if !tools.can_print_pdf() {
        eprintln!("poppler is not installed; skipping the PDF printing test");
        return;
    }
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let file = block_pdf(dir.path(), "doc.pdf", 3);
    let opts = PrintOptions {
        paper: *print::PaperSize::parse("l").unwrap(), // keeps the 600 dpi raster small
        pages: Some(PageSelection::parse("1,3").unwrap()),
        ..PrintOptions::default()
    };
    let plan = plan_print(&p, &[file], &opts, &tools).unwrap();
    assert_eq!(plan.jobs[0].sheets, Some(2));
    assert_eq!(plan.jobs[0].input_pages, Some(2));
    let report = run_print(&p, &plan, &quick(), &tools, Confirm::explicit(), &mut |_| {}).unwrap();
    assert_eq!(report.jobs[0].state, JobState::Completed);

    let sent = &fake.printed_jobs()[0];
    assert_eq!(sent.format, "image/pwg-raster");
    let pages = pwg::decode(&sent.data).expect("valid PWG raster");
    assert_eq!(pages.len(), 2, "pages 1 and 3");
    let h = &pages[0].header;
    assert_eq!((h.width, h.height, h.dpi), (2100, 3000, 600));
    assert_eq!(h.page_size_name, "oe_photo-l_3.5x5in");
    assert_eq!(h.media_type, "stationery");
    assert_eq!(h.total_pages, 2);
    // The 600x840 pt page is fitted onto the 2100x3000 px sheet (2100x2940, centred). Page 1
    // has a block over x 420..630, page 3 a wider one over x 420..1050; both cover y 1080..1920.
    let px = |pg: &pwg::Page, x: usize, y: usize| pg.pixels[(y * 2100 + x) * 3];
    assert!(px(&pages[0], 500, 1500) < 60, "page 1: the block");
    assert_eq!(px(&pages[0], 800, 1500), 255, "page 1: white right of its block");
    assert!(px(&pages[1], 800, 1500) < 60, "page 3: the wider block");
    assert!(pages.iter().all(|pg| px(pg, 3, 3) == 255 && px(pg, 1500, 1500) == 255));
    let ipp = &sent.request;
    assert_eq!(
        ipp_attr(ipp, tag::OPERATION_ATTRIBUTES, "document-format"),
        [IppValue::MimeMediaType("image/pwg-raster".into())]
    );
    assert!(ipp_attr(ipp, tag::JOB_ATTRIBUTES, "print-scaling").is_empty());
}

#[test]
fn a_landscape_sheet_is_composed_sideways_and_turned_onto_the_paper() {
    let tools = Tools::detect();
    if !tools.can_print_pdf() {
        eprintln!("poppler is not installed; skipping the landscape document test");
        return;
    }
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let file = block_pdf(dir.path(), "doc.pdf", 1);
    let opts = PrintOptions {
        paper: *print::PaperSize::parse("l").unwrap(),
        orientation: print::Orientation::Landscape,
        ..PrintOptions::default()
    };
    let plan = plan_print(&p, &[file], &opts, &tools).unwrap();
    run_print(&p, &plan, &quick(), &tools, Confirm::explicit(), &mut |_| {}).unwrap();
    let sent = &fake.printed_jobs()[0];
    let pages = pwg::decode(&sent.data).expect("valid PWG raster");
    let h = &pages[0].header;
    assert_eq!((h.width, h.height), (2100, 3000), "the paper is still portrait");
    // The upright 600x840 pt page is fitted onto the sideways 3000x2100 px sheet (1500x2100,
    // centred: x 750..2250). Its block (x 40..60 of 200, y 100..180 of 280) covers sheet
    // x 1050..1200, y 750..1350. A clockwise quarter turn sends (x, y) to (2099 - y, x), so on
    // the paper the block is the strip x 750..1350, y 1050..1200.
    let px = |x: usize, y: usize| pages[0].pixels[(y * 2100 + x) * 3];
    assert!(px(1000, 1100) < 60, "the block, turned");
    assert_eq!(px(1000, 1300), 255, "white below it");
    assert_eq!(px(300, 2700), 255);
}

#[test]
fn documents_through_stand_in_programs_cover_mono_duplex_and_office() {
    // Stand-ins make this independent of poppler / LibreOffice being installed.
    let tools_dir = tempfile::tempdir().unwrap();
    let script = |name: &str, body: &str| {
        let p = tools_dir.path().join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    let pdfinfo = script(
        "pdfinfo",
        "#!/bin/sh\necho 'Pages: 2'\nif [ \"$1\" = \"-f\" ]; then echo 'Page    1 size:  100 x 140 pts'; echo 'Page    2 size:  100 x 140 pts'; fi\n",
    );
    let pdftoppm = script(
        "pdftoppm",
        "#!/bin/sh\ngray=0\nwhile [ $# -gt 0 ]; do case \"$1\" in -gray) gray=1;; -r|-f|-l) shift;; -singlefile) ;; *) last=$1;; esac; shift; done\n\
         if [ $gray = 1 ]; then printf 'P5\\n10 14\\n255\\n' > \"$last.pgm\"; head -c 140 /dev/zero >> \"$last.pgm\"; else printf 'P6\\n10 14\\n255\\n' > \"$last.ppm\"; head -c 420 /dev/zero >> \"$last.ppm\"; fi\n",
    );
    let soffice = script(
        "soffice",
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do if [ \"$1\" = \"--outdir\" ]; then out=$2; fi; last=$1; shift; done\nn=$(basename \"$last\"); printf '%%PDF-1.4 stand-in' > \"$out/${n%.*}.pdf\"\n",
    );
    let tools = Tools {
        pdfinfo: Some(pdfinfo),
        pdftoppm: Some(pdftoppm),
        soffice: Some(soffice),
        tesseract: None,
    };
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("a.pdf");
    std::fs::write(&pdf, b"%PDF-1.4 x").unwrap();
    let docx = dir.path().join("memo.docx");
    std::fs::write(&docx, b"PK\x03\x04 stand-in").unwrap();

    let opts = PrintOptions {
        paper: *print::PaperSize::parse("card").unwrap(),
        color: false,
        duplex: Some(print::Duplex::Short),
        copies: 3,
        ..PrintOptions::default()
    };
    let plan = plan_print(&p, &[pdf, docx], &opts, &tools).unwrap();
    assert_eq!(plan.jobs.len(), 2);
    assert_eq!(plan.jobs[0].sheets, Some(1), "two pages, two-sided: one sheet");
    run_print(&p, &plan, &quick(), &tools, Confirm::explicit(), &mut |_| {}).unwrap();
    let sent = fake.printed_jobs();
    assert_eq!(sent.len(), 2);
    for j in &sent {
        let pages = pwg::decode(&j.data).unwrap();
        assert_eq!(pages.len(), 2, "front and back");
        assert!(pages.iter().all(|pg| pg.header.colorspace == pwg::ColorSpace::SGray
            && pg.header.duplex
            && pg.header.tumble));
        let ipp = &j.request;
        assert_eq!(
            ipp_attr(ipp, tag::JOB_ATTRIBUTES, "sides"),
            [IppValue::Keyword("two-sided-short-edge".into())]
        );
        assert_eq!(
            ipp_attr(ipp, tag::JOB_ATTRIBUTES, "print-color-mode"),
            [IppValue::Keyword("monochrome".into())]
        );
        assert_eq!(ipp_attr(ipp, tag::JOB_ATTRIBUTES, "copies"), [IppValue::Integer(3)]);
    }
    assert_eq!(
        sent[1].request.group(tag::OPERATION_ATTRIBUTES).unwrap().get("job-name").unwrap().values,
        [IppValue::Name("memo.docx".into())]
    );
}

#[test]
fn reverse_page_order_sends_the_last_sheet_first_and_keeps_duplex_sides_together() {
    // A stand-in for poppler whose pages are told apart by colour: page n is 40 * n.
    let tools_dir = tempfile::tempdir().unwrap();
    let script = |name: &str, body: &str| {
        let p = tools_dir.path().join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    let pdfinfo = script(
        "pdfinfo",
        "#!/bin/sh\necho 'Pages: 3'\nif [ \"$1\" = \"-f\" ]; then for i in 1 2 3; do echo \"Page    $i size:  100 x 140 pts\"; done; fi\n",
    );
    let pdftoppm = script(
        "pdftoppm",
        "#!/bin/sh\npage=1\nwhile [ $# -gt 0 ]; do case \"$1\" in -f) page=$2; shift;; -gray|-singlefile) ;; -r|-l) shift;; *) last=$1;; esac; shift; done\n\
         v=$((40 * page))\nprintf 'P6\\n10 14\\n255\\n' > \"$last.ppm\"; head -c 420 /dev/zero | tr '\\0' \"\\\\$(printf '%03o' $v)\" >> \"$last.ppm\"\n",
    );
    let tools = Tools { pdfinfo: Some(pdfinfo), pdftoppm: Some(pdftoppm), ..Tools::default() };
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("three.pdf");
    std::fs::write(&pdf, b"%PDF-1.4 x").unwrap();

    let centres = |job: &[u8]| -> Vec<u8> {
        pwg::decode(job)
            .unwrap()
            .iter()
            .map(|pg| {
                let (w, h) = (pg.header.width as usize, pg.header.height as usize);
                pg.pixels[(h / 2 * w + w / 2) * 3]
            })
            .collect()
    };
    let run = |opts: PrintOptions| {
        let plan = plan_print(&p, std::slice::from_ref(&pdf), &opts, &tools).unwrap();
        run_print(&p, &plan, &quick(), &tools, Confirm::explicit(), &mut |_| {}).unwrap();
        (plan, centres(&fake.printed_jobs().last().unwrap().data))
    };
    let base = PrintOptions {
        paper: *print::PaperSize::parse("card").unwrap(),
        ..PrintOptions::default()
    };

    let (_, normal) = run(base.clone());
    assert_eq!(normal, [40, 80, 120]);
    let (plan, reversed) =
        run(PrintOptions { page_order: print::PageOrder::Reverse, ..base.clone() });
    assert_eq!(reversed, [120, 80, 40]);
    assert!(
        plan.jobs[0].notes.iter().any(|n| n.contains("last sheet first")),
        "{:?}",
        plan.jobs[0].notes
    );

    // Two-sided: sheet 2 (page 3, blank back) first; pages 1 and 2 stay front and back of one sheet.
    let (plan, duplex) = run(PrintOptions {
        page_order: print::PageOrder::Reverse,
        duplex: Some(print::Duplex::Long),
        ..base.clone()
    });
    assert_eq!(duplex, [120, 255, 40, 80]);
    assert_eq!(plan.jobs[0].sheets, Some(2));

    // The JSON the daemon takes, and what is refused.
    let o =
        print::PrintSpec::from_json(r#"{"page_order":"reverse"}"#).unwrap().to_options().unwrap();
    assert!(o.page_order.is_reverse());
    assert!(
        print::PrintSpec::from_json(r#"{"page_order":"sideways"}"#).unwrap().to_options().is_err()
    );
    let photo = photo_file(dir.path(), "x.png", 100, 100);
    let e = plan_print(
        &p,
        &[photo],
        &PrintOptions { page_order: print::PageOrder::Reverse, ..PrintOptions::default() },
        &tools,
    )
    .unwrap_err();
    assert!(e.to_string().contains("page order is for documents"), "{e}");
}

#[test]
fn a_combination_the_printer_does_not_list_is_refused_before_printing() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let file = photo_file(dir.path(), "x.png", 100, 100);
    // Borderless plain paper does not exist in the database.
    let opts = PrintOptions {
        borderless: true,
        media_type: Some("stationery".into()),
        ..PrintOptions::default()
    };
    let plan = plan_print(&p, std::slice::from_ref(&file), &opts, &Tools::default()).unwrap();
    let e = run_print(&p, &plan, &quick(), &Tools::default(), Confirm::explicit(), &mut |_| {})
        .unwrap_err();
    assert!(e.to_string().contains("no borderless entry"), "{e}");
    assert!(fake.printed_jobs().is_empty() && fake.received_jobs().is_empty(), "nothing was sent");

    // `--no-check` style: the printer's own verdict comes through with a hint.
    fake.set_print_reject(Some(0x040B));
    let ok_opts = PrintOptions::default();
    let plan = plan_print(&p, std::slice::from_ref(&file), &ok_opts, &Tools::default()).unwrap();
    let e = run_print(&p, &plan, &quick(), &Tools::default(), Confirm::explicit(), &mut |_| {})
        .unwrap_err();
    assert!(e.to_string().contains("client-error-attributes-or-values-not-supported"), "{e}");
    assert!(e.hint().is_some_and(|h| h.contains("washi media")));
    assert!(fake.printed_jobs().is_empty(), "Validate-Job stopped it");
    fake.set_print_reject(None);

    // A printer that is not accepting jobs is refused up front.
    fake.set_accepting_jobs(false);
    let e = run_print(&p, &plan, &quick(), &Tools::default(), Confirm::explicit(), &mut |_| {})
        .unwrap_err();
    assert!(e.to_string().contains("not accepting jobs"), "{e}");
}

#[test]
fn aborted_and_cancelled_jobs_are_reported() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let file = photo_file(dir.path(), "x.png", 100, 100);
    let plan = plan_print(&p, &[file], &PrintOptions::default(), &Tools::default()).unwrap();

    fake.set_abort_jobs(true);
    let e = run_print(&p, &plan, &quick(), &Tools::default(), Confirm::explicit(), &mut |_| {})
        .unwrap_err();
    assert!(e.to_string().contains("aborted"), "{e}");
    fake.set_abort_jobs(false);

    // Cancelling while the printer is still working sends Cancel-Job.
    fake.set_job_completion_polls(1000);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let opts = RunOptions { cancel: Some(cancel), ..quick() };
    let e = run_print(&p, &plan, &opts, &Tools::default(), Confirm::explicit(), &mut |ev| {
        if matches!(ev, PrintProgress::State { .. }) {
            flag.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(fake.log().iter().any(|l| l == "IPP/Cancel-Job"));
}

#[test]
fn a_job_the_printer_forgets_after_it_ran_counts_as_done() {
    let fake = FakePrinter::start();
    fake.set_forget_finished_jobs(true);
    let p = printer_for(&fake);
    let dir = tempfile::tempdir().unwrap();
    let file = photo_file(dir.path(), "x.png", 100, 100);
    let plan = plan_print(&p, &[file], &PrintOptions::default(), &Tools::default()).unwrap();
    let report =
        run_print(&p, &plan, &quick(), &Tools::default(), Confirm::explicit(), &mut |_| {})
            .unwrap();
    assert_eq!(report.jobs[0].state, JobState::Completed);
}

#[test]
fn identify_printer_is_a_confirmed_job_operation() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    p.ipp()
        .identify(washi_core::net::ipp::IdentifyAction::Sound, "washi", Confirm::explicit())
        .unwrap();
    assert_eq!(fake.identify_requests(), ["sound"]);
    assert_eq!(fake.writes(), ["IPP/Identify-Printer"]);
}

// ---------------------------------------------------------------------------------------------
// Disc label
// ---------------------------------------------------------------------------------------------

#[test]
fn the_media_database_has_the_disc_tray_and_it_is_read_without_writes() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let db = p.ipp().media_database().unwrap();
    let disc = db.disc();
    assert_eq!(disc.len(), 2);
    assert!(disc.iter().all(|e| (e.width, e.height) == (12000, 12000)));
    assert!(disc.iter().any(|e| e.media_type.as_deref() == Some("com.canon-mtdisca")));
    assert!(db.entries.iter().any(|e| e.is_borderless()));
    assert!(fake.writes().is_empty(), "reading the database is read-only");
    // A broad request gets it empty, like the real printer.
    let broad = p.ipp().get_printer_attributes_raw(&[]).unwrap();
    assert!(washi_core::net::ipp::MediaDatabase::from_message(&broad).unwrap().is_empty());
}

#[test]
fn a_disc_label_is_rendered_and_printed_from_the_disc_tray() {
    let fake = FakePrinter::start();
    let p = printer_for(&fake);
    let label = DiscLabel {
        background_color: [30, 120, 200],
        media: disc::DiscMedia::B,
        ..DiscLabel::default()
    };
    let report =
        disc::run_disc(&p, &label, Quality::High, &quick(), Confirm::explicit(), &mut |_| {})
            .unwrap();
    assert_eq!(report.state, JobState::Completed);
    let sent = &fake.printed_jobs()[0];
    assert_eq!(sent.format, "image/pwg-raster");
    let job = sent.request.group(tag::JOB_ATTRIBUTES).unwrap();
    let IppValue::Collection(mc) = &job.get("media-col").unwrap().values[0] else {
        panic!("no media-col")
    };
    let member = |n: &str| mc.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(member("media-source"), [IppValue::Keyword("disc".into())]);
    assert_eq!(member("media-type"), [IppValue::Keyword("com.canon-mtdiscb".into())]);
    assert_eq!(member("media-left-margin"), [IppValue::Integer(10)]);
    let IppValue::Collection(size) = &member("media-size")[0] else { panic!("no size") };
    assert_eq!(size[0].1, [IppValue::Integer(12000)]);
    assert_eq!(size[1].1, [IppValue::Integer(12000)]);
    assert_eq!(job.get("print-quality").unwrap().values, [IppValue::Enum(5)]);

    let pages = pwg::decode(&sent.data).unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!((pages[0].header.width, pages[0].header.height), (2834, 2834));
    assert_eq!(pages[0].header.page_size_name, "om_square_120x120mm");
    let centre = &pages[0].pixels[(1417 * 2834 + 1417) * 3..][..3];
    assert_eq!(centre, [255, 255, 255], "the hub is not inked");
}
