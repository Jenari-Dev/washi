# Changelog

All notable changes to Washi are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.1.5] - 2026-10-18

### Changed
- Firmware update: Washi waits up to 30 minutes (was 10) for the printer to switch into update
  mode, and says that the printer downloads the update first while its screen can stay on the home
  menu. Verified on a PIXUS XK140 (1.060 → 1.070): the download took about 8 minutes.
- README: the hardware verification table now lists everything that was run on the XK140.

## [0.1.4] - 2026-10-18

### Added
- **Prevent paper abrasion.** The printer's `papergap` setting (`auto` / `wide`) is what Canon's
  own interfaces call "Prevent paper abrasion" (用紙のこすれ防止). The Utilities "Paper Gap" card and
  dialog are now an On / Off switch with that title (On is `wide`, Off is `auto`) and a description
  of what it does (raises the print head a little so that thick or curled paper is not scuffed;
  printing can be slower). `washi set paper-abrasion on|off` (`paper-gap auto|wide` stays as a
  hidden alias), `washi settings` shows On / Off, and `ApplySettings` takes `paper_abrasion` next to
  `paper_gap`. `reduce_ink_smudges` is not offered by the XK140 and stays hidden unless a printer
  reports it.
- **Test notification.** `SendTestNotification()` on the daemon shows a sample notification
  ("Washi test notification") through the same code as the real ones, in the app's language, and
  returns an error with the reason when the desktop has no notification service or washid runs
  with `--no-notify`. It does not touch the printer and works with `--read-only`. `washi
  notify-test`, and *App Settings → Notifications → Send a test notification* in the app.
- **`washi doctor`.** A read-only self-check, human output or `--json`: the versions of the
  command, the running `washid` and the app and whether they agree; whether the service is
  running, D-Bus-activatable and enabled as a systemd user unit; for the printer: mDNS, the CHMP,
  IPP and eSCL channels, the kept names, picture and web links; the notification service, helper
  programs (poppler, tesseract and its languages, LibreOffice), English and Japanese locales, and
  the settings and cache paths. Each line is a tick, a cross or a dash with a one-line hint; no
  serial number, hardware address or printer address is printed. *App Settings → Check setup*
  shows the same checks through the new `Doctor()` D-Bus method.
- **Flatpak smoke test.** `.github/workflows/flatpak.yml` (manual, and on changes to
  `packaging/flatpak/`) builds the bundle, installs it and runs `washi --version`, `washid
  --version`, `washi doctor --json` and the app offscreen with a screenshot; the same steps run in
  the release workflow's Flatpak job before the bundle is uploaded. `washi-app --version` prints
  the app's version without opening a window.

### Fixed
- **The printer's web links while it sleeps.** The XK140 serves `/JS_MDL/model.js` (ink shop,
  manual, error help, support) only while it is awake, so the links fell back to the printer's
  generic page most of the time. The parsed links are now kept per printer like its strings and
  picture (`~/.cache/washi/<key>-links.json`, read again after 30 days or a firmware change),
  refreshed in the background whenever the daemon sees the printer awake, and used while it
  sleeps. `washi links`, `GetWebLinks` (new `saved_at`) and the link dialogs say "from the
  printer (saved <date>)" when they come from the kept copy; without one the fallback to the
  printer's own page is as before.
- **A sleeping printer was shown as "up to date".** In auto power-off the printer does not check
  for firmware updates and answers `unavailable` to `networkupdate_available`, minutes before it
  says `available` once awake. A sleeping printer's `unavailable` is now ignored: the snapshot
  keeps what the printer said the last time it was on (`firmware_update_remembered`), or
  `unknown` without one, and `washi firmware`, `washi info` and the Device Information tab say
  "the printer checks for updates while it is on; last known: ..." instead. `available` is
  believed whenever it is read. `washi firmware update` and the update itself are refused while
  the printer sleeps unless it says `available` right then.

### Changed
- The fake printer behaves like the real one in auto power-off: `model.js` answers `404` and the
  device status says no firmware update is available.

## [0.1.3] - 2026-10-14

