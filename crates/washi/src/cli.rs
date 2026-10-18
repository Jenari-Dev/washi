// SPDX-License-Identifier: GPL-2.0-or-later
//! Command-line definition.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use washi_core::ivec::{DeviceSettings, InkGroup, OnOff};
use washi_core::{parse_auto_power_off, parse_quiet};

#[derive(Parser, Debug)]
#[command(
    name = "washi",
    version,
    about = "Washi: open-source Canon PRINT replacement for Canon CHMP/IVEC inkjet printers",
    long_about = "Washi talks to Canon inkjet printers over their CHMP/IVEC management channel (status, \
        maintenance), IPP (printing) and eSCL (scanning).\n\n\
        Reads (status, info, settings, caps, raw, discover, scanner, media, jobs, tools, doctor) are always \
        safe. Commands that change the printer or move paper or the scanner (maint, set, power, \
        scan, print, disc, identify, cancel) only print the exact request they would send unless \
        --confirm is given."
)]
pub struct Cli {
    /// Printer host name or address (`host`, `host:port`). Default: the saved host, else the
    /// first printer found by mDNS.
    #[arg(long, global = true, value_name = "HOST")]
    pub host: Option<String>,

    /// JSON output (read commands).
    #[arg(long, global = true)]
    pub json: bool,

    /// Actually perform a write command (maintenance, setting, scan, print, ...). Without it, a
    /// dry run prints the request that would be sent and opens no connection.
    #[arg(long, global = true)]
    pub confirm: bool,

    /// Open a new TCP connection for every request instead of keeping one alive.
    #[arg(long, global = true)]
    pub reconnect: bool,

