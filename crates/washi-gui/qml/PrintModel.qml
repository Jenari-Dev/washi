pragma Singleton

import QtQuick

// What the Photo Print and Document Print panels offer and how their choices become the
// `PrintFile` options of the service. Functions only, no UI; the only thing read from outside is
// the printer's own names for papers and trays (Store.printerText).
QtObject {
    id: model

    // The name of a media type (an IPP keyword; `com.canon-*` for Canon's papers) for a list or a
    // sentence. The printer's own name for it, in the language of the app, comes first
    // (Store.loadPrinterStrings reads them from the printer). Washi's own names below are the
    // fallback for a printer that does not say (never read, an older firmware, a language it has
    // no names in): plain descriptions, or product names the printer itself uses. A Canon keyword
    // nobody has described here shows as "Canon paper (code)" rather than a guessed name.
    function mediaLabel(key) {
        const own = Store.printerText("media-type", key, "");
        return own !== "" ? own : fallbackLabel(key);
    }

    function fallbackLabel(key) {
        switch (key) {
        case "stationery": return i18n("Plain paper");
        case "photographic": return i18n("Photo paper");
        case "envelope": return i18n("Envelope");
        case "auto": return i18n("Detect automatically");
        case "com.canon-mtplatinum": return i18n("Photo Paper Pro Platinum");
        case "com.canon-mtcrystal": return i18n("Photo Paper Pro Crystal Grade");
        case "com.canon-mtluster": return i18n("Photo Paper Pro Luster");
        case "com.canon-mtgr": return i18n("Photo Paper Plus Glossy II");
        case "com.canon-mtmat": return i18n("Matte Photo Paper");
        case "com.canon-mtglossy": return i18n("Glossy Photo Paper");
        case "com.canon-mthires": return i18n("High Resolution Paper");
        case "com.canon-mthagakiglossy": return i18n("Photo postcard (Hagaki)");
        case "com.canon-mthagakiglossya": return i18n("Photo postcard (Hagaki), address side");
        case "com.canon-mttshirt": return i18n("T-shirt transfers");
        case "com.canon-mtsemisuper": return i18n("Photo Paper Plus Semi-gloss");
        case "com.canon-mtfineart": return i18n("Fine art paper (other brands)");
        case "com.canon-mtcardstock": return i18n("Card stock");
        case "com.canon-mtinkjet": return i18n("Inkjet postcard (Hagaki)");
        case "com.canon-mtinkjeta": return i18n("Inkjet postcard (Hagaki), address side");
        case "com.canon-mthagaki": return i18n("Postcard (Hagaki)");
        case "com.canon-mthagakia": return i18n("Postcard (Hagaki), address side");
        default:
            if (key.indexOf("com.canon-") === 0) {
                const rest = key.substring("com.canon-".length).replace(/^mt/, "");
                return i18n("Canon paper (%1)", rest);
            }
            return key.replace(/[-_.]/g, " ");
        }
    }

    // Papers that belong on the Photo Print page's list first.
    function isPhotoMedia(key) {
        return key !== "stationery" && key !== "envelope" && key !== "auto" && key.indexOf("com.canon-mtcardstock") !== 0;
    }

    // The best default media type: Photo Paper Plus Glossy II (`mtgr`), else glossy photo paper,
    // else any photo paper, else plain.
    function defaultMedia(types, photo) {
        if (photo) {
            const pref = ["com.canon-mtgr", "com.canon-mtglossy", "photographic", "com.canon-mtluster", "com.canon-mtplatinum"];
            for (const p of pref) {
                if (types.indexOf(p) >= 0) {
                    return p;
                }
            }
            const any = types.find(t => isPhotoMedia(t));
            if (any) {
                return any;
            }
        }
        return types.indexOf("stationery") >= 0 ? "stationery" : (types[0] || "");
    }

    // ---- availability (from the service's media summary) --------------------------------------
    readonly property int bordered: 1
    readonly property int borderless: 2

    function flags(caps, source, type, size) {
        if (!caps || !caps.media) {
            return 0;
        }
        const s = caps.media[sourceKey(source)];
        const t = s ? s[type] : null;
        return t && t[size] ? t[size] : 0;
    }

    function borderlessAvailable(caps, source, type, size) {
        return (flags(caps, source, type, size) & borderless) !== 0;
    }

    function sourceKey(choice) {
        // The summary calls the cassette "main"; the PrintFile option calls it "cassette".
        return choice === "cassette" ? "main" : choice;
    }

    // The edge of a sheet the printer cannot print on, for the paper that is chosen: the margins
    // of the printer's bordered entry for this source, type and size, in mm and *as the sheet is
    // drawn* ({top, right, bottom, left}). The database lists them for the portrait sheet; a
    // landscape sheet is that sheet turned a quarter, so its top is the portrait sheet's right
    // edge and so on. null when nothing is lost there: no entry, margins the printer does not
    // state, or all zero (borderless printing is the caller's business: it asks for none).
    function unprintable(caps, source, type, size, landscape) {
        const s = caps && caps.margins ? caps.margins[sourceKey(source)] : null;
        const t = s ? s[type] : null;
        const m = t ? t[size] : null;
        if (!m || m.length !== 4 || m.some(v => !(v >= 0))) {
            return null;
        }
        const top = m[0] / 100;
        const bottom = m[1] / 100;
        const left = m[2] / 100;
        const right = m[3] / 100;
        if (top + bottom + left + right <= 0) {
            return null;
        }
        return landscape ? { top: right, right: bottom, bottom: left, left: top } : { top: top, right: right, bottom: bottom, left: left };
    }

    // Width and height in mm of the sheet as drawn.
    function sheetMm(sizeObj, orientation) {
        const w = (sizeObj ? sizeObj.width : 21000) / 100;
        const h = (sizeObj ? sizeObj.height : 29700) / 100;
        return orientation === "landscape" ? { w: h, h: w } : { w: w, h: h };
    }

    // Media types offered for a size from a source (printer's order), photo papers first if wanted.
    function typesFor(caps, source, size, photoFirst) {
        if (!caps || !caps.media) {
            return [];
        }
        const s = caps.media[sourceKey(source)] || ({});
        const all = Object.keys(s).filter(t => !!s[t][size]);
        const listed = (caps.media_types || []).filter(t => all.indexOf(t) >= 0);
        const rest = all.filter(t => listed.indexOf(t) < 0);
        const types = listed.concat(rest).filter(t => t !== "auto");
        if (photoFirst) {
            return types.filter(t => isPhotoMedia(t)).concat(types.filter(t => !isPhotoMedia(t)));
        }
        return types;
    }

    // Sizes the printer prints on with `type` from `source` (all sizes when type is "").
    function sizesFor(caps, source, type) {
        if (!caps || !caps.sizes) {
            return [];
        }
        const s = caps.media ? (caps.media[sourceKey(source)] || ({})) : ({});
        return caps.sizes.filter(z => {
            if (type === "") {
                return Object.keys(s).some(t => !!s[t][z.key]);
            }
            return !!(s[type] && s[type][z.key]);
        });
    }

    // The name of a paper size as people say it; the printer's own list has English labels only.
    function sizeLabel(z) {
        switch (z.key) {
        case "l": return i18n("L photo");
        case "2l": return i18n("2L / 5x7 in");
        case "postcard": return i18n("Postcard (Hagaki)");
        case "square": return i18n("Square 5x5 in");
        case "square-89": return i18n("Square 89 mm");
        case "card": return i18n("Business card");
        case "dl": return i18n("Envelope DL");
        case "env10": return i18n("Envelope #10");
        case "you4": return i18n("Envelope You 4");
        case "you6": return i18n("Envelope You 6");
        case "chou3": return i18n("Envelope Chou 3");
        case "chou4": return i18n("Envelope Chou 4");
        case "disc": return i18n("Disc 120 mm");
        default: return z.label;
        }
    }

    // ---- layout ---------------------------------------------------------------------------
    readonly property var layouts: [1, 2, 4, 6, 9, 16]

    function layoutLabel(n, photo) {
        if (n === 1) {
            return photo ? i18n("1 photo per sheet") : i18n("1 page per sheet");
        }
        return i18n("%1 in 1", n);
    }

    // Columns and rows of an n-up sheet; mirrors washi_core::print::layout::grid.
    function grid(n, portrait) {
        switch (n) {
        case 2: return portrait ? { cols: 1, rows: 2 } : { cols: 2, rows: 1 };
        case 4: return { cols: 2, rows: 2 };
        case 6: return portrait ? { cols: 2, rows: 3 } : { cols: 3, rows: 2 };
        case 9: return { cols: 3, rows: 3 };
        case 16: return { cols: 4, rows: 4 };
        default: return { cols: 1, rows: 1 };
        }
    }

    // ---- quality, source ------------------------------------------------------------------------
    function qualityOptions() {
        return [
            { value: "normal", text: i18n("Auto") },
            { value: "high", text: i18n("High") },
            { value: "draft", text: i18n("Fast (draft)") }
        ];
    }

    function sourceOptions(caps) {
        const have = caps && caps.sources ? caps.sources : ["auto", "rear", "main"];
        // The printer's own names for its sources, when it has them ("media-source.<keyword>").
        const out = [{ value: "auto", text: Store.printerText("media-source", "auto", i18n("Automatic")) }];
        if (have.indexOf("rear") >= 0) {
            out.push({ value: "rear", text: Store.printerText("media-source", "rear", i18n("Rear tray")) });
        }
        if (have.indexOf("main") >= 0) {
            out.push({ value: "cassette", text: Store.printerText("media-source", "main", i18n("Cassette")) });
        }
        return out;
    }

    function duplexOptions() {
        return [
            { value: "", text: i18n("Off") },
            { value: "long", text: i18n("Long-edge binding") },
            { value: "short", text: i18n("Short-edge binding") }
        ];
    }

    // ---- files ---------------------------------------------------------------------------------
    function extension(path) {
        const m = /\.([A-Za-z0-9]+)$/.exec(String(path));
        return m ? m[1].toLowerCase() : "";
    }

    function kindOf(path) {
        const e = extension(path);
        if (["jpg", "jpeg", "png", "bmp"].indexOf(e) >= 0) {
            return "photo";
        }
        if (e === "pdf") {
            return "pdf";
        }
        if (["doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "txt", "csv"].indexOf(e) >= 0) {
            return "office";
        }
        return "other";
    }

    // The file type lists of the file dialogs.
    readonly property var photoFilters: [i18n("Pictures (*.jpg *.jpeg *.png *.bmp)")]

    function documentFilters(office) {
        const f = [i18n("PDF documents (*.pdf)")];
        if (office) {
            f.push(i18n("Office documents (*.doc *.docx *.xls *.xlsx *.ppt *.pptx *.odt *.ods *.odp *.rtf *.txt)"));
        }
        return office ? [i18n("Documents (*.pdf *.doc *.docx *.xls *.xlsx *.ppt *.pptx *.odt *.ods *.odp *.rtf *.txt)")].concat(f) : f;
    }

    // ---- pages ---------------------------------------------------------------------------------
    // "1-6, 3, 12-14": whole numbers and ranges, separated by commas. Returns
    // {ok, error, pages} where `pages` is the sorted list when `total` is known (> 0).
    function parsePages(text, total) {
        const t = String(text).replace(/\s+/g, "");
        if (t === "") {
            return { ok: false, error: "empty", pages: [] };
        }
        const set = {};
        for (const part of t.split(",")) {
            if (part === "") {
                continue;
            }
            const m = /^(\d*)(-?)(\d*)$/.exec(part);
            if (!m || (m[1] === "" && m[3] === "") || (m[2] === "" && m[3] !== "")) {
                return { ok: false, error: "syntax", pages: [] };
            }
            let a = m[1] === "" ? 1 : parseInt(m[1]);
            let b = m[2] === "" ? a : (m[3] === "" ? (total > 0 ? total : a) : parseInt(m[3]));
            if (a < 1 || b < 1) {
                return { ok: false, error: "syntax", pages: [] };
            }
            if (a > b) {
                return { ok: false, error: "backwards", pages: [] };
            }
            if (total > 0 && (a > total || b > total)) {
                return { ok: false, error: "range", pages: [] };
            }
            for (let p = a; p <= b && p <= 100000; p++) {
                set[p] = true;
            }
        }
        const pages = Object.keys(set).map(k => parseInt(k)).sort((x, y) => x - y);
        return { ok: pages.length > 0, error: pages.length > 0 ? "" : "empty", pages: pages };
    }

    function pagesError(code) {
        switch (code) {
        case "syntax": return i18n("Use page numbers and ranges such as 1-6, 3, 12-14.");
        case "backwards": return i18n("A range must go from the lower page to the higher one.");
        case "range": return i18n("The document does not have that page.");
        case "empty": return i18n("Enter the pages to print.");
        default: return "";
        }
    }

    // ---- the options the service takes ---------------------------------------------------------------
    // s: {size, mediaType, source, borderless, quality, color, copies, pagesText (documents:
    // "" = all), layout, duplex, orientation, jobName}. Everything optional falls back to the
    // service's defaults.
    function buildOptions(s) {
        const o = {
            size: s.size,
            media_type: s.mediaType,
            source: s.source || "auto",
            quality: s.quality || "normal",
            color: s.color !== false,
            copies: s.copies || 1,
            layout: s.layout || 1,
            orientation: s.orientation || "auto"
        };
        if (s.borderless && (s.layout || 1) === 1) {
            o.borderless = true;
        }
        if (s.pagesText && String(s.pagesText).trim() !== "") {
            o.pages = String(s.pagesText).replace(/\s+/g, "");
        }
        if (s.duplex) {
            o.duplex = s.duplex;
        }
        if (s.jobName) {
            o.job_name = s.jobName;
        }
        return o;
    }

    // "2 copies of beach.jpg on Photo Paper Plus Glossy II (L photo), borderless"
    function describe(s, what) {
        const copies = i18np("%1 copy of %2 on %3 (%4)", "%1 copies of %2 on %3 (%4)", s.copies || 1, what, mediaLabel(s.mediaType), s.sizeLabel || s.size);
        return Fmt.list([
            copies,
            s.borderless && (s.layout || 1) === 1 ? i18n("borderless") : "",
            !s.color ? i18n("black and white") : "",
            (s.layout || 1) > 1 ? i18n("%1 per sheet", s.layout) : "",
            s.duplex ? i18n("two-sided") : ""
        ]);
    }

    // Aspect ratio (width / height) of a sheet as drawn in the preview.
    function sheetAspect(sizeObj, orientation, contentPortrait) {
        const w = sizeObj ? sizeObj.width : 21000;
        const h = sizeObj ? sizeObj.height : 29700;
        const portrait = orientation === "landscape" ? false : true;
        return portrait ? w / h : h / w;
    }
}
