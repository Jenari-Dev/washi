// SPDX-License-Identifier: GPL-2.0-or-later
//! `washi scan`, `print`, `disc`, `identify`, `jobs`, `cancel`, `media`, `scanner` and `tools`.
//!
//! Everything that makes the printer move (a scan, a print job, a disc label, Identify-Printer,
//! Cancel-Job) is a dry run unless `--confirm` is given: the exact request is printed and no
//! network connection is opened. Reads (`media`, `scanner`, `jobs`, `tools`) always run.

use std::fmt::Write as _;
use std::io::{BufRead, Write as _};

use anyhow::{Context, Result, bail};
use image::{DynamicImage, RgbImage};
use serde::Serialize;
use washi_core::disc::{self, Align, Background, BackgroundFit, DiscLabel, DiscMedia, TextItem};
use washi_core::net::escl::{ScannerCapabilities, ScannerStatus, SourceCaps};
use washi_core::net::ipp::{
    IdentifyAction, JobAttributes, MediaDatabase, MediaEntry, PrintCapabilities, WhichJobs,
};
use washi_core::print::{
    self, Duplex, PageSelection, PaperSize, PrintOptions, PrintProgress, RunOptions, Source,
};
use washi_core::scan::correct::{ColorCorrection, ImageCorrection, OutputMode};
use washi_core::scan::detect::{self, ScanKind};
use washi_core::scan::flow::{OriginalKind, ScanRequest, plan_scan, run_scan};
use washi_core::scan::save::{SaveFormat, SaveOptions, save};
use washi_core::scan::sizes::SizeChoice;
use washi_core::scan::stitch::{StitchDirection, StitchOptions, stitch};
use washi_core::scan::{AspectPreset, Page, Rotation, ScanSession, ocr};
use washi_core::{Config, Confirm, Printer, Tools};

use crate::cli::{
    Cli, CorrectArg, DiscArgs, DiscMediaArg, DuplexArg, FitArg, FormatArg, IdentifyArg, KindArg,
    ModeArg, OrientationArg, PrintArgs, QualityArg, ScanArgs, SourceArg, StitchArg,
};
use crate::commands::{connect, print_json};
use crate::render::fmt_duration;

const DRY_RUN_BANNER: &str = "DRY RUN: nothing was sent to the printer (no network access).";

/// A printer handle for dry runs: the host as given (or a placeholder), never resolved.
fn offline_printer(cli: &Cli) -> Printer {
    let host = cli
        .host
        .clone()
        .or_else(|| Config::load().ok().and_then(|c| c.printer.host))
        .unwrap_or_else(|| "printer.local".to_string());
    let mut p = Printer::from_target(&host);
    if let Some(port) = cli.ipp_port {
        p.set_ipp_port(port);
    }
    p
}

fn indent(text: &str, by: &str) -> String {
    text.lines().map(|l| format!("{by}{l}\n")).collect()
}

