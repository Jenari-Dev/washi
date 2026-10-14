#!/bin/sh
# Headless screenshots of every page (and a few dialogs) with Qt's offscreen platform.
#
#   tests/screens.sh real     washid --read-only against your real printer (WASHI_HOST=name-or-ip,
#                             else the host saved in ~/.config/washi/config.toml); only read
#                             operations can happen.
#   tests/screens.sh fake     a non-read-only washid against washi-fake-printer on 127.0.0.1, so
#                             the write dialogs and the print, scan and disc flows can be shown.
#                             Uses throw-away config/cache dirs.
#   tests/screens.sh sleep    the same with a fake printer that is asleep (the Wake button), writable
#                             and read-only.
#   tests/screens.sh r3       the screens of the third feature round in English and Japanese against
#                             fake printers with different states (a support code, a firmware update,
#                             extra cleaning types): banner tabs, support code, links, firmware,
#                             extra maintenance cards, text that follows the ring; output
#                             target/screens-r3/<name>-<lang>.png.
#   tests/screens.sh r4       the printer's own words and picture in English and Japanese: the paper
#                             list of Photo Print, the confirmation sentence, the home banner and the
#                             printer list, against a fake printer that serves a strings file and a
#                             picture written by this script (never a real printer's); also a live
#                             language switch and a local picture pack image winning over the
#                             printer's picture; output target/screens-r4/<name>.png.
#   tests/screens.sh r5       the polish round in English and Japanese against the fake printer: the
#                             disc label font family with Bold / Italic (Japanese text in a Japanese
#                             family, bold and italic), the scan stitch direction, the unprintable
#                             edge on the Photo Print and Document Print previews, and the page
#                             order of Document Print; output target/screens-r5/<name>.png.
#   tests/screens.sh i18n     the main pages and one dialog of each kind in English and in Japanese
#                             (WASHI_LANG=en|ja) against the fake printer; output
#                             target/screens-i18n/<name>-<lang>.png (override with WASHI_SCREENS).
#                             `tests/screens.sh i18n real` takes the pages that only read from your
#                             real printer instead (washid --read-only).
#
# Any mode also takes WASHI_LANG=ja (or en) from the environment: the app starts in that language.
#
# Needs target/release/{washi-app,washid,washi-fake-printer} (cargo build --release) and, for the
# sample pictures, ImageMagick (`magick`, or `convert`).
# Output: target/screens/<name>.png  (override with WASHI_SCREENS).
set -eu
root=$(cd "$(dirname "$0")/../../.." && pwd)
mode=${1:-fake}

if [ -z "${WASHI_SCREENS_INNER:-}" ]; then
    # Everything runs on a private session bus, so nothing touches the desktop's washid.
    exec env WASHI_SCREENS_INNER=1 dbus-run-session -- "$0" "$@"
fi

app=${WASHI_APP:-$root/target/release/washi-app}
washid=${WASHID_BIN:-$root/target/release/washid}
fake=${FAKE_BIN:-$root/target/release/washi-fake-printer}
out=${WASHI_SCREENS:-$root/target/screens}
[ "$mode" = i18n ] && out=${WASHI_SCREENS:-$root/target/screens-i18n}
[ "$mode" = r3 ] && out=${WASHI_SCREENS:-$root/target/screens-r3}
[ "$mode" = r4 ] && out=${WASHI_SCREENS:-$root/target/screens-r4}
[ "$mode" = r5 ] && out=${WASHI_SCREENS:-$root/target/screens-r5}
mkdir -p "$out"
work=$(mktemp -d)
pids=""
cleanup() {
    for p in $pids; do kill "$p" 2>/dev/null || true; done
    rm -rf "$work"
}
trap cleanup EXIT INT TERM

