# Using Washi: reference

The [README](../README.md) is the short version. This page has the details of the command-line tool,
the desktop app and the languages. The daemon's D-Bus interface is in [DBUS-API.md](DBUS-API.md),
the protocols in [PROTOCOL.md](PROTOCOL.md).

## Command line

The printer is found with `--host HOST` (`HOST` or `HOST:PORT`), else the host saved with
`washi config set-host HOST`, else the first printer that answers mDNS (`_canon-chmp._tcp`).

Read commands (always safe; `--json` gives machine-readable output):

```
washi discover                    # printers advertising _canon-chmp._tcp
washi status                      # power, state, ink levels with bars, paper per tray
washi watch                       # follow the printer; a line whenever something changes
washi info                        # model, firmware, lifetime counters
washi settings                    # current device settings and their allowed values
washi caps maintenance            # also: caps device, caps print, ...
washi raw GetStatus device        # raw IVEC reply (identifiers blanked; --no-redact to keep)
washi config                      # show / set the default printer host
washi firmware                    # firmware version and whether the printer offers an update
washi links [--code 1300] [--lang ja]   # the web pages the printer points to (nothing is opened)
```

Write commands (change the printer; dry run unless `--confirm`):

```
washi maint clean [--deep] [--group all|1|2]
washi maint nozzle-check
washi maint align [--auto | --check | --manual ID]
washi maint roller-clean          # the RollerCleaning operation (models that list it)
washi maint clean --roller        # roller / bottom plate cleaning as `Cleaning` types (models that
washi maint clean --platen        #   list `roller` / `platen`; UNVERIFIED, the XK140 lists neither)
washi firmware update             # UNVERIFIED, see below; only when the printer says an update is available
washi set auto-power-on on|off
washi set auto-power-off off|15|30|60|120|240 [--standalone]
washi set quiet off|on|HHMM-HHMM
washi set dry-level 1..5
washi set paper-gap auto|wide
washi set auto-firmware on|off
washi power off
washi power on                    # wake a printer in auto power-off (verified on the XK140)
washi set registration 1 A=1 B=-2 ...   # manual head alignment values of one set (UNVERIFIED write)
```

Add `-v` to log the HTTP exchanges (status codes and sizes, never bodies) to stderr.

### Sleeping printers

The printer powers itself off after a while (auto power-off). Then IVEC `GetStatus print` and
`maintenance` answer `NG Suspended`, but `GetStatus device` and IPP keep working. `washi status`
combines what answers into one `PrinterSnapshot`:

```
$ washi status
Printer   printer.local
Power     sleeping
          Printer is in auto power-off; it wakes up when a job arrives.
State     idle (reported over IPP)

Ink (IPP marker levels)
  M     Magenta        ████████████░░░░░░░░   60%  XKI-N21<M>SETUP
  ...
```

* Awake: everything comes from IVEC. Asleep: ink levels come from IPP `marker-levels`
  (`Black(PGBK)` -> `PGBK`, `Black(BK)` -> `BK`, `Cyan` -> `C`, ...), counters from `GetStatus
  device`, and tank models, trays and device info from the cache (marked in `from_cache`).
  Neither command wakes the printer.
* Unreachable (nothing answers on CHMP or IPP): the last known data is shown and the exit code is 1.
  A sleeping printer exits 0.
* `washi power on --confirm` wakes a sleeping printer. It sends `PowerOn` with the power-on mode
  `ModeA` (the printer answers `NG ParameterError` without it), then looks at `GetStatus print`
  for up to ten seconds and prints `Printer woke up` as soon as it no longer says `Suspended`.
  Verified on a PIXUS XK140 (firmware 1.060, 2026-10-14). Without `--confirm` it is a dry run.
* `washi status --json` prints the snapshot (power `on`/`sleeping`/`offline`, state, support code
  with a translation key, inks with status `ok`/`low`/`empty`, trays, device, counters, ...).
* Cache: `~/.cache/washi/<hash of the host name>.json` (`XDG_CACHE_HOME` is honoured; the file name
  carries no host or device identifier).
* `washi watch` streams snapshots from `washid` when it runs, otherwise polls (`--interval`,
  default 30 s; `--no-daemon` to always poll).

### Support codes, web links and firmware

* **Support codes.** The printer reports only a code (`1000`, `1300`, `D109`, ...). `washi-core`
  groups codes into families (no paper, cover open, paper jam, print head, ink tank, ink absorber,
  hardware error, notice, ...) with a stable message key (`support.paper-jam`), a severity
  (`info`, `warning`, `error`) and a short message of Washi's own in English and Japanese. The
  app shows "Support code 1300" (サポート番号 1300) in the banner and on the Status tab with the
  message and a help button; the daemon's notifications use the same words, and notices
  (severity `info`) stay quiet. A code in no family still names itself.
