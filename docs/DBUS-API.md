# The daemon `washid` and its D-Bus API

The desktop app and the notifications are built on the user daemon `washid`. This page documents how
to run it, its configuration and the D-Bus interface, for scripts and other front-ends.

## Running it

```
washid                      # session bus name io.github.Jenari_Dev.Washi
washid --read-only          # refuses every write; for UI tests and the first runs
washid --printer HOST       # also know HOST (repeatable); --poll-interval SECONDS
```

The packages install a systemd user unit (`systemctl --user enable --now washid`), a D-Bus
activation file and the desktop entry that launches `washi-app`; for a `~/.local` install,
`contrib/install-local.sh` puts `contrib/washid.service` and
`contrib/io.github.Jenari_Dev.Washi.service` in place. The app starts the daemon on demand through
D-Bus activation.

It polls the active printer (30 s by default), keeps the cache current, serializes all access to a
printer (one CHMP operation at a time; a maintenance job holds the printer, and snapshot requests
are answered from memory with `activity` set while it runs), and sends desktop notifications via
`org.freedesktop.Notifications` when an ink tank turns low or empty, when a new error code
appears, and when the printer comes back online. Each condition is announced once and re-armed
when it clears. Configuration, all optional (`~/.config/washi/config.toml`):

```toml
[daemon]
poll_interval_secs = 30      # minimum 5

[notifications]
enabled = true               # master switch
ink = true                   # low / empty tanks
errors = true                # new support code
online = true                # unreachable -> reachable again

[ui]
language = "system"          # "en", "ja" or "system": the language of the app and of the
                             # daemon's notifications (see "Languages" below)
```

D-Bus API, interface `io.github.Jenari_Dev.Washi1` on object `/io/github/Jenari_Dev/Washi`
(structured values are JSON strings):

