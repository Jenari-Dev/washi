# Contributing to Washi

Thank you for helping. Washi is a small project with one maintainer, so a short issue or a focused
pull request is the best way to start. English and Japanese are both fine in issues.

## Reporting a problem

Open an issue with the **bug report** template. Please include:

- the printer model and its firmware version (Device Information in the app, or `washi info`),
- what you did, what you expected and what happened,
- the output of `washi status --json` or `washi -v ...` when it is about the printer.

> **Never post serial numbers, MAC / hardware addresses, IP addresses of your network, cloud
> identifiers or printer host names in an issue.** Replies from the printer contain them. `washi raw`
> blanks them for you; check anything else you paste. Edit them out if you are not sure.

A report that something works on a model other than the PIXUS XK140 is just as valuable as a bug:
use the **printer report** template.

## Building

You need Rust 1.85 or newer, Qt 6 (base, declarative, svg), KDE's KI18n, gettext and a C++
compiler. On Arch Linux:

```
sudo pacman -S --needed rust base-devel qt6-base qt6-declarative qt6-svg ki18n gettext poppler
cargo build --release
```

Run the app against the fake printer, with nothing real involved:

```
target/release/washi-fake-printer --chmp-port 18080 --ipp-port 18631 &
target/release/washid --printer 127.0.0.1:18080 --ipp-port 18631 &
target/release/washi-app
```

`contrib/install-local.sh` installs a build into `~/.local` (binaries, user service, desktop entry).

## Testing the app without a printer

`crates/washi-gui/tests/qml/run.sh` is a Qt Quick Test runner with the app's own i18n and the real
catalogs, offscreen, against fakes of the service and the image engine: it also checks that every
visible text follows a switch of the language and that nothing is clipped in either language.
`crates/washi-gui/tests/screens.sh fake|sleep|i18n|r3` takes headless screenshots of the pages and
dialogs against fake printers in different states (`real` runs only `washid --read-only` against
your own printer). `screens.sh i18n` writes English and Japanese pages to `target/screens-i18n/`.
`washi-fake-printer` takes `--asleep`, `--scan-pages N`, `--scan-image FILE.jpg`, `--roller`,
`--code N`, `--firmware-update VERSION`, `--cleaning-types roller,platen` and `--no-model-js`.

## Before you open a pull request

These are the checks CI runs; please run them first:

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 po/check.py
QT_QPA_PLATFORM=offscreen crates/washi-gui/tests/qml/run.sh
```

- Keep changes small and say why in the commit message.
- Follow the existing style (`rustfmt.toml` is in the repository). New behaviour needs a test; the
  fake printer in `crates/washi-fake` serves the CHMP, IPP and eSCL write paths.
- **Anything that changes the printer, uses ink or paper or moves the scanner must stay behind the
  confirmation token** (`Confirm`), and must have a dry-run plan. Do not add a way around it.
- Do not add anything that touches service counters, ink or cartridge authentication, or the
  printer's firmware image.
- Contributions are licensed under GPL-2.0-or-later, like the rest of the project (the source
  files carry an `SPDX-License-Identifier` line; add it to new files).

## Where the protocol knowledge comes from

Washi's protocol code is written from what the author observed on a printer he owns, see
[`docs/PROTOCOL.md`](docs/PROTOCOL.md) and [`NOTICE.md`](NOTICE.md). To keep it that way:

- **Do not contribute anything taken from the printer maker's software, firmware, documents or
  web pages**: no decompiled or disassembled code, no strings, texts, images, icons, fonts or
  screenshots of their apps, no files from their installers. Facts you observed on your own
  printer yourself (a message name, an element, a value range) are welcome, written in your own
  words.
- Describe behaviour, not the maker's implementation. Mark things you have not run on a real
  printer as unverified.
- Names of the maker and its paper products may appear only to identify hardware and media.

## Translations

The interface is translated with gettext: `po/washi.pot` holds the English texts and `po/ja.po` the
Japanese translation.

1. Change or add an `i18n("...")` string in the QML, then run `po/update.sh`. It refreshes
   `po/washi.pot` and merges it into every `po/*.po`.
2. Translate what is new in the `.po` files and run `python3 po/check.py`. It fails on untranslated
   or fuzzy messages, a stale `washi.pot`, placeholders that do not match, English text in QML that
   is not marked for translation, and on Japanese typography (full-width punctuation, no spaces
   between Japanese words, half-width digits, 「」 for quotes).
3. A new language is a new `po/<code>.po` (`msginit -i po/washi.pot -l <code> -o po/<code>.po`) and an
   entry in the language switch (`crates/washi-gui/qml/LangSwitch.qml`). Please write the
   translation yourself and do not copy texts from the printer maker's software.

The daemon's own notifications are a small table in `crates/washid/src/messages.rs`; the support-code
messages are in `crates/washi-core/src/support.rs`.

## Screenshots

`tools/make-screenshots.sh` renders the screenshots in `docs/screenshots` from the fake printer in a
private namespace, so no real host name, address or serial number can appear. Do not add screenshots
taken against a real printer or of anyone else's software.

## Code of conduct

Be kind and assume good faith. Harassment is not tolerated; the maintainer may remove comments and
block accounts.