# Sample files for the print, scan and disc screens: a photo, a portrait picture, a scanned page.
samples() {
    mkdir -p "$work/samples"
    im=$(command -v magick || command -v convert || true)
    if [ -z "$im" ]; then
        echo "ImageMagick is needed for the sample pictures" >&2
        exit 1
    fi
    "$im" -size 1800x1200 plasma:fractal -blur 0x2 "$work/samples/photo.jpg" 2>/dev/null
    "$im" -size 1200x1800 plasma:fractal -blur 0x2 "$work/samples/portrait.jpg" 2>/dev/null
    "$im" -size 1240x1754 xc:'#fbf8f0' -fill '#222' -pointsize 52 -annotate +100+160 'Invoice 2026-1013' \
        -pointsize 30 -annotate +100+250 'A page of text so that the rotate, crop' \
        -annotate +100+300 'and correction screens have something to show.' \
        -fill '#444' -draw 'rectangle 100,450 1140,470' -draw 'rectangle 100,520 1140,530' \
        -draw 'rectangle 100,600 700,610' -fill '#2a5db0' -draw 'rectangle 100,900 500,1300' \
        -fill '#c93a3a' -draw 'circle 800,1100 800,1250' -rotate 2.2 -gravity center -crop 1240x1754+0+0 +repage \
        -quality 90 "$work/samples/scan.jpg" 2>/dev/null
}

# Words and a picture for a fake printer to serve, written here: made-up names (not a printer's)
# in English and Japanese, and a product-shot-like drawing on a transparent ground.
words() {
    mkdir -p "$work/words"
    cat >"$work/words/en.txt" <<'EOF_EN'
/* Sample strings for the screenshot check: original text, not from any printer. */
"media-type.photographic" = "Sample Photo Stock";
"media-type.com.canon-mtglossy" = "Sample Glossy Photo Stock";
"media-type.com.canon-mtluster" = "Sample Lustre Photo Stock";
"media-type.stationery" = "Sample Plain Stock";
"media-type.com.canon-mtdisca" = "Sample Disc (recommended)";
"media-type.com.canon-mtdiscb" = "Sample Disc (other)";
EOF_EN
    cat >"$work/words/ja.txt" <<'EOF_JA'
/* スクリーンショット確認用の見本：どのプリンターのものでもない、この確認用に書いた文言 */
"media-type.photographic" = "見本フォト用紙";
"media-type.com.canon-mtglossy" = "見本光沢フォト用紙";
"media-type.com.canon-mtluster" = "見本ラスターフォト用紙";
"media-type.stationery" = "見本普通紙";
"media-type.com.canon-mtdisca" = "見本ディスク（推奨）";
"media-type.com.canon-mtdiscb" = "見本ディスク（その他）";
EOF_JA
    # A printer-like drawing, 512 x 512, with the empty margin a product shot has.
    "$im" -size 512x512 xc:none -stroke '#8d96a8' -strokewidth 3 \
        -fill '#dfe5f0' -draw 'polygon 60,190 130,130 400,130 452,190' \
        -fill '#f4f6fa' -draw 'roundrectangle 60,190 452,360 14,14' \
        -fill '#243b6b' -stroke none -draw 'roundrectangle 330,225 420,262 6,6' \
        -fill '#9aa5ba' -draw 'rectangle 92,332 300,346' "$work/words/icon.png"
}

shot() { # name [VAR=value ...]
    name=$1
    shift
    env QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen WASHI_SCREENSHOT="$out/$name.png" "$@" "$app" 2>&1 | grep -v 'kf.i18n' || true
    echo "$out/$name.png"
}

