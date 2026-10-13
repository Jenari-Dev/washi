#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Build Washi and install it for the current user (no root needed):
#   ~/.local/bin/{washi,washid,washi-app}, the washid user service, D-Bus activation,
#   desktop entry and icon. Optional: --host HOST saves the default printer,
#   --image-pack DIR copies printer renders (*.png named by model, e.g. XK140.png) from DIR into
#   ~/.local/share/washi/printers/ for the banner (local use only; never redistributed).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
host=""
pack=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --host) host="$2"; shift 2 ;;
        --image-pack) pack="$2"; shift 2 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

bin="$HOME/.local/bin"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

echo "==> building the whole workspace (release)"
(cd "$here" && cargo build --release)

echo "==> installing binaries to $bin"
install -Dm755 "$here/target/release/washi" "$bin/washi"
install -Dm755 "$here/target/release/washid" "$bin/washid"
install -Dm755 "$here/target/release/washi-app" "$bin/washi-app"

echo "==> installing the translations to $share/locale"
for mo in "$here"/target/release/share/locale/*/LC_MESSAGES/washi.mo; do
    [[ -f "$mo" ]] || continue
    lang="$(basename "$(dirname "$(dirname "$mo")")")"
    install -Dm644 "$mo" "$share/locale/$lang/LC_MESSAGES/washi.mo"
done

echo "==> installing the user service, D-Bus activation, desktop entry and icon"
# Development builds before the first release used another application id; drop their files.
rm -f "$share"/dbus-1/services/io.github.[Jj]enari[Dd]ev.[Ww]ashi.service \
    "$share"/applications/io.github.[Jj]enari[Dd]ev.[Ww]ashi.desktop \
    "$share"/icons/hicolor/scalable/apps/io.github.[Jj]enari[Dd]ev.[Ww]ashi.svg
install -Dm644 "$here/contrib/washid.service" "$units/washid.service"
install -Dm644 "$here/contrib/io.github.Jenari_Dev.Washi.service" \
    "$share/dbus-1/services/io.github.Jenari_Dev.Washi.service"
install -Dm644 "$here/contrib/io.github.Jenari_Dev.Washi.svg" \
    "$share/icons/hicolor/scalable/apps/io.github.Jenari_Dev.Washi.svg"
install -Dm644 "$here/contrib/io.github.Jenari_Dev.Washi.metainfo.xml" \
    "$share/metainfo/io.github.Jenari_Dev.Washi.metainfo.xml"
sed "s|^Exec=washi-app|Exec=$bin/washi-app|" "$here/contrib/io.github.Jenari_Dev.Washi.desktop" \
    > "$share/applications/io.github.Jenari_Dev.Washi.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$share/applications" || true

if [[ -n "$pack" ]]; then
    echo "==> copying printer renders from $pack (local image pack)"
    mkdir -p "$share/washi/printers"
    cp -v "$pack"/*.png "$share/washi/printers/"
fi

if [[ -n "$host" ]]; then
    echo "==> saving default printer $host"
    "$bin/washi" config set-host "$host"
fi

echo "==> (re)starting washid"
systemctl --user daemon-reload
systemctl --user enable washid.service
systemctl --user restart washid.service
systemctl --user --no-pager --lines=5 status washid.service || true

echo "Done. Start Washi from the application menu or run: washi-app"
