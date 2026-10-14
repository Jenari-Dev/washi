# Architecture

Washi (和紙) is a Linux replacement for Canon's "Canon PRINT" app, for Canon inkjet printers that
speak CHMP/IVEC. It is a Rust workspace plus a Qt 6 Quick app, licensed GPL-2.0-or-later.

```
  washi-app ──(D-Bus)──▶ washid ───────┐
  Qt 6 Quick                │          │
                            ▼          ▼
                 desktop notifications   washi-core ──▶ the printer
  washi ───────────────────────────────▲               • CHMP + IVEC, HTTP port 80:
  command line                                             status, ink, settings, maintenance
                                                        • IPP, port 631: print jobs, media, marker levels
                                                        • eSCL, HTTP port 80: scanner
```

## Protocol choices

| Area | Protocol | Why |
|---|---|---|
| Status, ink, device information, settings, maintenance | CHMP + IVEC XML | the printer's own management interface; it gives what IPP does not (tank models, counters, settings, maintenance jobs) |
| Printing (photos, documents, disc labels) | IPP Everywhere direct to the printer (PWG raster, JPEG), the printer's media keywords, `media-source=disc` | no vendor driver needed; covers borderless, media types, quality and trays |
| Scanning | eSCL | a standard; every extra (auto crop, multi-photo, stitching, OCR) is done on the computer |
| Discovery | mDNS `_canon-chmp._tcp` | what the printer advertises |

The protocols are written down in [PROTOCOL.md](PROTOCOL.md). The printer's cloud account, its
survey and consent flags, and the maker's telemetry are not used; where the printer exposes a switch
(automatic firmware update, cloud link) Washi shows it honestly.

## Workspace

```
crates/washi-ivec   IVEC XML: typed request builders and tolerant response parsers. No I/O.
crates/washi-net    CHMP client (minimal HTTP/1.1 on TcpStream), IPP client, eSCL client, the
                    links, strings file and picture of the printer's own web server, mDNS discovery.
crates/washi-core   printer handle, config file, PrinterSnapshot + cache, maintenance and
                    settings flows, support codes, web links, firmware; scan editing, print
                    preparation (photos, PDF rasterising, n-up, PWG raster) and disc labels.
crates/washi        the command-line tool
crates/washid       user daemon: polling, D-Bus API, maintenance worker, notifications
crates/washi-gui    the desktop app washi-app (Qt 6 Quick through cxx-qt, KDE's KI18n)
crates/washi-fake   development and test only: a scriptable fake printer on 127.0.0.1
po/                 gettext catalogs (washi.pot, ja.po) and their checker
contrib/            desktop entry, icon, AppStream metadata, ~/.local installer
packaging/          systemd unit, D-Bus service, PKGBUILD, Flatpak manifest
```

Dependencies point one way: `washi-ivec` ← `washi-net` ← `washi-core` ← (`washi`, `washid`,
`washi-gui`). `washi-gui` does not talk to a printer itself: it asks `washid` over D-Bus, and uses
`washi-core` only for work on the computer (editing scans, previews of print jobs and disc labels).

### Safety in the types

- Read operations (`GetStatus`, `GetCapability`, `GetConfiguration`, IPP `Get-*`, eSCL capabilities
  and status) need nothing special. `ChmpClient::query` refuses every other operation.
- Everything that changes the printer, uses ink or paper, or moves the scanner needs a `Confirm`
  token. The token only exists on the code path that follows an explicit confirmation: `--confirm`
  in the CLI, the confirmation dialog in the app.
- Every write flow also exists as a `Plan` that lists the exact requests without opening a
  connection; that is the CLI's dry run.
- Settings are validated against the capability the printer reports, written inside a
  `StartJob`/`EndJob` bracket and read back; a mismatch is an error. `EndJob` is always attempted.
- `washid --read-only` refuses every write method before it looks at the arguments.
- Service counters and anything that resets them are not implemented.

### The daemon

`washid` owns the connection to the printer: one CHMP operation at a time, serialised. It polls the
active printer (30 s by default), keeps the snapshot cache in `~/.cache/washi`, answers snapshot
requests from memory while a maintenance job holds the printer, and sends desktop notifications
once per condition (a tank turns low or empty, a new support code, the printer is back online). A
sleeping printer is read through IPP and the cache without waking it.

The paper names and the picture of the printer come from the printer too: `GetPrinterStrings`
and `GetPrinterIcon` read its IPP `printer-strings-uri` and `printer-icons` (read-only, so also with
`--read-only`) and keep the results in `~/.cache/washi` under the hashed printer key. A kept copy is
answered at once; after 30 days or when the firmware version differs from the last snapshot's it is
read again in the background, so only the very first call for a language waits for the printer. A
printer that could not be read is not asked again for a minute. The text and the picture are
fetched from the user's own device at run time and kept in the user's cache: they are not part of
Washi and are not redistributed (see [NOTICE.md](../NOTICE.md)); Washi's own paper names are only
the fallback for a printer that says nothing.

The D-Bus API is the interface `io.github.Jenari_Dev.Washi1` on `/io/github/Jenari_Dev/Washi`.
Structured values are JSON strings; long-running jobs return an id and report through signals
(`MaintenanceProgress`, `ScanPageReady`, `PrintProgress`, ...). The full list is in
[DBUS-API.md](DBUS-API.md).

### The app

`washi-app` is a QML application. The UI thread never waits on D-Bus: a worker thread owns the bus
connection and answers come back as signals. Pages share a few QML singletons (`Store` for the
printer state, `Nav` for navigation and dialogs, `Theme`, `Fmt`). Heavy image work (scan editing,
previews) runs in `washi-core` on worker threads. Texts are written with KDE's `i18n()` and
translated through gettext catalogs; switching the language re-evaluates every binding at once.
The printer's own names for papers (`Store.printerText`) follow the same switch: the app asks the
daemon for the names in the language shown, and Washi's own table answers until they arrive. The
picture of a printer is the local picture pack image if there is one
(`~/.local/share/washi/printers/<MODEL>.png`), else the printer's own, else Washi's line art.

## Languages

English is the source language; `po/ja.po` is the Japanese translation, written for Washi. The
daemon's notifications and messages are a small built-in table in both languages. `po/check.py`
fails on untranslated or fuzzy messages, mismatching placeholders and Japanese typography problems.

## Tests

- `cargo test --workspace` needs no printer. Integration tests run the CLI and the daemon against
  `washi-fake`, which also serves every write path (scans, print jobs).
- `crates/washi-gui/tests/qml/run.sh` runs the QML tests offscreen against fakes of the daemon and
  the image engine, in both languages, including a check that nothing is clipped.
- `tools/make-screenshots.sh` renders the screenshots in `docs/screenshots` from the fake printer.

## Packaging

The same files go into every package: three binaries, the compiled catalogs, the desktop entry,
the icon, the AppStream metadata, a D-Bus activation file and, outside Flatpak, a systemd user unit.
See `packaging/arch` and `packaging/flatpak`; `contrib/install-local.sh` installs into `~/.local`.
