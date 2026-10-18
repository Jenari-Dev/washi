# Printer protocols used by Washi

Washi uses three channels to a Canon inkjet printer on the LAN. Two are open standards; the third is
the printer's local management interface, documented here from observation so that other free
software can interoperate. This page states facts only. It was verified on a PIXUS XK140 (firmware
1.060); other models that advertise `_canon-chmp._tcp` are expected to behave the same way, but
capabilities differ per model — always ask the printer (`GetCapability`) instead of assuming.

Things marked **unverified** have not been run against a real printer yet.

| Purpose | Protocol | Endpoint |
|---|---|---|
| Printing | IPP 2.0 / IPP Everywhere (PWG 5100.14) | `ipp://<host>:631/ipp/print` |
| Scanning | eSCL (Mopria) | `http://<host>/eSCL/` |
| Status, ink, settings, maintenance | CHMP + IVEC (below) | `http://<host>/canon/ij/command2/port1` |
| Web links of the printer's own page | plain HTTP | `http://<host>/JS_MDL/model.js` |

Discovery: mDNS. The printer advertises `_ipp._tcp`, `_ipps._tcp`, `_uscan._tcp`, `_uscans._tcp` and
`_canon-chmp._tcp` (TXT `mpath=http://<host>./canon/ij/command1/port1`). Prefer IPv4: `<host>.local`
often resolves to a link-local IPv6 address first.

## CHMP transport

Plain HTTP/1.1 on port 80, no authentication.

- A **session** is identified by a GUID in braces, e.g. `{0F8C…}`, sent in `X-CHMP-Session`.
  Headers on every request: `X-CHMP-Version: 1.3.0`, `X-CHMP-Session: {GUID}`, `Connection: Keep-Alive`.
  `X-CHMP-Timeout: <seconds>` is optional.
