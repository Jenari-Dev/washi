import QtQuick

// A stand-in for the Backend object: records every call and answers like washid would.
QtObject {
    id: t

    property bool connected: true
    property bool readonly: false
    property string version: "0.1.0-test"
    property var calls: []
    property int nextId: 0
    // What Backend does for the language: the language shown, and what chooseLanguage() saved
    // (the real one writes [ui] language to config.toml). L10n is the test runner's handle on the
    // same function the app calls to switch the engine.
    property string language: "en"
    property var savedLanguages: []
    property bool auto: true            // answer calls by themselves
    property var overrides: ({})        // method -> [ok, body]

    // What the service answers (shapes taken from a real washid reading the fake printer).
    property var printers: [{ host: "printer.test", name: null, source: "flag", active: true, power: "on", last_updated: 1 }]
    property var snapshot: ({
        power: "on", state: "idle", state_reasons: [], support: null,
        inks: [
            { color: "M", model: "XKI-N21<M>SETUP", level: 60, status: "ok", rgb: null, order: 1 },
            { color: "BK", model: "XKI-N21<BK>SETUP", level: 80, status: "ok", rgb: null, order: 2 },
            { color: "Y", model: "XKI-N21<Y>SETUP", level: 70, status: "ok", rgb: null, order: 3 },
            { color: "PGBK", model: "XKI-N20<PGBK>SETUP", level: 60, status: "ok", rgb: null, order: 4 },
            { color: "C", model: "XKI-N21<C>SETUP", level: 60, status: "ok", rgb: null, order: 5 }
        ],
        ink_source: "ivec",
        trays: [{ bin: "inputbin_p1", paper_type: "stationery", paper_size: "iso_a4_210x297mm" }],
        device: { manufacturer: "CANON", model: "XK140 series", firmware: "1.060", services: ["print"] },
        counters: { total_paper_num: { inputbin_p1: 6, inputbin_p2: 54 }, ink_replacement_count: { BK: 1, C: 1 }, head_replacement_count: 1 },
        from_cache: [], warnings: [], last_awake: 1, last_updated: 1
    })
    property var settings: ({
        current: {
            autopoweron: "ON", autopoweroff: { mode: "ON", time: 30 }, autopoweroff_standalone: { mode: "ON", time: 30 },
            silentmode: { mode: "OFF", start: "2100", end: "0700" }, papergap: "auto", drylevel: 3, auto_printer_rom_update: "ON",
            setregistration: { "1": { A: 0, B: 0, C: 0, D: 1, E: 1, F: 0, G: 0, H: 0 }, "2": { I: 0, J: 0, K: 0, L: 0, M: 0 }, "3": { a: 0, b: 0, c: 0, d: 0, e: 0 } }
        },
        allowed: {
            autopoweron: ["ON", "OFF"], autopoweroff: { modes: ["ON", "OFF"], times: [15, 30, 60, 120, 240] },
            setregistration: {
                "1": { A: { min: -5, max: 5 }, B: { min: -5, max: 5 }, C: { min: -5, max: 5 }, D: { min: -5, max: 5 }, E: { min: -5, max: 5 }, F: { min: -5, max: 5 }, G: { min: -3, max: 3 }, H: { min: -3, max: 3 } },
                "2": { I: { min: -5, max: 5 }, J: { min: -5, max: 5 }, K: { min: -5, max: 5 }, L: { min: -5, max: 5 }, M: { min: -5, max: 5 } },
                "3": { a: { min: -2, max: 2 }, b: { min: -2, max: 2 }, c: { min: -2, max: 2 }, d: { min: -2, max: 2 }, e: { min: -2, max: 2 } }
            },
            autopoweroff_standalone: { modes: ["ON", "OFF"], times: [15, 30, 60, 120, 240] },
            silentmode: { modes: ["ON", "OFF", "time"] }, papergap: ["auto", "wide"], drylevel: [1, 2, 3, 4, 5],
            auto_printer_rom_update: ["ON", "OFF"]
        }
    })
    property var caps: ({
        cleaning: [{ kind: "regular", inkgroups: ["all", "group1", "group2"] }, { kind: "deep", inkgroups: ["all", "group1", "group2"] }],
        testprint: [
            { kind: "nozzle_check", inputbin: "inputbin_p2", papernum: 1, papersize: "iso_a4_210x297mm", papertype: "stationery", requires_id: false },
            { kind: "auto_registration", inputbin: "inputbin_p2", papernum: 2, papersize: "iso_a4_210x297mm", papertype: "stationery", requires_id: false },
            { kind: "manual_registration", inputbin: "inputbin_p2", papernum: 3, papersize: "iso_a4_210x297mm", papertype: "stationery", requires_id: true },
            { kind: "regi_check", inputbin: "inputbin_p2", papernum: 1, papersize: "iso_a4_210x297mm", papertype: "stationery", requires_id: false }
        ],
        roller_cleaning: false
    })
    property var printCaps: ({
        accepting_jobs: true, state: "idle", sources: ["auto", "main", "rear"],
        media_types: ["stationery", "photographic", "envelope", "com.canon-mtglossy", "com.canon-mtluster"],
        sizes: [
            { key: "a4", label: "A4", ipp_name: "iso_a4_210x297mm", width: 21000, height: 29700 },
            { key: "a5", label: "A5", ipp_name: "iso_a5_148x210mm", width: 14800, height: 21000 },
            { key: "b5", label: "B5 (JIS)", ipp_name: "jis_b5_182x257mm", width: 18200, height: 25700 },
            { key: "l", label: "L photo", ipp_name: "oe_photo-l_3.5x5in", width: 8890, height: 12700 },
            { key: "card", label: "Business card", ipp_name: "om_business-card_55x91mm", width: 5500, height: 9100 }
        ],
        media: {
            auto: { "com.canon-mtglossy": { a4: 3, card: 1, l: 3 }, "com.canon-mtluster": { a4: 3, l: 3 }, envelope: { a4: 1, l: 1 }, photographic: { a4: 3, card: 1, l: 3 }, stationery: { a4: 1, card: 1, l: 1 } },
            main: { stationery: { a4: 3, a5: 1, b5: 1 } },
            rear: { "com.canon-mtglossy": { a4: 3, card: 1, l: 3 }, "com.canon-mtluster": { a4: 3, l: 3 }, envelope: { a4: 1, l: 1 }, photographic: { a4: 3, card: 1, l: 3 }, stationery: { a4: 1, card: 1, l: 1 } }
        },
        // The bordered entries' margins [top, bottom, left, right] in 1/100 mm: even ones, and
        // plain A4 from the front tray made uneven so that a turned sheet shows it.
        margins: (function () {
            const media = {
                auto: { "com.canon-mtglossy": ["a4", "card", "l"], "com.canon-mtluster": ["a4", "l"], envelope: ["a4", "l"], photographic: ["a4", "card", "l"], stationery: ["a4", "card", "l"] },
                main: { stationery: ["a4", "a5", "b5"] },
                rear: { "com.canon-mtglossy": ["a4", "card", "l"], "com.canon-mtluster": ["a4", "l"], envelope: ["a4", "l"], photographic: ["a4", "card", "l"], stationery: ["a4", "card", "l"] }
            };
            const out = {};
            for (const src of Object.keys(media)) {
                out[src] = {};
                for (const ty of Object.keys(media[src])) {
                    out[src][ty] = {};
                    for (const size of media[src][ty]) {
                        out[src][ty][size] = [500, 500, 340, 340];
                    }
                }
            }
            out.auto.stationery.a4 = [300, 1200, 340, 560];
            return out;
        })(),
        qualities: ["draft", "normal", "high"], duplex: true, copies_max: 99,
        document_formats: ["image/jpeg", "image/pwg-raster"]
    })
    property var scanCaps: ({
        capabilities: { version: "2.9", sources: [{ source: "Platen", color_modes: ["Grayscale8", "Rgb24"], resolutions: [{ x: 75, y: 75 }, { x: 100, y: 100 }, { x: 150, y: 150 }, { x: 200, y: 200 }, { x: 300, y: 300 }, { x: 600, y: 600 }, { x: 1200, y: 1200 }], max_width: 2550, max_height: 3508 }] },
        status: { state: "Idle", jobs: [] }
    })
    property var notifications: ({ enabled: true, ink: true, errors: true, online: true })
    // What GetWebLinks answers; `error` appears when the app asks with a support code.
    property var webLinks: ({
        ink: { url: "https://links.example.org/ink?DEV=XK140+series&LNG=EN", source: "maker" },
        help: { url: "https://links.example.org/help?DEV=XK140+series&LNG=EN", source: "maker" },
        support: { url: "http://example.org/support/", source: "maker" },
        printer_page: { url: "http://printer.test/", source: "printer" },
        maintenance_page: { url: "http://printer.test/index.html?page=PAGE_UTIL", source: "printer" },
        from_printer: true
    })

    // What the service says about the printer's own words and picture: names by language
    // (`{"media-type.<keyword>": "Name"}`), and the path of the picture ("" = none).
    property var words: ({ en: {}, ja: {} })
    property string iconPath: ""
    // The local picture pack image for the model (a file URL; "" = none installed).
    property string pack: ""

    signal reply(int id, bool ok, string body)
    signal busSignal(string name, string a, string b, string c)

    function packImage(model) {
        return pack;
    }

    function chooseLanguage(code) {
        if (["en", "ja", "system"].indexOf(code) < 0) {
            return "";
        }
        const lang = code === "system" ? "en" : code;
        savedLanguages = savedLanguages.concat([code]);
        L10n.apply(lang);
        language = lang;
        return lang;
    }

    property var env: ({})
    function envVar(name) {
        return name in env ? env[name] : "";
    }

    function answer(method, args) {
        if (method in overrides) {
            return overrides[method];
        }
        switch (method) {
        case "ListPrinters": return [true, JSON.stringify(printers)];
        case "GetSnapshot":
        case "Refresh": return [true, JSON.stringify(snapshot)];
        case "GetSettings": return [true, JSON.stringify(settings)];
        case "GetMaintenanceCapability": return [true, JSON.stringify(caps)];
        case "RunMaintenance": return [true, "job-1"];
        case "GetPrintCapabilities": return [true, JSON.stringify(printCaps)];
        case "GetScannerCapabilities": return [true, JSON.stringify(scanCaps)];
        case "StartScan": return [true, "scan-1"];
        case "PrintFile":
        case "PrintFiles":
        case "PrintDisc": return [true, "print-1"];
        case "CancelJob": return [true, "true"];
        case "Wake": return [true, JSON.stringify({ sent: true, verified: false, note: "the printer accepted the wake command but is not up yet" })];
        case "GetWebLinks": {
            const l = Object.assign({}, webLinks);
            if (args[1] !== "") {
                l.error = { url: "https://links.example.org/err?DEV=XK140+series&LNG=EN&ERR=" + args[1], source: "maker" };
            }
            return [true, JSON.stringify(l)];
        }
        case "GetPrinterStrings": return [true, JSON.stringify(words[args[1]] || {})];
        case "GetPrinterIcon": return [true, iconPath];
        case "UpdateFirmware": return [true, "job-1"];
        case "GetNotificationSettings": return [true, JSON.stringify(notifications)];
        case "SetNotificationSettings": notifications = Object.assign({}, notifications, JSON.parse(args[0])); return [true, JSON.stringify(notifications)];
        case "ForgetPrinter": return [true, JSON.stringify({ forgotten: true, persistent: true, active: null })];
        case "ApplySettings": return [true, JSON.stringify({ changed: true, changes: [{ field: "silentmode", before: "OFF", after: "time 2230-0600" }], current: settings.current })];
        case "x-Resolve": return [true, "192.0.2.10"];
        default: return [true, ""];
        }
    }

    function call(method, argsJson) {
        const id = ++nextId;
        calls = calls.concat([{ id: id, method: method, args: JSON.parse(argsJson) }]);
        if (auto) {
            const r = answer(method, JSON.parse(argsJson));
            Qt.callLater(() => t.reply(id, r[0], r[1]));
        }
        return id;
    }

    function called(method) {
        return calls.filter(c => c.method === method);
    }

    function clearCalls() {
        calls = [];
    }
}
