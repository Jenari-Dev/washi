// SPDX-License-Identifier: GPL-2.0-or-later
//! QML-facing image work: the scan session behind the Scan screens, previews of photo sheets and
//! disc labels, and a few small file helpers.
//!
//! Everything slow runs on a worker thread and answers with the `done(id, ok, body)` signal
//! (`body` is a JSON object; on failure `{"message": "..."}`). Previews are written as PNG files
//! in a private temporary directory and handed to QML as paths; every render gets a new file
//! name, so an `Image` never shows a stale cache entry. Nothing here talks to a printer.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type Imaging = super::ImagingRust;

        // ---- environment and small file helpers ------------------------------------------
        /// JSON of the optional programs found: `{"pdftoppm": bool, "soffice": bool, ...}`.
        #[qinvokable]
        fn tools(self: &Imaging) -> QString;

        /// `~/.local/share/washi` (created on demand by the writers below).
        #[qinvokable]
        #[cxx_name = "dataDir"]
        fn data_dir(self: &Imaging) -> QString;

        /// The user's home directory.
        #[qinvokable]
        #[cxx_name = "homeDir"]
        fn home_dir(self: &Imaging) -> QString;

        /// The file's text, "" when it cannot be read.
        #[qinvokable]
        #[cxx_name = "readText"]
        fn read_text(self: &Imaging, path: &QString) -> QString;

        /// Write a text file (parent directories are created). False on failure.
        #[qinvokable]
        #[cxx_name = "writeText"]
        fn write_text(self: &Imaging, path: &QString, text: &QString) -> bool;

        /// Names of the files in `dir` ending in `ext` (e.g. ".json"), newest first, as JSON.
        #[qinvokable]
        #[cxx_name = "listFiles"]
        fn list_files(self: &Imaging, dir: &QString, ext: &QString) -> QString;

        /// Delete a file below the data directory (disc designs). False when it is elsewhere.
        #[qinvokable]
        #[cxx_name = "removeDataFile"]
        fn remove_data_file(self: &Imaging, path: &QString) -> bool;

        /// `{"width", "height"}` of an image file read from its header, "" when it is none.
        #[qinvokable]
        #[cxx_name = "imageSize"]
        fn image_size(self: &Imaging, path: &QString) -> QString;

        /// A uniform random-ish token for file names.
        #[qinvokable]
        fn token(self: &Imaging) -> QString;

        // ---- scan session -------------------------------------------------------------------
        #[qinvokable]
        #[cxx_name = "scanReset"]
        fn scan_reset(self: &Imaging);

        /// Add the JPEG at `path` as a new page (decoded off the UI thread); `mode` is the
        /// colour mode of the scan (color, gray, bw). `done` carries `{"index", "count"}`.
        #[qinvokable]
        #[cxx_name = "scanAddPage"]
        fn scan_add_page(self: Pin<&mut Imaging>, path: &QString, dpi: i32, mode: &QString) -> i32;

        #[qinvokable]
        #[cxx_name = "scanCount"]
        fn scan_count(self: &Imaging) -> i32;

        /// `{"width","height","rotated_width","rotated_height","dpi","edited","edits"}`.
        #[qinvokable]
        #[cxx_name = "scanPageInfo"]
        fn scan_page_info(self: &Imaging, index: i32) -> QString;

        /// Turn a page: `left` or `right`.
        #[qinvokable]
        #[cxx_name = "scanRotate"]
        fn scan_rotate(self: &Imaging, index: i32, dir: &QString) -> bool;

        #[qinvokable]
        #[cxx_name = "scanDelete"]
        fn scan_delete(self: &Imaging, index: i32) -> bool;

        /// "Revert to original".
        #[qinvokable]
        #[cxx_name = "scanRevert"]
        fn scan_revert(self: &Imaging, index: i32) -> bool;

        /// Render a page to a PNG whose longest edge is at most `max_edge`. `mode`: `page`
        /// (the saved recipe), `work` (the working copy of the edit screens, whole recipe) or
        /// `work-geometry` (the working copy turned and de-slanted only: no crop, no colour
        /// work, as the Rotate/Crop screen shows it). `done` carries `{"path","width","height"}`.
        #[qinvokable]
        #[cxx_name = "scanRender"]
        fn scan_render(self: Pin<&mut Imaging>, index: i32, max_edge: i32, mode: &QString) -> i32;

        // ---- edit screens (a working copy of one page) -----------------------------------------
        /// Start editing page `index`; returns the state JSON (see `editState`).
        #[qinvokable]
        #[cxx_name = "editBegin"]
        fn edit_begin(self: &Imaging, index: i32) -> QString;

        /// `{"edits", "rotated_width", "rotated_height", "index"}` of the working copy.
        #[qinvokable]
        #[cxx_name = "editState"]
        fn edit_state(self: &Imaging) -> QString;

        #[qinvokable]
        #[cxx_name = "editRotate"]
        fn edit_rotate(self: &Imaging, dir: &QString) -> QString;

        /// Slant correction in degrees (clockwise positive, clamped to +-10).
        #[qinvokable]
        #[cxx_name = "editSlant"]
        fn edit_slant(self: &Imaging, degrees: f64) -> QString;

        /// Trimming box as fractions of the turned, de-slanted page.
        #[qinvokable]
        #[cxx_name = "editCrop"]
        fn edit_crop(self: &Imaging, x: f64, y: f64, w: f64, h: f64) -> QString;

        #[qinvokable]
        #[cxx_name = "editClearCrop"]
        fn edit_clear_crop(self: &Imaging) -> QString;

        /// Fit the trimming box to an aspect-ratio preset (`custom`, `original`, `square`,
        /// `3:2`, `4:3`, `7:5`, `10:8`, `16:9`); returns the state JSON.
        #[qinvokable]
        #[cxx_name = "editAspect"]
        fn edit_aspect(self: &Imaging, preset: &QString) -> QString;

        /// Replace the image correction (the JSON of `ImageCorrection`).
        #[qinvokable]
        #[cxx_name = "editCorrection"]
        fn edit_correction(self: &Imaging, json: &QString) -> QString;

        /// Straighten the page automatically. `done` carries `{"slant"}` in degrees.
        #[qinvokable]
        #[cxx_name = "editAutoDeskew"]
        fn edit_auto_deskew(self: Pin<&mut Imaging>) -> i32;

        /// Keep the working copy as the page's recipe.
        #[qinvokable]
        #[cxx_name = "editCommit"]
        fn edit_commit(self: &Imaging) -> bool;

        #[qinvokable]
        #[cxx_name = "editCancel"]
        fn edit_cancel(self: &Imaging);

        // ---- save and extras -----------------------------------------------------------------
        /// Save pages. JSON: `{"path", "format": "pdf"|"jpeg"|"pdf-ocr"|"txt", "range":
        /// "all"|"current", "current", "single_file", "lang"}`. `done` carries `{"files"}`.
        #[qinvokable]
        #[cxx_name = "scanSave"]
        fn scan_save(self: Pin<&mut Imaging>, json: &QString) -> i32;

        /// Host-side extras on a page: `{"op": "classify", "index"}` (document / photo /
        /// blank), `{"op": "crop-photos", "index"}` (one page per photo on the glass),
        /// `{"op": "stitch", "a", "b", "direction": "left-right"|"top-bottom"}`.
        #[qinvokable]
        #[cxx_name = "scanExtra"]
        fn scan_extra(self: Pin<&mut Imaging>, json: &QString) -> i32;

        // ---- print and disc previews -----------------------------------------------------------
        /// The photo sheet as it will be printed. JSON: `{"files", "size", "borderless",
        /// "layout", "orientation", "max_edge"}`. `done` carries `{"path","width","height"}`.
        #[qinvokable]
        #[cxx_name = "photoPreview"]
        fn photo_preview(self: Pin<&mut Imaging>, json: &QString) -> i32;

        /// The disc label as it will be printed (the `PrintDisc` JSON, plus `px`).
        #[qinvokable]
        #[cxx_name = "discPreview"]
        fn disc_preview(self: Pin<&mut Imaging>, json: &QString, px: i32) -> i32;

        /// Rasterise one page of a PDF with poppler (the fallback when QtPdf is missing).
        #[qinvokable]
        #[cxx_name = "pdfPage"]
        fn pdf_page(self: Pin<&mut Imaging>, path: &QString, page: i32, max_edge: i32) -> i32;

        /// Page count of a PDF through poppler's `pdfinfo`, 0 when it cannot be read.
        #[qinvokable]
        #[cxx_name = "pdfPages"]
        fn pdf_pages(self: &Imaging, path: &QString) -> i32;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        fn done(self: Pin<&mut Imaging>, id: i32, ok: bool, body: QString);
    }

    impl cxx_qt::Threading for Imaging {}
}

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use image::{DynamicImage, RgbImage};
use serde_json::{Value, json};
use washi_core::Tools;
use washi_core::disc::DiscSpec;
use washi_core::print::{self, Orientation, PaperSize, photo};
use washi_core::scan::detect::{self, ScanKind};
use washi_core::scan::ocr;
use washi_core::scan::ops::{AspectPreset, CropRect};
use washi_core::scan::save::{self, SaveFormat, SaveOptions, SaveRange};
use washi_core::scan::stitch::{self, StitchDirection, StitchOptions};
use washi_core::scan::{ImageCorrection, OutputMode, Page, ScanSession};