case "$mode" in
real)
    if [ -n "${WASHI_HOST:-}" ]; then
        "$washid" --read-only --printer "$WASHI_HOST" >"$work/washid.log" 2>&1 &
    else
        "$washid" --read-only >"$work/washid.log" 2>&1 &
    fi
    pids="$pids $!"
    sleep 2
    samples
    for page in home consumables status device-info utilities printer-list app-settings photo-print document-print scan functions recommended-apps more disc-label; do
        shot "$page" WASHI_PAGE=$page
    done
    # What the real printer's own media list (read-only IPP) makes of the print pages.
    shot photo-print-files WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_SHOT_DELAY_MS=3000
    shot document-print-files WASHI_PAGE=document-print WASHI_DOC="$root/contrib/testpage.pdf" WASHI_SHOT_DELAY_MS=3000
    shot scan-setup WASHI_PAGE=scan WASHI_SHOT_DELAY_MS=2000
    shot disc-label-design WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$work/samples/photo.jpg" "WASHI_DISC_TEXT=Summer mix|2026" WASHI_SHOT_DELAY_MS=2000
    # The printer sleeps most of the time: the Wake button is there, but read-only mode refuses.
    shot wake-home WASHI_PAGE=home WASHI_SHOT_DELAY_MS=1500
    shot wake-utilities WASHI_PAGE=utilities WASHI_SHOT_DELAY_MS=1500
    shot wake-status WASHI_PAGE=status WASHI_SHOT_DELAY_MS=1500
    shot nav-expanded WASHI_PAGE=utilities WASHI_NAV=expanded
    shot dialog-ink-models WASHI_PAGE=consumables WASHI_DIALOG=ink-models
    shot dialog-order-ink WASHI_PAGE=home WASHI_DIALOG=order-ink
    shot dialog-license WASHI_PAGE=app-settings WASHI_DIALOG=license
    ;;
fake)
    export XDG_CONFIG_HOME=$work/config XDG_CACHE_HOME=$work/cache XDG_STATE_HOME=$work/state XDG_DATA_HOME=$work/data
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
    samples
    # The scanner delivers two pages of a text page.
    # shellcheck disable=SC2086
    "$fake" --chmp-port 18080 --ipp-port 18631 --scan-pages 2 --scan-image "$work/samples/scan.jpg" ${FAKE_ARGS:-} </dev/null >"$work/fake.log" 2>&1 &
    pids="$pids $!"
    sleep 0.5
    # shellcheck disable=SC2086
    "$washid" --printer 127.0.0.1:18080 --ipp-port 18631 ${WASHID_ARGS:-} >"$work/washid.log" 2>&1 &
    pids="$pids $!"
    sleep 1
    shot fake-home WASHI_PAGE=home
    shot fake-utilities WASHI_PAGE=utilities
    shot photo-print WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_SHOT_DELAY_MS=3000
    shot photo-print-4up WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg:$work/samples/portrait.jpg:$work/samples/photo.jpg:$work/samples/portrait.jpg:$work/samples/photo.jpg" WASHI_LAYOUT=4 WASHI_ORIENTATION=portrait WASHI_SHOT_DELAY_MS=3500
    shot document-print WASHI_PAGE=document-print WASHI_DOC="$root/contrib/testpage.pdf" WASHI_SHOT_DELAY_MS=3000
    shot dialog-load-paper WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_DIALOG=nav-print-load-paper WASHI_SHOT_DELAY_MS=2500
    shot dialog-print-confirm WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_DIALOG=nav-print-confirm WASHI_SHOT_DELAY_MS=2500
    shot dialog-print-done WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_DIALOG=nav-print-start WASHI_SHOT_DELAY_MS=7000
    shot scan-setup WASHI_PAGE=scan WASHI_SHOT_DELAY_MS=2000
    shot scan-preview WASHI_PAGE=scan WASHI_SCAN_STEP=preview WASHI_SHOT_DELAY_MS=6500
    shot scan-rotate-crop WASHI_PAGE=scan WASHI_SCAN_STEP=crop WASHI_SHOT_DELAY_MS=7500
    shot scan-image-correction WASHI_PAGE=scan WASHI_SCAN_STEP=correct WASHI_SHOT_DELAY_MS=7500
    shot disc-label WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$work/samples/photo.jpg" "WASHI_DISC_TEXT=Summer mix|2026" WASHI_SHOT_DELAY_MS=2500
    shot dialog-disc-tray WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$work/samples/photo.jpg" "WASHI_DISC_TEXT=Summer mix|2026" WASHI_DIALOG=nav-disc-tray WASHI_SHOT_DELAY_MS=2500
    shot dialog-disc-preview WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$work/samples/photo.jpg" "WASHI_DISC_TEXT=Summer mix|2026" WASHI_DIALOG=nav-disc-preview WASHI_SHOT_DELAY_MS=3500
    shot dialog-manual-alignment WASHI_PAGE=utilities WASHI_DIALOG=manual-align-enter
    shot dialog-manual-alignment-intro WASHI_PAGE=utilities WASHI_DIALOG=manual-align
    shot fake-cleaning-options WASHI_PAGE=utilities WASHI_DIALOG=cleaning
    shot fake-cleaning-confirm WASHI_PAGE=utilities WASHI_DIALOG=cleaning-confirm
    shot fake-quiet-mode WASHI_PAGE=utilities WASHI_DIALOG=quiet
    shot fake-auto-power-off WASHI_PAGE=utilities WASHI_DIALOG=autopoweroff
    shot fake-add-printer WASHI_PAGE=printer-list WASHI_DIALOG=discover
    # A real job on the fake printer: wait for it to finish before the next run.
    shot fake-nozzle-running WASHI_PAGE=utilities WASHI_DIALOG=nozzle-run
    sleep 12
    shot fake-nozzle-result WASHI_PAGE=utilities WASHI_DIALOG=nozzle-done
    ;;
