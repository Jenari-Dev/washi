# Arch Linux package

`PKGBUILD` builds Washi from the release tarball of the tag `v0.1.3`
(`https://github.com/Jenari-Dev/washi/archive/refs/tags/v0.1.3.tar.gz`).

## Build and install

```
git clone https://github.com/Jenari-Dev/washi.git
cd washi/packaging/arch
makepkg -si          # builds, runs the tests, installs with pacman
```

`makepkg --nocheck` skips the test suite (it needs no printer). The build takes a few minutes: it
compiles the Rust workspace and the Qt app (cxx-qt). Dependencies: `cargo`, `gettext` and
`pkgconf` to build; `qt6-base`, `qt6-declarative`, `qt6-svg`, `ki18n` and `poppler` at run time.

The package installs the three programs (`washi`, `washid`, `washi-app`), the Japanese
translation, the desktop entry, icon, AppStream metadata, a D-Bus activation file and a systemd
user unit. To get desktop notifications without opening the app:

```
systemctl --user enable --now washid
```

## Optional dependencies

| Package | For |
|---|---|
| `tesseract`, `tesseract-data-eng`, `tesseract-data-jpn` | text recognition (OCR) of scans |
| `libreoffice` | printing Office documents |
| `qt6-webengine` | PDF preview with QtPdf in Document Print (poppler is used without it) |
| `qt6-wayland` | native Wayland |
| `xdg-utils` | opening web links |

## Notes

- `options=('!lto')`: the code cxx-qt generates does not survive link-time optimisation.
- `sha256sums=('SKIP')` is replaced with the real checksum of the tarball once a release is tagged
  (`updpkgsums` from `pacman-contrib`); `.SRCINFO` is regenerated with `makepkg --printsrcinfo >
  .SRCINFO` after every change of the PKGBUILD.
- The project is GPL-2.0-or-later; the binaries are distributed under the terms of the
  GPL-3.0-or-later because some Rust crates are Apache-2.0 only (see `NOTICE.md`).

## AUR

A `washi` package for the AUR can use this PKGBUILD and `.SRCINFO` unchanged (after the checksum
is filled in). Please report packaging problems in the project's issue tracker.
