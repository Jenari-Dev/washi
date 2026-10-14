# Changelog

All notable changes to Washi are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

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