/// Previews kept on disk before the oldest are deleted.
const KEEP_OUTPUTS: usize = 96;
/// Decoded photos kept for sheet previews.
const KEEP_PHOTOS: usize = 12;

struct Work {
    index: usize,
    page: Page,
}

#[derive(Default)]
struct Inner {
    scan: ScanSession,
    work: Option<Work>,
    photos: HashMap<PathBuf, Arc<RgbImage>>,
    photo_order: VecDeque<PathBuf>,
    outputs: VecDeque<PathBuf>,
    dir: Option<PathBuf>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct ImagingRust {
    inner: Arc<Mutex<Inner>>,
    next_id: Arc<AtomicI32>,
    seq: Arc<AtomicU64>,
}

impl Default for ImagingRust {
    fn default() -> Self {
        ImagingRust {
            inner: Arc::new(Mutex::new(Inner::default())),
            next_id: Arc::new(AtomicI32::new(0)),
            seq: Arc::new(AtomicU64::new(0)),
        }
    }
}

type Job = Result<Value, String>;

fn err(msg: impl std::fmt::Display) -> String {
    msg.to_string()
}

fn q(s: &str) -> QString {
    QString::from(s)
}

fn jstr(v: &Value) -> QString {
    QString::from(&v.to_string())
}

/// The private directory for previews: `<runtime dir or temp>/washi/gui-<pid>` (mode 0700).
fn preview_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join("washi");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&base)?;
    remove_stale_dirs(&base);
    let dir = base.join(format!("gui-{}", std::process::id()));
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
    Ok(dir)
}