* **Web links.** The printer's own web page serves `GET /JS_MDL/model.js`, from which Washi reads
  four addresses (ink shop, manual, error help with the support code filled in, maker support) and
  nothing else; `washi links` shows them. They are checked (http / https only, a plain host name)
  and fall back to the printer's own web page `http://<host>/` when the printer does not offer
  one. The app always asks first ("This opens a Canon web page in your web browser (host)...") and
  opens the page only after "Open". The Utilities tab has a card that opens the printer's own
  maintenance page (the printer asks for its administrator password there).
* **Firmware.** Device Information shows the version and whether the printer offers an update
  (`networkupdate_available`; only `unavailable` counts as "no update"), a "Check for Updates"
  button (a fresh read) and "Update Firmware…", which is enabled only when an update is offered.
  The update dialog has a strong warning and a box to tick; then Washi sends `VendorCmd`
  `UpdateFRom` (device service, no job), follows the printer until it has gone away and come
  back, and shows the new version. **The update is unverified on a real printer**; `washi firmware update` is a dry run unless
  `--confirm`.

### Scanning, printing and disc labels

Everything that moves paper or the scanner is a **dry run unless `--confirm`**: the exact eSCL or
IPP request is printed and no connection is opened. Reads (`scanner`, `media`, `jobs`, `tools`)
always run.

```
washi scanner                     # what the flatbed offers (eSCL capabilities + state)
washi media [--source disc] [--size a4]   # the paper the printer offers (media-col-database)
washi jobs                        # jobs the printer is working on
washi tools                       # which optional helper programs are installed

washi scan [--size A4|letter|a5|b5|l|2l|postcard|card|auto] [--res 75..1200]
           [--mode color|gray|bw] [--type document|photo] [--format pdf|jpeg] -o PATH
           [--pages N] [--split] [--correct document|photo|magazine] [--deskew]
           [--show-through] [--sharpen 0..100] [--rotate 90|180|270|left|right]
           [--aspect 3:2|4:3|7:5|10:8|16:9|square] [--crop-photos] [--auto]
           [--stitch left-right|top-bottom] [--ocr [LANG]] [--no-intent]

washi print FILE... [--size a4] [--media-type plain|photo|glossy|...|com.canon-*]
            [--source auto|rear|cassette] [--borderless] [--quality draft|normal|high]
            [--color|--mono] [--copies N] [--pages 1-3,5] [--layout 1|2|4|6|9|16]
            [--duplex long|short] [--orientation auto|portrait|landscape]
            [--page-order normal|reverse]
            [--name NAME] [--no-check] [--no-wait]

washi disc [--image IMG] [--text TEXT]... [--inner 22] [--outer 116] [--media-type a|b]
           [--font-family NAME [--bold] [--italic] | --font FILE.ttf]
           [--ring TEXT [--ring-radius 48] [--ring-start 0] [--ring-ccw] [--ring-spacing 0]]
           [--preview LABEL.png]       # --preview renders only, no printer access

washi identify [--action flash|sound]  # "find my printer"
washi cancel JOB                       # cancel a job (see `washi jobs`)
```

* **Scan**: one `POST /eSCL/ScanJobs`, then `GET .../NextDocument` until `404`. The scanner
  delivers JPEG; PDF is written on the host. The XK140 has no 1-bit mode, so "bw" is a grey scan
  thresholded on the host. `--size auto` scans the whole platen and crops the object on the glass
  (`--crop-photos` cuts out every photo). `--pages N` is "+ Next scan": you are asked to change the
  original between scans.
* **Colour correction** (`washi_core::scan::correct`): *for document* finds the paper white per
  channel, scales it to white, whitens near-white and adds a little contrast; *for photo* is
  automatic levels (0.5 % clipped each end, gamma toward mid-grey); *for magazine* descreens with a
  resolution-scaled blur, then photo levels and a light unsharp mask. *Reduce show-through* lifts
  everything brighter than a threshold toward white.
* **Photos** (JPEG / PNG / BMP) become an sRGB JPEG laid out on the paper (EXIF orientation
  applied, embedded ICC profiles converted) and are sent as `image/jpeg` with `print-scaling`
  fill (borderless) or fit.
