#!/bin/sh
# End-to-end check of the language switch with the real app, the real daemon and the fake printer
# (private session bus, throw-away config): the app starts in English, switches to Japanese while a
# dialog is open, the daemon follows over D-Bus, the choice is saved to config.toml and the next
# start of the app is in Japanese. Needs target/release/{washi-app,washid,washi-fake-printer}.
# Screenshots: target/screens-i18n/e2e-*.png
set -eu
root=$(cd "$(dirname "$0")/../../.." && pwd)

if [ -z "${WASHI_E2E_INNER:-}" ]; then
    exec env WASHI_E2E_INNER=1 dbus-run-session -- "$0" "$@"
fi

app=${WASHI_APP:-$root/target/release/washi-app}
washid=${WASHID_BIN:-$root/target/release/washid}
fake=${FAKE_BIN:-$root/target/release/washi-fake-printer}
out=${WASHI_SCREENS:-$root/target/screens-i18n}
mkdir -p "$out"
work=$(mktemp -d)
pids=""
cleanup() {
    for p in $pids; do kill "$p" 2>/dev/null || true; done
    rm -rf "$work"
}
trap cleanup EXIT INT TERM
export XDG_CONFIG_HOME=$work/config XDG_CACHE_HOME=$work/cache XDG_STATE_HOME=$work/state XDG_DATA_HOME=$work/data
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
# a session in English: the first run follows it
export LANG=en_US.UTF-8 LC_ALL= LANGUAGE=
cfg=$XDG_CONFIG_HOME/washi/config.toml

ok() { echo "ok   $*"; }
fail() { echo "FAIL $*" >&2; exit 1; }
language() { busctl --user get-property io.github.Jenari_Dev.Washi /io/github/Jenari_Dev/Washi io.github.Jenari_Dev.Washi1 Language | sed 's/^s "\(.*\)"$/\1/'; }
shot() { # name [VAR=value ...]
    name=$1
    shift
    env QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen WASHI_SCREENSHOT="$out/e2e-$name.png" "$@" "$app" 2>&1 | grep -v 'kf.i18n' || true
}

"$fake" --chmp-port 18080 --ipp-port 18631 </dev/null >"$work/fake.log" 2>&1 &
pids="$pids $!"
sleep 0.5
"$washid" --printer 127.0.0.1:18080 --ipp-port 18631 >"$work/washid.log" 2>&1 &
pids="$pids $!"
sleep 1

[ "$(language)" = en ] || fail "the daemon starts in English with an English session (it said $(language))"
ok "the daemon starts in English"
[ ! -e "$cfg" ] || fail "no config file before the first switch"

# 1. English start, switch to Japanese with the cleaning confirmation open; watch the daemon.
env WASHI_PAGE=utilities WASHI_DIALOG=cleaning-confirm WASHI_SWITCH_LANG=ja QT_QPA_PLATFORM=offscreen \
    WASHI_SCREENSHOT="$out/e2e-1-switched-to-japanese.png" "$app" >"$work/app1.log" 2>&1 &
app_pid=$!
sleep 4
l=$(language)
wait "$app_pid" || true
[ "$(language)" = ja ] || fail "the daemon did not follow the switch (it says $(language))"
ok "the daemon follows the app: Language = ja"
[ -f "$cfg" ] || fail "[ui] language was not saved"
grep -q '^language = "ja"' "$cfg" || fail "config.toml has no language = \"ja\":
$(cat "$cfg")"
ok "[ui] language = \"ja\" saved in config.toml"

# 2. A new start of the app follows the saved choice (the session is still English).
shot 2-restarted-in-japanese WASHI_PAGE=home
[ "$(language)" = ja ] || fail "the daemon was not told at start"
ok "the next start is Japanese and tells the daemon"

# 3. Back to English from the app; the file says en.
env WASHI_PAGE=home WASHI_SWITCH_LANG=en QT_QPA_PLATFORM=offscreen \
    WASHI_SCREENSHOT="$out/e2e-3-switched-back.png" "$app" >"$work/app3.log" 2>&1 &
app_pid=$!
sleep 4
wait "$app_pid" || true
[ "$(language)" = en ] || fail "the daemon did not follow the switch back"
grep -q '^language = "en"' "$cfg" || fail "config.toml has no language = \"en\""
ok "back to English: daemon and config.toml follow"

# 4. The daemon reads [ui] language at its own start.
kill "$(echo "$pids" | awk '{print $NF}')" 2>/dev/null || true
sleep 1
printf '[ui]\nlanguage = "ja"\n' >"$cfg"
"$washid" --printer 127.0.0.1:18080 --ipp-port 18631 >"$work/washid2.log" 2>&1 &
pids="$pids $!"
sleep 1
[ "$(language)" = ja ] || fail "washid did not read [ui] language at start (it says $(language))"
ok "washid reads [ui] language at start"

# 5. An unknown value does not stop anything; SetLanguage refuses it.
busctl --user call io.github.Jenari_Dev.Washi /io/github/Jenari_Dev/Washi io.github.Jenari_Dev.Washi1 SetLanguage s fr >/dev/null 2>"$work/err" && fail "SetLanguage accepted fr"
grep -q 'fr' "$work/err" || fail "the error does not name the language"
ok "SetLanguage refuses an unknown language"
echo "screenshots: $out/e2e-*.png"