/// Delete `gui-<pid>` directories of app instances that are no longer running (a crash leaves
/// its previews behind).
fn remove_stale_dirs(base: &Path) {
    let Ok(rd) = std::fs::read_dir(base) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(pid) = name.strip_prefix("gui-").and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if pid != std::process::id() && !Path::new(&format!("/proc/{pid}")).exists() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Remove this process's preview directory (called when the app quits).
pub fn cleanup() {
    if let Some(base) = dirs::runtime_dir().or_else(|| Some(std::env::temp_dir())) {
        let _ =
            std::fs::remove_dir_all(base.join("washi").join(format!("gui-{}", std::process::id())));
    }
}

impl Inner {
    /// A fresh file path for a preview; the oldest previews are deleted beyond [`KEEP_OUTPUTS`].
    fn out_path(&mut self, seq: u64, tag: &str, ext: &str) -> Result<PathBuf, String> {
        if self.dir.is_none() {
            self.dir = Some(preview_dir().map_err(|e| err(format!("no preview directory: {e}")))?);
        }
        let p = self.dir.clone().unwrap_or_default().join(format!("{tag}-{seq}.{ext}"));
        self.outputs.push_back(p.clone());
        while self.outputs.len() > KEEP_OUTPUTS {
            if let Some(old) = self.outputs.pop_front() {
                let _ = std::fs::remove_file(old);
            }
        }
        Ok(p)
    }

    fn small_photo(&mut self, path: &Path) -> Result<Arc<RgbImage>, String> {
        if let Some(p) = self.photos.get(path) {
            return Ok(p.clone());
        }
        let img = photo::load_photo(path).map_err(err)?;
        let long = img.width().max(img.height());
        let img = if long > 1600 {
            let f = 1600.0 / long as f32;
            image::imageops::resize(
                &img,
                ((img.width() as f32 * f).round() as u32).max(1),
                ((img.height() as f32 * f).round() as u32).max(1),
                image::imageops::FilterType::Triangle,
            )
        } else {
            img
        };
        let arc = Arc::new(img);
        self.photos.insert(path.to_path_buf(), arc.clone());
        self.photo_order.push_back(path.to_path_buf());
        while self.photo_order.len() > KEEP_PHOTOS {
            if let Some(old) = self.photo_order.pop_front() {
                self.photos.remove(&old);
            }
        }
        Ok(arc)
    }
}

fn save_png(img: &DynamicImage, path: &Path) -> Result<(), String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::{ExtendedColorType, ImageEncoder};
    let file =
        std::fs::File::create(path).map_err(|e| err(format!("cannot write a preview: {e}")))?;
    let enc = PngEncoder::new_with_quality(
        std::io::BufWriter::new(file),
        CompressionType::Fast,
        FilterType::Sub,
    );
    let (w, h) = (img.width(), img.height());
    match img {
        DynamicImage::ImageLuma8(g) => enc.write_image(g.as_raw(), w, h, ExtendedColorType::L8),
        other => enc.write_image(other.to_rgb8().as_raw(), w, h, ExtendedColorType::Rgb8),
    }
    .map_err(|e| err(format!("cannot encode a preview: {e}")))
}

/// Shrink so that the longest edge is at most `max_edge`.
fn fit_edge(img: DynamicImage, max_edge: u32) -> DynamicImage {
    let long = img.width().max(img.height());
    if max_edge == 0 || long <= max_edge {
        return img;
    }
    let f = max_edge as f32 / long as f32;
    img.resize_exact(
        ((img.width() as f32 * f).round() as u32).max(1),
        ((img.height() as f32 * f).round() as u32).max(1),
        image::imageops::FilterType::Triangle,
    )
}

fn parse_json(s: &QString) -> Result<Value, String> {
    serde_json::from_str(&s.to_string()).map_err(|e| err(format!("bad request: {e}")))
}

fn page_json(page: &Page) -> Value {
    let (w, h) = page.original_size();
    let (rw, rh) = page.rotated_size();
    json!({
        "width": w, "height": h, "rotated_width": rw, "rotated_height": rh,
        "dpi": page.dpi, "edited": page.is_edited(),
        "edits": serde_json::to_value(&page.edits).unwrap_or(Value::Null),
    })
}

fn mode_of(s: &str) -> OutputMode {
    OutputMode::parse(s).unwrap_or(OutputMode::Color)
}

fn index_of(i: i32) -> Result<usize, String> {
    usize::try_from(i).map_err(|_| err("no such page"))
}

