#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Checks of the translations of Washi's interface (run from anywhere: `python3 po/check.py`).

  1. every po/<lang>.po compiles with msgfmt and has 0 untranslated and 0 fuzzy messages;
  2. po/washi.pot is up to date with the strings in the QML files;
  3. a translation has the same placeholders (%1, %2 ...) as its source string, in every form;
  4. every i18n call has a string literal to extract, and no text property of the QML files
     holds a bare English literal (user text must go through i18n);
  5. Japanese typography: full-width punctuation (、。「」), no ASCII full stops or commas after
     Japanese, no spaces between Japanese words, half-width digits, no full-width Latin, a space
     before Latin units (120 mm, 600 dpi), and no English words left that are not names;
  6. the support code families of crates/washi-core/src/support.rs each have a text in
     Fmt.supportMessage (qml/Fmt.qml) with the same English, and po/ja.po says what the table
     says in Japanese, so the app and the daemon's notifications speak with one voice.

Exit status 1 when anything is found.
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PO = ROOT / "po"
QML = ROOT / "crates" / "washi-gui" / "qml"

CJK = r"぀-ヿ一-鿿㐀-䶿"
problems = []


def bad(msg):
    problems.append(msg)


# ---- a small PO reader ------------------------------------------------------------------------
def unescape(s):
    return s.replace('\\n', '\n').replace('\\"', '"').replace('\\\\', '\\')


def read_po(path):
    """Entries as dicts: msgctxt, msgid, msgid_plural, msgstr (list), flags."""
    entries, cur, field, flags = [], None, None, []
    for line in path.read_text("utf-8").split("\n") + [""]:
        if line.startswith("#,"):
            flags += [f.strip() for f in line[2:].split(",")]
            continue
        if line.startswith("#"):
            continue
        if line.strip() == "":
            if cur is not None and "msgid" in cur:
                cur["flags"] = flags
                entries.append(cur)
            cur, field, flags = None, None, []
            continue
        m = re.match(r'^(msgctxt|msgid_plural|msgid|msgstr(?:\[(\d+)\])?) "(.*)"$', line)
        if m:
            name, idx, val = m.group(1), m.group(2), unescape(m.group(3))
            cur = cur if cur is not None else {"msgstr": []}
            if name.startswith("msgstr"):
                cur["msgstr"].append(val)
                field = ("msgstr", len(cur["msgstr"]) - 1)
            else:
                cur[name] = val
                field = (name, None)
            continue
        m = re.match(r'^"(.*)"$', line)
        if m and field:
            name, i = field
            if name == "msgstr":
                cur["msgstr"][i] += unescape(m.group(1))
            else:
                cur[name] += unescape(m.group(1))
    return [e for e in entries if e["msgid"] != ""]


# ---- 1. completeness ---------------------------------------------------------------------------
def check_completeness(po):
    out = subprocess.run(["msgfmt", "--statistics", "--check-format", "--check-header", "-o", "/dev/null", str(po)],
                         capture_output=True, text=True)
    if out.returncode != 0:
        bad(f"{po.name}: msgfmt failed: {out.stderr.strip()}")
        return
    stats = out.stderr.strip()
    untranslated = re.search(r"(\d+) untranslated", stats)
    fuzzy = re.search(r"(\d+) fuzzy", stats)
    if untranslated and int(untranslated.group(1)) > 0:
        bad(f"{po.name}: {untranslated.group(1)} untranslated messages")
    if fuzzy and int(fuzzy.group(1)) > 0:
        bad(f"{po.name}: {fuzzy.group(1)} fuzzy messages")
    print(f"{po.name}: {stats}")


# ---- 2. up to date -----------------------------------------------------------------------------
def check_pot_current():
    r = subprocess.run(["bash", str(PO / "update.sh"), "--check"], capture_output=True, text=True)
    if r.returncode != 0:
        bad("po/washi.pot is out of date: run po/update.sh")