sleep)
    export XDG_CONFIG_HOME=$work/config XDG_CACHE_HOME=$work/cache XDG_STATE_HOME=$work/state XDG_DATA_HOME=$work/data
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
    "$fake" --chmp-port 18080 --ipp-port 18631 --asleep </dev/null >"$work/fake.log" 2>&1 &
    pids="$pids $!"
    sleep 0.5
    "$washid" --printer 127.0.0.1:18080 --ipp-port 18631 >"$work/washid.log" 2>&1 &
    washid_pid=$!
    pids="$pids $washid_pid"
    sleep 1
    shot sleeping-home WASHI_PAGE=home WASHI_SHOT_DELAY_MS=1500
    shot sleeping-utilities WASHI_PAGE=utilities WASHI_SHOT_DELAY_MS=1500
    shot sleeping-status WASHI_PAGE=status WASHI_SHOT_DELAY_MS=1500
    shot dialog-wake-confirm WASHI_PAGE=utilities WASHI_DIALOG=nav-wake-confirm WASHI_SHOT_DELAY_MS=1500
    # Read-only: the same printer, the Wake button refuses.
    kill "$washid_pid" 2>/dev/null || true
    sleep 0.5
    "$washid" --read-only --printer 127.0.0.1:18080 --ipp-port 18631 >"$work/washid-ro.log" 2>&1 &
    pids="$pids $!"
    sleep 1
    shot sleeping-readonly-home WASHI_PAGE=home WASHI_SHOT_DELAY_MS=1500
    shot sleeping-readonly-utilities WASHI_PAGE=utilities WASHI_SHOT_DELAY_MS=1500
    ;;