### Added
- **Disc label fonts.** Every text has a font family picker (the installed families, Japanese ones
  such as Noto Sans CJK JP included) and Bold / Italic switches. The printed image uses the same
  face as the canvas: fontconfig picks the file for the family, weight and slant (a face of a
  font collection, a named instance of a variable font), a family without a bold or italic cut is
  thickened or slanted the way fontconfig says Qt draws it, and letters the font lacks come from
  another installed font instead of printing as boxes. Saved designs keep `family`, `bold` and
  `italic` (older designs load as regular, upright text). `washi disc` takes `--font-family`,
  `--bold` and `--italic`; the `PrintDisc` text takes `family`, `bold` and `italic` (and still
  `font` for a file).
- **Scan, Stitch: left and right or top and bottom.** The setup page has a "Join the scans" choice
  and the Join button of the preview has a menu with the same two entries.
- **The edge the printer cannot print on** is drawn as a dashed frame on the Photo Print and
  Document Print previews, from the margins of the printer's own `media-col-database` entry for the
  chosen tray, paper type and size (turned with the sheet in landscape; borderless shows none). A
  photo with margins is drawn inside the frame. `GetPrintCapabilities` has a new `margins` map.
- **Page Order (Normal / Reverse) for Document Print.** Reverse prints the last sheet first. The
  sheets are turned round, not the pages: pages keep their place on a sheet (n-up), and a two-sided
  sheet keeps its front before its back (a lone last page comes out first with a blank back). The
  preview shows the sheets in printing order. `washi print --page-order reverse`, and `page_order`
  in the `PrintFile` options.

### Changed
- The disc label request no longer carries a font file chosen by the app: it names the family and
  the styles, and the service resolves the file (`TextSpec.font` still works for a file).
- A font collection is opened at the face fontconfig reports. The Japanese cut is preferred for
  Japanese text in the default font (fontconfig alone ranked the Korean cut of Noto Sans CJK first).
- The Flatpak builds poppler and poppler-data from their release tarballs (pinned by checksum)
  instead of cloning them from git.

## [0.1.2] - 2026-10-14

### Added
- **The printer's own paper names, in the language you use.** Washi reads the display names the
  printer itself publishes (IPP `printer-strings-uri`, a `.strings` file) and shows them for the
  media types in Photo Print, Document Print, the disc label page and the print confirmation, and
  for the paper sources when the printer names them. The names follow the English / Japanese switch
  at once. Washi's own names remain the fallback for a printer that says nothing.
- **The printer's own picture.** The banner and the printer list show the product picture the
  printer publishes (IPP `printer-icons`), with its transparent margin cut off. The order is: your
  picture pack image (`~/.local/share/washi/printers/<MODEL>.png`), the printer's picture, Washi's
  line drawing.
- D-Bus `GetPrinterStrings(host, language)` and `GetPrinterIcon(host)`, read-only, allowed with
  `--read-only`. Both are kept in `~/.cache/washi` under the hashed printer key and read again
  after 30 days or when the printer's firmware version changes; a kept copy is answered at once and
  refreshed in the background, so the interface never waits for them.
- The fake printer can serve strings and a picture (`--strings LANG=FILE`, `--icon FILE.png`).

### Changed
- Corrected paper names where the printer shows Washi's guesses were wrong: `com.canon-mtgr` is
  Photo Paper Plus Glossy II (it was "Photo Paper Glossy") and `com.canon-mtglossy` is Glossy Photo
  Paper (it was "Photo Paper Plus Glossy II"); Photo Paper Pro Crystal Grade; the two faces of the
  postcards are told apart; names that were guessed (glossy postcards, "Fine Art Paper") are
  described without claiming a product. The preferred photo paper on the Photo Print page is
  Photo Paper Plus Glossy II (`com.canon-mtgr`), then glossy photo paper.
- Drop-down entries are always shown as plain text, never as markup, because paper names now come
  from the network.
- Canon PRINT's exact accent blue (`#1354BF`, hover `#124FB3`, pressed `#0E3F8F`) instead of a
  colour estimated from screenshots.
- A bigger printer picture in the banner: the picture keeps a minimum size, and picture-pack images
  are shown without their transparent margin (a trimmed copy is kept in `~/.cache/washi/pack`).

