#!/bin/sh
# Smoke test of a built Flatpak bundle: install it for the current user (with the KDE runtime from
# Flathub), then run the command-line tool, the daemon and the app from inside the sandbox.
#
#   tools/flatpak-smoke.sh Washi-0.1.3-x86_64.flatpak
#
# Fails on a non-zero exit or when the app does not leave a screenshot. Output (version lines,
# the doctor report, the screenshot) goes to $WASHI_SMOKE_OUT (default ./smoke-out). There is no
# printer in a CI container: `washi doctor` must report that, not crash.
set -eu

bundle=${1:?usage: flatpak-smoke.sh BUNDLE.flatpak}
app=io.github.Jenari_Dev.Washi
out=${WASHI_SMOKE_OUT:-$PWD/smoke-out}
mkdir -p "$out"
out=$(cd "$out" && pwd)

echo "==> install the KDE runtime and the bundle"
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user --noninteractive flathub org.kde.Platform//6.11
flatpak install --user --noninteractive --bundle "$bundle"

# The exported D-Bus activation file of the daemon is found through XDG_DATA_DIRS.
XDG_DATA_DIRS="$HOME/.local/share/flatpak/exports/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export XDG_DATA_DIRS

# A session bus for the run, when the container has none.
session() {
    if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && command -v dbus-run-session >/dev/null 2>&1; then
        dbus-run-session -- "$@"
    else
        "$@"
    fi
}

echo "==> washi --version"
session flatpak run --command=washi "$app" --version >"$out/washi-version.txt"
cat "$out/washi-version.txt"
echo "==> washid --version"
session flatpak run --command=washid "$app" --version >"$out/washid-version.txt"
cat "$out/washid-version.txt"
grep -q '^washi ' "$out/washi-version.txt"
grep -q '^washid ' "$out/washid-version.txt"
[ "$(sed 's/^washi //' "$out/washi-version.txt")" = "$(sed 's/^washid //' "$out/washid-version.txt")" ] || {
    echo "washi and washid report different versions" >&2
    exit 1
}

echo "==> washi doctor --json (no printer: it must report that, not crash)"
session flatpak run --command=washi "$app" doctor --json >"$out/doctor.json"
python3 - "$out/doctor.json" <<'EOF_PY'
import json, sys

report = json.load(open(sys.argv[1]))
checks = [c for g in report["groups"] for c in g["checks"]]
ids = {c["id"] for c in checks}
assert "washi.cli" in ids and "files.cache" in ids, sorted(ids)
assert all(c["status"] in ("ok", "fail", "skip") for c in checks)
print(f"doctor: {report['summary']}")
EOF_PY

echo "==> the app, offscreen, with a screenshot"
rm -f "$out/app.png"
session timeout 180 flatpak run --filesystem="$out" \
    --env=QT_QPA_PLATFORM=offscreen --env=WASHI_SCREENSHOT="$out/app.png" --env=WASHI_PAGE=app-settings \
    "$app"
[ -s "$out/app.png" ] || {
    echo "the app left no screenshot" >&2
    exit 1
}
ls -l "$out/app.png"
echo "flatpak smoke test: ok"
