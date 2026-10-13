import QtQuick

// A stand-in for the Imaging object: an in-memory scan session with the same calls and answers
// (JSON strings, `done(id, ok, body)` for the slow ones), recording what was asked.
QtObject {
    id: im

    property var log: []
    property var pages: []              // [{path, dpi, mode, edits}]
    property var work: null
    property int nextId: 0
    property bool ocr: false
    property var files: ({})            // path -> text (readText / writeText)
    property var removed: []
    property bool failRender: false

    signal done(int id, bool ok, string body)

    function record(name, arg) {
        log = log.concat([{ name: name, arg: arg }]);
    }

    function called(name) {
        return log.filter(l => l.name === name);
    }

    function clearLog() {
        log = [];
    }

    function later(ok, obj) {
        const id = ++nextId;
        Qt.callLater(() => im.done(id, ok, JSON.stringify(obj)));
        return id;
    }

    function freshEdits(mode) {
        return {
            rotation: "None", slant_degrees: 0, crop: null,
            correction: { color_correction: "None", reduce_show_through: false, show_through_threshold: 200, color_mode: mode === "bw" ? "BlackWhite" : (mode === "gray" ? "Grayscale" : "Color"), bw_threshold: null, unsharp_amount: 0 }
        };
    }

    function state() {
        if (!work) {
            return "null";
        }
        const swap = work.edits.rotation === "Right" || work.edits.rotation === "Left";
        return JSON.stringify({ index: work.index, rotated_width: swap ? 140 : 100, rotated_height: swap ? 100 : 140, edits: work.edits });
    }

    // ---- environment
    function tools() { return JSON.stringify({ pdf: true, office: false, ocr: im.ocr }); }
    function dataDir() { return "/data/washi"; }
    function homeDir() { return "/users/test"; }
    function readText(path) { return path in files ? files[path] : ""; }
    function writeText(path, text) { files[path] = text; return true; }
    function listFiles(dir, ext) {
        return JSON.stringify(Object.keys(files).filter(p => p.indexOf(dir + "/") === 0 && p.endsWith(ext)).map(p => p.substring(dir.length + 1)));
    }
    function removeDataFile(path) { removed = removed.concat([path]); delete files[path]; return true; }
    function fontFile(family) { return family === "" ? "" : "/fonts/" + family.replace(/ /g, "") + ".ttf"; }
    function imageSize(path) { return path.indexOf("portrait") >= 0 ? JSON.stringify({ width: 1200, height: 1800 }) : JSON.stringify({ width: 1800, height: 1200 }); }
    function token() { return "tok" + (++nextId); }

    // ---- scan session
    function scanReset() { record("scanReset"); pages = []; work = null; }
    function scanAddPage(path, dpi, mode) {
        record("scanAddPage", { path: path, dpi: dpi, mode: mode });
        pages = pages.concat([{ path: path, dpi: dpi, mode: mode, edits: freshEdits(mode) }]);
        return later(true, { index: pages.length - 1, count: pages.length });
    }
    function scanCount() { return pages.length; }
    function scanPageInfo(i) { return i < pages.length ? JSON.stringify({ width: 100, height: 140, edits: pages[i].edits }) : ""; }
    function scanRotate(i, dir) {
        record("scanRotate", { i: i, dir: dir });
        if (i >= pages.length) return false;
        const e = JSON.parse(JSON.stringify(pages[i].edits));
        e.rotation = dir === "left" ? "Left" : "Right";
        pages[i].edits = e;
        return true;
    }
    function scanDelete(i) { record("scanDelete", i); if (i >= pages.length) return false; pages = pages.filter((p, k) => k !== i); work = null; return true; }
    function scanRevert(i) { record("scanRevert", i); if (i >= pages.length) return false; pages[i].edits = freshEdits(pages[i].mode); return true; }
    function scanRender(i, edge, mode) {
        record("scanRender", { i: i, edge: edge, mode: mode });
        if (failRender) return later(false, { message: "render failed" });
        return later(true, { path: "/tmp/fake-" + mode + "-" + i + "-" + nextId + ".png", width: mode === "work-geometry" ? 100 : 100, height: 140 });
    }

    // ---- edit screens
    function editBegin(i) {
        record("editBegin", i);
        if (i >= pages.length) return "";
        work = { index: i, edits: JSON.parse(JSON.stringify(pages[i].edits)) };
        return state();
    }
    function editState() { return state(); }
    function editRotate(dir) { record("editRotate", dir); work.edits.rotation = dir === "left" ? "Left" : "Right"; return state(); }
    function editSlant(d) { record("editSlant", d); work.edits.slant_degrees = Math.max(-10, Math.min(10, d)); return state(); }
    function editCrop(x, y, w, h) { record("editCrop", [x, y, w, h]); work.edits.crop = { x: x, y: y, w: w, h: h }; return state(); }
    function editClearCrop() { work.edits.crop = null; return state(); }
    function editAspect(preset) {
        record("editAspect", preset);
        work.edits.crop = preset === "custom" ? work.edits.crop : (preset === "square" ? { x: 0, y: 0.2, w: 1, h: 100 / 140 } : { x: 0, y: 0.1, w: 1, h: 0.8 });
        return state();
    }
    function editCorrection(json) { record("editCorrection", json); work.edits.correction = JSON.parse(json); return state(); }
    function editAutoDeskew() { record("editAutoDeskew"); work.edits.slant_degrees = -1.5; return later(true, { slant: -1.5 }); }
    function editCommit() {
        record("editCommit");
        if (!work) return false;
        pages[work.index].edits = work.edits;
        work = null;
        return true;
    }
    function editCancel() { record("editCancel"); work = null; }

    // ---- save and extras
    function scanSave(json) {
        record("scanSave", JSON.parse(json));
        const req = JSON.parse(json);
        return later(true, { files: [req.path + (req.format === "jpeg" ? ".jpg" : ".pdf")] });
    }
    function scanExtra(json) {
        const req = JSON.parse(json);
        record("scanExtra", req);
        if (req.op === "classify") return later(true, { kind: "document" });
        if (req.op === "crop-photos") {
            const src = pages[req.index];
            const out = pages.filter((p, k) => k !== req.index);
            out.splice(req.index, 0, { path: src.path, dpi: src.dpi, mode: src.mode, edits: freshEdits(src.mode) }, { path: src.path, dpi: src.dpi, mode: src.mode, edits: freshEdits(src.mode) });
            pages = out;
            return later(true, { added: 2, count: pages.length, first: req.index });
        }
        if (req.op === "stitch") {
            pages = pages.filter((p, k) => k !== req.a && k !== req.b).concat([{ path: "/tmp/stitched.jpg", dpi: 300, mode: "color", edits: freshEdits("color") }]);
            return later(true, { index: Math.min(req.a, req.b), count: pages.length });
        }
        return later(false, { message: "unknown" });
    }

    // ---- previews
    function photoPreview(json) { record("photoPreview", JSON.parse(json)); return later(true, { path: "/tmp/fake-sheet.png", width: 100, height: 140 }); }
    function discPreview(json, px) { record("discPreview", JSON.parse(json)); return later(true, { path: "/tmp/fake-disc.png", width: px, height: px }); }
    function pdfPage(path, page, edge) { return later(true, { path: "/tmp/fake-pdf.png", width: 100, height: 140 }); }
    function pdfPages(path) { return 3; }
}
