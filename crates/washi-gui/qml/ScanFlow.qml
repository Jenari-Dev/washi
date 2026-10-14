import QtQuick

// The state and steps of the Scan screens (setup -> preview/edit -> rotate/crop or image
// correction -> back to preview), kept apart from the drawing so that the steps can be tested.
// A scan is started through the service (StartScan); its pages arrive as files and are loaded
// into the host's scan session (the Img singleton); everything after that is host-side image work.
Item {
    id: flow

    visible: false
    width: 0
    height: 0

    // setup | preview | crop | correct
    property string step: "setup"

    // ---- what to scan ------------------------------------------------------------------------
    property string kind: "document"            // document | photo
    property string sizeKey: "a4"
    property int resolution: 300
    property string colorMode: "color"           // color | gray | bw
    // normal | auto | multi-photo | stitch | ocr (the "More scan modes" menu)
    property string mode: "normal"
    // Stitch: where the second scan continues the first, "left-right" (to the right of it) or
    // "top-bottom" (below it).
    property string stitchDirection: "left-right"
    readonly property var resolutions: ScanModel.resolutions(Store.scanCaps)

    // ---- the scan in progress and the pages ------------------------------------------------------
    property bool scanning: false
    // What the scan screens say about a failure or a finding: message sources (see Fmt.text()),
    // so a text shown now is in the right language after a switch.
    property var errorSource: ""
    property var noticeSource: ""
    readonly property string error: Fmt.text(errorSource)
    readonly property string notice: Fmt.text(noticeSource)
    property string scanSession: ""
    property int count: 0
    property int current: 0
    property string mainPath: ""
    property int mainWidth: 0
    property int mainHeight: 0
    // True while "+ Next scan" waits for the next original to be put on the glass.
    property bool awaitingNext: false
    property alias pages: pageModel
    // What the last auto scan decided ("", document, photo, blank).
    property string detected: ""

    ListModel {
        id: pageModel
    }

    // ---- editing (rotate/crop, image correction) -------------------------------------------------------
    property var editState: null
    property string editPath: ""
    property int editWidth: 0
    property int editHeight: 0
    property bool editBusy: false

    // ---- saving --------------------------------------------------------------------------------
    property string format: "pdf"
    property string range: "all"
    property bool singleFile: true
    property bool saving: false
    property var savedFiles: []
    property var saveErrorSource: ""
    readonly property string saveError: Fmt.text(saveErrorSource)

    readonly property bool canScan: Store.canJob && !scanning
    readonly property bool hasPages: count > 0
    readonly property var currentEdits: editState ? editState.edits : null
    readonly property bool tesseract: Img.tools().ocr

    property int _renderToken: 0
    property int _editToken: 0
    property int _pending: 0        // pages still being loaded for the current scan

    // ---- scanning --------------------------------------------------------------------------------
    function settings() {
        return { sizeKey: sizeKey, resolution: resolution, colorMode: colorMode, kind: kind, mode: mode };
    }

    function startScan() {
        if (!canScan) {
            return false;
        }
        scanning = true;
        errorSource = "";
        noticeSource = "";
        scanSession = "";
        awaitingNext = false;
        Store.startScan(ScanModel.scanSpec(settings()), (ok, body) => {
            if (ok) {
                scanSession = body;
            } else {
                scanning = false;
                errorSource = Fmt.errorSource(body);
            }
        });
        return true;
    }

    function cancelScan() {
        if (scanning && scanSession !== "") {
            Store.cancelJob(scanSession, null);
        }
    }

    Connections {
        target: Store
        function onScanPageReady(session, index, path) {
            if (!flow.scanning || (flow.scanSession !== "" && session !== flow.scanSession)) {
                return;
            }
            flow.scanSession = session;
            flow._pending++;
            Img.addPage(path, flow.resolution, flow.colorMode, (ok, r) => {
                flow._pending--;
                if (!ok) {
                    flow.errorSource = r.message ? () => i18n("The scanned page could not be read: %1", r.message) : () => i18n("The scanned page could not be read.");
                    flow._maybeDone();
                    return;
                }
                flow._pageLoaded(r.index);
            });
        }
        function onScanFinished(session, ok, message) {
            if (!flow.scanning || (flow.scanSession !== "" && session !== flow.scanSession)) {
                return;
            }
            flow.scanning = false;
            if (!ok) {
                flow.errorSource = message;
            }
            flow._maybeDone();
        }
    }

    // A page of the new scan is in the session: run the mode's extra step, then show it.
    function _pageLoaded(index) {
        if (mode === "multi-photo") {
            Img.extra({ op: "crop-photos", index: index }, (ok, r) => {
                if (!ok) {
                    noticeSource = Fmt.imagingNotice(r.message);
                }
                _afterPages(index);
            });
        } else if (mode === "auto") {
            Img.extra({ op: "classify", index: index }, (ok, r) => {
                if (ok) {
                    _applyDetected(index, r.kind);
                }
                _afterPages(index);
            });
        } else {
            _afterPages(index);
        }
    }

    // Auto scan: set the colour correction and the format the finding calls for.
    function _applyDetected(index, found) {
        detected = found;
        if (found === "blank") {
            noticeSource = () => i18n("The scan looks empty. Check that the original is face down on the glass.");
            return;
        }
        Img.engine.editBegin(index);
        const st = Img.parse(Img.engine.editState());
        const preset = found === "photo" ? "Photo" : "Document";
        Img.engine.editCorrection(JSON.stringify(ScanModel.withCorrection(st ? st.edits : null, { color_correction: preset })));
        Img.engine.editCommit();
        format = found === "photo" ? "jpeg" : "pdf";
        noticeSource = found === "photo" ? () => i18n("This looks like a photo: it is saved as JPEG.") : () => i18n("This looks like a document: it is saved as PDF.");
    }

    function _afterPages(index) {
        syncPages();
        current = Math.min(index, Math.max(0, count - 1));
        if (mode === "ocr" && (format === "pdf" || format === "jpeg") && tesseract) {
            format = "pdf-ocr";
        }
        step = "preview";
        refreshMain();
    }

    function _maybeDone() {
        if (!scanning && _pending <= 0 && count === 0) {
            step = "setup";
        }
    }

    // Make the page list match the host's session and refresh the thumbnails.
    function syncPages() {
        const n = Img.engine.scanCount();
        while (pageModel.count > n) {
            pageModel.remove(pageModel.count - 1);
        }
        while (pageModel.count < n) {
            pageModel.append({ thumb: "" });
        }
        count = n;
        for (let i = 0; i < n; i++) {
            refreshThumb(i);
        }
    }

    function refreshThumb(i) {
        Img.render(i, 180, "page", (ok, r) => {
            if (ok && i < pageModel.count) {
                pageModel.setProperty(i, "thumb", r.path);
            }
        });
    }

    function refreshMain() {
        if (count === 0) {
            mainPath = "";
            return;
        }
        const t = ++_renderToken;
        Img.render(current, 1500, "page", (ok, r) => {
            if (t === _renderToken && ok) {
                mainPath = r.path;
                mainWidth = r.width;
                mainHeight = r.height;
            }
        });
    }

    function select(i) {
        if (i >= 0 && i < count) {
            current = i;
            refreshMain();
        }
    }

    // ---- the strip's tools ----------------------------------------------------------------------------
    function rotate(dir) {
        if (hasPages && Img.engine.scanRotate(current, dir)) {
            refreshThumb(current);
            refreshMain();
        }
    }

    function deleteCurrent() {
        if (!hasPages) {
            return;
        }
        Img.engine.scanDelete(current);
        pageModel.remove(current);
        count = pageModel.count;
        if (count === 0) {
            mainPath = "";
            current = 0;
            step = "setup";
            return;
        }
        current = Math.min(current, count - 1);
        // The thumbnails after the removed page moved up; their pictures are still right.
        refreshMain();
    }

    function revert() {
        if (hasPages && Img.engine.scanRevert(current)) {
            refreshThumb(current);
            refreshMain();
        }
    }

    function nextScan() {
        awaitingNext = true;
    }

    function scanNext() {
        awaitingNext = false;
        startScan();
    }

    // Stitch mode: join the last two pages, in `direction` (default: the chosen one).
    function stitchLastTwo(direction) {
        if (count < 2) {
            return;
        }
        const dir = direction === "top-bottom" || direction === "left-right" ? direction : stitchDirection;
        Img.extra({ op: "stitch", a: count - 2, b: count - 1, direction: dir }, (ok, r) => {
            if (ok) {
                syncPages();
                current = Math.min(r.index, count - 1);
                refreshMain();
            } else {
                noticeSource = () => i18n("The two scans could not be joined: %1", r.message);
            }
        });
    }

    function newScan() {
        Img.engine.scanReset();
        pageModel.clear();
        count = 0;
        current = 0;
        mainPath = "";
        detected = "";
        noticeSource = "";
        errorSource = "";
        savedFiles = [];
        step = "setup";
    }

    function back() {
        if (step === "crop") {
            cropCancel();
        } else if (step === "correct") {
            correctCancel();
        } else if (step === "preview") {
            step = "setup";
        }
    }

    // ---- rotate / crop ------------------------------------------------------------------------------
    function _state(json) {
        const st = Img.parse(json);
        if (st) {
            editState = st;
        }
        return st;
    }

    function _renderEdit(mode) {
        const t = ++_editToken;
        editBusy = true;
        Img.render(current, 1300, mode, (ok, r) => {
            if (t !== _editToken) {
                return;
            }
            editBusy = false;
            if (ok) {
                editPath = r.path;
                editWidth = r.width;
                editHeight = r.height;
            }
        });
    }

    function openCrop() {
        if (!hasPages) {
            return;
        }
        editPath = "";
        if (_state(Img.engine.editBegin(current))) {
            step = "crop";
            _renderEdit("work-geometry");
        }
    }

    function cropRotate(dir) {
        _state(Img.engine.editRotate(dir));
        _renderEdit("work-geometry");
    }

    function setSlant(deg) {
        _state(Img.engine.editSlant(deg));
        slantTimer.restart();
    }

    Timer {
        id: slantTimer
        interval: 70
        onTriggered: flow._renderEdit("work-geometry")
    }

    function autoStraighten() {
        editBusy = true;
        Img.wait(Img.engine.editAutoDeskew(), (ok, r) => {
            editBusy = false;
            if (ok) {
                _state(Img.engine.editState());
                _renderEdit("work-geometry");
            }
        });
    }

    function setCrop(x, y, w, h) {
        _state(Img.engine.editCrop(x, y, w, h));
    }

    function setAspect(preset) {
        _state(Img.engine.editAspect(preset));
    }

    function cropOk() {
        if (step !== "crop") {
            return;
        }
        Img.engine.editCommit();
        editState = null;
        step = "preview";
        refreshThumb(current);
        refreshMain();
    }

    function cropCancel() {
        if (step !== "crop") {
            return;
        }
        Img.engine.editCancel();
        editState = null;
        step = "preview";
    }

    // ---- image correction ---------------------------------------------------------------------------
    function openCorrect() {
        if (!hasPages) {
            return;
        }
        editPath = "";
        if (_state(Img.engine.editBegin(current))) {
            step = "correct";
            _renderEdit("work");
        }
    }

    function setCorrection(patch) {
        _state(Img.engine.editCorrection(JSON.stringify(ScanModel.withCorrection(currentEdits, patch))));
        correctTimer.restart();
    }

    Timer {
        id: correctTimer
        interval: 120
        onTriggered: flow._renderEdit("work")
    }

    function correctOk() {
        if (step !== "correct") {
            return;
        }
        Img.engine.editCommit();
        editState = null;
        step = "preview";
        refreshThumb(current);
        refreshMain();
    }

    function correctCancel() {
        if (step !== "correct") {
            return;
        }
        Img.engine.editCancel();
        editState = null;
        step = "preview";
    }

    // ---- saving ----------------------------------------------------------------------------------
    // `path` is the file name chosen in the dialog (the extension is added when missing).
    function save(path, cb) {
        if (!hasPages || saving) {
            return;
        }
        saving = true;
        saveErrorSource = "";
        Img.save({
            path: path,
            format: format,
            range: range,
            current: current,
            single_file: singleFile,
            lang: "eng"
        }, (ok, r) => {
            saving = false;
            if (ok) {
                savedFiles = r.files;
            } else {
                saveErrorSource = () => i18n("The file could not be saved: %1", r.message);
            }
            if (cb) {
                cb(ok, r);
            }
        });
    }
}
