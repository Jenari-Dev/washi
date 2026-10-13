#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Render the README / AppStream screenshots (docs/screenshots/<name>-<lang>.png), headless.
#
#   cargo build --release            # washi-app, washid, washi-fake-printer
#   tools/make-screenshots.sh [OUTDIR]
#
# Everything runs against washi-fake-printer (never a real printer), inside a private user, mount
# and network namespace in which the fake printer answers on the name `printer.local` and on ports
# 80 / 631, so no real host name or address is ever drawn. The configuration, cache and data
# directories are empty temporary ones: a locally installed printer picture pack is not used, only
# Washi's own line art. The sample pictures are generated here with ImageMagick. The PNGs are
# stripped of all metadata.
#
# Needs: unshare, ip (iproute2), dbus-run-session, ImageMagick (`magick`).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/docs/screenshots}

if [ -z "${WASHI_SHOTS_STAGE:-}" ]; then
    # Stage 1: a private user + mount + network namespace (no root needed).
    exec unshare --map-root-user --mount --net env WASHI_SHOTS_STAGE=2 "$0" "$@"
fi

if [ "$WASHI_SHOTS_STAGE" = 2 ]; then
    # Stage 2: name the fake printer, empty the user's directories, then continue on a private
    # session bus.
    ip link set lo up
    work=$(mktemp -d)
    printf '127.0.0.1 localhost\n127.0.0.1 printer.local printer\n' >"$work/hosts"
    sed 's/^hosts:.*/hosts: files dns/' /etc/nsswitch.conf >"$work/nsswitch.conf"
    mount --bind "$work/hosts" /etc/hosts
    mount --bind "$work/nsswitch.conf" /etc/nsswitch.conf
    HOME="$work/home" XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache" \
        XDG_STATE_HOME="$work/state" XDG_DATA_HOME="$work/data" XDG_RUNTIME_DIR="$work/run"
    mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
    mkdir -m 700 "$XDG_RUNTIME_DIR"
    export HOME XDG_CONFIG_HOME XDG_CACHE_HOME XDG_STATE_HOME XDG_DATA_HOME XDG_RUNTIME_DIR
    exec env WASHI_SHOTS_STAGE=3 WASHI_SHOTS_WORK="$work" dbus-run-session -- "$0" "$@"
fi

app=${WASHI_APP:-$root/target/release/washi-app}
washid=${WASHID_BIN:-$root/target/release/washid}
fake=${FAKE_BIN:-$root/target/release/washi-fake-printer}
mkdir -p "$out"
work=$WASHI_SHOTS_WORK
pids=""
cleanup() {
    for p in $pids; do kill "$p" 2>/dev/null || true; done
    rm -rf "$work"
}
trap cleanup EXIT INT TERM

# Our own art only: no printer picture pack, no real data directory.
unset WASHI_LANG WASHI_HOST

im=$(command -v magick || true)
[ -n "$im" ] || { echo "ImageMagick (magick) is needed for the sample pictures" >&2; exit 1; }

samples="$work/samples"
mkdir -p "$samples"
# Landscape and portrait "photos" drawn from shapes, and a page of text for the scanner.
"$im" -size 1800x1200 gradient:'#27407f-#f7b36c' \
    -fill '#fff0c4' -draw 'circle 1260,700 1260,540' -blur 0x7 \
    -fill '#35496d' -draw 'polygon 0,820 300,640 560,780 880,560 1240,800 1500,680 1800,820 1800,1200 0,1200' \
    -fill '#1c2c4a' -draw 'polygon 0,990 420,870 820,960 1220,880 1800,1010 1800,1200 0,1200' \
    -blur 0x2 -quality 90 "$samples/photo.jpg"
"$im" -size 1200x1800 gradient:'#1f5f6b-#f4d7a0' \
    -fill '#fff7dc' -draw 'circle 380,520 380,380' -blur 0x7 \
    -fill '#2a4d4b' -draw 'polygon 0,1150 260,980 520,1100 820,900 1200,1120 1200,1800 0,1800' \
    -fill '#17302f' -draw 'polygon 0,1400 400,1280 800,1360 1200,1300 1200,1800 0,1800' \
    -blur 0x2 -quality 90 "$samples/portrait.jpg"
# A light picture for the disc label, so that dark text on it can be read.
"$im" -size 1400x1400 gradient:'#fbe7b5-#bfe3ee' \
    -fill '#f5a9b8' -draw 'circle 420,980 420,720' -fill '#9ad1c7' -draw 'circle 1000,420 1000,200' \
    -fill '#ffd36e' -draw 'circle 1040,1060 1040,900' -blur 0x40 -quality 90 "$samples/disc.jpg"
"$im" -size 1240x1754 xc:'#fbf8f0' -fill '#222' -pointsize 54 -annotate +100+160 'Sample page' \
    -pointsize 30 -annotate +100+250 'A page of text so that the rotate, crop' \
    -annotate +100+300 'and correction screens have something to show.' \
    -fill '#555' -draw 'rectangle 100,450 1140,462' -draw 'rectangle 100,520 1140,532' \
    -draw 'rectangle 100,590 760,602' -fill '#2a5db0' -draw 'rectangle 100,900 500,1300' \
    -fill '#c93a3a' -draw 'circle 800,1100 800,1250' -rotate 2.2 -gravity center \
    -crop 1240x1754+0+0 +repage -quality 90 "$samples/scan.jpg"

"$fake" --chmp-port 80 --ipp-port 631 --scan-pages 2 --scan-image "$samples/scan.jpg" \
    </dev/null >"$work/fake.log" 2>&1 &
pids="$pids $!"
sleep 0.5
"$washid" --printer printer.local --ipp-port 631 >"$work/washid.log" 2>&1 &
pids="$pids $!"
sleep 1

shot() { # name lang page-environment...
    name=$1 lang=$2
    shift 2
    raw="$work/$name-$lang.png"
    env QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen WASHI_LANG="$lang" \
        WASHI_SCREENSHOT="$raw" "$@" "$app" 2>&1 | grep -v 'kf.i18n' || true
    [ -s "$raw" ] || { echo "no screenshot for $name-$lang" >&2; exit 1; }
    "$im" "$raw" -strip -define png:exclude-chunk=all -define png:compression-level=9 \
        "$out/$name-$lang.png"
    echo "$out/$name-$lang.png"
}

for lang in en ja; do
    if [ "$lang" = ja ]; then
        disc_text="夏のドライブ 2026 - 好きな曲|A面"
        disc_ring="0:50:0:cw:3"
    else
        disc_text="Summer mix 2026 - road trip songs|Side A"
        disc_ring="0:50:0:cw:3"
    fi
    shot home "$lang" WASHI_PAGE=home WASHI_SIZE=1040x600
    shot consumables "$lang" WASHI_PAGE=consumables
    shot utilities "$lang" WASHI_PAGE=utilities WASHI_SIZE=1040x820 WASHI_SHOT_DELAY_MS=1000
    shot photo-print "$lang" WASHI_PAGE=photo-print WASHI_FILE="$samples/photo.jpg" WASHI_SHOT_DELAY_MS=3000
    shot scan-editor "$lang" WASHI_PAGE=scan WASHI_SCAN_STEP=preview WASHI_SHOT_DELAY_MS=6500
    shot disc-label "$lang" WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$samples/disc.jpg" \
        "WASHI_DISC_TEXT=$disc_text" "WASHI_DISC_RING=$disc_ring" WASHI_DISC_SELECT=none \
        WASHI_SHOT_DELAY_MS=2500
done