i18n)
    langs=${WASHI_LANGS:-"en ja"}
    if [ "${2:-}" = real ]; then
        if [ -n "${WASHI_HOST:-}" ]; then
            "$washid" --read-only --printer "$WASHI_HOST" >"$work/washid.log" 2>&1 &
        else
            "$washid" --read-only >"$work/washid.log" 2>&1 &
        fi
        pids="$pids $!"
        sleep 2
        samples
        for lang in $langs; do
            for page in home consumables status device-info utilities printer-list app-settings; do
                shot "real-$page-$lang" WASHI_LANG=$lang WASHI_PAGE=$page
            done
            shot "real-dialog-license-$lang" WASHI_LANG=$lang WASHI_PAGE=app-settings WASHI_DIALOG=license
        done
    else
        export XDG_CONFIG_HOME=$work/config XDG_CACHE_HOME=$work/cache XDG_STATE_HOME=$work/state XDG_DATA_HOME=$work/data
        mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
        samples
        # shellcheck disable=SC2086
        "$fake" --chmp-port 18080 --ipp-port 18631 --scan-pages 2 --scan-image "$work/samples/scan.jpg" ${FAKE_ARGS:-} </dev/null >"$work/fake.log" 2>&1 &
        pids="$pids $!"
        sleep 0.5
        # shellcheck disable=SC2086
        "$washid" --printer 127.0.0.1:18080 --ipp-port 18631 ${WASHID_ARGS:-} >"$work/washid.log" 2>&1 &
        pids="$pids $!"
        sleep 1
        for lang in $langs; do
            l="WASHI_LANG=$lang"
            shot "home-$lang" $l WASHI_PAGE=home
            shot "functions-$lang" $l WASHI_PAGE=functions
            shot "consumables-$lang" $l WASHI_PAGE=consumables
            shot "status-$lang" $l WASHI_PAGE=status
            shot "device-info-$lang" $l WASHI_PAGE=device-info
            shot "utilities-$lang" $l WASHI_PAGE=utilities
            shot "printer-list-$lang" $l WASHI_PAGE=printer-list
            shot "app-settings-$lang" $l WASHI_PAGE=app-settings
            shot "more-$lang" $l WASHI_PAGE=more
            shot "recommended-apps-$lang" $l WASHI_PAGE=recommended-apps
            shot "photo-print-$lang" $l WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_SHOT_DELAY_MS=3000
            shot "document-print-$lang" $l WASHI_PAGE=document-print WASHI_DOC="$root/contrib/testpage.pdf" WASHI_SHOT_DELAY_MS=3000
            shot "scan-setup-$lang" $l WASHI_PAGE=scan WASHI_SHOT_DELAY_MS=2000
            shot "scan-preview-$lang" $l WASHI_PAGE=scan WASHI_SCAN_STEP=preview WASHI_SHOT_DELAY_MS=6500
            shot "scan-rotate-crop-$lang" $l WASHI_PAGE=scan WASHI_SCAN_STEP=crop WASHI_SHOT_DELAY_MS=7500
            shot "scan-image-correction-$lang" $l WASHI_PAGE=scan WASHI_SCAN_STEP=correct WASHI_SHOT_DELAY_MS=7500
            shot "disc-label-$lang" $l WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$work/samples/photo.jpg" "WASHI_DISC_TEXT=Summer mix|2026" WASHI_SHOT_DELAY_MS=2500
            shot "dialog-load-paper-$lang" $l WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_DIALOG=nav-print-load-paper WASHI_SHOT_DELAY_MS=2500
            shot "dialog-print-confirm-$lang" $l WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_DIALOG=nav-print-confirm WASHI_SHOT_DELAY_MS=2500
            shot "dialog-cleaning-options-$lang" $l WASHI_PAGE=utilities WASHI_DIALOG=cleaning
            shot "dialog-cleaning-confirm-$lang" $l WASHI_PAGE=utilities WASHI_DIALOG=cleaning-confirm
            shot "dialog-quiet-mode-$lang" $l WASHI_PAGE=utilities WASHI_DIALOG=quiet
            shot "dialog-auto-power-off-$lang" $l WASHI_PAGE=utilities WASHI_DIALOG=autopoweroff
            shot "dialog-manual-alignment-$lang" $l WASHI_PAGE=utilities WASHI_DIALOG=manual-align
            shot "dialog-add-printer-$lang" $l WASHI_PAGE=printer-list WASHI_DIALOG=discover
            shot "dialog-ink-models-$lang" $l WASHI_PAGE=consumables WASHI_DIALOG=ink-models
            shot "dialog-order-ink-$lang" $l WASHI_PAGE=home WASHI_DIALOG=order-ink
            shot "dialog-license-$lang" $l WASHI_PAGE=app-settings WASHI_DIALOG=license
            shot "nav-expanded-$lang" $l WASHI_PAGE=utilities WASHI_NAV=expanded
        done
    fi
    ;;