| Member | Meaning |
|---|---|
| `ListPrinters() -> s` | `[{host, name, source, active, power, last_updated}]` |
| `Discover() -> s` | mDNS browse (~3 s), merge into the list, return it. Allowed read-only |
| `SetActivePrinter(host)` | select the polled printer; an unknown well-formed host is added and remembered |
| `GetSnapshot(host) -> s` | latest `PrinterSnapshot` (from memory if younger than the poll interval, else read); `""` = active |
| `Refresh(host) -> s` | read now, device information (model, firmware version, update available) included |
| `GetSettings(host) -> s` | `{current, allowed}` device settings |
| `GetMaintenanceCapability(host) -> s` | cleaning types/ink groups and test prints |
| `RunMaintenance(host, op_json) -> s` | WRITE: start a job, returns its id. `{"confirm": true, "op": "clean", "kind": "deep", "group": "all"}`; ops: `clean` (`kind` regular, deep, or **roller** / **platen** for models that list those `Cleaning` types: no group; unverified), `nozzle_check`, `align_auto`, `align_check`, `align_manual` (+`id`), `roller_clean` (the `RollerCleaning` operation) |
| `ApplySettings(host, changes_json) -> s` | WRITE: validate, write, read back; returns `{changed, changes: [{field, before, after}], current}`. Keys: `auto_power_on`, `auto_power_off`, `auto_power_off_standalone`, `quiet`, `dry_level`, `paper_gap`, `auto_firmware`, `cloud_link`, `setregistration` (manual head alignment values, `{"1": {"A": 1, "B": -2, ...}}`: set ids 1 to 3, one value per sign, checked against the ranges `GetSettings` reports; send whole sets; **writing is unverified**); needs `"confirm": true` |
| `GetScannerCapabilities(host) -> s` | `{"capabilities": ..., "status": ...}` of the eSCL scanner. Read-only, allowed with `--read-only` |
| `StartScan(host, settings_json) -> s` | WRITE: scan one page, returns the session id. `{"confirm": true, "size": "a4", "resolution": 300, "mode": "color", "type": "document"}` (`size`: a4, letter, a5, b5, l, 2l, postcard, card, auto; `mode`: color, gray, bw; `type`: document, photo) |
| `PrintFile(host, path, options_json) -> s` | WRITE: print a photo, PDF or Office file (absolute path), returns the job id. Options (all optional besides `confirm`): `size`, `media_type`, `source`, `borderless`, `quality`, `color`, `copies`, `pages`, `layout`, `duplex`, `orientation` (`auto` turns every photo or page to fill the sheet; `portrait` / `landscape` keep content upright on a sheet used that way), `job_name` |
| `PrintDisc(host, label_json) -> s` | WRITE: print a disc label, returns the job id. `{"confirm": true, "image": "/path/art.png", "texts": [{"text": "My mix", "y_mm": 32, "size_pt": 20}], "inner_mm": 22, "outer_mm": 116, "media": "a"}`. A text takes `"ring": {"radius_mm": 48, "start_deg": 0, "clockwise": true, "letter_spacing_pt": 0}` to follow the ring (its position and rotation are then unused) |
| `CancelJob(job) -> b` | stop a running scan or print job at its next opportunity (between sheets and while the printer works; a page that is already being scanned is finished) |
| `PrintFiles(host, paths_json, options_json) -> s` | WRITE: like `PrintFile` for several files (a JSON array of absolute paths): photos for the 2/4/6/9/16-in-1 layouts share sheets in the order given |
| `GetPrintCapabilities(host) -> s` | READ: what the printer prints on, boiled down from IPP and its 1335-entry `media-col-database`: `{sources, media_types, sizes: [{key, label, width, height}], media: {source: {media type: {size key: bits}}}, qualities, duplex, copies_max, ...}`; the bits are 1 = with margins, 2 = borderless. Cached for 30 minutes. Allowed read-only |
| `GetWebLinks(host, code) -> s` | READ: the web pages the app can point to, `{ink, help, error, support, printer_page, maintenance_page: {url, source: "maker" \| "printer"}, from_printer}`: read from the printer's `model.js` and device capability (cached for 30 minutes), `error` only with a non-empty support `code`, the maker's addresses get the app's language, every link falls back to the printer's own page. Allowed read-only; nothing is opened |
| `GetPrinterStrings(host, language) -> s` | READ: the printer's own display names for its keywords in a language (`en`, `ja`, ...) as a JSON object `{"media-type.<keyword>": "Name", ...}`; `{}` when the printer has none in that language. From IPP `printer-strings-uri` (`.strings` file), kept in `~/.cache/washi`: a kept copy is answered at once and read again in the background after 30 days or when the firmware version changed; the first call for a language waits for the printer, and a printer that could not be read is not asked again for a minute. Allowed read-only |
| `GetPrinterIcon(host) -> s` | READ: the path of a PNG picture of the printer (IPP `printer-icons`, transparent margin cut off) in `~/.cache/washi`, or `""` when the printer offers none. Refreshed like `GetPrinterStrings`. Allowed read-only |
| `UpdateFirmware(host, options_json) -> s` | WRITE: **unverified** firmware update (`VendorCmd UpdateFRom`), `{"confirm": true}`; only when the printer says an update is available. Returns the job id; progress arrives as `MaintenanceProgress` (stages `sent`, `restarting`, `waiting`, `back`), the end as `MaintenanceFinished` whose message names the old and the new version |
| `Wake(host, options_json) -> s` | WRITE: send IVEC `PowerOn` (power-on mode `ModeA`) to a printer in auto power-off (`{"confirm": true}`). Verified on a real XK140 (firmware 1.060). The call waits up to about 8 seconds, reading the printer again, so `SnapshotChanged` shows it coming up, and returns `{"sent": true, "verified": bool, "note"}`; `verified` is true when that follow-up status showed the printer awake. A slower printer keeps being read in the background |
| `GetNotificationSettings() -> s`, `SetNotificationSettings(json) -> s` | The `[notifications]` switches `{"enabled", "ink", "errors", "online"}`; Set takes any subset and saves them to `config.toml` (other keys are kept). Set is refused with `--read-only`, which writes no files |
| `ForgetPrinter(host) -> s` | Remove a printer from the list: `{"forgotten": true, "persistent": bool, "active": host-or-null}`. Never touches the printer. Printers from `--printer` / `config.toml` (and everything when read-only) return when the daemon restarts |
| `SetLanguage(language) -> s` | `en`, `ja` or `system` (the daemon's own locale): the language of the daemon's notifications and of the messages in its errors and job signals; returns the one now in use. The app calls it when the user switches language and whenever it connects; it does not touch `config.toml` (the app saves `[ui] language`), is not a write to the printer and works with `--read-only` |
| `Version() -> s`, properties `ReadOnly`, `PollIntervalSecs`, `Language` | |
| Signals `SnapshotChanged(host, json)`, `PrintersChanged()`, `MaintenanceProgress(job, stage, message)`, `MaintenanceFinished(job, ok, message)` | stages: `job-started`, `accepted`, `waiting`, `job-ended`, `closing` |
| Signals `ScanPageReady(session, page_index, temp_path)`, `ScanFinished(session, ok, message)` | `temp_path` is a JPEG in a private (0700) directory under `$XDG_RUNTIME_DIR/washi/`; directories older than a day are removed |
| Signals `PrintProgress(job, stage, message)`, `PrintFinished(job, ok, message)` | stages: `preflight`, `preparing`, `validated`, `sending`, `submitted`, `state`, `finished` |

Errors are named `io.github.Jenari_Dev.Washi.Error.{ReadOnly,UnknownPrinter,InvalidArgs,NotConfirmed,Busy,Printer}`.
Printers are addressed by the `host` string from `ListPrinters`; unknown hosts are refused.

```
busctl --user call io.github.Jenari_Dev.Washi /io/github/Jenari_Dev/Washi \
    io.github.Jenari_Dev.Washi1 GetSnapshot s ""
```

The daemon never logs host names, serial numbers or hardware addresses (printers appear as a short
hash).