    /// Log HTTP-level exchanges (status codes and sizes) to stderr.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// IPP port of the printer (default 631); for tests against a local fake printer.
    #[arg(long, global = true, hide = true, env = "WASHI_IPP_PORT", value_name = "PORT")]
    pub ipp_port: Option<u16>,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Find printers on the network (mDNS `_canon-chmp._tcp`).
    Discover {
        /// Seconds to listen.
        #[arg(long, default_value_t = 3)]
        timeout: u64,
    },
    /// Printer state, ink levels and paper per tray (works while the printer sleeps).
    Status,
    /// Follow the printer: print a line whenever something changes (Ctrl+C to stop).
    Watch {
        /// Seconds between polls.
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(2..))]
        interval: u64,
        /// Stop after N readings (for scripts and tests).
        #[arg(long, hide = true)]
        count: Option<u32>,
        /// Poll the printer directly even when washid is running.
        #[arg(long)]
        no_daemon: bool,
    },
    /// Model, firmware and lifetime counters.
    Info,
    /// Current device settings and their allowed values.
    Settings,
    /// Capabilities of a service (device, maintenance, print, ...).
    Caps {
        /// IVEC service type.
        service: String,
    },
    /// Print the raw IVEC reply for GetStatus, GetCapability or GetConfiguration.
    Raw {
        /// GetStatus, GetCapability or GetConfiguration.
        op: String,
        /// IVEC service type.
        service: String,
        /// Do not blank serial numbers, hardware addresses and cloud identifiers.
        #[arg(long)]
        no_redact: bool,
    },
    /// Maintenance: cleaning, nozzle check, head alignment, roller cleaning.
    Maint {
        #[command(subcommand)]
        cmd: MaintCmd,
    },
    /// Change a device setting.
    Set {
        #[command(subcommand)]
        cmd: SetCmd,
    },
    /// Power control.
    Power {
        #[command(subcommand)]
        cmd: PowerCmd,
    },
    /// Firmware version, whether the printer offers an update, and the update itself
    /// (UNVERIFIED on a real printer). `update` needs --confirm.
    Firmware {
        #[command(subcommand)]
        cmd: Option<FirmwareCmd>,
    },
    /// The web pages the printer points to (ink shop, manual, help for a support code) and its
    /// own web page. Read-only; nothing is opened.
    Links {
        /// Find the help page for this support code.
        #[arg(long, value_name = "CODE")]
        code: Option<String>,
        /// Language of the maker's pages.
        #[arg(long, value_enum, default_value = "en")]
        lang: LinkLang,
    },
    /// Show or edit washi's own configuration file (does not touch the printer).
    Config {
        #[command(subcommand)]
        cmd: Option<ConfigCmd>,
    },
    /// Scan with the flatbed (eSCL). Dry run unless --confirm.
    Scan(Box<ScanArgs>),
    /// Print photos and documents (IPP). Dry run unless --confirm.
    Print(Box<PrintArgs>),
    /// Print a label on a printable disc (CD/DVD) through the disc tray. Dry run unless --confirm.
    Disc(Box<DiscArgs>),
    /// "Find my printer": make the printer flash or beep (IPP Identify-Printer). Needs --confirm.
    Identify {
        #[arg(long, value_enum, default_value = "flash")]
        action: IdentifyArg,
    },
    /// Print jobs the printer is working on (read-only).
    Jobs,
    /// Cancel a print job by its id (see `washi jobs`). Needs --confirm.
    Cancel { job: i32 },
    /// What paper the printer offers: sources, sizes, types, borderless, disc tray (read-only).
    Media {
        /// Only this source: auto, rear, cassette or disc.
        #[arg(long, value_enum)]
        source: Option<SourceArg>,
        /// Show the media types offered for this paper size (e.g. a4, l, 2l).
        #[arg(long)]
        size: Option<String>,
    },
    /// What the scanner can do and its state (eSCL capabilities and status; read-only).
    Scanner,
    /// Which optional helper programs are installed (poppler, LibreOffice, tesseract).
    Tools,
    /// Check the setup: the versions of washi, washid and the app, how the service starts, the
    /// printer's channels, helper programs, the notification service, locales and paths. Read-only;
    /// each line is a tick, a cross or a dash with a hint (`--json` for scripts).
    Doctor,
    /// Show a sample desktop notification through the running washid (does not touch the
    /// printer): finds out whether notifications reach your desktop.
    NotifyTest,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentifyArg {
    Flash,
    Sound,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceArg {
    Auto,
    Rear,
    Cassette,
    Disc,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityArg {
    Draft,
    Normal,
    High,
}

impl QualityArg {
    pub fn name(self) -> &'static str {
        match self {
            QualityArg::Draft => "draft",
            QualityArg::Normal => "normal",
            QualityArg::High => "high",
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplexArg {
    Long,
    Short,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageOrderArg {
    Normal,
    Reverse,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrientationArg {
    Auto,
    Portrait,
    Landscape,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeArg {
    Color,
    Gray,
    Bw,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindArg {
    Document,
    Photo,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatArg {
    Pdf,
    Jpeg,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrectArg {
    Document,
    Photo,
    Magazine,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum StitchArg {
    LeftRight,
    TopBottom,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscMediaArg {
    A,
    B,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitArg {
    Cover,
    Contain,
}

/// `washi scan`
#[derive(Args, Debug)]
pub struct ScanArgs {
    /// Original size: a4, letter, a5, b5, l, 2l, postcard, card, or auto (scan the whole platen
    /// and crop the object on the host).
    #[arg(long, default_value = "a4", value_name = "SIZE")]
    pub size: String,
    /// Resolution in dpi: 75, 100, 150, 200, 300, 600 or 1200.
    #[arg(long, default_value_t = 300, value_name = "DPI")]
    pub res: u32,
    #[arg(long, value_enum, default_value = "color")]
    pub mode: ModeArg,
    /// Original type (sets the scanner's Document / Photo intent).
    #[arg(long = "type", value_enum, default_value = "document")]
    pub kind: KindArg,
    #[arg(long, value_enum, default_value = "pdf")]
    pub format: FormatArg,
    /// Output file. With several pages and JPEG (or --split) the pages get _001, _002, ...
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,
    /// Scan this many pages ("+ Next scan"): you are asked to change the original in between.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=200))]
    pub pages: u32,
    /// One file per page instead of one PDF.
    #[arg(long)]
    pub split: bool,
    /// Colour correction: for a document (whiten the paper), a photo (auto levels) or a magazine
    /// (remove the print dots).
    #[arg(long, value_enum)]
    pub correct: Option<CorrectArg>,
    /// Straighten a crooked page automatically (slant correction).
    #[arg(long)]
    pub deskew: bool,
    /// Reduce show-through of the page behind.
    #[arg(long)]
    pub show_through: bool,
    /// Unsharp mask strength, 0 to 100.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100), value_name = "PERCENT")]
    pub sharpen: Option<u8>,
    /// Turn every page: 90, 180, 270 (clockwise), left or right.
    #[arg(long, value_name = "DEGREES")]
    pub rotate: Option<String>,
    /// Trim every page to an aspect ratio: square, 3:2, 4:3, 7:5, 10:8, 16:9.
    #[arg(long, value_name = "RATIO")]
    pub aspect: Option<String>,
    /// Do not send the eSCL Intent (use if the scanner rejects the request).
    #[arg(long)]
    pub no_intent: bool,
    /// Cut every photo on the platen out and save each one (multi-photo crop).
    #[arg(long)]
    pub crop_photos: bool,
    /// Auto scan: scan the platen, decide document or photo, and process accordingly.
    #[arg(long, conflicts_with_all = ["crop_photos", "stitch"])]
    pub auto: bool,
    /// Scan two halves of a big original and join them.
    #[arg(long, value_enum)]
    pub stitch: Option<StitchArg>,
    /// Recognise text with tesseract (language, default eng). Writes <output>.txt, and with
    /// PDF output a searchable PDF.
    #[arg(long, num_args = 0..=1, default_missing_value = "eng", value_name = "LANG")]
    pub ocr: Option<String>,
}

/// `washi print`
#[derive(Args, Debug)]
pub struct PrintArgs {
    /// Files to print: JPEG / PNG / BMP photos, PDF, or Office documents (needs LibreOffice).
    #[arg(required = true, value_name = "FILE")]
    pub files: Vec<PathBuf>,
    /// Paper size: a4, a5, b5, letter, legal, l, 2l, 4x6, postcard, 8x10, card, dl, ...
    #[arg(long, default_value = "a4", value_name = "SIZE")]
    pub size: String,
    /// Media type: plain, photo, glossy, matte, luster, ..., or a raw keyword such as
    /// com.canon-mtluster. Default: plain for documents, photo for images.
    #[arg(long, value_name = "TYPE")]
    pub media_type: Option<String>,
    #[arg(long, value_enum, default_value = "auto")]
    pub source: SourceArg,
    /// Print without margins (photo papers; the printer must offer a borderless entry).
    #[arg(long)]
    pub borderless: bool,
    #[arg(long, value_enum, default_value = "normal")]
    pub quality: QualityArg,
    /// Print in colour (the default).
    #[arg(long, conflicts_with = "mono")]
    pub color: bool,
    /// Print in black and white.
    #[arg(long)]
    pub mono: bool,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=99))]
    pub copies: u32,
    /// Pages to print (documents), e.g. 1-3,5.
    #[arg(long, value_name = "PAGES")]
    pub pages: Option<String>,
    /// Pages (or photos) per sheet: 1, 2, 4, 6, 9 or 16.
    #[arg(long, default_value_t = 1, value_name = "N")]
    pub layout: u8,
    /// Two-sided printing, binding on the long or the short edge (documents).
    #[arg(long, value_enum)]
    pub duplex: Option<DuplexArg>,
    /// Sheet orientation: auto turns every photo or page to fill the sheet; portrait and
    /// landscape keep content upright on the sheet used that way.
    #[arg(long, value_enum, default_value = "auto")]
    pub orientation: OrientationArg,
    /// Page order (documents): normal prints the first sheet first, reverse the last sheet first
    /// (pages keep their place on a sheet; the two sides of a two-sided sheet stay together).
    #[arg(long, value_enum, default_value = "normal")]
    pub page_order: PageOrderArg,
    /// Job name shown on the printer (default: the file name).
    #[arg(long)]
    pub name: Option<String>,
    /// Do not compare the job with the printer's media database before sending.
    #[arg(long)]
    pub no_check: bool,
    /// Return as soon as the printer has accepted the job.
    #[arg(long)]
    pub no_wait: bool,
}

/// `washi disc`
#[derive(Args, Debug)]
pub struct DiscArgs {
    /// Background picture (JPEG / PNG / BMP).
    #[arg(long, value_name = "IMAGE")]
    pub image: Option<PathBuf>,
    /// Text on the disc; repeat for several lines (centred, stacked below the hub).
    #[arg(long, value_name = "TEXT")]
    pub text: Vec<String>,
    /// Text size in points.
    #[arg(long, default_value_t = 18.0, value_name = "PT")]
    pub text_size: f64,
    /// Text colour, #RRGGBB.
    #[arg(long, default_value = "#000000", value_name = "COLOR")]
    pub text_color: String,
    /// Where the text block is centred: X,Y in mm from the disc centre (right, down).
    #[arg(long, default_value = "0,32", value_name = "X,Y")]
    pub text_pos: String,
    /// Font file for the text, used as it is (default: found through fontconfig).
    #[arg(long)]
    pub font: Option<PathBuf>,
    /// Font family for the text, as fontconfig names it (`fc-list : family` lists them), e.g.
    /// "Noto Sans CJK JP". Not used with --font.
    #[arg(long, value_name = "NAME", conflicts_with = "font")]
    pub font_family: Option<String>,
    /// The bold cut of --font-family (the default font when no family is given); a family that
    /// has none is thickened.
    #[arg(long, conflicts_with = "font")]
    pub bold: bool,
    /// The italic cut of --font-family; a family that has none is slanted.
    #[arg(long, conflicts_with = "font")]
    pub italic: bool,
    /// A text that follows the ring (one line), e.g. a title along the edge. Uses --text-size,
    /// --text-color and --font; the start angle is the middle of the text.
    #[arg(long, value_name = "TEXT")]
    pub ring: Option<String>,
    /// Radius of the ring text's baseline in mm (5 to 60).
    #[arg(long, default_value_t = 48.0, value_name = "MM")]
    pub ring_radius: f64,
    /// Where the ring text is centred: degrees clockwise from the top (0 = top, 180 = bottom).
    #[arg(long, default_value_t = 0.0, value_name = "DEG", allow_hyphen_values = true)]
    pub ring_start: f64,
    /// Run the ring text counter-clockwise (letters' tops point at the centre), e.g. along the
    /// bottom edge with --ring-start 180.
    #[arg(long)]
    pub ring_ccw: bool,
    /// Extra space after every letter of the ring text, in points.
    #[arg(long, default_value_t = 0.0, value_name = "PT", allow_hyphen_values = true)]
    pub ring_spacing: f64,
    /// Inner printable diameter in mm (17 to 45).
    #[arg(long, default_value_t = 22.0)]
    pub inner: f64,
    /// Outer printable diameter in mm (114 to 118).
    #[arg(long, default_value_t = 116.0)]
    pub outer: f64,
    /// Disc media type: a (printable, recommended) or b (other).
    #[arg(long, value_enum, default_value = "a")]
    pub media_type: DiscMediaArg,
    #[arg(long, value_enum, default_value = "cover")]
    pub fit: FitArg,
    /// Zoom of the background picture (1 = as fitted).
    #[arg(long, default_value_t = 1.0)]
    pub scale: f64,
    /// Rotate the background picture clockwise by this many degrees.
    #[arg(long, default_value_t = 0.0)]
    pub rotate: f32,
    /// Background colour, #RRGGBB.
    #[arg(long, default_value = "#ffffff", value_name = "COLOR")]
    pub bg_color: String,
    #[arg(long, value_enum, default_value = "normal")]
    pub quality: QualityArg,
    /// Only render the label to this PNG and stop (no printer access, no --confirm needed).
    #[arg(long, value_name = "FILE.png")]
    pub preview: Option<PathBuf>,
    /// Return as soon as the printer has accepted the job.
    #[arg(long)]
    pub no_wait: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupArg {
    All,
    #[value(name = "1")]
    One,
    #[value(name = "2")]
    Two,
}

impl From<GroupArg> for InkGroup {
    fn from(g: GroupArg) -> Self {
        match g {
            GroupArg::All => InkGroup::All,
            GroupArg::One => InkGroup::Group1,
            GroupArg::Two => InkGroup::Group2,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum MaintCmd {
    /// Print head cleaning (use --deep for deep cleaning).
    Clean {
        #[arg(long, conflicts_with_all = ["roller", "platen"])]
        deep: bool,
        /// Clean the paper feed rollers as a `Cleaning` type, for models whose maintenance
        /// capability lists `roller` (UNVERIFIED; see `washi caps maintenance`).
        #[arg(long, conflicts_with_all = ["platen", "group"])]
        roller: bool,
        /// Clean the bottom plate, for models whose maintenance capability lists `platen`
        /// (UNVERIFIED).
        #[arg(long, conflicts_with = "group")]
        platen: bool,
        /// Ink group to clean.
        #[arg(long, value_enum, default_value = "all")]
        group: GroupArg,
    },
    /// Print the nozzle check pattern (1 sheet).
    NozzleCheck,
    /// Print head alignment (automatic by default, 2 sheets).
    Align {
        /// Automatic alignment.
        #[arg(long, conflicts_with_all = ["check", "manual"])]
        auto: bool,
        /// Print the alignment check sheet (1 sheet).
        #[arg(long, conflicts_with_all = ["auto", "manual"])]
        check: bool,
        /// Print manual alignment pattern ID (1-3, 3 sheets).
        #[arg(long, value_name = "ID", value_parser = clap::value_parser!(u8).range(1..=3), conflicts_with_all = ["auto", "check"])]
        manual: Option<u8>,
    },
    /// Clean the paper feed rollers.
    RollerClean,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    On,
    Off,
}

impl From<Toggle> for OnOff {
    fn from(t: Toggle) -> Self {
        match t {
            Toggle::On => OnOff::On,
            Toggle::Off => OnOff::Off,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperGapArg {
    Auto,
    Wide,
}

#[derive(Subcommand, Debug)]
pub enum SetCmd {
    /// Turn the printer on automatically when a print job arrives.
    AutoPowerOn {
        #[arg(value_enum)]
        value: Toggle,
    },
    /// Turn the printer off after N idle minutes: off, 15, 30, 60, 120 or 240.
    AutoPowerOff {
        value: String,
        /// Change the setting used when the printer is not networked.
        #[arg(long)]
        standalone: bool,
    },
    /// Quiet mode: off, on, or a daily window HHMM-HHMM (e.g. 2100-0700).
    Quiet { value: String },
    /// Ink drying time, 1 (shortest) to 5 (longest).
    DryLevel {
        #[arg(value_parser = clap::value_parser!(u8).range(1..=5))]
        level: u8,
    },
    /// Prevent paper abrasion: raise the print head a little so that thick or curled paper is
    /// not scuffed (printing can be slower). On is the printer's paper gap `wide`, off is `auto`.
    PaperAbrasion {
        #[arg(value_enum)]
        value: Toggle,
    },
    /// The printer's own name for "prevent paper abrasion": `wide` = on, `auto` = off.
    #[command(hide = true)]
    PaperGap {
        #[arg(value_enum)]
        value: PaperGapArg,
    },
    /// Automatic firmware updates.
    AutoFirmware {
        #[arg(value_enum)]
        value: Toggle,
    },
    /// Manual print head alignment values of one set (print the pattern first with
    /// `washi maint align --manual ID`), e.g. `registration 1 A=1 B=-2 C=0 D=1 E=1 F=0 G=0 H=0`.
    /// Send every sign of the set. Ranges are checked against what the printer reports.
    /// Writing these values is unverified on a real printer.
    Registration {
        #[arg(value_parser = clap::value_parser!(u8).range(1..=3))]
        id: u8,
        /// `SIGN=VALUE` pairs, e.g. A=1 or e=-2.
        #[arg(required = true, value_name = "SIGN=VALUE")]
        values: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum FirmwareCmd {
    /// Show the firmware version and whether an update is available (read-only; the default).
    Status,
    /// Ask the printer to update its firmware, then wait for it to restart. UNVERIFIED: the
    /// request is the one Canon's own app sends; it takes several minutes, cannot be undone and
    /// the printer must not be switched off meanwhile. Only works when an update is available.
    Update,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkLang {
    En,
    Ja,
}

#[derive(Subcommand, Debug)]
pub enum PowerCmd {
    /// Switch the printer off.
    Off,
    /// Wake a printer in auto power-off (verified on the XK140, firmware 1.060). Sends PowerOn
    /// with the power-on mode the printer expects, then waits a few seconds for it to come up.
    On,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Save the default printer host.
    SetHost { host: String },
    /// Forget the default printer host.
    ClearHost,
}

impl SetCmd {
    /// The change set and a one-line description.
    pub fn to_settings(&self) -> Result<DeviceSettings> {
        let mut s = DeviceSettings::default();
        match self {
            SetCmd::AutoPowerOn { value } => s.autopoweron = Some((*value).into()),
            SetCmd::AutoPowerOff { value, standalone } => {
                let p = parse_auto_power_off(value)?;
                if *standalone {
                    s.autopoweroff_standalone = Some(p);
                } else {
                    s.autopoweroff = Some(p);
                }
            }
            SetCmd::Quiet { value } => s.silentmode = Some(parse_quiet(value)?),
            SetCmd::DryLevel { level } => s.drylevel = Some(*level),
            SetCmd::PaperAbrasion { value } => {
                s.papergap = Some(
                    match value {
                        Toggle::On => "wide",
                        Toggle::Off => "auto",
                    }
                    .to_string(),
                );
            }
            SetCmd::PaperGap { value } => {
                s.papergap = Some(
                    match value {
                        PaperGapArg::Auto => "auto",
                        PaperGapArg::Wide => "wide",
                    }
                    .to_string(),
                );
            }
            SetCmd::AutoFirmware { value } => s.auto_printer_rom_update = Some((*value).into()),
            SetCmd::Registration { id, values } => {
                let mut items = std::collections::BTreeMap::new();
                for v in values {
                    let (sign, num) = v.split_once('=').ok_or_else(|| {
                        anyhow::anyhow!("{v:?} is not SIGN=VALUE (for example A=1)")
                    })?;
                    let sign = sign.trim();
                    if sign.chars().count() != 1 || !sign.chars().all(|c| c.is_ascii_alphabetic()) {
                        anyhow::bail!("{sign:?} is not a sign (a single letter such as A or e)");
                    }
                    let value: i32 = num
                        .trim()
                        .parse()
                        .map_err(|_| anyhow::anyhow!("{num:?} is not a whole number"))?;
                    items.insert(sign.to_string(), value);
                }
                s.setregistration.insert(*id, items);
            }
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use washi_core::ivec::SilentKind;

    use super::*;

    #[test]
    fn definition_is_consistent() {
        Cli::command().debug_assert();
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("washi").chain(args.iter().copied()))
    }

    #[test]
    fn global_flags_work_after_the_subcommand() {
        let c = parse(&["maint", "nozzle-check", "--confirm", "--host", "p.local"]).unwrap();
        assert!(c.confirm);
        assert_eq!(c.host.as_deref(), Some("p.local"));
        let c = parse(&["status", "--json"]).unwrap();
        assert!(c.json && !c.confirm);
    }

    #[test]
    fn maint_arguments() {
        assert!(parse(&["maint", "clean", "--deep", "--group", "2"]).is_ok());
        assert!(parse(&["maint", "clean", "--group", "3"]).is_err());
        assert!(parse(&["maint", "align", "--manual", "3"]).is_ok());
        assert!(parse(&["maint", "align", "--manual", "4"]).is_err());
        assert!(parse(&["maint", "align", "--auto", "--check"]).is_err());
        assert!(parse(&["maint", "roller-clean"]).is_ok());
        assert!(parse(&["maint", "clean", "--roller"]).is_ok());
        assert!(parse(&["maint", "clean", "--platen"]).is_ok());
        assert!(parse(&["maint", "clean", "--roller", "--platen"]).is_err());
        assert!(parse(&["maint", "clean", "--roller", "--deep"]).is_err());
        assert!(parse(&["maint", "clean", "--platen", "--group", "1"]).is_err());
    }

    #[test]
    fn firmware_and_links_arguments() {
        assert!(matches!(parse(&["firmware"]).unwrap().cmd, Cmd::Firmware { cmd: None }));
        assert!(matches!(
            parse(&["firmware", "update", "--confirm"]).unwrap().cmd,
            Cmd::Firmware { cmd: Some(FirmwareCmd::Update) }
        ));
        assert!(parse(&["firmware", "install"]).is_err());
        assert!(parse(&["links", "--code", "1300", "--lang", "ja"]).is_ok());
        assert!(parse(&["links", "--lang", "fr"]).is_err());
    }

    #[test]
    fn set_arguments() {
        assert!(parse(&["set", "dry-level", "0"]).is_err());
        assert!(parse(&["set", "dry-level", "6"]).is_err());
        assert!(parse(&["set", "dry-level", "5"]).is_ok());
        assert!(parse(&["set", "paper-gap", "narrow"]).is_err());
        assert!(parse(&["set", "paper-abrasion", "wide"]).is_err());
        assert!(parse(&["set", "paper-abrasion", "on"]).is_ok());
        assert!(parse(&["set", "auto-power-on", "maybe"]).is_err());
        assert!(parse(&["set", "auto-firmware", "off"]).is_ok());
    }

    #[test]
    fn registration_arguments() {
        let c = parse(&["set", "registration", "2", "I=1", "J=-3"]).unwrap();
        let Cmd::Set { cmd } = c.cmd else { panic!("not a set command") };
        let s = cmd.to_settings().unwrap();
        assert_eq!(s.setregistration[&2]["I"], 1);
        assert_eq!(s.setregistration[&2]["J"], -3);
        assert!(parse(&["set", "registration", "4", "A=1"]).is_err());
        assert!(parse(&["set", "registration", "1"]).is_err());
        let bad = |v: &str| {
            let c = parse(&["set", "registration", "1", v]).unwrap();
            let Cmd::Set { cmd } = c.cmd else { panic!() };
            cmd.to_settings().unwrap_err().to_string()
        };
        assert!(bad("A").contains("SIGN=VALUE"));
        assert!(bad("AB=1").contains("not a sign"));
        assert!(bad("A=x").contains("whole number"));
        assert!(matches!(parse(&["power", "on"]).unwrap().cmd, Cmd::Power { cmd: PowerCmd::On }));
        assert!(parse(&["print", "x.jpg", "--orientation", "landscape"]).is_ok());
        assert!(parse(&["print", "x.jpg", "--orientation", "diagonal"]).is_err());
    }

    #[test]
    fn value_parsers() {
        assert_eq!(parse_auto_power_off("off").unwrap().mode, OnOff::Off);
        assert_eq!(parse_auto_power_off("120").unwrap().time, Some(120));
        assert!(parse_auto_power_off("45").is_err());
        assert!(parse_auto_power_off("soon").is_err());
        assert_eq!(parse_quiet("OFF").unwrap().kind, SilentKind::Off);
        assert_eq!(parse_quiet("on").unwrap().kind, SilentKind::On);
        let w = parse_quiet("2100-0700").unwrap();
        assert_eq!(
            (w.kind, w.start.as_deref(), w.end.as_deref()),
            (SilentKind::Time, Some("2100"), Some("0700"))
        );
        assert!(parse_quiet("2100").is_err());
        assert!(parse_quiet("2500-0700").is_err());
        assert!(parse_quiet("21:00-07:00").is_err());
    }

    #[test]
    fn set_commands_map_to_settings() {
        let s =
            SetCmd::AutoPowerOff { value: "60".into(), standalone: true }.to_settings().unwrap();
        assert_eq!(s.autopoweroff_standalone.unwrap().time, Some(60));
        assert!(s.autopoweroff.is_none());
        let s = SetCmd::PaperGap { value: PaperGapArg::Wide }.to_settings().unwrap();
        assert_eq!(s.papergap.as_deref(), Some("wide"));
        let s = SetCmd::PaperAbrasion { value: Toggle::On }.to_settings().unwrap();
        assert_eq!(s.papergap.as_deref(), Some("wide"));
        let s = SetCmd::PaperAbrasion { value: Toggle::Off }.to_settings().unwrap();
        assert_eq!(s.papergap.as_deref(), Some("auto"));
    }
}
