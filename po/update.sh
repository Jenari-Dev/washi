#!/usr/bin/env bash
# Extract the translatable strings of the app (QML i18n*() calls) into po/washi.pot and merge
# them into every po/<lang>.po. Run it after changing a string; then translate what is new.
#
#   po/update.sh           update washi.pot and the .po files
#   po/update.sh --check   only fail when washi.pot is out of date (no files are written)
#
# Needs gettext (xgettext, msgmerge). Strings follow KDE's conventions: i18n("text"),
# i18nc("context", "text"), i18np("one", "many", n), i18ncp("context", "one", "many", n).
set -eu
cd "$(dirname "$0")/.."

extract() {
    # shellcheck disable=SC2046
    xgettext --from-code=UTF-8 -L JavaScript --no-location \
        --keyword=i18n:1 --keyword=i18nc:1c,2 --keyword=i18np:1,2 --keyword=i18ncp:1c,2,3 \
        --add-comments=TRANSLATORS \
        --flag=i18n:1:no-javascript-format --flag=i18nc:2:no-javascript-format \
        --flag=i18np:1:no-javascript-format --flag=i18np:2:no-javascript-format \
        --flag=i18ncp:2:no-javascript-format --flag=i18ncp:3:no-javascript-format \
        --package-name=washi --copyright-holder=JenAriDev \
        --msgid-bugs-address="" \
        -o "$1" $(ls crates/washi-gui/qml/*.qml) 2> >(grep -v "msgid-bugs-address\|Makevars\|MSGID_BUGS\|^ *the MSGID\|Warning: " >&2 || true)
}

if [ "${1:-}" = "--check" ]; then
    tmp=$(mktemp)
    trap 'rm -f "$tmp"' EXIT
    extract "$tmp"
    # Compare the messages, not the header (it has a date).
    if ! diff -q <(msgcat --no-wrap "$tmp" | sed '1,/^$/d') <(msgcat --no-wrap po/washi.pot | sed '1,/^$/d') >/dev/null 2>&1; then
        echo "po/washi.pot is out of date: run po/update.sh" >&2
        exit 1
    fi
    exit 0
fi

extract po/washi.pot
for po in po/*.po; do
    [ -f "$po" ] || continue
    msgmerge --update --backup=none --no-fuzzy-matching --no-wrap "$po" po/washi.pot
    # Messages that no longer exist are dropped, not kept as `#~` entries.
    msgattrib --no-obsolete --no-wrap -o "$po" "$po"
done
echo "strings: $(grep -c "^msgid " po/washi.pot) (including the header)"