# ---- 3. placeholders -----------------------------------------------------------------------------
def check_placeholders(po, entries):
    ph = lambda s: sorted(re.findall(r"%\d", s))
    for e in entries:
        forms = e["msgstr"]
        src = [e["msgid"]] + ([e["msgid_plural"]] if "msgid_plural" in e else [])
        # plural forms may leave the number out (一つしかない言語): compare to the plural source
        want = set(ph(src[-1]))
        for i, f in enumerate(forms):
            have = set(ph(f))
            if have != want and not (have <= want and "msgid_plural" in e):
                bad(f"{po.name}: placeholders differ in {e['msgid'][:50]!r}: {sorted(want)} vs {sorted(have)}")
        # keep newlines and leading/trailing spaces in step
        for f in forms:
            if e["msgid"].endswith("\n") != f.endswith("\n"):
                bad(f"{po.name}: trailing newline differs in {e['msgid'][:50]!r}")


# ---- 4. QML: i18n calls and bare literals -----------------------------------------------------
TEXT_PROPS = r"(?:text|title|placeholderText|label|description|subtitle|caption|hint|toolTip|ToolTip\.text|acceptText|buttonText|secondaryText|tertiaryText|displayText|summary|heading|notice|message|tip)"
# Strings that are names, formats or commands in every language.
LITERAL_OK = {
    "PDF", "JPEG", "A4", "A5", "B5", "3:2", "4:3", "7:5", "10:8", "16:9", "Washi", "EN", "日本語",
    "systemctl --user start washid", "–",
}


def check_qml():
    call = re.compile(r"\bi18n(?:c|p|cp)?\s*\(\s*(.)")
    for f in sorted(QML.glob("*.qml")):
        text = f.read_text("utf-8")
        for n, line in enumerate(text.split("\n"), 1):
            code = line.split("//")[0] if '"' not in line.split("//")[0][-1:] else line
            for m in call.finditer(code):
                if m.group(1) not in "\"'":
                    bad(f"{f.name}:{n}: i18n call without a string literal to extract: {line.strip()[:80]}")
            for m in re.finditer(r"\b" + TEXT_PROPS + r'\s*:\s*"([^"\\]*(?:\\.[^"\\]*)*)"', code):
                s = m.group(1)
                if s in LITERAL_OK or not re.search(r"[A-Za-z]{2,}", s):
                    continue
                bad(f'{f.name}:{n}: bare string literal (use i18n): "{s}"')
            for m in re.finditer(r"\b" + TEXT_PROPS + r"\s*:\s*'([^']*)'", code):
                if re.search(r"[A-Za-z]{2,}", m.group(1)):
                    bad(f"{f.name}:{n}: bare string literal (use i18n): '{m.group(1)}'")


# ---- 5. Japanese -----------------------------------------------------------------------------------
NAMES = set("""Canon XK140 series Washi PGBK BK PDF JPEG PNG BMP dpi OCR IPP LAN Wi Fi GIMP Inkscape Scribus
LibreOffice Simple Scan Photo Paper Pro Platinum Crystal Grade Luster Glossy Matte Plus Semi gloss High Resolution Fine Art
Office poppler pdftoppm pdfinfo washid systemctl user enable now start config toml txt jpg jpeg doc docx xls xlsx ppt
pptx odt ods odp rtf bmp png pdf UI Remote HH MM in mm pt GPL GNU PIXUS PIXMA PRINT IP TCP HTTP mDNS JIS DL You Chou
IPv Level PowerOn read only readonly washi doctor Summer mix Hagaki Letter Legal GNOME Bus DTP Web""".split())