impl qobject::Imaging {
    /// Run `work` on a thread; its result arrives as `done(id, ok, body)`.
    fn spawn_job<F>(self: Pin<&mut Self>, work: F) -> i32
    where
        F: FnOnce() -> Job + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let (ok, body) = match work() {
                Ok(v) => (true, v.to_string()),
                Err(m) => (false, json!({ "message": m }).to_string()),
            };
            let _ = qt.queue(move |mut o| o.as_mut().done(id, ok, QString::from(&body)));
        });
        id
    }

    fn shared(&self) -> (Arc<Mutex<Inner>>, Arc<AtomicU64>) {
        (self.inner.clone(), self.seq.clone())
    }

    // ---- environment ----------------------------------------------------------------------

    pub fn tools(&self) -> QString {
        let t = Tools::detect();
        jstr(&json!({
            "pdftoppm": t.pdftoppm.is_some(), "pdfinfo": t.pdfinfo.is_some(),
            "soffice": t.soffice.is_some(), "tesseract": t.tesseract.is_some(),
            "pdf": t.can_print_pdf(), "office": t.can_convert_office(), "ocr": t.can_ocr(),
        }))
    }

    pub fn data_dir(&self) -> QString {
        q(&data_dir().to_string_lossy())
    }

    pub fn home_dir(&self) -> QString {
        q(&dirs::home_dir().unwrap_or_default().to_string_lossy())
    }

    pub fn read_text(&self, path: &QString) -> QString {
        q(&std::fs::read_to_string(path.to_string()).unwrap_or_default())
    }

    pub fn write_text(&self, path: &QString, text: &QString) -> bool {
        let path = PathBuf::from(path.to_string());
        if let Some(dir) = path.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return false;
            }
        }
        std::fs::write(path, text.to_string()).is_ok()
    }

    pub fn list_files(&self, dir: &QString, ext: &QString) -> QString {
        let ext = ext.to_string();
        let mut files: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(dir.to_string())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let modified = e.metadata().ok()?.modified().ok()?;
                (e.metadata().ok()?.is_file() && name.ends_with(&ext)).then_some((modified, name))
            })
            .collect();
        files.sort_by(|a, b| b.cmp(a));
        jstr(&json!(files.into_iter().map(|f| f.1).collect::<Vec<_>>()))
    }

    pub fn remove_data_file(&self, path: &QString) -> bool {
        let p = PathBuf::from(path.to_string());
        let data = data_dir();
        let inside = p.starts_with(&data) && !p.components().any(|c| c.as_os_str() == "..");
        inside && p.is_file() && std::fs::remove_file(p).is_ok()
    }

    pub fn image_size(&self, path: &QString) -> QString {
        match image::image_dimensions(path.to_string()) {
            Ok((w, h)) => jstr(&json!({ "width": w, "height": h })),
            Err(_) => q(""),
        }
    }

    pub fn token(&self) -> QString {
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        q(&format!("{t:x}{n:x}"))
    }

    // ---- scan session -----------------------------------------------------------------------

    pub fn scan_reset(&self) {
        let mut g = lock(&self.inner);
        g.scan = ScanSession::new();
        g.work = None;
    }

    pub fn scan_add_page(self: Pin<&mut Self>, path: &QString, dpi: i32, mode: &QString) -> i32 {
        let (inner, _) = self.shared();
        let (path, mode) = (PathBuf::from(path.to_string()), mode_of(&mode.to_string()));
        let dpi = u32::try_from(dpi).unwrap_or(300).clamp(1, 2400);
        self.spawn_job(move || {
            let bytes = std::fs::read(&path)
                .map_err(|e| err(format!("cannot read the scanned page: {e}")))?;
            let mut page = Page::from_bytes(&bytes, dpi).map_err(err)?;
            page.set_correction(ImageCorrection { color_mode: mode, ..ImageCorrection::default() });
            let mut g = lock(&inner);
            let index = g.scan.add_page(page);
            Ok(json!({ "index": index, "count": g.scan.len() }))
        })
    }

    pub fn scan_count(&self) -> i32 {
        i32::try_from(lock(&self.inner).scan.len()).unwrap_or(0)
    }

    pub fn scan_page_info(&self, index: i32) -> QString {
        let g = lock(&self.inner);
        match index_of(index).ok().and_then(|i| g.scan.page(i)) {
            Some(p) => jstr(&page_json(p)),
            None => q(""),
        }
    }

    pub fn scan_rotate(&self, index: i32, dir: &QString) -> bool {
        let mut g = lock(&self.inner);
        let Some(p) = index_of(index).ok().and_then(|i| g.scan.page_mut(i)) else { return false };
        match dir.to_string().as_str() {
            "left" => p.rotate_left(),
            "right" => p.rotate_right(),
            _ => return false,
        }
        true
    }

    pub fn scan_delete(&self, index: i32) -> bool {
        let mut g = lock(&self.inner);
        g.work = None;
        index_of(index).ok().is_some_and(|i| g.scan.delete_page(i).is_ok())
    }

    pub fn scan_revert(&self, index: i32) -> bool {
        let mut g = lock(&self.inner);
        index_of(index).ok().is_some_and(|i| g.scan.revert_page(i).is_ok())
    }

    pub fn scan_render(self: Pin<&mut Self>, index: i32, max_edge: i32, mode: &QString) -> i32 {
        let (inner, seq) = self.shared();
        let mode = mode.to_string();
        let max_edge = u32::try_from(max_edge).unwrap_or(0);
        self.spawn_job(move || {
            let page = {
                let g = lock(&inner);
                match mode.as_str() {
                    "page" => index_of(index)
                        .ok()
                        .and_then(|i| g.scan.page(i))
                        .cloned()
                        .ok_or_else(|| err("no such page"))?,
                    "work" | "work-geometry" => {
                        g.work.as_ref().map(|w| w.page.clone()).ok_or_else(|| err("no page is being edited"))?
                    }
                    other => return Err(err(format!("unknown render mode {other:?}"))),
                }
            };
            let mut page = page;
            if mode == "work-geometry" {
                // Only what the Rotate/Crop screen draws: turned and de-slanted.
                page.edits.crop = None;
                page.edits.correction = ImageCorrection {
                    color_mode: OutputMode::Color,
                    ..ImageCorrection::default()
                };
            }
            let img = page.render_preview(max_edge.max(16));
            let n = seq.fetch_add(1, Ordering::Relaxed);
            let tag = if mode == "page" { format!("p{index}") } else { "w".to_string() };
            let path = lock(&inner).out_path(n, &tag, "png")?;
            save_png(&img, &path)?;
            Ok(json!({ "path": path.to_string_lossy(), "width": img.width(), "height": img.height() }))
        })
    }

    // ---- edit screens ----------------------------------------------------------------------

    pub fn edit_begin(&self, index: i32) -> QString {
        let mut g = lock(&self.inner);
        let Some(i) = index_of(index).ok().filter(|i| *i < g.scan.len()) else { return q("") };
        let page = g.scan.pages()[i].clone();
        g.work = Some(Work { index: i, page });
        jstr(&work_state(g.work.as_ref()))
    }

    pub fn edit_state(&self) -> QString {
        jstr(&work_state(lock(&self.inner).work.as_ref()))
    }

    pub fn edit_rotate(&self, dir: &QString) -> QString {
        self.with_work(|p| match dir.to_string().as_str() {
            "left" => p.rotate_left(),
            "right" => p.rotate_right(),
            _ => {}
        })
    }

    pub fn edit_slant(&self, degrees: f64) -> QString {
        self.with_work(|p| p.set_slant(degrees as f32))
    }

    pub fn edit_crop(&self, x: f64, y: f64, w: f64, h: f64) -> QString {
        self.with_work(|p| {
            p.set_crop(Some(CropRect { x: x as f32, y: y as f32, w: w as f32, h: h as f32 }))
        })
    }

    pub fn edit_clear_crop(&self) -> QString {
        self.with_work(|p| p.set_crop(None))
    }

    pub fn edit_aspect(&self, preset: &QString) -> QString {
        let preset = AspectPreset::parse(&preset.to_string());
        self.with_work(|p| {
            if let Some(a) = preset {
                p.set_aspect(a);
            }
        })
    }

    pub fn edit_correction(&self, json: &QString) -> QString {
        let parsed = serde_json::from_str::<ImageCorrection>(&json.to_string());
        self.with_work(|p| {
            if let Ok(c) = parsed {
                p.set_correction(c);
            }
        })
    }

    pub fn edit_auto_deskew(self: Pin<&mut Self>) -> i32 {
        let (inner, _) = self.shared();
        self.spawn_job(move || {
            let page = lock(&inner)
                .work
                .as_ref()
                .map(|w| w.page.clone())
                .ok_or_else(|| err("no page is being edited"))?;
            let mut page = page;
            let angle = page.auto_deskew();
            let mut g = lock(&inner);
            if let Some(w) = g.work.as_mut() {
                w.page.set_slant(angle);
            }
            Ok(json!({ "slant": angle }))
        })
    }

    pub fn edit_commit(&self) -> bool {
        let mut g = lock(&self.inner);
        match g.work.take() {
            Some(w) => match g.scan.page_mut(w.index) {
                Some(slot) => {
                    *slot = w.page;
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    pub fn edit_cancel(&self) {
        lock(&self.inner).work = None;
    }

    /// Apply `f` to the working copy and return its state.
    fn with_work(&self, f: impl FnOnce(&mut Page)) -> QString {
        let mut g = lock(&self.inner);
        if let Some(w) = g.work.as_mut() {
            f(&mut w.page);
        }
        jstr(&work_state(g.work.as_ref()))
    }

    // ---- save and extras -----------------------------------------------------------------------

    pub fn scan_save(self: Pin<&mut Self>, json: &QString) -> i32 {
        let (inner, _) = self.shared();
        let req = parse_json(json);
        self.spawn_job(move || save_session(&inner, &req?))
    }

    pub fn scan_extra(self: Pin<&mut Self>, json: &QString) -> i32 {
        let (inner, _) = self.shared();
        let req = parse_json(json);
        self.spawn_job(move || run_extra(&inner, &req?))
    }

    // ---- previews ------------------------------------------------------------------------------

    pub fn photo_preview(self: Pin<&mut Self>, json: &QString) -> i32 {
        let (inner, seq) = self.shared();
        let req = parse_json(json);
        self.spawn_job(move || {
            let img = photo_sheet(&inner, &req?)?;
            let n = seq.fetch_add(1, Ordering::Relaxed);
            let path = lock(&inner).out_path(n, "sheet", "png")?;
            save_png(&img, &path)?;
            Ok(json!({ "path": path.to_string_lossy(), "width": img.width(), "height": img.height() }))
        })
    }

    pub fn disc_preview(self: Pin<&mut Self>, json: &QString, px: i32) -> i32 {
        let (inner, seq) = self.shared();
        let req = parse_json(json);
        let px = u32::try_from(px).unwrap_or(700).clamp(64, 1400);
        self.spawn_job(move || {
            let img = disc_image(&req?, px)?;
            let n = seq.fetch_add(1, Ordering::Relaxed);
            let path = lock(&inner).out_path(n, "disc", "png")?;
            save_png(&DynamicImage::ImageRgb8(img.clone()), &path)?;
            Ok(json!({ "path": path.to_string_lossy(), "width": img.width(), "height": img.height() }))
        })
    }

    pub fn pdf_page(self: Pin<&mut Self>, path: &QString, page: i32, max_edge: i32) -> i32 {
        let (inner, seq) = self.shared();
        let path = PathBuf::from(path.to_string());
        let page = u32::try_from(page).unwrap_or(1).max(1);
        let max_edge = u32::try_from(max_edge).unwrap_or(900).clamp(64, 4000);
        self.spawn_job(move || {
            let tools = Tools::detect();
            let exe = tools.pdftoppm.ok_or_else(|| err("pdftoppm (poppler) is not installed"))?;
            let dir = tempfile::tempdir().map_err(err)?;
            let base = dir.path().join("p");
            let status = std::process::Command::new(exe)
                .args(["-png", "-singlefile", "-f", &page.to_string(), "-l", &page.to_string()])
                .args(["-scale-to", &max_edge.to_string()])
                .arg(&path)
                .arg(&base)
                .status()
                .map_err(err)?;
            if !status.success() {
                return Err(err("poppler could not render the page"));
            }
            let img = image::open(base.with_extension("png")).map_err(err)?;
            let n = seq.fetch_add(1, Ordering::Relaxed);
            let out = lock(&inner).out_path(n, "pdf", "png")?;
            save_png(&img, &out)?;
            Ok(json!({ "path": out.to_string_lossy(), "width": img.width(), "height": img.height() }))
        })
    }

    pub fn pdf_pages(&self, path: &QString) -> i32 {
        let tools = Tools::detect();
        print::document::pdf_info(&tools, Path::new(&path.to_string()))
            .map_or(0, |i| i32::try_from(i.pages).unwrap_or(0))
    }
}

/// Save pages of the session: `{"path", "format", "range", "current", "single_file", "lang"}`.
fn save_session(inner: &Mutex<Inner>, req: &Value) -> Job {
    let path = PathBuf::from(req["path"].as_str().ok_or_else(|| err("no file name"))?);
    let format = req["format"].as_str().unwrap_or("pdf").to_string();
    let single = req["single_file"].as_bool().unwrap_or(true);
    let current = req["current"].as_u64().unwrap_or(0) as usize;
    let range = if req["range"].as_str() == Some("current") {
        SaveRange::Current(current)
    } else {
        SaveRange::All
    };
    let lang = req["lang"].as_str().unwrap_or("eng").to_string();
    let session = lock(inner).scan.clone();
    let files: Vec<PathBuf> = match format.as_str() {
        "pdf" | "jpeg" => {
            let opts = SaveOptions {
                format: if format == "pdf" { SaveFormat::Pdf } else { SaveFormat::Jpeg },
                range,
                single_file: single,
                jpeg_quality: 90,
            };
            save::save(&session, &path, &opts).map_err(err)?
        }
        "pdf-ocr" | "txt" => {
            let tools = Tools::detect();
            let idx = save::select_pages(&session, &range).map_err(err)?;
            let images: Vec<DynamicImage> =
                idx.iter().map(|i| session.pages()[*i].render()).collect();
            let dpi = session.pages()[idx[0]].dpi;
            if format == "pdf-ocr" {
                let bytes = ocr::searchable_pdf(&images, &lang, dpi, &tools).map_err(err)?;
                let out = save::page_path(&path, SaveFormat::Pdf, 1, 1);
                write_all(&out, &bytes)?;
                vec![out]
            } else {
                let mut text = String::new();
                for (n, img) in images.iter().enumerate() {
                    if n > 0 {
                        text.push_str("\n\u{c}\n");
                    }
                    text.push_str(&ocr::ocr_text(img, &lang, dpi, &tools).map_err(err)?);
                }
                let out = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt")) {
                    path.clone()
                } else {
                    let mut s = path.clone().into_os_string();
                    s.push(".txt");
                    PathBuf::from(s)
                };
                write_all(&out, text.as_bytes())?;
                vec![out]
            }
        }
        other => return Err(err(format!("unknown format {other:?}"))),
    };
    Ok(json!({ "files": files.iter().map(|f| f.to_string_lossy()).collect::<Vec<_>>() }))
}

/// Host-side extras on the session (`classify`, `crop-photos`, `stitch`).
fn run_extra(inner: &Mutex<Inner>, req: &Value) -> Job {
    let op = req["op"].as_str().unwrap_or("").to_string();
    let get = |k: &str| -> Result<usize, String> {
        req[k].as_u64().map(|v| v as usize).ok_or_else(|| err(format!("missing {k}")))
    };
    match op.as_str() {
        "classify" => {
            let page =
                lock(inner).scan.page(get("index")?).cloned().ok_or_else(|| err("no such page"))?;
            let kind = match detect::classify(&page.original().to_rgb8()) {
                ScanKind::Blank => "blank",
                ScanKind::Document => "document",
                ScanKind::Photo => "photo",
            };
            Ok(json!({ "kind": kind }))
        }
        "crop-photos" => {
            let i = get("index")?;
            let page = lock(inner).scan.page(i).cloned().ok_or_else(|| err("no such page"))?;
            let photos = detect::crop_photos(&page.original().to_rgb8());
            if photos.is_empty() {
                return Err(err("no photo was found on the glass"));
            }
            let mut g = lock(inner);
            let _ = g.scan.delete_page(i);
            let n = photos.len();
            for (k, img) in photos.into_iter().enumerate() {
                let mut p = Page::new(DynamicImage::ImageRgb8(img), page.dpi);
                p.set_correction(page.edits.correction);
                // Keep the pages where the glass page was.
                let at = g.scan.add_page(p);
                let _ = g.scan.move_page(at, (i + k).min(at));
            }
            Ok(json!({ "added": n, "count": g.scan.len(), "first": i }))
        }
        "stitch" => {
            let (a, b) = (get("a")?, get("b")?);
            let dir = match req["direction"].as_str() {
                Some("top-bottom") => StitchDirection::TopBottom,
                _ => StitchDirection::LeftRight,
            };
            let (pa, pb) = {
                let g = lock(inner);
                (
                    g.scan.page(a).cloned().ok_or_else(|| err("no such page"))?,
                    g.scan.page(b).cloned().ok_or_else(|| err("no such page"))?,
                )
            };
            let joined = stitch::stitch(
                &pa.original().to_rgb8(),
                &pb.original().to_rgb8(),
                dir,
                &StitchOptions::default(),
            );
            let mut page = Page::new(DynamicImage::ImageRgb8(joined), pa.dpi);
            page.set_correction(pa.edits.correction);
            let mut g = lock(inner);
            let (lo, hi) = (a.min(b), a.max(b));
            let _ = g.scan.delete_page(hi);
            let _ = g.scan.delete_page(lo);
            let at = g.scan.add_page(page);
            let _ = g.scan.move_page(at, lo.min(at));
            Ok(json!({ "index": lo, "count": g.scan.len() }))
        }
        other => Err(err(format!("unknown operation {other:?}"))),
    }
}

/// The photo sheet of a print preview: `{"files", "size", "borderless", "layout", "orientation",
/// "max_edge"}` -> the sheet as it will be printed, shown the way the user holds it.
fn photo_sheet(inner: &Mutex<Inner>, req: &Value) -> Result<DynamicImage, String> {
    let files: Vec<PathBuf> = req["files"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(PathBuf::from)).collect())
        .unwrap_or_default();
    if files.is_empty() {
        return Err(err("no photo"));
    }
    let paper = *PaperSize::parse(req["size"].as_str().unwrap_or("a4")).map_err(err)?;
    let layout = u8::try_from(req["layout"].as_u64().unwrap_or(1)).unwrap_or(1);
    let orientation =
        Orientation::parse(req["orientation"].as_str().unwrap_or("auto")).map_err(err)?;
    let fill = req["borderless"].as_bool().unwrap_or(false);
    let max_edge = req["max_edge"].as_u64().unwrap_or(640) as u32;
    let wanted = if layout == 1 { 1 } else { usize::from(layout) };
    let mut photos = Vec::new();
    for f in files.iter().take(wanted) {
        let small = lock(inner).small_photo(f)?;
        photos.push((*small).clone());
    }
    let canvas = if layout == 1 {
        photo::compose_single_oriented(&photos[0], &paper, fill, orientation)
    } else {
        photo::compose_sheet_at(&photos, &paper, layout, orientation, 60)
    };
    // The canvas is the paper as the printer takes it (portrait); a landscape sheet was turned
    // onto it. The preview shows the sheet the way the user holds it.
    let canvas =
        if orientation.is_landscape() { image::imageops::rotate270(&canvas) } else { canvas };
    Ok(fit_edge(DynamicImage::ImageRgb8(canvas), max_edge))
}

/// The disc label of a preview: the `PrintDisc` JSON (without `confirm`), `px` pixels square.
fn disc_image(req: &Value, px: u32) -> Result<RgbImage, String> {
    let mut spec = req.clone();
    if let Some(o) = spec.as_object_mut() {
        o.remove("px");
    }
    let spec = DiscSpec::from_json(&spec.to_string()).map_err(err)?;
    let (label, _) = spec.to_label().map_err(err)?;
    label.render_sized(px).map_err(err)
}

fn work_state(w: Option<&Work>) -> Value {
    match w {
        Some(w) => {
            let (rw, rh) = w.page.rotated_size();
            json!({
                "index": w.index, "rotated_width": rw, "rotated_height": rh,
                "edits": serde_json::to_value(&w.page.edits).unwrap_or(Value::Null),
            })
        }
        None => Value::Null,
    }
}

fn write_all(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| err(format!("cannot create {}: {e}", dir.display())))?;
    }
    std::fs::write(path, bytes).map_err(|e| err(format!("cannot write {}: {e}", path.display())))
}

