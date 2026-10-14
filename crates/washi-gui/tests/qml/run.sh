#!/bin/sh
# Headless QML tests with the app's own i18n: builds a small Qt Quick Test runner (runner.cpp)
# that sets up KI18n's QML context like the app does, compiles po/*.po to catalogs, copies the
# app's QML next to the tests (singletons via qmldir) and runs everything on Qt's offscreen
# platform. Config and cache go to a temp dir, so nothing of yours is touched.
#
#   tests/qml/run.sh                 all tests
#   tests/qml/run.sh tst_language    one file (extra arguments go to the runner: -functions ...)
#
# Needs Qt 6 (Quick Test), KI18n and gettext's msgfmt, as the app build does.
set -e
here=$(cd "$(dirname "$0")" && pwd)
gui="$here/../.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "==> catalogs"
for po in "$gui"/../../po/*.po; do
    [ -f "$po" ] || continue
    lang=$(basename "$po" .po)
    mkdir -p "$work/locale/$lang/LC_MESSAGES"
    msgfmt --check-format -o "$work/locale/$lang/LC_MESSAGES/washi.mo" "$po"
done

echo "==> runner"
moc=${MOC:-/usr/lib/qt6/moc}
cp "$here/runner.cpp" "$work/runner.cpp"
"$moc" "$work/runner.cpp" -o "$work/runner.moc"
# shellcheck disable=SC2046
g++ -std=c++20 -fPIC -O0 -I"$work" -I"$gui/src" -I/usr/include/KF6/KI18n \
    "$work/runner.cpp" "$gui/src/i18n.cpp" -o "$work/runner" \
    $(pkg-config --cflags --libs Qt6QuickTest Qt6Quick Qt6Qml Qt6Gui Qt6Core) \
    -lKF6I18n -lKF6I18nQml

mkdir -p "$work/qml"
cp "$gui"/qml/*.qml "$here"/tst_*.qml "$here"/FakeTransport.qml "$here"/FakeImaging.qml "$here"/Audit.qml "$here"/*-picture.png "$work/qml/"
{
    for s in Theme Icons Fmt Prefs Lang Nav Store Img PrintModel ScanModel DiscMath; do
        printf 'singleton %s 1.0 %s.qml\n' "$s" "$s"
    done
} > "$work/qml/qmldir"

cd "$work/qml"
export XDG_CONFIG_HOME="$work/cfg" XDG_CACHE_HOME="$work/cache" XDG_DATA_HOME="$work/data"
export WASHI_LOCALE_DIR="$work/locale" LANG=C.UTF-8 LC_ALL=C.UTF-8 LANGUAGE=
# A test file given by name runs alone.
if [ $# -gt 0 ] && [ -f "$here/$1.qml" ]; then
    file="$1"
    shift
    set -- -input "$work/qml/$file.qml" "$@"
else
    set -- -input "$work/qml" "$@"
fi
QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen "$work/runner" "$@"
