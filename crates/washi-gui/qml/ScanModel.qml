pragma Singleton

import QtQuick

// Choices of the Scan screens and how they become the service's `StartScan` settings.
QtObject {
    id: model

    // Original sizes: the keys StartScan takes (washi_core::scan::sizes).
    function sizes() {
        return [
            { value: "auto", text: i18n("Auto") },
            { value: "a4", text: "A4" },
            { value: "letter", text: i18n("Letter") },
            { value: "a5", text: "A5" },
            { value: "b5", text: "B5" },
            { value: "l", text: i18n("L (89 x 127 mm)") },
            { value: "2l", text: i18n("2L (127 x 178 mm)") },
            { value: "postcard", text: i18n("Postcard") },
            { value: "card", text: i18n("Business card") }
        ];
    }

    function colorModes() {
        return [
            { value: "color", text: i18n("Color") },
            { value: "gray", text: i18n("Grayscale") },
            { value: "bw", text: i18n("Black and white") }
        ];
    }

    // The scanner's resolutions (square ones) from GetScannerCapabilities, else the XK140's.
    function resolutions(scanCaps) {
        const src = scanCaps && scanCaps.capabilities && scanCaps.capabilities.sources ? scanCaps.capabilities.sources[0] : null;
        const list = src && src.resolutions ? src.resolutions.filter(r => r.x === r.y).map(r => r.x) : [];
        return list.length > 0 ? list : [75, 100, 150, 200, 300, 600, 1200];
    }

    function defaultResolution(kind, available) {
        const want = kind === "photo" ? 600 : 300;
        return available.indexOf(want) >= 0 ? want : available[Math.floor(available.length / 2)];
    }

    function resolutionHint(dpi) {
        if (dpi <= 100) return i18n("Fast and small, for a quick look.");
        if (dpi <= 200) return i18n("Good for reading on a screen.");
        if (dpi <= 300) return i18n("Sharp enough for documents. A good default.");
        if (dpi <= 600) return i18n("For photos and fine detail. Bigger files.");
        return i18n("For enlargements. Very big files, and slow.");
    }

    // The settings for StartScan. `mode` is the scan mode of the menu: "auto" and "multi-photo"
    // scan the whole glass and let the host find what is on it.
    function scanSpec(s) {
        const wholeGlass = s.mode === "auto" || s.mode === "multi-photo";
        const spec = {
            size: wholeGlass ? "auto" : s.sizeKey,
            resolution: s.resolution,
            mode: s.colorMode,
            type: s.kind
        };
        if (wholeGlass) {
            spec.auto_crop = false;
        }
        return spec;
    }

    function aspectPresets() {
        return [
            { value: "custom", text: i18n("Custom") },
            { value: "original", text: i18n("Original aspect ratio") },
            { value: "square", text: i18n("Square") },
            { value: "3:2", text: "3:2" },
            { value: "4:3", text: "4:3" },
            { value: "7:5", text: "7:5" },
            { value: "10:8", text: "10:8" },
            { value: "16:9", text: "16:9" }
        ];
    }

    function formats(tools) {
        const f = [{ value: "pdf", text: "PDF", glyph: "document" }, { value: "jpeg", text: "JPEG", glyph: "photo" }];
        if (tools && tools.ocr) {
            f.push({ value: "pdf-ocr", text: i18n("PDF (searchable, OCR)"), glyph: "document" });
            f.push({ value: "txt", text: i18n("Text (OCR)"), glyph: "document" });
        }
        return f;
    }

    function scanModes(tools) {
        const m = [
            { value: "normal", text: i18n("Scan"), tip: i18n("Scan one original with the settings on the right.") },
            { value: "auto", text: i18n("Auto scan"), tip: i18n("Scan the whole glass, find out whether it is a document or a photo, and prepare it accordingly.") },
            { value: "multi-photo", text: i18n("Multi-photo crop"), tip: i18n("Scan several photos at once and cut each one out as its own page.") },
            { value: "stitch", text: i18n("Stitch"), tip: i18n("Scan an original bigger than the glass in two parts and join them.") }
        ];
        if (tools && tools.ocr) {
            m.push({ value: "ocr", text: i18n("Text recognition (OCR)"), tip: i18n("Scan a document and turn it into a searchable PDF or a text file.") });
        }
        return m;
    }

    function pad(n) {
        return (n < 10 ? "0" : "") + n;
    }

    // Scan_20261013_153012
    function suggestedName(date) {
        const d = date || new Date();
        return "Scan_" + d.getFullYear() + pad(d.getMonth() + 1) + pad(d.getDate()) + "_" + pad(d.getHours()) + pad(d.getMinutes()) + pad(d.getSeconds());
    }

    function extensionFor(format) {
        return format === "jpeg" ? ".jpg" : (format === "txt" ? ".txt" : ".pdf");
    }

    // The correction JSON of the image pipeline, with changes applied (immutable).
    function withCorrection(edits, patch) {
        const c = Object.assign({}, edits && edits.correction ? edits.correction : {
            color_correction: "None", reduce_show_through: false, show_through_threshold: 200,
            color_mode: "Color", bw_threshold: null, unsharp_amount: 0
        }, patch);
        return c;
    }

    function correctionPresets() {
        return [
            { value: "None", text: i18n("None") },
            { value: "Document", text: i18n("For document") },
            { value: "Photo", text: i18n("For photo") },
            { value: "Magazine", text: i18n("For magazine") }
        ];
    }

    function outputModes() {
        return [
            { value: "Color", text: i18n("Color") },
            { value: "Grayscale", text: i18n("Grayscale") },
            { value: "BlackWhite", text: i18n("Black and white") }
        ];
    }
}