r3)
    export XDG_CONFIG_HOME=$work/config XDG_CACHE_HOME=$work/cache XDG_STATE_HOME=$work/state XDG_DATA_HOME=$work/data
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
    samples
    langs=${WASHI_LANGS:-"en ja"}
    # One fake printer (and one washid) per state; each is stopped by its PID before the next.
    start_fake() {
        # shellcheck disable=SC2086
        "$fake" --chmp-port 18080 --ipp-port 18631 "$@" </dev/null >"$work/fake.log" 2>&1 &
        fake_pid=$!
        pids="$pids $fake_pid"
        sleep 0.5
        "$washid" --printer 127.0.0.1:18080 --ipp-port 18631 >"$work/washid.log" 2>&1 &
        washid_pid=$!
        pids="$pids $washid_pid"
        sleep 1
    }
    stop_fake() {
        kill "$washid_pid" "$fake_pid" 2>/dev/null || true
        sleep 0.5
        rm -rf "$XDG_CACHE_HOME"/washi "$XDG_STATE_HOME"/washi
    }
    # 1. A healthy printer: the tabs in the banner, the web maintenance page card, the questions
    #    before a link opens, a text that follows the ring, the empty print page with its drop hint.
    start_fake
    for lang in $langs; do
        l="WASHI_LANG=$lang"
        shot "consumables-tabs-$lang" $l WASHI_PAGE=consumables
        shot "status-tabs-$lang" $l WASHI_PAGE=status
        shot "device-info-tabs-$lang" $l WASHI_PAGE=device-info
        shot "utilities-tabs-$lang" $l WASHI_PAGE=utilities
        shot "utilities-web-card-$lang" $l WASHI_PAGE=utilities WASHI_SIZE=1040x1500 WASHI_SHOT_DELAY_MS=1500
        shot "dialog-link-ink-$lang" $l WASHI_PAGE=consumables WASHI_DIALOG=order-ink
        shot "dialog-link-maintenance-$lang" $l WASHI_PAGE=utilities WASHI_DIALOG=link-maintenance
        shot "photo-print-empty-$lang" $l WASHI_PAGE=photo-print
        shot "disc-ring-top-$lang" $l WASHI_PAGE=disc-label "WASHI_DISC_TEXT=Summer mix 2026 - road trip songs|Side A" "WASHI_DISC_RING=0:50:0:cw:3" WASHI_DISC_SELECT=none WASHI_SHOT_DELAY_MS=2500
        shot "disc-ring-selected-$lang" $l WASHI_PAGE=disc-label "WASHI_DISC_TEXT=Summer mix 2026 - road trip songs|Side A" "WASHI_DISC_RING=0:50:0:cw:3" WASHI_SHOT_DELAY_MS=2500
        shot "disc-ring-bottom-$lang" $l WASHI_PAGE=disc-label "WASHI_DISC_TEXT=Track list on the other side|2026" "WASHI_DISC_RING=0:48:180:ccw:1;1:30:0:cw:0" WASHI_DISC_IMAGE="$work/samples/photo.jpg" WASHI_SHOT_DELAY_MS=2500
    done
    stop_fake
    # 2. The printer reports a support code (a jam), then a warning code (no paper), then a notice.
    for code in 1300 1000 D109; do
        start_fake --code "$code"
        for lang in $langs; do
            l="WASHI_LANG=$lang"
            shot "status-support-$code-$lang" $l WASHI_PAGE=status WASHI_SHOT_DELAY_MS=1500
            [ "$code" = 1300 ] && shot "dialog-link-error-$lang" $l WASHI_PAGE=status WASHI_DIALOG=link-error WASHI_SHOT_DELAY_MS=1500
            [ "$code" = 1300 ] && shot "home-support-$lang" $l WASHI_PAGE=home WASHI_SHOT_DELAY_MS=1500
        done
        stop_fake
    done
    # 3. A firmware update is offered.
    start_fake --firmware-update 1.061
    for lang in $langs; do
        l="WASHI_LANG=$lang"
        shot "device-info-firmware-$lang" $l WASHI_PAGE=device-info WASHI_SIZE=1040x1050 WASHI_SHOT_DELAY_MS=1500
        shot "dialog-firmware-$lang" $l WASHI_PAGE=device-info WASHI_DIALOG=firmware WASHI_SHOT_DELAY_MS=1500
    done
    stop_fake
    # 4. A model with more maintenance: roller and bottom plate cleaning (and a type nobody knows).
    start_fake --cleaning-types roller,platen,choke
    for lang in $langs; do
        l="WASHI_LANG=$lang"
        shot "utilities-extra-cards-$lang" $l WASHI_PAGE=utilities WASHI_SHOT_DELAY_MS=1500
        shot "dialog-platen-clean-$lang" $l WASHI_PAGE=utilities WASHI_DIALOG=cleaning-platen WASHI_SHOT_DELAY_MS=1500
    done
    stop_fake
    ;;
