# Changelog

All notable changes to Washi are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

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