## [0.1.1] - 2026-10-14

### Fixed
- **Wake works**: `PowerOn` now sends the power-on mode the printer expects
  (`<ivec:poweronmode>ModeA</ivec:poweronmode>`). Without it the XK140 answered `NG ParameterError`
  and stayed asleep. Verified on a PIXUS XK140 (firmware 1.060): the printer comes out of auto
  power-off.

### Changed
- `washi power on --confirm` looks at the print status for up to ten seconds and says
  `Printer woke up` as soon as it stops saying `Suspended`.
- The service's `Wake` call waits for that follow-up status (up to about eight seconds) and
  reports `verified: true` when the printer is awake. The Wake button and the documentation no
  longer call the command untried; wake moves to "verified on the printer" in the README.
- The fake printer answers `NG ParameterError` to a `PowerOn` without the power-on mode, like the
  real one, so the tests would have caught the bug.

## [0.1.0] - 2026-10-14

First public release. Verified on a PIXUS XK140 (firmware 1.060); see the hardware table in the
README for what has run on a real printer and what has only run against the fake one.

### Added
- **Status and ink**: online / sleeping / offline, ink level of every tank with its model number,
  paper in the trays, device information (firmware, connection, counters), support codes with a
  short message of Washi's own and a help button, firmware status. A sleeping printer is read
  through IPP and the cache without waking it.
- **Maintenance**: cleaning, deep cleaning, nozzle check, automatic and manual print head
  alignment, roller and bottom plate cleaning where a model offers them. Every job runs inside the
  printer's job bracket, shows its progress and can be cancelled.
- **Settings**: quiet mode, auto power on and off, ink drying time, paper gap, automatic firmware
  update. Every change is validated against the printer's capabilities and read back.
- **Printing** over IPP: photos (JPEG, PNG, BMP) with borderless printing, media type and tray
  taken from the printer's own media database, 1 to 16 pages per sheet; PDF and Office documents
  with page ranges and two-sided printing. Drag and drop onto the preview.
- **Scanning** over eSCL: documents and photos, several pages, rotate, crop, slant correction,
  colour correction, auto crop, multi-photo crop, stitching, PDF / JPEG output, optional OCR.
- **Disc labels**: a designer for the printable ring (picture, text, background colour, text that
  follows the ring), a preview of what is sent, and printing from the disc tray.
- **Desktop notifications** from the `washid` daemon when a tank is low or empty, when the printer
  reports an error, and when it is back online.
- **English and Japanese** interface, notifications and desktop entry, with a live language switch.
- The `washi` command-line tool with JSON output and dry runs for every write.
- A fake printer (`washi-fake`) for development and tests.
- Packaging: Arch Linux (`PKGBUILD`), Flatpak, a `~/.local` installer, systemd user unit and D-Bus
  activation file, AppStream metadata.

### Safety
- Every operation that changes the printer, uses ink or paper, or moves the scanner needs an
  explicit confirmation (`--confirm` on the command line, which otherwise only prints what it would
  send; a dialog in the app). `washid --read-only` refuses all of them.
- No telemetry. Washi only talks to the printer you choose and opens web pages when you ask it to.

### Not included
- Wi-Fi setup of a new printer ("cableless setup"); see [NOTICE.md](NOTICE.md).
- Canon's accounts, cloud services, chat and subscription features, and its usage statistics.

### Verified on real hardware, and what is not
Run on a PIXUS XK140 so far: every read operation (status, ink, device information, settings and
capabilities, the printer's media database and scanner capabilities), one nozzle check, one printed
PDF page (plain A4, draft) and one A4 greyscale scan.

Everything else that writes is implemented and tested against the fake printer only: cleaning and
deep cleaning, print head alignment, changing settings, the Wake button, the firmware update,
photo and borderless printing, disc printing, colour and multi-page scans. Roller and bottom plate
cleaning exist for models that list them (the XK140 does not). The README has the full table.
Please report what you try, with the model and firmware.

[0.1.1]: https://github.com/Jenari-Dev/washi/releases/tag/v0.1.1
[0.1.0]: https://github.com/Jenari-Dev/washi/releases/tag/v0.1.0