r4)
    export XDG_CONFIG_HOME=$work/config XDG_CACHE_HOME=$work/cache XDG_STATE_HOME=$work/state XDG_DATA_HOME=$work/data
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
    samples
    words
    langs=${WASHI_LANGS:-"en ja"}
    "$fake" --chmp-port 18080 --ipp-port 18631 --strings en="$work/words/en.txt" \
        --strings ja="$work/words/ja.txt" --icon "$work/words/icon.png" </dev/null >"$work/fake.log" 2>&1 &
    pids="$pids $!"
    sleep 0.5
    "$washid" --printer 127.0.0.1:18080 --ipp-port 18631 >"$work/washid.log" 2>&1 &
    pids="$pids $!"
    sleep 1
    for lang in $langs; do
        l="WASHI_LANG=$lang"
        shot "photo-print-$lang" $l WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_SHOT_DELAY_MS=3500
        shot "photo-print-list-$lang" $l WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_OPEN=media WASHI_SHOT_DELAY_MS=3500
        shot "dialog-print-confirm-$lang" $l WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_DIALOG=nav-print-confirm WASHI_SHOT_DELAY_MS=2500
        shot "home-$lang" $l WASHI_PAGE=home WASHI_SHOT_DELAY_MS=1500
        shot "printer-list-$lang" $l WASHI_PAGE=printer-list WASHI_SHOT_DELAY_MS=1500
        shot "disc-label-$lang" $l WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$work/samples/photo.jpg" "WASHI_DISC_TEXT=Summer mix|2026" WASHI_SHOT_DELAY_MS=2500
    done
    # The window starts in English and is switched to Japanese while it is open: the names follow.
    shot "photo-print-list-switched-to-ja" WASHI_LANG=en WASHI_SWITCH_LANG=ja WASHI_PAGE=photo-print WASHI_FILE="$work/samples/photo.jpg" WASHI_OPEN=media WASHI_SHOT_DELAY_MS=3500
    # A picture pack image of your own comes before the printer's picture (here a plain drawing).
    mkdir -p "$XDG_DATA_HOME/washi/printers"
    "$im" -size 420x270 xc:none -fill '#f2c14e' -stroke '#7a5a10' -strokewidth 4 \
        -draw 'roundrectangle 20,60 400,220 18,18' "$XDG_DATA_HOME/washi/printers/XK140.png"
    shot "home-pack-image-en" WASHI_LANG=en WASHI_PAGE=home WASHI_SHOT_DELAY_MS=1500
    ;;
