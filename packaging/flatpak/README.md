# Flatpak

`io.github.Jenari_Dev.Washi.yml` builds Washi on the KDE 6.11 runtime (`org.kde.Platform` and
`org.kde.Sdk`, which is built on freedesktop 25.08, so the Rust extension is the 25.08 branch of
`org.freedesktop.Sdk.Extension.rust-stable`). It builds two small modules first: `poppler-data` and
`poppler` (only `pdftoppm` and `pdfinfo`, which Document Print uses to rasterise PDF pages).

## Build it

```
flatpak install --user flathub org.kde.Platform//6.11 org.kde.Sdk//6.11 \
    org.freedesktop.Sdk.Extension.rust-stable//25.08
python3 tools/flatpak-cargo-sources.py        # packaging/flatpak/cargo-sources.json (not committed)
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/io.github.Jenari_Dev.Washi.yml
flatpak run io.github.Jenari_Dev.Washi
```

`tools/flatpak-cargo-sources.py` lists the crates of `Cargo.lock` as flatpak-builder sources (the
build has no network), the same job as flatpak-builder-tools' `flatpak-cargo-generator.py` for a
lock file with crates.io dependencies only. The release workflow does the same and builds the
bundle with `flatpak/flatpak-github-actions`.

## What is different inside the sandbox

- **Permissions:** network (the printer is on the LAN; mDNS discovery too), Wayland and X11, DRI,
  and the notification service. No filesystem access: files come through the portal's file
  dialogs, and the manifest sets `QT_QPA_PLATFORMTHEME=xdgdesktopportal` so that Qt uses them.
- **The daemon is its own sandbox instance.** `washi-app` reaches `washid` by D-Bus activation, and
  Flatpak starts the activated service in a new instance of the same application. They share
  `~/.var/app/io.github.Jenari_Dev.Washi` (configuration, cache, data) and the files the portal
  hands out, but not the runtime directory, so `washid` writes the pages of a scan to
  `$XDG_RUNTIME_DIR/app/$FLATPAK_ID/washi`, which both instances see.
- **No systemd unit.** Notifications need the daemon to run: it runs while the app is open (and
  stays after the app is closed until the session ends). To start it at login, add
  `flatpak run --command=washid io.github.Jenari_Dev.Washi` to your session's autostart.
- **LibreOffice and Tesseract are not in the sandbox,** so Office files and OCR are not offered;
  the app checks for the tools at run time and hides what is missing.
- **QtPdf** may not be in the runtime (it comes with Qt WebEngine's sources); when the QML module
  is missing, Document Print falls back to previews rendered by poppler, which is bundled.

## Qt version

The app is developed against Qt 6.12; the KDE 6.11 runtime has Qt 6.11. The QML was checked against
the type information of Qt 6.12 for members that are newer than 6.11 and uses none. `build.rs`
finds the Qt and KI18n headers through `qmake` (`QT_INSTALL_HEADERS`), or `QT_INCLUDE_DIR` and
`KI18N_INCLUDE_DIR`, so the runtime's header layout does not matter.