/// `~/.local/share/washi` (honours `XDG_DATA_HOME`).
fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("washi")
}

#[cfg(test)]
mod tests {
    use washi_core::scan::ops::Rotation;

    use super::*;

    #[test]
    fn previews_get_new_names_and_old_ones_are_deleted() {
        let mut inner = Inner::default();
        let first = inner.out_path(1, "t", "png").unwrap();
        std::fs::write(&first, b"x").unwrap();
        assert!(first.exists());
        for n in 2..(KEEP_OUTPUTS as u64 + 5) {
            let p = inner.out_path(n, "t", "png").unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        assert!(!first.exists(), "the oldest preview was removed");
        assert_eq!(inner.outputs.len(), KEEP_OUTPUTS);
        cleanup();
        assert!(!inner.dir.clone().unwrap().exists());
    }

    fn gradient_file(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let p = dir.join(name);
        RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 80]))
            .save(&p)
            .unwrap();
        p
    }

    #[test]
    fn the_photo_sheet_preview_follows_size_orientation_and_layout() {
        let dir = tempfile::tempdir().unwrap();
        let wide = gradient_file(dir.path(), "wide.png", 900, 600);
        let inner = Mutex::new(Inner::default());
        let req = |orientation: &str, layout: u8, files: Vec<&Path>| {
            json!({ "files": files, "size": "l", "borderless": true, "layout": layout,
                    "orientation": orientation, "max_edge": 400 })
        };
        // L paper is 89 x 127 mm: portrait is taller than wide, landscape the other way round.
        let p = photo_sheet(&inner, &req("portrait", 1, vec![&wide])).unwrap();
        assert!(p.height() > p.width());
        let l = photo_sheet(&inner, &req("landscape", 1, vec![&wide])).unwrap();
        assert!(l.width() > l.height(), "{}x{}", l.width(), l.height());
        assert!(l.width().max(l.height()) <= 400);
        // The landscape sheet shows the photo upright: red grows to the right.
        let (w, cy) = (l.width(), l.height() / 2);
        assert!(
            l.to_rgb8().get_pixel(w * 4 / 5, cy).0[0] > l.to_rgb8().get_pixel(w / 5, cy).0[0] + 60
        );
        // n-up: the first `layout` photos only.
        let two = photo_sheet(&inner, &req("portrait", 2, vec![&wide, &wide, &wide])).unwrap();
        assert!(two.height() > two.width());
        assert!(photo_sheet(&inner, &req("portrait", 1, vec![])).is_err());
        assert!(photo_sheet(&inner, &json!({ "files": [wide], "size": "a3" })).is_err());
        assert!(photo_sheet(&inner, &json!({ "files": ["/no/such.jpg"], "size": "l" })).is_err());
        // The decoded photo is kept for the next preview.
        assert_eq!(lock(&inner).photos.len(), 1);
    }

    #[test]
    fn the_disc_preview_is_the_masked_ring() {
        let spec =
            json!({ "inner_mm": 22, "outer_mm": 116, "background_color": "#ff0000", "media": "a" });
        let img = disc_image(&spec, 240).unwrap();
        assert_eq!(img.dimensions(), (240, 240));
        // 240 px for 120 mm: 2 px per mm. The ring is red; the hub and the corners are white.
        assert_eq!(img.get_pixel(120, 120).0, [255, 255, 255], "hub");
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255], "corner");
        assert_eq!(img.get_pixel(120, 20).0, [255, 0, 0], "ring");
        assert!(disc_image(&json!({ "inner_mm": 5 }), 240).is_err(), "out of range");
        assert!(disc_image(&json!({ "bogus": 1 }), 240).is_err(), "typos are errors");
    }

    #[test]
    fn the_disc_preview_sets_ring_text_on_the_ring() {
        if washi_core::disc::find_font("Washi").is_none() {
            return;
        }
        let spec = json!({
            "texts": [{
                "text": "WASHI WASHI", "size_pt": 20, "color": "#ff0000", "align": "center",
                "ring": { "radius_mm": 46, "start_deg": 0, "clockwise": true }
            }]
        });
        let img = disc_image(&spec, 480).unwrap();
        // 480 px for 120 mm: 4 px per mm, centre at 240. Red ink lies on the circle of radius
        // 46 mm and above (outwards), and nothing in the bottom half.
        let mut n = 0;
        for (x, y, p) in img.enumerate_pixels() {
            if p.0[0] > 200 && p.0[1] < 90 && p.0[2] < 90 {
                n += 1;
                let (dx, dy) = (f64::from(x) + 0.5 - 240.0, f64::from(y) + 0.5 - 240.0);
                assert!(dx.hypot(dy) / 4.0 > 45.0, "ink inside the baseline circle: {dx},{dy}");
                assert!(dy < 0.0, "ink in the bottom half: {dx},{dy}");
            }
        }
        assert!(n > 150, "ink: {n}");
        let bad = json!({ "texts": [{ "text": "x", "ring": { "radius_mm": 99 } }] });
        assert!(disc_image(&bad, 240).is_err(), "a radius outside the disc is refused");
    }

    fn session_with(pages: Vec<RgbImage>) -> Mutex<Inner> {
        let mut inner = Inner::default();
        for p in pages {
            inner.scan.add_page(Page::new(DynamicImage::ImageRgb8(p), 100));
        }
        Mutex::new(inner)
    }

    #[test]
    fn saving_writes_the_requested_files() {
        let dir = tempfile::tempdir().unwrap();
        let page = |v: u8| RgbImage::from_pixel(60, 40, image::Rgb([v, v, v]));
        let inner = session_with(vec![page(200), page(120), page(40)]);
        let base = dir.path().join("out");
        let save = |extra: Value| {
            let mut req = json!({ "path": base.to_string_lossy() });
            req.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            save_session(&inner, &req)
        };
        let r = save(json!({ "format": "pdf", "range": "all", "single_file": true })).unwrap();
        let files = r["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].as_str().unwrap().ends_with("out.pdf"));
        assert!(std::fs::read(dir.path().join("out.pdf")).unwrap().starts_with(b"%PDF"));
        let r = save(json!({ "format": "jpeg", "range": "all" })).unwrap();
        assert_eq!(r["files"].as_array().unwrap().len(), 3, "one JPEG per page");
        assert!(dir.path().join("out_003.jpg").exists());
        let r = save(json!({ "format": "jpeg", "range": "current", "current": 1 })).unwrap();
        assert_eq!(r["files"].as_array().unwrap().len(), 1);
        // Mistakes are errors, and nothing is written for them.
        assert!(save(json!({ "format": "tiff" })).is_err());
        assert!(save(json!({ "format": "pdf", "range": "current", "current": 9 })).is_err());
        assert!(save_session(&inner, &json!({ "format": "pdf" })).is_err(), "no file name");
    }

    #[test]
    fn extras_cut_out_photos_stitch_and_classify() {
        // A grey platen with two coloured "photos".
        let mut platen = RgbImage::from_pixel(600, 400, image::Rgb([250, 250, 250]));
        for (x0, c) in [(40u32, [200u8, 30, 30]), (330, [30, 60, 200])] {
            for y in 60..300 {
                for x in x0..x0 + 220 {
                    let shade = ((x + y) % 20) as u8;
                    platen.put_pixel(x, y, image::Rgb([c[0].saturating_sub(shade), c[1], c[2]]));
                }
            }
        }
        let inner = session_with(vec![platen]);
        let r = run_extra(&inner, &json!({ "op": "classify", "index": 0 })).unwrap();
        assert_eq!(r["kind"], "photo");
        let r = run_extra(&inner, &json!({ "op": "crop-photos", "index": 0 })).unwrap();
        assert_eq!(r["added"], 2);
        assert_eq!(lock(&inner).scan.len(), 2, "the glass page became one page per photo");

        // Stitch two halves: the result replaces both.
        let left = RgbImage::from_pixel(100, 80, image::Rgb([10, 200, 10]));
        let right = RgbImage::from_pixel(100, 80, image::Rgb([200, 10, 10]));
        let inner = session_with(vec![left, right]);
        let r = run_extra(
            &inner,
            &json!({ "op": "stitch", "a": 0, "b": 1, "direction": "left-right" }),
        )
        .unwrap();
        assert_eq!(r["count"], 1);
        let joined = lock(&inner).scan.page(0).unwrap().original().to_rgb8();
        assert!(joined.width() > 100 && joined.height() == 80, "{:?}", joined.dimensions());
        assert!(run_extra(&inner, &json!({ "op": "stitch", "a": 0, "b": 5 })).is_err());
        assert!(run_extra(&inner, &json!({ "op": "nope" })).is_err());
        assert!(run_extra(&inner, &json!({ "op": "classify" })).is_err(), "missing index");
    }

    #[test]
    fn the_working_copy_is_kept_apart_from_the_page_until_it_is_committed() {
        let inner = session_with(vec![RgbImage::from_pixel(60, 40, image::Rgb([9, 9, 9]))]);
        let mut g = lock(&inner);
        let page = g.scan.pages()[0].clone();
        g.work = Some(Work { index: 0, page });
        g.work.as_mut().unwrap().page.rotate_right();
        assert_eq!(g.scan.pages()[0].edits.rotation, Rotation::None, "the page is untouched");
        let state = work_state(g.work.as_ref());
        assert_eq!(state["rotated_width"], 40, "the working copy is turned");
        assert_eq!(state["edits"]["rotation"], "Right");
        assert_eq!(work_state(None), Value::Null);
    }

    #[test]
    fn stale_preview_directories_of_dead_instances_are_removed() {
        let base = tempfile::tempdir().unwrap();
        let dead = base.path().join("gui-4194000");
        let mine = base.path().join(format!("gui-{}", std::process::id()));
        let other = base.path().join("scan-1-1");
        for d in [&dead, &mine, &other] {
            std::fs::create_dir(d).unwrap();
            std::fs::write(d.join("x.png"), b"x").unwrap();
        }
        remove_stale_dirs(base.path());
        assert!(!dead.exists(), "no process 4194000 is running");
        assert!(mine.exists() && other.exists(), "ours and the daemon's are left alone");
    }

    #[test]
    fn png_previews_round_trip_for_colour_and_grey() {
        let dir = tempfile::tempdir().unwrap();
        let rgb = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 4, image::Rgb([1, 2, 3])));
        let p = dir.path().join("a.png");
        save_png(&rgb, &p).unwrap();
        assert_eq!(image::open(&p).unwrap().to_rgb8().get_pixel(3, 2).0, [1, 2, 3]);
        let grey = DynamicImage::ImageLuma8(image::GrayImage::from_pixel(5, 5, image::Luma([77])));
        save_png(&grey, &p).unwrap();
        assert_eq!(image::open(&p).unwrap().to_luma8().get_pixel(1, 1).0, [77]);
        assert_eq!(fit_edge(rgb, 4).width(), 4);
    }
}