def check_japanese(po, entries):
    for e in entries:
        for f in e["msgstr"]:
            tag = f"{po.name}: {f[:40]!r}"
            if re.search(rf"[{CJK}]\s*[.](?:\s|$)", f) or re.search(rf"[{CJK}][.]$", f):
                bad(f"{tag}: ASCII full stop after Japanese")
            if re.search(rf"[{CJK}],", f):
                bad(f"{tag}: ASCII comma after Japanese")
            if re.search(rf"[{CJK}] +[{CJK}]", f):
                bad(f"{tag}: space between Japanese words")
            if re.search(r"[０-９Ａ-Ｚａ-ｚ]", f):
                bad(f"{tag}: full-width digits or letters")
            if re.search(r"[ｦ-ﾟ]", f):
                bad(f"{tag}: half-width katakana")
            if re.search(r"[“”]", f):
                bad(f"{tag}: use 「」 for quotation marks")
            if re.search(r"\d(?:mm|dpi|pt)\b", f):
                bad(f"{tag}: a Latin unit needs a space (120 mm)")
            if re.search(r"[:]\s*$", f) and re.search(rf"[{CJK}]", f):
                bad(f"{tag}: use ： after Japanese labels")
            if re.search(r"\.\.\.", f):
                bad(f"{tag}: use … for an ellipsis")
            words = [w for w in re.findall(r"[A-Za-z][A-Za-z']{2,}", f) if w not in NAMES]
            if words and not re.search(rf"[{CJK}]", f):
                bad(f"{tag}: English left (not translated?): {' '.join(words)}")
            elif words:
                bad(f"{tag}: English words in the Japanese: {' '.join(words)}")


# ---- 6. support codes: the table of washi-core, the app's texts and the Japanese ---------------------
SUPPORT_RS = ROOT / "crates" / "washi-core" / "src" / "support.rs"


def check_support_texts(entries_by_lang):
    """Every family of the support code table has a case in Fmt.supportMessage with the same
    English text, and the Japanese catalog says the same as the table (the daemon's notifications
    use the table's Japanese, the app uses the catalog: the person must read the same words)."""
    rs = SUPPORT_RS.read_text("utf-8")
    string = r'"((?:[^"\\]|\\.)*)"'
    families = re.findall(r"info!\(\s*" + string + r",\s*\w+,\s*" + string + r",\s*" + string + r"\s*\)", rs, re.S)
    if len(families) < 20:
        bad(f"support.rs: found only {len(families)} families")
        return
    fmt = (QML / "Fmt.qml").read_text("utf-8")
    cases = dict(re.findall(r'case "support\.([a-z-]+)": return i18n\("((?:[^"\\]|\\.)*)"', fmt))
    default = re.search(r'default: return i18n\("((?:[^"\\]|\\.)*)", support\.code\)', fmt)
    ja = {e["msgid"]: e["msgstr"][0] for e in entries_by_lang.get("ja", [])}
    for key, en, ja_text in [(k, e, j) for k, e, j in families]:
        en = en.replace("{code}", "%1")
        ja_text = ja_text.replace("{code}", "%1")
        if key == "unknown":
            qml_en = default.group(1) if default else None
        else:
            qml_en = cases.get(key)
        if qml_en is None:
            bad(f"Fmt.qml: no text for the support code family {key!r}")
            continue
        if qml_en != en:
            bad(f"Fmt.qml: English of support.{key} differs from support.rs:\n    {qml_en}\n    {en}")
        if "ja" in entries_by_lang and ja.get(en) != ja_text:
            bad(f"ja.po: Japanese of support.{key} differs from support.rs:\n    {ja.get(en)}\n    {ja_text}")
    known = {k for k, _, _ in families}
    for key in cases:
        if key not in known:
            bad(f"Fmt.qml: support.{key} is not a family of support.rs")


def main():
    pos = sorted(PO.glob("*.po"))
    if not pos:
        bad("no po/*.po files")
    check_pot_current()
    by_lang = {}
    for po in pos:
        check_completeness(po)
        entries = read_po(po)
        by_lang[po.stem] = entries
        check_placeholders(po, entries)
        if po.stem == "ja":
            check_japanese(po, entries)
    check_qml()
    check_support_texts(by_lang)
    if problems:
        print(f"\n{len(problems)} problem(s):")
        for p in problems:
            print("  " + p)
        return 1
    print("translations: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
