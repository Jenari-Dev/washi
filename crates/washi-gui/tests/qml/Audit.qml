import QtQuick
import QtQuick.Controls as QQC2

// Walks the items of a page or dialog the way a person looks at it: every visible text, and what
// is wrong with how it is laid out. Used by tst_language.qml and tst_layout.qml.
QtObject {
    id: audit

    // True for the Text and the TextInput of QtQuick (both have text and contentWidth).
    function isText(item) {
        return typeof item.text === "string" && typeof item.contentWidth === "number" && typeof item.font === "object";
    }

    function isCombo(item) {
        return typeof item.displayText === "string" && typeof item.popup === "object";
    }

    // The visible items under `item` (not through hidden ones), `item` first.
    function items(item, out) {
        out = out || [];
        if (!item || !item.visible || item.opacity === 0 || item.width <= 0 && item.height <= 0 && !item.children) {
            return out;
        }
        out.push(item);
        const kids = item.children || [];
        for (let i = 0; i < kids.length; i++) {
            items(kids[i], out);
        }
        // A Loader's content is its `item`, not a child in every Qt version.
        if (item.item && item.item !== item && out.indexOf(item.item) < 0) {
            items(item.item, out);
        }
        return out;
    }

    // The content of a popup (dialogs): header, content, footer.
    function popupItems(popup) {
        let out = [];
        for (const part of [popup.header, popup.contentItem, popup.footer]) {
            if (part) {
                out = out.concat(items(part));
            }
        }
        return out;
    }

    // Every visible text of the items, in tree order. Fields that hold what the user typed are
    // left out; combo boxes contribute their current text and the entries they offer.
    function texts(list) {
        const out = [];
        for (const it of list) {
            if (isCombo(it) && it.objectName === "fontCombo") {
                // the font families installed on the computer are what they are
                out.push(it.displayText);
            } else if (isCombo(it)) {
                out.push(it.displayText);
                for (let i = 0; i < it.count; i++) {
                    out.push(String(it.textAt(i)));
                }
            } else if (isText(it) && typeof it.inputMask !== "string" && typeof it.placeholderText !== "string" && it.text !== "") {
                out.push(it.text);
            } else if (typeof it.placeholderText === "string" && it.placeholderText !== "") {
                out.push(it.placeholderText);
            }
        }
        return out;
    }

    // Words of three letters or more in a text, ignoring what is a name in every language.
    readonly property var names: ["Canon", "XK140", "series", "Washi", "PGBK", "BK", "Wi", "Fi", "PDF", "JPEG", "PNG", "BMP", "dpi", "OCR", "IPP", "LAN", "GIMP", "Inkscape", "Scribus", "LibreOffice", "Simple", "Scan", "Photo", "Paper", "Pro", "Platinum", "Crystal", "Grade", "Luster", "Glossy", "Matte", "Plus", "Semi", "gloss", "High", "Resolution", "Fine", "Art", "Office", "poppler", "pdftoppm", "pdfinfo", "washid", "systemctl", "user", "enable", "now", "start", "config", "toml", "txt", "jpg", "jpeg", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "bmp", "png", "pdf", "UI", "Remote", "HH", "MM", "ink", "Level", "in", "mm", "pt", "tank", "Summer", "mix", "test", "page", "Invoice", "IP", "TCP", "readonly", "only", "printer", "local", "HTTP", "GPL", "mDNS", "JIS", "You", "Chou", "DL", "WASHI", "washi", "doctor", "GNU", "PIXUS", "PIXMA", "PRINT", "XKI", "SETUP", "Web", "links", "example", "org"]

    function englishWords(text) {
        const words = String(text).match(/[A-Za-z][A-Za-z']{2,}/g) || [];
        return words.filter(w => names.indexOf(w) < 0);
    }

    // What is wrong with how the items are laid out (strings; empty = fine). `limit` is the
    // width of the window.
    function problems(list, root, limit) {
        const out = [];
        for (const it of list) {
            if (isText(it) && it.text !== "" && typeof it.truncated === "boolean") {
                const p = it.mapToItem(root, 0, 0);
                const label = "'" + it.text.substring(0, 40) + "'";
                if (it.truncated) {
                    out.push("elided: " + label);
                }
                if (typeof it.wrapMode === "number" && it.wrapMode === Text.NoWrap && it.contentWidth > it.width + 1) {
                    out.push("wider than its box (" + Math.round(it.contentWidth) + " > " + Math.round(it.width) + "): " + label);
                }
                if (p.x + Math.min(it.contentWidth, it.width) > limit + 1 || p.x < -1) {
                    out.push("outside the window: " + label + " at x=" + Math.round(p.x) + " w=" + Math.round(it.contentWidth));
                }
                if (typeof it.maximumLineCount === "number" && it.maximumLineCount < 100000 && it.lineCount >= it.maximumLineCount && it.truncated) {
                    out.push("cut off after " + it.lineCount + " lines: " + label);
                }
                // A short label squeezed into a narrow box and broken in two: sized for other text.
                if (typeof it.lineCount === "number" && it.lineCount > 1 && it.implicitWidth <= 150 && it.maximumLineCount >= 100000 && it.width < it.implicitWidth && it.text.indexOf("\n") < 0) {
                    out.push("short text wrapped (" + Math.round(it.width) + " < " + Math.round(it.implicitWidth) + "): " + label);
                }
            }
            // (a check box with a long label is meant to wrap)
            if (typeof it.pressed === "boolean" && typeof it.checkable === "boolean" && it.contentItem && it.width > 0
                    && !(typeof it.contentItem.wrapMode === "number" && it.contentItem.wrapMode !== Text.NoWrap)) {
                const need = it.contentItem.implicitWidth;
                if (need > it.width - it.leftPadding - it.rightPadding + 1) {
                    out.push("button too narrow for its label (" + Math.round(need) + " > " + Math.round(it.width - it.leftPadding - it.rightPadding) + "): " + (it.text || it.objectName));
                }
            }
        }
        return out;
    }
}