* **Documents**: the printer takes no PDF, so each page is rendered by poppler's `pdftoppm` at
  600 dpi and sent as PWG raster (`image/pwg-raster`, `srgb_8` / `sgray_8`). Office files are
  converted by `soffice --headless` first. `pdftoppm`, `pdfinfo`, `soffice` and `tesseract` are
  optional and found at run time (`WASHI_PDFTOPPM` etc. override); `washi tools` lists them.
* Before a job is sent the printer is asked what it accepts (read-only `Get-Printer-Attributes`),
  and the paper size / tray / type / borderless combination is checked against its
  `media-col-database`, whose margins are then used. `Validate-Job` runs before `Print-Job`, and
  the job is followed with `Get-Job-Attributes` until it is `completed`.
* **Disc labels** are rendered at 600 dpi (2834 x 2834 px) and masked to the printable ring
  (inner 17 to 45 mm, default 22; outer 114 to 118 mm, default 116); everything outside the ring is
  white, so no ink reaches the tray. They print with `media-source=disc`, `media-type`
  `com.canon-mtdisca` (`a`) or `com.canon-mtdiscb` (`b`), media size 120 x 120 mm
  (`om_square_120x120mm`), margins 0.1 mm: the two disc entries of the printer's own database.
* **Text that follows the ring** (`--ring`, "Follow the ring" in the app): the letters sit on a
  circle about the disc centre. `--ring-radius` is the radius of the baseline (mm), `--ring-start`
  the angle in degrees clockwise from the top (0 = top, 90 = right, 180 = bottom), the text is
  centred on it (the app's alignment says whether it begins, is centred or ends there),
  `--ring-ccw` runs counter-clockwise (letter tops point at the centre, so a line along the bottom
  edge reads upright with `--ring-start 180`) and `--ring-spacing` adds room after each letter, in
  points. The geometry is one pure function (`washi_core::disc::RingText::layout`) that the app's
  canvas mirrors, so the preview and the printed image agree.
* **Fonts** (`--font-family`, `--bold`, `--italic`; the font picker and the B / I switches in the
  app): the family goes to fontconfig (`fc-match`) with the weight and slant, and the file it names
  is drawn, the way the app's canvas gets the same face from Qt. A family kept in a collection is
  opened at its own face (`Noto Sans CJK JP`, not the first one); a variable font is set to the
  weight fontconfig picked; a family without a bold or italic cut is thickened or slanted the way
  fontconfig says it would be (`embolden`, the 0.2 slant matrix). Letters the chosen font lacks
  (Japanese in a Latin family) come from another font, so a label does not print boxes when some
  font on the computer has the letter. With no family, text uses the default sans-serif font, a
  Japanese one for Japanese text. `--font FILE` uses that file as it is. Designs saved by older
  versions have no style keys and stay regular and upright.

## The desktop app: `washi-app`

```
washid &                                   # or systemctl --user enable --now washid
washi-app
```

The window has a navigation rail on the left, a banner with the printer and its ink chips, and
white settings panels. Everything that changes something asks first and goes through `washid`,
which refuses under `--read-only`.

* **Home / Printer Management / Printer List / App Settings**: status, ink, device information
  (link type and IPv4 address come from the printer's `network_interface` and a name lookup;
  the firmware version, a "Check for Updates" button and "Update Firmware…"), the Utilities
  (cleaning, nozzle check, print head alignment automatic and *manual*, quiet mode, auto power
  on / off, drying time, paper gap, automatic firmware update; **roller cleaning** and **bottom
  plate cleaning** cards appear only on models whose maintenance capability lists them, and a card
  that opens the printer's own maintenance web page is always there), notification switches saved
  by the service, and "remove printer". The four Printer Management tabs sit in the bottom
  edge of the banner, each with a small coloured icon. A support code shows in the banner and on
  the Status tab with a message and a "Help for this Support Code" button. Every link that leaves
  the app (Order Ink, Manual, Remote UI, help for a code) asks first and says whose page it is. A sleeping printer shows an optional **Wake printer** button
  (`PowerOn` with the power-on mode `ModeA`, verified on the XK140; a maintenance job, a print or
  the power button wake it just as well, so nothing is disabled while it sleeps). After a nozzle check the follow-up offers a cleaning for
  missing or faint lines and a print head alignment for crooked grids.
* **Photo Print**: pick JPEG / PNG / BMP files (several for the n-in-1 layouts), Page Size,
  Orientation, Media Type (named from the printer's own `media-type-supported`, with our own
  names for the `com.canon-*` papers), Paper Source, Borderless (only where the printer's media
  database has a borderless entry for that paper and size), Photo Layout, Quality, Color, Copies.
  The preview is the sheet as it will be printed, composed by the same code that prints it. When
  the paper has margins (not borderless), a dashed frame marks the edge the printer cannot print on,
  from the margins of the printer's own `media-col-database` entry for that tray, paper type and
  size (turned with the sheet in landscape); a photo is drawn inside the frame, because the job asks
  the printer to fit it there.
* **Document Print**: PDFs (Office files too when LibreOffice is installed), a page preview from QtPdf
  with page / sheet navigation, Pages (`1-6, 3, 12-14`), Page Layout n-up, Two-sided Printing,
  and the same paper settings. Printing PDFs needs poppler. Files can be dragged from a file
  manager onto the preview of both print pages.
* **Scan**: setup (guidance drawings, Original Type, Size, Resolution, Color Mode and a "More scan
  modes" menu: Auto scan, Multi-photo crop, Stitch, and OCR when `tesseract` is installed) ->
  preview and edit (thumbnails with rotate / delete, "+ Next scan", Rotate/Crop, Image Correction,
  Revert, Format PDF / JPEG, Range, "Combine all pages into one file") -> a dark Rotate/Crop
  screen with a draggable trimming box, slant correction and aspect-ratio presets -> Image
  Correction with the result shown live. The image work runs in `washi-core` on worker threads.
* **Disc Label**: a designer of our own: the printable ring with its outer and inner diameter, a
  picture to move, scale and turn (or drop a picture onto the disc), texts, a background colour,
  the disc type, a preview of the image that is sent, the tray guidance, and designs saved as JSON
  in `~/.local/share/washi/discs/`. A text can "Follow the ring": radius, start angle,
  clockwise or counter-clockwise and letter spacing; the canvas draws it with the same formulas as
  the printed image and it is dragged round the disc with the mouse. Every text has a font family
  picker (the families installed on the computer, Japanese ones included) and Bold / Italic.

## Languages

The desktop app, the daemon's notifications and the desktop entry are available in English and
Japanese; the `washi` command-line tool stays English.

* **Switching**: the `EN | 日本語` switch in the top right corner of the window, on every page.
  It takes effect at once (no restart): every text, dialog, tooltip, list entry and formatted
  value is updated, and what the app shows of the service's messages follows (the app tells the
  service with `SetLanguage`).
* **Choice**: `[ui] language` in `~/.config/washi/config.toml`, `"en"`, `"ja"` or `"system"`. A
  first run (no key) follows the session's locale: `ja_*` is Japanese, everything else English
  (`LC_ALL`, `LC_MESSAGES`, `LANG`, and `LANGUAGE` as gettext reads them). `WASHI_LANG=en|ja`
  overrides it for one start of the app (screenshots, tests).
* **How**: the app uses KDE's KI18n (`i18n()`, `i18nc()`, `i18np()` in QML). Switching sets
  `KLocalizedString::setLanguages()` and calls `QQmlEngine::retranslate()`, which re-evaluates
  every binding that calls `i18n()`; text that is stored for later (progress lines, failures,
  notices) is kept as a function that builds the text when it is shown (`Fmt.text()`). The few
  texts of the daemon are a built-in English / Japanese table (`crates/washid/src/messages.rs`).
  The strings are extracted with gettext into `po/washi.pot`; `po/ja.po` is the Japanese
  translation. `cargo build` compiles `po/*.po` with `msgfmt` to
  `target/<profile>/share/locale/<lang>/LC_MESSAGES/washi.mo`, which `washi-app` finds next to
  itself; an install puts them in `<prefix>/share/locale/` (`contrib/install-local.sh` does this
  for `~/.local`). Needs gettext (`msgfmt`, `xgettext`, `msgmerge`).
* **Translating**: change or add an `i18n("...")` string, run `po/update.sh` (it refreshes
  `po/washi.pot` and merges it into every `po/*.po`), translate what is new and run
  `python3 po/check.py`: it fails on untranslated or fuzzy messages, a stale `washi.pot`,
  placeholders that do not match, bare English literals in QML text properties and on Japanese
  typography (full-width punctuation, no spaces between Japanese words, half-width digits and
  units such as `120 mm`, 「」 for quotes, no English words left apart from names). Model and
  product names (`Canon XK140 series`, `PGBK`, paper names) stay in Latin script. A new language
  is a new `po/<code>.po` (`msginit -i po/washi.pot -l <code> -o po/<code>.po`) and an entry in
  the language switch (`qml/LangSwitch.qml`).