- **Command:** `POST /canon/ij/command2/port1`, `Content-Type: application/octet-stream`,
  body = IVEC XML. Reply `200` without body (no `Content-Length`, not chunked — don't read until close).
- **Reply:** `GET /canon/ij/command2/port1` on the **same TCP connection** with the same session.
  Reply `200`, `Transfer-Encoding: chunked`, body = IVEC XML. A GET on a different connection gets
  `409`, and so does an empty POST on the same one. The first GET has always carried the whole
  reply, about 0.3 to 0.5 s after the POST.
- **End session:** `POST` with `Content-Length: 0` and `Connection: Close`.
- Idle connections are closed by the printer after roughly 12–45 s; reconnect before the next command
  (never resend a command whose reply was lost).
- Other paths: `command1/port1` returns the device ID when asked with `X-CHMP-Property:
  DeviceID(Print)` (or `Scan`, `Fax`); `command2/port3` carries the printer's own scan protocol,
  which Washi does not use (it scans over eSCL); `command2/port2` and `command1/port3` are bulk-data
  ports.

## IVEC messages

```xml
<?xml version="1.0" encoding="utf-8" ?>
<cmd xmlns:ivec="http://www.canon.com/ns/cmd/2008/07/common/"
     xmlns:vcn="http://www.canon.com/ns/cmd/2008/07/canon/">
  <ivec:contents>
    <ivec:operation>GetStatus</ivec:operation>
    <ivec:param_set servicetype="print"></ivec:param_set>
  </ivec:contents>
</cmd>
```

Replies use `<operation>Response`, e.g. `GetStatusResponse`, with `<ivec:response>OK|NG</ivec:response>`
and `<ivec:response_detail>` (`NotSupportService`, `DataError`, `DeviceBusy`, `Suspended`,
`NotStart`, `ParameterError`, …). Services: `print`, `device`, `maintenance` (the XK140 also lists `pli`, `eis`; the
`scan` service is not served on `command2/port1`).

### Queries (safe, read-only)

| Operation | Service | Returns (selection) |
|---|---|---|
| `GetStatus` | `print` | `status` (`idle`, `processing`…), `current_support_code`, per-tray current paper (`currentpapertype`, `currentpapersize` + size in 1/100 mm, attribute `physicalinputbin`), `marker_info/ink` (`model`, `color`, `icon` none/warning/error, `level` %, `order`), `jobstatus` while a job runs |
| `GetStatus` | `device` | `total_paper_num` per tray, `ink_replacement_count` per colour, `head_replacement_count`, `networkupdate_available` (`unavailable` when there is no update), network interfaces (`communication_method`). While the printer is in auto power-off, `networkupdate_available` says `unavailable` even when an update is on offer (it reads `available` minutes after the printer wakes up), so a sleeping printer's `unavailable` is not an answer |
| `GetStatus` | `maintenance` | `jobinfo jobID=…` (`maintenance_operation`, `type`, `jobprogress`, `inputbin`, `papertype`) while a maintenance job runs; empty otherwise |
| `GetCapability` | `device` | model, firmware version (`firmver`), services, `pagelink` entries (relative addresses of the printer's own web pages, e.g. `page="maintenance"` is `index.html?page=PAGE_UTIL`), allowed values of every device setting (below) |
| `GetCapability` | `maintenance` | `Cleaning/type value=…` (+ `inkgroup` list), `TestPrint/type value=…` (+ tray, paper size/type, sheets) |
| `GetCapability` | `print` | media per tray, borderless/duplex per media, print qualities, margins |
| `GetConfiguration` | `device` | current device settings |
| `GetConfiguration` | `print` | default job settings |

Some replies carry device identifiers (serial number, hardware addresses, cloud ids). Washi does not
store them and blanks them when it prints a raw reply.

### Device settings

Written with `SetConfiguration` (service `device`) using the same element structure that
`GetConfiguration` returns, inside a job (see below). Ranges come from `GetCapability device`.

| Element | Values (XK140) |
|---|---|
| `autopoweron` | `ON`, `OFF` |
| `autopoweroff/mode`, `autopoweroff/time` | `ON`/`OFF`; minutes `15,30,60,120,240` |
| `autopoweroff_standalone/…` | same (when not connected) |
| `silentmode/mode`, `starttime`, `endtime` | `ON`, `OFF`, `time`; `HHMM` |
| `papergap` | `auto`, `wide` (the printer's Remote UI and Canon's apps call this "Prevent paper abrasion"; `wide` raises the print head a little for thick or curled paper) |
| `drylevel` | `1`–`5` |
| `setregistration id="1..3"/item sign="A".."M","a".."e"` | integer ranges per sign (e.g. −5…5; **writing is unverified**) |
| `vcn:auto_printer_rom_update` | `ON`, `OFF` (automatic firmware update) |
| `vcn:mqtt_connection` | `ON`, `OFF` (the printer's cloud link) |

Washi reads every setting back after a write and treats a mismatch as an error.

### Jobs and maintenance

Changes and maintenance run inside a job:

1. `StartJob` — `servicetype` = `maintenance` or `device`, `<ivec:jobID>00000002</ivec:jobID>`,
   `<ivec:bidi>1</ivec:bidi>` (`0`/`1`, not `ON`/`OFF`).
2. The operation with the same `jobID`:
   - `Cleaning` — `<ivec:type>regular|deep</ivec:type>`, `<ivec:inkgroup>all|group1|group2</ivec:inkgroup>`.
     Other models may list more types, e.g. `roller` (roller cleaning) and `platen` (bottom plate
     cleaning), which take no ink group (**unverified**; the XK140 lists neither). Some models list a
     separate `RollerCleaning` operation instead (**unverified**). Use only what
     `GetCapability maintenance` lists.
   - `TestPrint` — `<ivec:type>nozzle_check|auto_registration|manual_registration|regi_check</ivec:type>`
     (`manual_registration` also takes `<ivec:id>`). All four use A4 plain paper from the cassette on
     the XK140; the number of sheets is in the capability reply.
   - `SetConfiguration` — setting elements as above.
3. Poll `GetStatus print` / `GetStatus maintenance` until `jobinfo` is empty and the status is `idle`.
   While a maintenance job runs, `GetStatus print` may answer `NG Suspended` — that means "busy".
4. `EndJob` with the same `jobID`. If the printer has already closed the finished job it answers
   `NG NotStart`; treat that as success. `CancelJob` takes the same `jobID`.

Observed on the XK140 (nozzle check, run by hand): StartJob, TestPrint and EndJob all answered `OK`,
the pattern printed correctly, and the whole job took about 25 s including the wake-up.

A job sent to a printer in auto power-off wakes it (when `autopoweron` is `ON`). While asleep,
`GetStatus print`/`maintenance` answer `NG Suspended`; `device` queries and IPP keep working.

### Operations without a job

| Operation | Service | Notes |
|---|---|---|
| `PowerOff` | `device` | no parameters, no job |
| `PowerOn` | `device` | no job; one parameter, `<ivec:poweronmode>ModeA</ivec:poweronmode>`; wakes a printer in auto power-off. Without the parameter the printer answers `NG ParameterError` and stays asleep. Verified on the XK140 (firmware 1.060, 2026-10-14): with `ModeA` the reply is `OK` and `GetStatus print` stops saying `Suspended` after a few seconds. Any job wakes the printer as well |
| `VendorCmd` | `device` | `<vcn:ijoperation>UpdateFRom</vcn:ijoperation>`: asks the printer to update its own firmware (**unverified**). Only send it when `networkupdate_available` says an update is offered; then the printer goes away and comes back with the new `firmver`. Never switch the printer off meanwhile |

Counters that belong to servicing the printer are not documented here and not used by Washi.

## The printer's web page

The printer's web server (port 80, the "Remote UI") serves `GET /JS_MDL/model.js`, a small script of
`var g_…` settings. Four of them are web addresses: ink shop, manual, help for an error (ends in
`ERR=NO` — replace `NO` with the support code) and support (HTML-escaped: `&amp;`).

**The request must name a language**, otherwise the printer answers `404 A specification file is
not found`: send `Accept-Language: en` or `ja`. The printer words the addresses for that language.
The same applies to the message tables under `/LANG/`. The pages of the Remote UI itself, for
example `index.html?page=PAGE_UTIL` (maintenance, quiet mode, firmware update, …), ask for the
printer's administrator password.

**It is served only while the printer is awake.** In auto power-off every variant gets the same
`404` (with or without `Accept-Language`, a version query, a cookie or a referer); a few minutes
after the printer wakes up the same request answers `200`. Washi therefore keeps the parsed
addresses (`~/.cache/washi/<key>-links.json`, like the printer's strings and picture; read again
after 30 days or a firmware change) and uses the kept copy while the printer sleeps.

Washi validates every address (http or https, a plain host name, no credentials) before it offers
to open it.

## IPP notes (XK140)

- `document-format-supported`: `image/jpeg`, `image/pwg-raster` (600 dpi, `srgb_8`/`sgray_8`),
  `image/urf`, `application/octet-stream` — **no PDF**: documents are rasterised by the client.
- Operations: Print-Job, Validate-Job, Create-Job, Send-Document, Cancel-Job, Get-Job-Attributes,
  Get-Jobs, Get-Printer-Attributes, Cancel-My-Jobs, Close-Job and Identify-Printer (actions `flash`
  and `sound`). `page-ranges-supported` is false: pages are selected on the client.
- `media-source-supported`: `auto`, `main` (cassette), `rear`, `disc`.
- `media-type-supported` includes `stationery`, `photographic`, `envelope` and a list of vendor
  keywords starting with `com.canon-`; the printer's own media database says which size and type
  combinations exist in which tray and whether they can be printed borderless.
- `media-col-database` is only returned when requested on its own (about 366 KB, 1335 entries on the
  XK140); a request for `all` returns it empty. An entry has `media-size` (1/100 mm), the four
  margins, `media-source` and `media-type`.
- JPEG jobs: 16 to 19200 px per side, up to 16 MB. PWG raster: 600×600 dpi only, and
  `pwg-raster-document-sheet-back` is `rotated`.
- Disc printing: `media-size` 120×120 mm (`om_square_120x120mm`), `media-source=disc`, `media-type`
  `com.canon-mtdisca` / `com.canon-mtdiscb`, margins 0.1 mm. In IVEC the size is
  `custom_canon_disc_120x120mm`, and the printable area is 2834×2834 px at 600 dpi.
- `marker-levels` keeps working while the CHMP print service sleeps; it is what Washi shows then.

## The printer's own words and picture (IPP Everywhere)

Printers that follow PWG 5100.13 (IPP Everywhere) name two more things in `Get-Printer-Attributes`.
On the XK140 (firmware 1.060) both are read-only and need no password:

- `printer-strings-uri`: a localised **strings file** with the printer's own display names for the
  keywords it uses. The reply follows the request's `attributes-natural-language`: `en` gives
  `http://<host>./strings/en.txt`, `ja` gives `.../strings/ja.txt`, and a language the printer does
  not list gives an empty value. `printer-strings-languages-supported` lists the languages (the
  XK140: `ja en fr de it es pt nl da no sv fi pl cs ru tr ar zh-cn zh-tw ko th id`). The file is
  fetched with a plain `GET` from the printer's web server (port 80) and is in Apple's `.strings`
  format: `"key" = "value";` lines, `/* */` and `//` comments, the escapes `\"` and `\\`, UTF-8
  or UTF-16 with a byte order mark. The keys are `<attribute>.<keyword>`; on the XK140 all of them
  are `media-type.<keyword>` (the `com.canon-` papers, in the language of the request: the Japanese
  file has the Japanese market's names). Keys for other attributes, such as `media-source.*`, are
  used when a printer has them.
- `printer-icons`: the addresses of **pictures of the printer** (PNG with an alpha channel, 128×128
  and 512×512 on the XK140), `GET` from the same web server.

Washi's rules for these:

- The host and port in a returned address are ignored. Only its path (and query) is requested, from
  the printer Washi already talks to, so a printer cannot send the app to another machine. Only
  `http` addresses are accepted.
- When the address does not change with the language, `<directory>/<language>.<extension>` is used
  instead, but only for a language in `printer-strings-languages-supported`.
- Strings: at most 512 KB, 4096 entries, keys of ASCII graphic characters (160 at most), values of
  256 characters at most with control and bidirectional-control characters removed. A syntax error
  costs the entry it is in, not the file.
- Pictures: PNG only, at most 2 MB and 2048×2048 pixels; of several pictures the largest is used.
  The transparent margin is cut off so that the product shot fills its space in the banner.
- Both are kept per printer in `~/.cache/washi` and read again after 30 days or when the firmware
  version changes (see [ARCHITECTURE.md](ARCHITECTURE.md)).

## eSCL notes (XK140)

Platen only (no feeder); colour modes `Grayscale8` and `RGB24` (no `BlackAndWhite1`); 75, 100, 150,
200, 300, 600 and 1200 dpi; JPEG and PDF; intents Document, Photo, TextAndGraphic, Preview and Auto;
scan area 2550×3508 units of 1/300 inch. Standard `POST /eSCL/ScanJobs` → `GET {job}/NextDocument`
until `404`. The capabilities carry the serial number and UUID; Washi does not keep them.

## Support codes

`current_support_code` is a code such as `1000` (out of paper) or `1600` (ink out); the printer does not
send message text. Washi groups codes into families with its own short messages (English and
Japanese); the manual of your model has the full list.