fn fmt_bytes(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} B"),
        1024..=1_048_575 => format!("{:.0} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

// ---------------------------------------------------------------------------------------------
// scan
// ---------------------------------------------------------------------------------------------

fn parse_rotation(s: &str) -> Result<Rotation> {
    Ok(match s.trim().to_ascii_lowercase().as_str() {
        "90" | "right" | "cw" => Rotation::Right,
        "180" => Rotation::Half,
        "270" | "left" | "ccw" => Rotation::Left,
        "0" | "none" => Rotation::None,
        other => bail!("--rotate must be 90, 180, 270, left or right, got {other:?}"),
    })
}

fn scan_request(a: &ScanArgs) -> Result<ScanRequest> {
    if a.auto {
        // IJ Scan Utility's Auto: the whole platen, colour, 300 dpi; the host decides the rest.
        return Ok(ScanRequest {
            size: SizeChoice::Auto,
            resolution: 300,
            mode: OutputMode::Color,
            kind: OriginalKind::Document,
            use_intent: !a.no_intent,
            auto_crop: false,
        });
    }
    Ok(ScanRequest {
        size: SizeChoice::parse(&a.size)?,
        resolution: a.res,
        mode: match a.mode {
            ModeArg::Color => OutputMode::Color,
            ModeArg::Gray => OutputMode::Grayscale,
            ModeArg::Bw => OutputMode::BlackWhite,
        },
        kind: match a.kind {
            KindArg::Document => OriginalKind::Document,
            KindArg::Photo => OriginalKind::Photo,
        },
        use_intent: !a.no_intent,
        auto_crop: !a.crop_photos,
    })
}

fn save_format(a: &ScanArgs) -> SaveFormat {
    match a.format {
        FormatArg::Pdf => SaveFormat::Pdf,
        FormatArg::Jpeg => SaveFormat::Jpeg,
    }
}

/// Ask the user to put the next original on the glass. `false` = they are done.
fn prompt_next(n: u32) -> Result<bool> {
    print!("Put page {n} on the glass and press Enter (q + Enter to stop here): ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    let read = std::io::stdin().lock().read_line(&mut line)?;
    Ok(read > 0 && !line.trim().eq_ignore_ascii_case("q"))
}

fn scan_dry_run(cli: &Cli, a: &ScanArgs, req: &ScanRequest) -> Result<()> {
    let plan = plan_scan(&offline_printer(cli), req)?;
    let tools = Tools::detect();
    let mut o = String::new();
    let _ = writeln!(o, "{DRY_RUN_BANNER}");
    let _ = writeln!(
        o,
        "Scan: {} original, {} dpi, {}, {} -> {} ({})",
        req.size.label(),
        req.resolution,
        match req.mode {
            OutputMode::Color => "colour",
            OutputMode::Grayscale => "grey",
            OutputMode::BlackWhite => "black and white",
        },
        if req.kind == OriginalKind::Photo { "photo" } else { "document" },
        a.output.display(),
        if save_format(a) == SaveFormat::Pdf { "PDF" } else { "JPEG" }
    );
    let pages = if a.stitch.is_some() { 2 } else { a.pages };
    if pages > 1 {
        let _ = writeln!(o, "{pages} scans, you are asked to change the original in between.");
    }
    let _ = writeln!(
        o,
        "\nThe scanner is first asked for its capabilities (GET /eSCL/ScannerCapabilities, read-only), then:\n"
    );
    o.push_str(&indent(&plan.request, "  "));
    let _ = writeln!(o, "\nThen on this computer:");
    for s in &plan.host_steps {
        let _ = writeln!(o, "  - {s}");
    }
    if a.auto {
        let _ = writeln!(
            o,
            "  - decide document or photo; documents are cropped, straightened and whitened; photos are cut out one by one"
        );
    }
    if a.crop_photos {
        let _ = writeln!(o, "  - cut every photo on the platen out (multi-photo crop)");
    }
    if let Some(s) = a.stitch {
        let _ = writeln!(
            o,
            "  - join the two scans ({})",
            if s == StitchArg::LeftRight { "left to right" } else { "top to bottom" }
        );
    }
    if a.deskew
        || a.correct.is_some()
        || a.show_through
        || a.sharpen.is_some()
        || a.rotate.is_some()
        || a.aspect.is_some()
    {
        let _ = writeln!(
            o,
            "  - image edits: deskew={} correct={:?} show-through={} sharpen={:?} rotate={:?} aspect={:?}",
            a.deskew, a.correct, a.show_through, a.sharpen, a.rotate, a.aspect
        );
    }
    if let Some(l) = &a.ocr {
        let _ = writeln!(
            o,
            "  - OCR with tesseract ({l}): {}",
            if tools.can_ocr() {
                "found"
            } else {
                "NOT INSTALLED; the real run will stop before scanning"
            }
        );
    }
    let _ = writeln!(o, "  - save to {}", a.output.display());
    let _ = writeln!(o, "\nRun again with --confirm to scan.");
    print!("{o}");
    Ok(())
}

fn apply_edits(
    session: &mut ScanSession,
    a: &ScanArgs,
    force_deskew: bool,
    force_document: bool,
) -> Result<()> {
    let rotation = a.rotate.as_deref().map(parse_rotation).transpose()?;
    let aspect = match &a.aspect {
        Some(r) => {
            Some(AspectPreset::parse(r).with_context(|| format!("--aspect: unknown ratio {r:?}"))?)
        }
        None => None,
    };
    for i in 0..session.len() {
        let page = session.page_mut(i).context("page vanished")?;
        if let Some(r) = rotation {
            page.rotate(r);
        }
        if a.deskew || force_deskew {
            let angle = page.auto_deskew();
            if angle.abs() >= 0.1 {
                println!("  page {}: straightened by {angle:.1} degrees", i + 1);
            }
        }
        let mut c: ImageCorrection = page.edits.correction;
        c.color_correction = match a.correct {
            Some(CorrectArg::Document) => ColorCorrection::Document,
            Some(CorrectArg::Photo) => ColorCorrection::Photo,
            Some(CorrectArg::Magazine) => ColorCorrection::Magazine,
            None if force_document => ColorCorrection::Document,
            None => c.color_correction,
        };
        c.reduce_show_through |= a.show_through;
        if let Some(s) = a.sharpen {
            c.unsharp_amount = f32::from(s) / 100.0;
        }
        page.set_correction(c);
        if let Some(p) = aspect {
            page.set_aspect(p);
        }
    }
    Ok(())
}

/// Replace every page by the photos cut out of it (pages with no photo found stay as they are).
fn crop_photos(session: &mut ScanSession) -> ScanSession {
    let mut out = ScanSession::new();
    for p in session.pages() {
        let rgb = p.render().to_rgb8();
        let photos = detect::crop_photos(&rgb);
        if photos.is_empty() {
            println!("  no photo found on the platen; keeping the whole scan");
            out.add_page(p.clone());
        } else {
            println!("  found {} photo(s)", photos.len());
            for ph in photos {
                let mut page = Page::new(DynamicImage::ImageRgb8(ph), p.dpi);
                page.set_correction(ImageCorrection {
                    color_mode: p.edits.correction.color_mode,
                    ..ImageCorrection::default()
                });
                out.add_page(page);
            }
        }
    }
    out
}

pub fn scan(cli: &Cli, a: &ScanArgs) -> Result<()> {
    let req = scan_request(a)?;
    if a.output.as_os_str().is_empty() {
        bail!("give an output file with -o");
    }
    if !cli.confirm {
        return scan_dry_run(cli, a, &req);
    }
    let tools = Tools::detect();
    if a.ocr.is_some() && !tools.can_ocr() {
        bail!("--ocr needs tesseract, which is not installed (nothing was scanned)");
    }
    if let Some(l) = &a.ocr {
        ocr::check_language(l)?;
    }
    let printer = connect(cli)?;
    let want = if a.stitch.is_some() { 2 } else { a.pages };
    let mut session = ScanSession::new();
    for n in 1..=want {
        if n > 1 && !prompt_next(n)? {
            break;
        }
        println!("Scanning page {n} ({} dpi) ...", req.resolution);
        let started = std::time::Instant::now();
        let pages = run_scan(&printer, &req, Confirm::explicit(), &mut |_| {})?;
        println!("  done in {}", fmt_duration(started.elapsed()));
        for sp in pages {
            session.add_page(sp.page);
        }
    }
    if session.is_empty() {
        bail!("the scanner delivered no pages");
    }

    if let Some(dir) = a.stitch {
        if session.len() != 2 {
            bail!("--stitch needs two scans");
        }
        let (p0, p1) = (session.render_page(0)?.to_rgb8(), session.render_page(1)?.to_rgb8());
        let dir = if dir == StitchArg::LeftRight {
            StitchDirection::LeftRight
        } else {
            StitchDirection::TopBottom
        };
        let joined: RgbImage = stitch(&p0, &p1, dir, &StitchOptions::default());
        println!("  joined the two scans into {} x {} px", joined.width(), joined.height());
        let dpi = session.page(0).map_or(300, |p| p.dpi);
        session = ScanSession::new();
        session.add_page(Page::new(DynamicImage::ImageRgb8(joined), dpi));
    }

    let mut force_deskew = false;
    let mut force_document = false;
    let mut format = save_format(a);
    if a.auto {
        let mut next = ScanSession::new();
        for p in session.pages() {
            match detect::classify(&p.original().to_rgb8()) {
                ScanKind::Blank => bail!("nothing was found on the platen"),
                ScanKind::Document => {
                    println!("  looks like a document");
                    force_deskew = true;
                    force_document = true;
                    let rgb = p.original().to_rgb8();
                    let img = detect::auto_crop(&rgb).unwrap_or(rgb);
                    next.add_page(Page::new(DynamicImage::ImageRgb8(img), p.dpi));
                }
                ScanKind::Photo => {
                    println!("  looks like photo(s)");
                    if a.format != FormatArg::Pdf
                        || a.output.extension().is_none_or(|e| !e.eq_ignore_ascii_case("pdf"))
                    {
                        format = SaveFormat::Jpeg;
                    }
                    let mut one = ScanSession::new();
                    one.add_page(p.clone());
                    let cropped = crop_photos(&mut one);
                    for q in cropped.pages() {
                        next.add_page(q.clone());
                    }
                }
            }
        }
        session = next;
    } else if a.crop_photos {
        session = crop_photos(&mut session);
    }
    apply_edits(&mut session, a, force_deskew, force_document)?;

    let opts = SaveOptions { format, single_file: !a.split, ..SaveOptions::default() };
    let mut files = save(&session, &a.output, &opts)?;
    if let Some(lang) = &a.ocr {
        let dpi = session.page(0).map_or(300, |p| p.dpi);
        let imgs: Vec<DynamicImage> =
            (0..session.len()).map(|i| session.render_page(i)).collect::<Result<_, _>>()?;
        let mut text = String::new();
        for (i, img) in imgs.iter().enumerate() {
            println!("Recognising text on page {} ...", i + 1);
            if i > 0 {
                text.push_str("\n\x0c\n");
            }
            text.push_str(&ocr::ocr_text(img, lang, dpi, &tools)?);
        }
        let txt = files[0].with_extension("txt");
        std::fs::write(&txt, text).with_context(|| format!("cannot write {}", txt.display()))?;
        if format == SaveFormat::Pdf && opts.single_file {
            let pdf = ocr::searchable_pdf(&imgs, lang, dpi, &tools)?;
            std::fs::write(&files[0], pdf)
                .with_context(|| format!("cannot write {}", files[0].display()))?;
        }
        files.push(txt);
    }
    for f in &files {
        println!("Saved {}", f.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// print
// ---------------------------------------------------------------------------------------------

fn source_of(s: SourceArg) -> Source {
    match s {
        SourceArg::Auto => Source::Auto,
        SourceArg::Rear => Source::Rear,
        SourceArg::Cassette => Source::Cassette,
        SourceArg::Disc => Source::Disc,
    }
}

fn quality_of(q: QualityArg) -> Result<washi_core::net::ipp::Quality> {
    Ok(print::parse_quality(q.name())?)
}

fn print_options(a: &PrintArgs) -> Result<PrintOptions> {
    Ok(PrintOptions {
        paper: *PaperSize::parse(&a.size)?,
        media_type: a.media_type.as_deref().map(print::media_type).transpose()?,
        source: source_of(a.source),
        borderless: a.borderless,
        quality: quality_of(a.quality)?,
        color: !a.mono,
        copies: a.copies,
        pages: a.pages.as_deref().map(PageSelection::parse).transpose()?,
        layout: a.layout,
        duplex: a.duplex.map(|d| match d {
            DuplexArg::Long => Duplex::Long,
            DuplexArg::Short => Duplex::Short,
        }),
        orientation: match a.orientation {
            OrientationArg::Auto => print::Orientation::Auto,
            OrientationArg::Portrait => print::Orientation::Portrait,
            OrientationArg::Landscape => print::Orientation::Landscape,
        },
        job_name: a.name.clone(),
    })
}

fn progress_line(ev: &PrintProgress) -> Option<String> {
    Some(match ev {
        PrintProgress::Preflight => "Checking what the printer accepts (read-only) ...".to_string(),
        PrintProgress::Preparing { job, jobs, side, sides } => {
            if side >= sides {
                return None;
            }
            format!("  [{}/{}] rendering sheet side {} of {sides} ...", job + 1, jobs, side + 1)
        }
        PrintProgress::Validated { ignored } if ignored.is_empty() => {
            "  the printer accepts the job (Validate-Job)".to_string()
        }
        PrintProgress::Validated { ignored } => {
            format!("  the printer accepts the job but will ignore: {}", ignored.join(", "))
        }
        PrintProgress::Sending { bytes, .. } => format!("  sending {} ...", fmt_bytes(*bytes)),
        PrintProgress::Submitted { id, .. } => format!("  the printer took the job: id {id}"),
        PrintProgress::State { id, state, reasons, impressions } => {
            let r: Vec<&str> =
                reasons.iter().map(String::as_str).filter(|r| *r != "none").collect();
            format!(
                "  job {id}: {}{}{}",
                state.as_str(),
                impressions
                    .filter(|n| *n > 0)
                    .map_or(String::new(), |n| format!(", {n} page(s) done")),
                if r.is_empty() { String::new() } else { format!(" ({})", r.join(", ")) }
            )
        }
        PrintProgress::Finished { id, state } => format!("  job {id} {}", state.as_str()),
    })
}

/// `washi print PICTURE --source disc`: the picture becomes the background of a disc label.
fn print_on_disc(cli: &Cli, a: &PrintArgs) -> Result<()> {
    let [file] = a.files.as_slice() else {
        bail!(
            "the disc tray takes one picture at a time (or use `washi disc` for labels with text)"
        );
    };
    if print::classify_file(file)? != print::FileKind::Photo {
        bail!(
            "{}: disc printing takes a picture (JPEG, PNG or BMP); use `washi disc` for labels",
            file.display()
        );
    }
    let media = match a.media_type.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("a" | "disc" | "disc-a" | "com.canon-mtdisca") => DiscMedia::A,
        Some("b" | "disc-b" | "com.canon-mtdiscb") => DiscMedia::B,
        Some(other) => bail!("--media-type for a disc is a or b, got {other:?}"),
    };
    let label =
        DiscLabel { media, background: Some(Background::new(file)), ..DiscLabel::default() };
    disc_dry_or_run(cli, &label, quality_of(a.quality)?, a.no_wait)
}

pub fn print(cli: &Cli, a: &PrintArgs) -> Result<()> {
    if a.source == SourceArg::Disc {
        return print_on_disc(cli, a);
    }
    let opts = print_options(a)?;
    let tools = Tools::detect();
    if !cli.confirm {
        let printer = offline_printer(cli);
        let plan = print::plan_print(&printer, &a.files, &opts, &tools)?;
        let mut o = String::new();
        let _ = writeln!(o, "{DRY_RUN_BANNER}");
        let _ = writeln!(o, "Print to {}:\n", plan.printer_uri);
        o.push_str(&plan.describe(&printer.ipp()));
        for w in &plan.warnings {
            let _ = writeln!(o, "WARNING: {w}");
        }
        let _ = writeln!(
            o,
            "\nBefore sending, the printer is asked what it accepts (Get-Printer-Attributes, read-only) and the paper combination is checked against its media database (see `washi media`).\nRun again with --confirm to print."
        );
        print!("{o}");
        return Ok(());
    }
    let printer = connect(cli)?;
    let plan = print::plan_print(&printer, &a.files, &opts, &tools)?;
    for w in &plan.warnings {
        eprintln!("warning: {w}");
    }
    let run = RunOptions { check_media: !a.no_check, wait: !a.no_wait, ..RunOptions::default() };
    let started = std::time::Instant::now();
    let report = print::run_print(&printer, &plan, &run, &tools, Confirm::explicit(), &mut |ev| {
        if let Some(l) = progress_line(&ev) {
            println!("{l}");
        }
    })?;
    for j in &report.jobs {
        println!("{}: job {} {}", j.label, j.job_id, j.state.as_str());
        if !j.ignored.is_empty() {
            println!("  (the printer ignored: {})", j.ignored.join(", "));
        }
    }
    println!("Done in {}.", fmt_duration(started.elapsed()));
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// disc
// ---------------------------------------------------------------------------------------------

fn parse_xy(s: &str) -> Result<(f64, f64)> {
    let (x, y) =
        s.split_once(',').with_context(|| format!("--text-pos must be X,Y in mm, got {s:?}"))?;
    Ok((
        x.trim().parse().context("--text-pos X is not a number")?,
        y.trim().parse().context("--text-pos Y is not a number")?,
    ))
}

fn disc_label(a: &DiscArgs) -> Result<DiscLabel> {
    let mut l = DiscLabel::default();
    l.geometry.inner_mm = a.inner;
    l.geometry.outer_mm = a.outer;
    l.geometry.validate()?;
    l.media = match a.media_type {
        DiscMediaArg::A => DiscMedia::A,
        DiscMediaArg::B => DiscMedia::B,
    };
    l.background_color = disc::parse_color(&a.bg_color)?;
    if let Some(p) = &a.image {
        disc::check_image(p)?;
        let mut b = Background::new(p);
        b.fit = match a.fit {
            FitArg::Cover => BackgroundFit::Cover,
            FitArg::Contain => BackgroundFit::Contain,
        };
        b.scale = a.scale;
        b.rotation_deg = a.rotate;
        l.background = Some(b);
    }
    let (x, y) = parse_xy(&a.text_pos)?;
    let color = disc::parse_color(&a.text_color)?;
    let line_mm = a.text_size * 1.5 / 72.0 * 25.4;
    let n = a.text.len() as f64;
    for (i, t) in a.text.iter().enumerate() {
        let mut item = TextItem::new(t);
        item.x_mm = x;
        item.y_mm = y + (i as f64 - (n - 1.0) / 2.0) * line_mm;
        item.size_pt = a.text_size.clamp(4.0, 200.0);
        item.color = color;
        item.align = Align::Center;
        item.font = a.font.clone();
        item.family = a.font_family.clone().filter(|f| !f.trim().is_empty());
        item.bold = a.bold;
        item.italic = a.italic;
        l.texts.push(item);
    }
    if let Some(t) = &a.ring {
        let mut item = TextItem::new(t);
        item.size_pt = a.text_size.clamp(4.0, 200.0);
        item.color = color;
        item.align = Align::Center;
        item.font = a.font.clone();
        item.family = a.font_family.clone().filter(|f| !f.trim().is_empty());
        item.bold = a.bold;
        item.italic = a.italic;
        let ring = disc::RingText {
            radius_mm: a.ring_radius,
            start_deg: a.ring_start,
            clockwise: !a.ring_ccw,
            letter_spacing_pt: a.ring_spacing,
        };
        ring.validate()?;
        item.ring = Some(ring);
        l.texts.push(item);
    }
    Ok(l)
}

pub fn disc_cmd(cli: &Cli, a: &DiscArgs) -> Result<()> {
    let label = disc_label(a)?;
    let quality = quality_of(a.quality)?;
    if let Some(out) = &a.preview {
        let img = label.render_sized(1200)?;
        img.save(out).with_context(|| format!("cannot write {}", out.display()))?;
        println!("Wrote a preview of the label to {} (no printer access).", out.display());
        return Ok(());
    }
    if label.background.is_none() && label.texts.is_empty() {
        bail!(
            "nothing to print: give --image, --text and/or --ring (or --preview to look at an empty ring)"
        );
    }
    disc_dry_or_run(cli, &label, quality, a.no_wait)
}

/// The dry run of a disc label, or the real print with `--confirm`.
fn disc_dry_or_run(
    cli: &Cli,
    label: &DiscLabel,
    quality: washi_core::net::ipp::Quality,
    no_wait: bool,
) -> Result<()> {
    if !cli.confirm {
        let plan = disc::plan_disc(&offline_printer(cli), label, quality)?;
        let mut o = String::new();
        let _ = writeln!(o, "{DRY_RUN_BANNER}");
        let _ = writeln!(
            o,
            "Disc label on {}:\n",
            plan.job
                .media
                .as_ref()
                .map_or(String::new(), |m| m.media_type.clone().unwrap_or_default())
        );
        for n in &plan.notes {
            let _ = writeln!(o, "  - {n}");
        }
        let _ = writeln!(o, "\nValidate-Job, then:\n");
        o.push_str(&indent(&plan.request, "  "));
        let _ = writeln!(o, "    + document data (image/pwg-raster, 1 page, 2834 x 2834 px)");
        let _ = writeln!(o, "\nRun again with --confirm to print.");
        print!("{o}");
        return Ok(());
    }
    let printer = connect(cli)?;
    let run = RunOptions { wait: !no_wait, ..RunOptions::default() };
    println!("Rendering the label ...");
    let r = disc::run_disc(&printer, label, quality, &run, Confirm::explicit(), &mut |ev| {
        if let Some(l) = progress_line(&ev) {
            println!("{l}");
        }
    })?;
    println!("Disc label: job {} {}.", r.job_id, r.state.as_str());
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// identify, jobs, cancel
// ---------------------------------------------------------------------------------------------

pub fn identify(cli: &Cli, action: IdentifyArg) -> Result<()> {
    let action = match action {
        IdentifyArg::Flash => IdentifyAction::Flash,
        IdentifyArg::Sound => IdentifyAction::Sound,
    };
    if !cli.confirm {
        let p = offline_printer(cli);
        println!("{DRY_RUN_BANNER}\n");
        print!("{}", indent(&p.ipp().identify_request(action, "washi").describe(), "  "));
        println!("\nRun again with --confirm to make the printer {}.", action.as_str());
        return Ok(());
    }
    let p = connect(cli)?;
    p.ipp().identify(action, "washi", Confirm::explicit())?;
    println!("Identify-Printer ({}) sent.", action.as_str());
    Ok(())
}

pub fn jobs(cli: &Cli) -> Result<()> {
    let p = connect(cli)?;
    let list = p.ipp().get_jobs(WhichJobs::NotCompleted)?;
    if cli.json {
        return print_json(&list);
    }
    print!("{}", render_jobs(&list));
    Ok(())
}

pub fn render_jobs(list: &[JobAttributes]) -> String {
    if list.is_empty() {
        return "No jobs are waiting or printing.\n".to_string();
    }
    let mut o = String::new();
    for j in list {
        let _ = writeln!(
            o,
            "job {:>5}  {}  {}",
            j.id.map_or("?".to_string(), |i| i.to_string()),
            j.state.map_or("unknown", |s| s.as_str()),
            j.state_reasons.join(", ")
        );
    }
    o
}

pub fn cancel(cli: &Cli, job: i32) -> Result<()> {
    if !cli.confirm {
        let p = offline_printer(cli);
        println!("{DRY_RUN_BANNER}\n");
        print!("{}", indent(&p.ipp().cancel_job_request(job, "washi").describe(), "  "));
        println!("\nRun again with --confirm to cancel job {job}.");
        return Ok(());
    }
    let p = connect(cli)?;
    p.ipp().cancel_job(job, "washi", Confirm::explicit())?;
    println!("Cancel-Job {job} sent.");
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// media, scanner, tools
// ---------------------------------------------------------------------------------------------

fn size_name(w: i32, h: i32) -> String {
    match PaperSize::by_dimensions(w, h) {
        Some(p) => {
            format!("{} ({:.0} x {:.0} mm)", p.label, f64::from(w) / 100.0, f64::from(h) / 100.0)
        }
        None => format!("{:.1} x {:.1} mm", f64::from(w) / 100.0, f64::from(h) / 100.0),
    }
}

/// `washi media`: what the media database says.
pub fn render_media(
    db: &MediaDatabase,
    caps: Option<&PrintCapabilities>,
    source: Option<&str>,
    size: Option<&PaperSize>,
) -> String {
    let mut o = String::new();
    if db.is_empty() {
        return "The printer returned no media database.\n".to_string();
    }
    if let Some(sz) = size {
        let _ =
            writeln!(o, "Media types for {} from {}:", sz.label, source.unwrap_or("any source"));
        let mut any = false;
        for src in db.sources().into_iter().filter(|s| source.is_none_or(|w| w == s)) {
            let entries: Vec<&MediaEntry> = db
                .for_source(&src)
                .filter(|e| (e.width, e.height) == (sz.width, sz.height))
                .collect();
            if entries.is_empty() {
                continue;
            }
            any = true;
            let mut bordered: Vec<&str> = entries
                .iter()
                .filter(|e| !e.is_borderless())
                .filter_map(|e| e.media_type.as_deref())
                .collect();
            let mut borderless: Vec<&str> = entries
                .iter()
                .filter(|e| e.is_borderless())
                .filter_map(|e| e.media_type.as_deref())
                .collect();
            bordered.sort_unstable();
            borderless.sort_unstable();
            let _ = writeln!(o, "  source {src}:");
            let _ = writeln!(
                o,
                "    with margins : {}",
                if bordered.is_empty() { "-".into() } else { bordered.join(", ") }
            );
            let _ = writeln!(
                o,
                "    borderless   : {}",
                if borderless.is_empty() { "-".into() } else { borderless.join(", ") }
            );
            if let Some(m) = entries.iter().find_map(|e| e.margins()).filter(|m| !m.is_zero()) {
                let _ = writeln!(
                    o,
                    "    margins (1/100 mm): top {} bottom {} left {} right {}",
                    m.top, m.bottom, m.left, m.right
                );
            }
        }
        if !any {
            let _ = writeln!(o, "  none: the printer does not offer that size from that source.");
        }
        return o;
    }
    let _ = writeln!(o, "{} entries in the printer's media-col-database.\n", db.entries.len());
    for src in db.sources().into_iter().filter(|s| source.is_none_or(|w| w == s)) {
        let label = match src.as_str() {
            "main" => "main (the cassette)",
            "rear" => "rear tray",
            "disc" => "disc tray",
            "auto" => "auto (the printer picks)",
            other => other,
        };
        let _ = writeln!(o, "Source {label}:");
        for (w, h) in db.sizes(Some(&src)) {
            let bl = db
                .for_source(&src)
                .filter(|e| (e.width, e.height) == (w, h) && e.is_borderless())
                .count();
            let n = db.for_source(&src).filter(|e| (e.width, e.height) == (w, h)).count() - bl;
            let _ = writeln!(
                o,
                "  {:<34} {n:>3} with margins{}",
                size_name(w, h),
                if bl > 0 { format!(", {bl} borderless") } else { String::new() }
            );
        }
        let _ = writeln!(o);
    }
    let disc = db.disc();
    if !disc.is_empty() && source.is_none_or(|s| s == "disc") {
        let _ = writeln!(o, "Disc tray entries (what `washi disc` uses):");
        for e in disc {
            let m = e.margins().map_or("margins not stated".to_string(), |m| {
                format!("margins {}/{}/{}/{} (1/100 mm)", m.top, m.bottom, m.left, m.right)
            });
            let _ = writeln!(
                o,
                "  {}  media-source={}  media-type={}  {m}",
                size_name(e.width, e.height),
                e.source.as_deref().unwrap_or("-"),
                e.media_type.as_deref().unwrap_or("-")
            );
        }
    }
    if let Some(c) = caps {
        let _ = writeln!(o, "\nAccepts: {}", c.document_formats.join(", "));
        if !c.raster_types.is_empty() {
            let _ = writeln!(
                o,
                "PWG raster: {} at {} dpi, back sides {}",
                c.raster_types.join(", "),
                c.raster_resolutions
                    .iter()
                    .map(|(x, y)| format!("{x}x{y}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                c.raster_sheet_back.as_deref().unwrap_or("?")
            );
        }
    }
    o
}

pub fn media(cli: &Cli, source: Option<SourceArg>, size: Option<&str>) -> Result<()> {
    let p = connect(cli)?;
    let ipp = p.ipp();
    let db = ipp.media_database()?;
    let size = size.map(PaperSize::parse).transpose()?;
    if cli.json {
        let entries: Vec<&MediaEntry> = db
            .entries
            .iter()
            .filter(|e| source.is_none_or(|s| e.source.as_deref() == Some(source_of(s).ipp())))
            .filter(|e| size.is_none_or(|z| (e.width, e.height) == (z.width, z.height)))
            .collect();
        return print_json(&entries);
    }
    let caps = ipp.print_capabilities().ok();
    print!("{}", render_media(&db, caps.as_ref(), source.map(|s| source_of(s).ipp()), size));
    Ok(())
}

fn range_text(r: &Option<washi_core::net::escl::RangeCap>) -> String {
    r.map_or("not adjustable".to_string(), |r| format!("{}..{}", r.min, r.max))
}

fn inches_mm(units: u32) -> f64 {
    f64::from(units) / 300.0 * 25.4
}

fn render_source(o: &mut String, s: &SourceCaps) {
    let name = match (s.source, s.duplex) {
        (washi_core::net::escl::InputSource::Platen, _) => "Platen",
        (_, true) => "Feeder (two-sided)",
        _ => "Feeder",
    };
    let _ = writeln!(
        o,
        "{name}  scan area {} x {} units of 1/300 inch ({:.1} x {:.1} mm)",
        s.max_width,
        s.max_height,
        inches_mm(s.max_width),
        inches_mm(s.max_height)
    );
    let modes: Vec<&str> = s.color_modes.iter().map(|m| m.as_str()).collect();
    let _ = writeln!(
        o,
        "  colour modes : {}{}",
        modes.join(", "),
        s.default_color_mode
            .as_ref()
            .map_or(String::new(), |d| format!("  (default {})", d.as_str()))
    );
    let res = if s.resolutions.is_empty() {
        format!("{} .. {}", range_text(&s.x_resolution_range), range_text(&s.y_resolution_range))
    } else {
        s.square_resolutions().iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
    };
    let _ = writeln!(
        o,
        "  resolutions  : {res} dpi{}",
        s.default_resolution.map_or(String::new(), |d| format!("  (default {})", d.x))
    );
    let _ = writeln!(o, "  formats      : {}", s.formats.join(", "));
    let intents: Vec<&str> = s.intents.iter().map(|i| i.as_str()).collect();
    let _ = writeln!(o, "  intents      : {}", intents.join(", "));
    let _ = writeln!(o, "  content types: {}", s.content_types.join(", "));
    let _ = writeln!(
        o,
        "  adjustments  : brightness {}, contrast {}, sharpen {}, threshold {}",
        range_text(&s.brightness),
        range_text(&s.contrast),
        range_text(&s.sharpen),
        range_text(&s.threshold)
    );
}

/// `washi scanner`.
pub fn render_scanner(caps: &ScannerCapabilities, status: Option<&ScannerStatus>) -> String {
    let mut o = String::new();
    let _ = writeln!(
        o,
        "Scanner  {} (eSCL {})",
        caps.make_and_model.as_deref().unwrap_or("unknown model"),
        caps.version
    );
    if let Some(s) = status {
        let _ = writeln!(o, "State    {}", s.state.as_str());
        for j in &s.jobs {
            let _ = writeln!(o, "Job      {}  {}", j.uri, j.state);
        }
    }
    for s in &caps.sources {
        render_source(&mut o, s);
    }
    if !caps.media_types.is_empty() {
        let _ = writeln!(o, "Media    {}", caps.media_types.join(", "));
    }
    o
}

#[derive(Serialize)]
struct ScannerJson<'a> {
    capabilities: &'a ScannerCapabilities,
    status: Option<&'a ScannerStatus>,
}

pub fn scanner(cli: &Cli) -> Result<()> {
    let p = connect(cli)?;
    let escl = p.escl();
    let caps = escl.capabilities()?;
    let status = escl.status().ok();
    if cli.json {
        return print_json(&ScannerJson { capabilities: &caps, status: status.as_ref() });
    }
    print!("{}", render_scanner(&caps, status.as_ref()));
    Ok(())
}

pub fn tools(cli: &Cli) -> Result<()> {
    let t = Tools::detect();
    if cli.json {
        return print_json(&t);
    }
    for (name, what, path) in t.report() {
        match path {
            Some(p) => println!("{name:<10} {what:<34} {}", p.display()),
            None => println!("{name:<10} {what:<34} not installed"),
        }
    }
    if !t.can_print_pdf() {
        println!("\nPDF printing needs poppler (pdftoppm and pdfinfo).");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use washi_core::net::escl::{ScannerCapabilities, ScannerStatus};

    use super::*;

    const CAPS: &str = include_str!("../../washi-net/tests/fixtures/escl-capabilities-xk140.xml");
    const STATUS: &str = include_str!("../../washi-net/tests/fixtures/escl-status-idle.xml");

    #[test]
    fn scanner_summary_lists_what_the_xk140_offers() {
        let caps = ScannerCapabilities::parse(CAPS).unwrap();
        let st = ScannerStatus::parse(STATUS).unwrap();
        let t = render_scanner(&caps, Some(&st));
        assert!(t.contains("Canon XK140 series (eSCL 2.9)") && t.contains("State    Idle"), "{t}");
        assert!(t.contains("215.9 x 297.0 mm"), "{t}");
        assert!(t.contains("Grayscale8, RGB24  (default RGB24)"), "{t}");
        assert!(t.contains("75, 100, 150, 200, 300, 600, 1200 dpi  (default 300)"), "{t}");
        assert!(
            t.contains("image/jpeg, application/pdf")
                && t.contains("Document, Photo, Preview, TextAndGraphic, Auto")
        );
        assert!(t.contains("brightness not adjustable"));
    }

    #[test]
    fn rotation_words() {
        assert_eq!(parse_rotation("90").unwrap(), Rotation::Right);
        assert_eq!(parse_rotation("LEFT").unwrap(), Rotation::Left);
        assert_eq!(parse_rotation("180").unwrap(), Rotation::Half);
        assert!(parse_rotation("45").is_err());
    }

    #[test]
    fn xy_positions_and_byte_sizes() {
        assert_eq!(parse_xy("0, 32").unwrap(), (0.0, 32.0));
        assert_eq!(parse_xy("-5.5,10").unwrap(), (-5.5, 10.0));
        assert!(parse_xy("12").is_err() && parse_xy("a,b").is_err());
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(2048), "2 KB");
        assert_eq!(fmt_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn job_listing() {
        assert!(render_jobs(&[]).contains("No jobs"));
        let j = JobAttributes {
            id: Some(7),
            uri: None,
            state: Some(washi_core::net::ipp::JobState::Processing),
            state_reasons: vec!["job-printing".into()],
            state_message: None,
            impressions_completed: None,
            sheets_completed: None,
            ignored: vec![],
        };
        let t = render_jobs(&[j]);
        assert!(t.contains("job     7  processing  job-printing"), "{t}");
    }

    #[test]
    fn media_listing_names_the_disc_tray() {
        let entry = |w, h, m: i32, src: &str, ty: &str| MediaEntry {
            width: w,
            height: h,
            top: Some(m),
            bottom: Some(m),
            left: Some(m),
            right: Some(m),
            source: Some(src.into()),
            media_type: Some(ty.into()),
        };
        let db = MediaDatabase {
            entries: vec![
                entry(21000, 29700, 500, "rear", "stationery"),
                entry(21000, 29700, 0, "rear", "com.canon-mtglossy"),
                entry(12000, 12000, 10, "disc", "com.canon-mtdisca"),
                entry(12000, 12000, 10, "disc", "com.canon-mtdiscb"),
            ],
        };
        let all = render_media(&db, None, None, None);
        assert!(
            all.contains("4 entries")
                && all.contains("Source rear tray:")
                && all.contains("Source disc tray:"),
            "{all}"
        );
        assert!(all.contains("A4 (210 x 297 mm)") && all.contains("1 borderless"), "{all}");
        assert!(all.contains("1 with margins, 1 borderless"), "{all}");
        assert!(
            all.contains("media-source=disc  media-type=com.canon-mtdisca  margins 10/10/10/10"),
            "{all}"
        );
        let a4 = render_media(&db, None, Some("rear"), PaperSize::parse("a4").ok());
        assert!(
            a4.contains("with margins : stationery")
                && a4.contains("borderless   : com.canon-mtglossy"),
            "{a4}"
        );
        let none = render_media(&db, None, Some("disc"), PaperSize::parse("a4").ok());
        assert!(none.contains("does not offer"));
        assert!(
            render_media(&MediaDatabase::default(), None, None, None).contains("no media database")
        );
    }
}
