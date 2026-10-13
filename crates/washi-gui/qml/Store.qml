pragma Singleton

import QtQuick
import QtCore

// Application state: the printers the daemon knows, the active printer's snapshot, its settings
// and maintenance capability. All of it comes from washid; `transport` (the Backend object, or
// a fake in the tests) carries the calls. Nothing here talks to a printer directly.
QtObject {
    id: store

    // Backend: call(method, argsJson) -> id, signals reply(id, ok, body) and busSignal(...).
    property var transport: null

    readonly property bool connected: transport ? transport.connected : false
    readonly property bool readOnly: transport ? transport.readonly : false
    readonly property string serviceVersion: transport ? transport.version : ""

    // ---- printers --------------------------------------------------------------------------
    property var printers: []
    property bool printersLoaded: false
    property bool discovering: false

    readonly property var activePrinter: {
        for (let i = 0; i < printers.length; i++) {
            if (printers[i].active) {
                return printers[i];
            }
        }
        return null;
    }
    readonly property string activeHost: activePrinter ? activePrinter.host : ""
    readonly property bool hasPrinter: activeHost !== ""

    // ---- snapshot of the active printer ------------------------------------------------------
    property var snap: ({})
    property bool snapLoaded: false
    property bool refreshing: false
    // The last failure to read the printer, as the message source of Fmt.text().
    property var snapErrorSource: ""
    readonly property string snapError: Fmt.text(snapErrorSource)
    property string activeIp: ""

    readonly property string power: snapLoaded ? (snap.power || "offline") : "unknown"
    readonly property var inks: Fmt.sortInks(snap.inks)
    readonly property string modelName: snap.device && snap.device.model ? snap.device.model : ""
    readonly property string firmware: snap.device && snap.device.firmware ? snap.device.firmware : ""
    readonly property bool busy: !!snap.activity
    readonly property string displayName: {
        if (modelName !== "") {
            return /^canon/i.test(modelName) ? modelName : "Canon " + modelName;
        }
        return activePrinter ? printerLabel(activePrinter) : "";
    }
    // The support code the printer reports (awake only) and how urgent its family is:
    // "error", "warning" or "info" (a notice that does not change the printer's state).
    readonly property var support: snap.support || null
    readonly property string supportSeverity: support ? (support.severity || "error") : ""
    readonly property string statusKind: {
        if (!hasPrinter) {
            return "none";
        }
        if (!snapLoaded) {
            return "unknown";
        }
        if (power === "offline") {
            return "offline";
        }
        if (power === "sleeping") {
            return "sleeping";
        }
        if (supportSeverity === "error") {
            return "error";
        }
        if (busy) {
            return "busy";
        }
        if (inks.some(i => i.status === "empty")) {
            return "error";
        }
        if (supportSeverity === "warning") {
            return "warning";
        }
        if (inks.some(i => i.status === "low")) {
            return "warning";
        }
        return "online";
    }
    // True when the support code is the headline of the Status tab: statusKind is what the code
    // says (an error or a warning), or the printer is fine and the code is a notice.
    readonly property bool supportShown: (supportSeverity === "error" || supportSeverity === "warning") ? statusKind === supportSeverity : (supportSeverity === "info" && statusKind === "online")
    readonly property string statusText: {
        switch (statusKind) {
        case "online": return i18n("Online");
        case "sleeping": return i18n("Sleeping");
        case "offline": return i18n("Offline");
        case "busy": return i18n("Busy");
        case "warning": return supportShown ? i18n("Support code %1", support.code) : i18n("Ink low");
        case "error": return supportShown ? i18n("Support code %1", support.code) : i18n("Ink empty");
        case "unknown": return i18n("Checking…");
        default: return "";
        }
    }
    readonly property string statusTip: {
        if (supportShown) {
            return Fmt.supportMessage(support);
        }
        switch (statusKind) {
        case "sleeping": return i18n("The printer is in auto power-off. It wakes up by itself when a job arrives.");
        case "offline": return i18n("The printer cannot be reached on the network.");
        case "busy": return snap.activity || "";
        default: return "";
        }
    }

    // How the printer is attached ("wifi", "lan" or "unknown") and the address it resolves to.
    readonly property string connection: snap.connection || (snap.device && snap.device.connection) || "unknown"
    readonly property string address: snap.address || activeIp

    // Jobs (printing, scanning, disc labels) need a running, writable service and a printer
    // that is not occupied; a sleeping printer wakes up for a job.
    readonly property bool canJob: connected && !readOnly && hasPrinter && power !== "offline" && power !== "unknown" && !busy

    // Writes need a running, non-read-only service.
    readonly property bool canChange: connected && !readOnly && hasPrinter && (power === "on" || power === "sleeping")
    // A sleeping printer wakes up by itself for a maintenance job (observed on the real XK140:
    // StartJob and TestPrint were accepted while it slept), so only an unreachable printer or
    // a running job blocks Start.
    readonly property bool canMaintain: connected && !readOnly && hasPrinter && (power === "on" || power === "sleeping") && !busy

    // Whether the printer offers a firmware update: "available", "unavailable" or "unknown".
    readonly property string firmwareUpdate: snap.firmware_update || "unknown"
    // Writes that wake or restart the printer need it reachable and free.
    readonly property bool canUpdateFirmware: canChange && power === "on" && !busy && firmwareUpdate === "available"

    // ---- settings and maintenance capability -----------------------------------------------------
    property var settings: null     // {current, allowed}
    property var maintCap: null     // {cleaning: [...], testprint: [...]}
    property bool capsLoading: false
    property bool capsFromCache: false
    property var capsErrorSource: ""
    readonly property string capsError: Fmt.text(capsErrorSource)

    // What the printer prints on and scans with (read-only queries, cached by the service).
    property var printCaps: null    // {sources, media_types, sizes, media, qualities, duplex, ...}
    property bool printCapsLoading: false
    property var printCapsErrorSource: ""
    readonly property string printCapsError: Fmt.text(printCapsErrorSource)
    property var scanCaps: null     // {capabilities: {sources: [...]}, status: {...}}
    property bool scanCapsLoading: false
    property var scanCapsErrorSource: ""
    readonly property string scanCapsError: Fmt.text(scanCapsErrorSource)
    // Desktop notification switches kept by the service: {enabled, ink, errors, online}.
    property var notifications: null

    signal maintenanceProgress(string job, string stage, string message)
    signal maintenanceFinished(string job, bool ok, string message)
    signal scanPageReady(string session, int index, string path)
    signal scanFinished(string session, bool ok, string message)
    signal printProgress(string job, string stage, string message)
    signal printFinished(string job, bool ok, string message)

    // ---- helpers ---------------------------------------------------------------------------------
    // Name and address as the service knows them.
    function printerLabel(p) {
        const n = (p.name || "").trim();
        return n !== "" ? n : p.host;
    }

    // Name for lists: the active printer is shown by its model once that is known.
    function listLabel(p) {
        return p.active && modelName !== "" ? displayName : printerLabel(p);
    }

    function parse(body) {
        try {
            return JSON.parse(body);
        } catch (e) {
            return null;
        }
    }

    // An environment variable of the app ("" if unset): the screenshot checks preselect things.
    function env(name) {
        return transport && transport.envVar ? transport.envVar(name) : "";
    }

    function errorBody(name, message) {
        return JSON.stringify({ name: name, message: message || "" });
    }

    // ---- calls -------------------------------------------------------------------------------
    property var _pending: ({})
    property var _early: ({})

    // Call a daemon method; cb(ok, body) runs when the answer arrives.
    function call(method, args, cb) {
        if (!transport) {
            if (cb) {
                Qt.callLater(cb, false, errorBody("NotRunning", ""));
            }
            return -1;
        }
        const id = transport.call(method, JSON.stringify(args || []));
        if (id in _early) {
            const r = _early[id];
            delete _early[id];
            if (cb) {
                cb(r[0], r[1]);
            }
        } else if (cb) {
            _pending[id] = cb;
        }
        return id;
    }

    function _onReply(id, ok, body) {
        const cb = _pending[id];
        if (cb) {
            delete _pending[id];
            cb(ok, body);
        } else {
            // The answer beat the id back to the caller (cannot happen with the real backend,
            // which always answers from its worker thread, but a fake may reply at once).
            _early[id] = [ok, body];
        }
    }

    function _onBus(name, a, b, c) {
        switch (name) {
        case "SnapshotChanged":
            if (a === activeHost) {
                const s = parse(b);
                if (s) {
                    applySnapshot(s);
                }
            } else {
                _setPower(a, (parse(b) || {}).power);
            }
            break;
        case "PrintersChanged":
            loadPrinters(false);
            break;
        case "MaintenanceProgress":
            maintenanceProgress(a, b, c);
            break;
        case "MaintenanceFinished":
            maintenanceFinished(a, b === "true", c);
            break;
        case "ScanPageReady":
            scanPageReady(a, parseInt(b), c);
            break;
        case "ScanFinished":
            scanFinished(a, b === "true", c);
            break;
        case "PrintProgress":
            printProgress(a, b, c);
            break;
        case "PrintFinished":
            printFinished(a, b === "true", c);
            break;
        }
    }

    // ---- loading -------------------------------------------------------------------------------
    function bootstrap() {
        loadPrinters(true);
    }

    function loadPrinters(first) {
        call("ListPrinters", [], (ok, body) => {
            if (!ok) {
                return;
            }
            const list = parse(body) || [];
            const stale = snapLoaded;
            applyPrinters(list);
            if (first && list.length === 0 && Prefs.autoDetect) {
                discover(null);
            } else if (first && stale && hasPrinter) {
                loadSnapshot();     // the service (re)started: our snapshot is from before
            }
        });
    }

    function applyPrinters(list) {
        const before = activeHost;
        printers = list;
        printersLoaded = true;
        if (activeHost !== before) {
            resetPrinterState();
        }
        if (hasPrinter && !snapLoaded) {
            loadSnapshot();
        }
        if (hasPrinter && activeIp === "" && !snap.address) {
            resolveIp();
        }
    }

    function resetPrinterState() {
        snap = ({});
        snapLoaded = false;
        snapErrorSource = "";
        settings = null;
        maintCap = null;
        capsFromCache = false;
        capsErrorSource = "";
        activeIp = "";
        printCaps = null;
        printCapsLoading = false;
        printCapsErrorSource = "";
        scanCaps = null;
        scanCapsLoading = false;
        scanCapsErrorSource = "";
    }

    function _setPower(host, power) {
        if (!power) {
            return;
        }
        const list = printers.map(p => p.host === host && p.power !== power ? Object.assign({}, p, { power: power }) : p);
        if (list.some((p, i) => p !== printers[i])) {
            printers = list;
        }
    }

    function loadSnapshot() {
        const host = activeHost;
        if (host === "") {
            return;
        }
        call("GetSnapshot", [host], (ok, body) => {
            if (host !== activeHost) {
                return;
            }
            if (ok && parse(body)) {
                applySnapshot(parse(body));
            } else {
                snapErrorSource = Fmt.errorSource(body);
                snap = { power: "offline" };
                snapLoaded = true;
            }
        });
    }

    function applySnapshot(s) {
        snap = s;
        snapLoaded = true;
        snapErrorSource = "";
        _setPower(activeHost, s.power);
        // A job was running (the service answers nothing but the snapshot meanwhile): read the
        // settings again now that the printer is free.
        if (capsFromCache && !s.activity && (s.power === "on" || s.power === "sleeping")) {
            loadCaps(true);
        }
    }

    // Read the printer now ("Update Information").
    function refresh() {
        const host = activeHost;
        if (host === "" || refreshing) {
            return;
        }
        refreshing = true;
        call("Refresh", [host], (ok, body) => {
            refreshing = false;
            if (host !== activeHost) {
                return;
            }
            if (ok && parse(body)) {
                applySnapshot(parse(body));
                loadCaps(true);
            } else {
                snapErrorSource = Fmt.errorSource(body);
            }
        });
    }

    function resolveIp() {
        const host = activeHost;
        call("x-Resolve", [host], (ok, body) => {
            if (host === activeHost && ok) {
                activeIp = body;
            }
        });
    }

    function setActive(host, cb) {
        call("SetActivePrinter", [host], (ok, body) => {
            if (ok) {
                loadPrinters(false);
            }
            if (cb) {
                cb(ok, body);
            }
        });
    }

    // Move the active printer by one in the list (the banner's arrows).
    function cycle(delta) {
        if (printers.length < 2) {
            return;
        }
        let i = printers.findIndex(p => p.active);
        i = (i + delta + printers.length) % printers.length;
        setActive(printers[i].host, null);
    }

    function discover(cb) {
        if (discovering) {
            return;
        }
        discovering = true;
        call("Discover", [], (ok, body) => {
            discovering = false;
            if (ok && parse(body)) {
                applyPrinters(parse(body));
            }
            if (cb) {
                cb(ok, body);
            }
        });
    }

    // Read the other printers' state for the Printer List page (read-only queries).
    function refreshOthers() {
        for (const p of printers) {
            if (!p.active) {
                const host = p.host;
                call("GetSnapshot", [host], (ok, body) => {
                    if (ok) {
                        _setPower(host, (parse(body) || {}).power);
                    }
                });
            }
        }
    }

    // ---- settings / maintenance capability (cached for when the printer sleeps) ---------------------
    property Settings cacheFile: Settings {
        location: StandardPaths.writableLocation(StandardPaths.GenericCacheLocation) + "/washi/gui-caps.ini"
        category: "caps"
        property string json: "{}"
    }

    function _cacheGet(host) {
        const all = parse(cacheFile.json) || {};
        return all[host] || {};
    }

    function _cachePut(host, key, value) {
        const all = parse(cacheFile.json) || {};
        const entry = all[host] || {};
        entry[key] = value;
        all[host] = entry;
        cacheFile.json = JSON.stringify(all);
    }

    function loadCaps(force) {
        const host = activeHost;
        if (host === "" || capsLoading) {
            return;
        }
        if (!force && settings && !capsFromCache) {
            return;
        }
        capsLoading = true;
        let left = 2;
        let failure = "";      // a message source
        const done = () => {
            if (--left > 0) {
                return;
            }
            capsLoading = false;
            if (host !== activeHost) {
                return;
            }
            capsErrorSource = failure;
            if (failure !== "") {
                // Asleep or unreachable: fall back to what we saw last time.
                const c = _cacheGet(host);
                capsFromCache = !!(c.settings || c.maint);
                if (!settings && c.settings) {
                    settings = c.settings;
                }
                if (!maintCap && c.maint) {
                    maintCap = c.maint;
                }
            } else {
                capsFromCache = false;
            }
        };
        call("GetSettings", [host], (ok, body) => {
            const v = ok ? parse(body) : null;
            if (v && host === activeHost) {
                settings = v;
                _cachePut(host, "settings", v);
            } else if (!ok) {
                failure = Fmt.errorSource(body);
            }
            done();
        });
        call("GetMaintenanceCapability", [host], (ok, body) => {
            const v = ok ? parse(body) : null;
            if (v && host === activeHost) {
                maintCap = v;
                _cachePut(host, "maint", v);
            } else if (!ok) {
                failure = Fmt.errorSource(body);
            }
            done();
        });
    }

    // ---- writes (only ever called after the user confirmed in a dialog) --------------------------------
    // The daemon refuses without "confirm": true; this adds it. Read-only mode never reaches the bus.
    function runMaintenance(spec, cb) {
        if (readOnly || !hasPrinter) {
            if (cb) {
                cb(false, errorBody("ReadOnly", ""));
            }
            return;
        }
        call("RunMaintenance", [activeHost, JSON.stringify(Object.assign({ confirm: true }, spec))], cb);
    }

    function applySettings(changes, cb) {
        if (readOnly || !hasPrinter) {
            if (cb) {
                cb(false, errorBody("ReadOnly", ""));
            }
            return;
        }
        const host = activeHost;
        call("ApplySettings", [host, JSON.stringify(Object.assign({ confirm: true }, changes))], (ok, body) => {
            const r = ok ? parse(body) : null;
            if (r && r.current && host === activeHost) {
                settings = { current: r.current, allowed: settings ? settings.allowed : ({}) };
                _cachePut(host, "settings", settings);
            }
            if (cb) {
                cb(ok, body);
            }
        });
    }

    // The web pages the printer points to (read-only): cb(ok, links) with
    // links = {ink, help, error, support, printer_page, maintenance_page: {url, source}, from_printer}.
    // `code` is the support code to find help for ("" for none).
    function getWebLinks(code, cb) {
        const host = activeHost;
        if (host === "") {
            Qt.callLater(cb, false, null);
            return;
        }
        call("GetWebLinks", [host, code || ""], (ok, body) => cb(ok, ok ? parse(body) : null));
    }

    // Update the printer's firmware (unverified on real printers); progress and the end arrive
    // as maintenanceProgress / maintenanceFinished of the job id given to cb(ok, jobId).
    function updateFirmware(cb) {
        if (_writeGuard(cb)) {
            call("UpdateFirmware", [activeHost, JSON.stringify({ confirm: true })], cb);
        }
    }

    // ---- print and scan capabilities (read-only) ---------------------------------------------------
    function loadPrintCaps(force) {
        const host = activeHost;
        if (host === "" || printCapsLoading || (printCaps && !force)) {
            return;
        }
        printCapsLoading = true;
        call("GetPrintCapabilities", [host], (ok, body) => {
            printCapsLoading = false;
            if (host !== activeHost) {
                return;
            }
            const v = ok ? parse(body) : null;
            if (v) {
                printCaps = v;
                printCapsErrorSource = "";
            } else {
                printCapsErrorSource = Fmt.errorSource(body);
            }
        });
    }

    function loadScanCaps(force) {
        const host = activeHost;
        if (host === "" || scanCapsLoading || (scanCaps && !force)) {
            return;
        }
        scanCapsLoading = true;
        call("GetScannerCapabilities", [host], (ok, body) => {
            scanCapsLoading = false;
            if (host !== activeHost) {
                return;
            }
            const v = ok ? parse(body) : null;
            if (v) {
                scanCaps = v;
                scanCapsErrorSource = "";
            } else {
                scanCapsErrorSource = Fmt.errorSource(body);
            }
        });
    }

    function loadNotifications() {
        call("GetNotificationSettings", [], (ok, body) => {
            const v = ok ? parse(body) : null;
            if (v) {
                notifications = v;
            }
        });
    }

    // Change some of the notification switches ({ink: false, ...}); the service saves them.
    function setNotifications(patch, cb) {
        if (readOnly) {
            if (cb) {
                cb(false, errorBody("ReadOnly", ""));
            }
            return;
        }
        call("SetNotificationSettings", [JSON.stringify(patch)], (ok, body) => {
            const v = ok ? parse(body) : null;
            if (v) {
                notifications = v;
            }
            if (cb) {
                cb(ok, body);
            }
        });
    }

    // ---- jobs (only ever called after the user asked for them) -------------------------------------
    function _writeGuard(cb) {
        if (readOnly || !hasPrinter) {
            if (cb) {
                cb(false, errorBody("ReadOnly", ""));
            }
            return false;
        }
        return true;
    }

    // Scan one page; cb(ok, sessionId). Pages arrive as scanPageReady, the end as scanFinished.
    function startScan(spec, cb) {
        if (_writeGuard(cb)) {
            call("StartScan", [activeHost, JSON.stringify(Object.assign({ confirm: true }, spec))], cb);
        }
    }

    // Print files (a photo, a PDF, or several photos for the n-in-1 layouts); cb(ok, jobId).
    function printFiles(paths, options, cb) {
        if (!_writeGuard(cb)) {
            return;
        }
        const opts = JSON.stringify(Object.assign({ confirm: true }, options));
        if (paths.length === 1) {
            call("PrintFile", [activeHost, paths[0], opts], cb);
        } else {
            call("PrintFiles", [activeHost, JSON.stringify(paths), opts], cb);
        }
    }

    function printDisc(label, cb) {
        if (_writeGuard(cb)) {
            call("PrintDisc", [activeHost, JSON.stringify(Object.assign({ confirm: true }, label))], cb);
        }
    }

    function cancelJob(job, cb) {
        call("CancelJob", [job], cb);
    }

    // Wake a printer that is in auto power-off (IVEC PowerOn: unverified on real printers).
    function wake(cb) {
        if (_writeGuard(cb)) {
            call("Wake", [activeHost, JSON.stringify({ confirm: true })], cb);
        }
    }

    // Remove a printer from the list (never touches the printer).
    function forgetPrinter(host, cb) {
        call("ForgetPrinter", [host], (ok, body) => {
            if (ok) {
                loadPrinters(false);
            }
            if (cb) {
                cb(ok, body);
            }
        });
    }

    // Forget everything (the QML tests start each case from a clean slate).
    function reset() {
        printers = [];
        printersLoaded = false;
        discovering = false;
        resetPrinterState();
        _pending = ({});
        _early = ({});
        capsLoading = false;
        refreshing = false;
        notifications = null;
    }

    // ---- wiring ---------------------------------------------------------------------------------
    property Connections link: Connections {
        target: store.transport
        ignoreUnknownSignals: true
        function onReply(id, ok, body) {
            store._onReply(id, ok, body);
        }
        function onBusSignal(name, a, b, c) {
            store._onBus(name, a, b, c);
        }
    }

    // The daemon words its notifications and error messages in the app's language.
    function syncLanguage() {
        if (connected) {
            call("SetLanguage", [Lang.current], null);
        }
    }

    onConnectedChanged: {
        if (connected) {
            bootstrap();
            syncLanguage();
        }
    }

    property Timer poller: Timer {
        interval: 20000
        repeat: true
        running: store.connected
        onTriggered: store.loadPrinters(false)
    }
}