r5)
    export XDG_CONFIG_HOME=$work/config XDG_CACHE_HOME=$work/cache XDG_STATE_HOME=$work/state XDG_DATA_HOME=$work/data
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
    samples
    # A five-page document with the page number large on every page.
    im=$(command -v magick || command -v convert)
    for n in 1 2 3 4 5; do
        "$im" -size 620x877 xc:white -gravity center -fill '#2a5db0' -pointsize 360 -annotate 0 "$n" \
            -gravity north -fill '#444' -pointsize 28 -annotate +0+40 "Page $n of 5" "$work/samples/p$n.png"
    done
    "$im" "$work/samples/p1.png" "$work/samples/p2.png" "$work/samples/p3.png" "$work/samples/p4.png" \
        "$work/samples/p5.png" -units PixelsPerInch -density 75 "$work/samples/five.pdf"
    langs=${WASHI_LANGS:-"en ja"}
    "$fake" --chmp-port 18080 --ipp-port 18631 --scan-pages 2 --scan-image "$work/samples/scan.jpg" </dev/null >"$work/fake.log" 2>&1 &
    pids="$pids $!"
    sleep 0.5
    "$washid" --printer 127.0.0.1:18080 --ipp-port 18631 >"$work/washid.log" 2>&1 &
    pids="$pids $!"
    sleep 1
    for lang in $langs; do
        l="WASHI_LANG=$lang"
        # Disc label: a Japanese family in bold, an italic one, and text that follows the ring.
        shot "disc-fonts-$lang" $l WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$work/samples/photo.jpg" "WASHI_DISC_TEXT=夏のミックス 2026|Summer mix" \
            "WASHI_DISC_FONT=0:Noto Sans CJK JP:b:-;1:DejaVu Serif:-:i" WASHI_SIZE=1040x1250 WASHI_SHOT_DELAY_MS=2500
        # Scan, Stitch: the direction in the setup, and the join button with its menu.
        shot "scan-stitch-setup-$lang" $l WASHI_PAGE=scan WASHI_SCAN_MODE=stitch WASHI_STITCH=top-bottom WASHI_SHOT_DELAY_MS=2000
        shot "scan-stitch-preview-$lang" $l WASHI_PAGE=scan WASHI_SCAN_MODE=stitch WASHI_STITCH=top-bottom WASHI_SCAN_STEP=preview WASHI_SHOT_DELAY_MS=6500
        # Print previews: the dashed frame of the edge the printer cannot print on (photos in L are
        # borderless by default, so the photo frame shows with borderless switched off).
        shot "photo-print-edge-$lang" $l WASHI_PAGE=photo-print WASHI_FILE="$work/samples/portrait.jpg" WASHI_BORDERLESS=0 WASHI_SHOT_DELAY_MS=3500
        shot "document-print-edge-$lang" $l WASHI_PAGE=document-print WASHI_DOC="$root/contrib/testpage.pdf" WASHI_SHOT_DELAY_MS=3500
        # Document Print, Page Order: a 5-page document printed two-sided with the last sheet first
        # (the preview walks the sheets in printing order; its first sheet is the lone page 5).
        shot "document-print-order-$lang" $l WASHI_PAGE=document-print WASHI_DOC="$work/samples/five.pdf" WASHI_PAGE_ORDER=reverse WASHI_DUPLEX=long WASHI_SIZE=1040x1000 WASHI_SHOT_DELAY_MS=3500
        shot "disc-fonts-printed-$lang" $l WASHI_PAGE=disc-label WASHI_DISC_IMAGE="$work/samples/photo.jpg" "WASHI_DISC_TEXT=夏のミックス 2026|Summer mix" \
            "WASHI_DISC_FONT=0:Noto Sans CJK JP:b:-;1:DejaVu Serif:-:i" WASHI_DIALOG=nav-disc-preview WASHI_SHOT_DELAY_MS=3500
    done
    ;;
*)
    echo "usage: $0 real|fake|sleep|i18n|r3|r4|r5 [real]" >&2
    exit 2
    ;;
esac
