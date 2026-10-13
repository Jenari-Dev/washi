import QtQuick
import QtTest 1.2

// Every page and dialog in both languages: nothing is cut off, elided, wider than its box or
// outside the window, at the smallest window the app allows (860 wide) and at its default size.
// Japanese text is wider than English in places, and wraps anywhere, so this is where a label that
// was sized for English shows up.
Item {
    id: root
    width: 1040
    height: 760

    FakeTransport {
        id: fake
    }
    FakeImaging {
        id: imaging
    }
    Audit {
        id: audit
    }

    // The area a page gets next to the collapsed navigation rail.
    Item {
        id: frame
        x: 48
        y: 0
        width: 812 - 0
        height: 700
        Loader {
            id: loader
            anchors.fill: parent
        }
    }
    NavRail {
        id: rail
        x: 0
        y: 0
        height: 700
        expanded: true
        visible: false
    }

    DiscoverDialog { id: discoverDlg }
    InkModelsDialog { id: inkDlg }
    LicenseDialog { id: licenseDlg }
    MaintenanceHost { id: maint }
    ManualAlignHost { id: manualAlign }
    SettingDialog { id: setting }
    LoadPaperDialog { id: loadPaper }
    PrintJobDialog { id: printJob }
    LinkHost { id: links }
    FirmwareHost { id: firmware }

    Component { id: topBarComp; TopBar {} }
    Component { id: homeComp; HomePage {} }
    Component { id: functionsComp; FunctionsPage {} }
    Component { id: managementComp; ManagementPage {} }
    Component { id: moreComp; MorePage {} }
    Component { id: recommendedComp; RecommendedPage {} }
    Component { id: listComp; PrinterListPage {} }
    Component { id: settingsComp; AppSettingsPage {} }
    Component { id: downComp; ServiceDown {} }
    Component { id: noPrinterComp; NoPrinter {} }
    Component { id: photoComp; PrintPage { documentMode: false } }
    Component { id: docComp; PrintPage { documentMode: true } }
    Component { id: scanComp; ScanPage {} }
    Component { id: discComp; DiscLabelPage {} }

    TestCase {
        name: "Layout"
        when: windowShown

        function boot(patch, readOnly) {
            loader.sourceComponent = null;
            for (const d of [discoverDlg, inkDlg, licenseDlg, maint, manualAlign, setting, loadPaper, printJob]) {
                if (d.close) d.close();
            }
            firmware.cancel();
            Store.reset();
            Store.transport = null;
            Img.reset();
            Img.engine = imaging;
            imaging.pages = [];
            imaging.work = null;
            Prefs.hideLoadPaperHint = true;
            Nav.page = "home";
            Nav.history = [];
            fake.connected = false;
            fake.readonly = !!readOnly;
            fake.env = ({});
            const s = JSON.parse(JSON.stringify(fake.snapshot));
            for (const k in patch) {
                s[k] = patch[k];
            }
            fake.overrides = ({ GetSnapshot: [true, JSON.stringify(s)] });
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
            Store.loadCaps(true);
            tryVerify(() => Store.maintCap !== null && Store.settings !== null);
            Store.loadPrintCaps(true);
            tryVerify(() => Store.printCaps !== null);
            Store.loadScanCaps(true);
            tryVerify(() => Store.scanCaps !== null);
            Store.loadNotifications();
        }

        function show(comp, width) {
            frame.width = width || 812;
            loader.sourceComponent = comp;
            tryVerify(() => loader.item !== null);
            wait(120);
            return loader.item;
        }

        // Audit the page `what` in `lang` at `width`; returns the problems.
        function auditPage(lang, width) {
            Lang.choose(lang);
            wait(80);
            const list = audit.items(loader.item);
            return audit.problems(list, frame, frame.width);
        }

        function auditPopup(popup, lang) {
            Lang.choose(lang);
            wait(80);
            return audit.problems(audit.popupItems(popup), root, root.width);
        }

        // One page, both languages, both widths.
        function checkPage(name, comp, prepare) {
            for (const width of [812, 992]) {
                for (const lang of ["en", "ja"]) {
                    Lang.choose(lang);
                    const item = show(comp, width);
                    if (prepare) {
                        prepare(item);
                        wait(150);
                    }
                    const p = auditPage(lang, width);
                    if (p.length > 0) {
                        fail(name + " [" + lang + ", " + width + "]:\n  " + p.join("\n  "));
                    }
                }
            }
        }

        // The audit itself: it has to see the faults it is for.
        function test_the_audit_finds_what_it_looks_for() {
            const holder = createTemporaryObject(faultComp, frame);
            wait(60);
            const p = audit.problems(audit.items(holder), frame, frame.width);
            const all = p.join("\n");
            verify(all.indexOf("elided: 'A text that does not fit'") >= 0, all);
            verify(all.indexOf("wider than its box") >= 0, all);
            verify(all.indexOf("outside the window") >= 0, all);
            verify(all.indexOf("button too narrow") >= 0, all);
            verify(all.indexOf("short text wrapped") >= 0, all);
            // and English words in Japanese text
            compare(audit.englishWords("消耗品 Canon XK140 series"), []);
            compare(audit.englishWords("The printer"), ["The", "printer"].filter(w => audit.names.indexOf(w) < 0));
            compare(audit.englishWords("プリンターが Cleaning を開始"), ["Cleaning"]);
        }

        Component {
            id: faultComp
            Item {
                width: 300
                height: 200
                Text {
                    width: 60
                    text: "A text that does not fit"
                    elide: Text.ElideRight
                }
                Text {
                    y: 30
                    width: 40
                    wrapMode: Text.NoWrap
                    text: "A text wider than its box"
                }
                Text {
                    x: 900
                    y: 60
                    text: "Far outside"
                }
                WButton {
                    y: 90
                    width: 40
                    text: "A button label"
                }
                Text {
                    y: 130
                    width: 50
                    wrapMode: Text.WordWrap
                    text: "セット 1（A〜H）"
                }
            }
        }

        function test_home_functions_more_recommended() {
            boot({});
            checkPage("home", homeComp);
            checkPage("functions", functionsComp);
            checkPage("more", moreComp);
            checkPage("recommended", recommendedComp);
        }

        function test_management_tabs() {
            for (const tab of ["consumables", "status", "device-info", "utilities"]) {
                boot({});
                Nav.page = tab;
                checkPage("management/" + tab, managementComp);
            }
        }

        function test_management_with_a_sleeping_printer_and_errors() {
            boot({ power: "sleeping", ink_source: "ipp" });
            Nav.page = "utilities";
            checkPage("utilities (sleeping)", managementComp);
            boot({ support: { code: "1300", message_key: "support.1300", message: "x" } });
            Nav.page = "status";
            checkPage("status (error)", managementComp);
            boot({ power: "offline" });
            Nav.page = "status";
            checkPage("status (offline)", managementComp);
            boot({ inks: [{ color: "BK", model: "XKI-N21<BK>SETUP", level: 10, status: "low", rgb: null, order: 1 }, { color: "PGBK", model: "XKI-N20<PGBK>SETUP", level: 0, status: "empty", rgb: null, order: 2 }] });
            Nav.page = "status";
            checkPage("status (ink low)", managementComp);
        }

        function test_read_only_banner_and_top_bar() {
            // The strip over every page, with and without the read-only notice, as wide as the
            // content next to the smallest window's rail and next to the default one.
            for (const readOnly of [false, true]) {
                boot({}, readOnly);
                checkPage("home (read-only: " + readOnly + ")", homeComp);
                checkPage("top bar (read-only: " + readOnly + ")", topBarComp, item => {
                    verify(item.implicitHeight >= 36);
                    verify(findChild(item, "langSwitch") !== null);
                    compare(findChild(item, "readOnlyBanner").visible, readOnly);
                });
            }
        }

        function test_printers_and_settings() {
            boot({});
            checkPage("printer list", listComp);
            checkPage("app settings", settingsComp);
            Store.reset();
            Store.transport = null;
            fake.connected = false;
            checkPage("service down", downComp);
            boot({});
            fake.overrides = ({ ListPrinters: [true, "[]"] });
            Store.loadPrinters(false);
            wait(100);
            checkPage("no printer", noPrinterComp);
        }

        function test_print_pages() {
            boot({});
            checkPage("photo print (empty)", photoComp);
            checkPage("document print (empty)", docComp);
            checkPage("photo print", photoComp, item => item.setFiles(["/p/a.jpg", "/p/b.jpg"]));
            checkPage("document print", docComp, item => {
                item.setFiles(["/d/doc.pdf"]);
                item.allPages = false;
                item.pagesText = "5-1";
            });
        }

        function test_scan_screens() {
            boot({});
            checkPage("scan setup", scanComp);
            for (const step of ["preview", "crop", "correct"]) {
                checkPage("scan " + step, scanComp, item => {
                    const flow = item.flow;
                    if (flow.count === 0) {
                        flow.startScan();
                        tryVerify(() => flow.scanSession === "scan-1" || flow.scanSession !== "");
                        fake.busSignal("ScanPageReady", flow.scanSession, "0", "/run/washi/p1.jpg");
                        fake.busSignal("ScanPageReady", flow.scanSession, "1", "/run/washi/p2.jpg");
                        fake.busSignal("ScanFinished", flow.scanSession, "true", "2 pages scanned.");
                        tryVerify(() => flow.step === "preview" && flow.mainPath !== "");
                    }
                    flow.noticeSource = () => i18n("This looks like a document: it is saved as PDF.");
                    if (step === "crop") {
                        flow.openCrop();
                    } else if (step === "correct") {
                        flow.openCorrect();
                    }
                });
            }
        }

        function test_disc_label() {
            boot({});
            checkPage("disc label", discComp, item => {
                item.design = JSON.parse(JSON.stringify(item.design));
                item.addText("Summer mix");
            });
        }

        function test_navigation_rail() {
            boot({});
            rail.visible = true;
            rail.managementOpen = true;
            rail.moreOpen = true;
            for (const lang of ["en", "ja"]) {
                Lang.choose(lang);
                wait(100);
                const p = audit.problems(audit.items(rail), rail, rail.width);
                compare(p, [], "navigation rail [" + lang + "]");
            }
            rail.visible = false;
        }

        function test_dialogs() {
            boot({});
            const dialogs = [
                ["add printer", () => discoverDlg.open(), discoverDlg],
                ["ink models", () => inkDlg.open(), inkDlg],
                ["license", () => licenseDlg.open(), licenseDlg],
                ["order ink", () => links.open({ kind: "ink" }), null],
                ["manual link", () => links.open({ kind: "help" }), null],
                ["support code help", () => links.open({ kind: "error", code: "1300" }), null],
                ["printer web page", () => links.open({ kind: "printer" }), null],
                ["maintenance web page", () => links.open({ kind: "maintenance" }), null],
                ["firmware update", () => firmware.open(), null],
                ["roller cleaning", () => maint.open({ op: "clean", kind: "roller" }), null],
                ["bottom plate cleaning", () => maint.open({ op: "clean", kind: "platen" }), null],
                ["cleaning options", () => maint.open({ op: "clean" }), null],
                ["nozzle check", () => maint.open({ op: "nozzle_check" }), null],
                ["alignment", () => maint.open({ op: "align_auto" }), null],
                ["quiet mode", () => setting.open({ field: "quiet" }), null],
                ["auto power off", () => setting.open({ field: "autopoweroff" }), null],
                ["ink drying", () => setting.open({ field: "dry_level" }), null],
                ["paper gap", () => setting.open({ field: "paper_gap" }), null],
                ["firmware", () => setting.open({ field: "auto_firmware", value: true }), null],
                ["manual alignment", () => manualAlign.open(), null],
                ["load paper", () => loadPaper.openFor("rear", true, true), loadPaper],
                ["load cassette", () => loadPaper.openFor("cassette", false, false), loadPaper],
                ["load disc tray", () => loadPaper.openFor("disc", false, true), loadPaper],
                ["print progress", () => printJob.start(["/p/a.jpg"], {}, "a.jpg"), printJob]
            ];
            for (const d of dialogs) {
                for (const lang of ["en", "ja"]) {
                    Lang.choose(lang);
                    for (const x of [discoverDlg, inkDlg, licenseDlg, maint, manualAlign, setting, loadPaper, printJob]) {
                        if (x.close) x.close();
                    }
                    firmware.cancel();
                    for (const c of links.data) {
                        if (c.close) c.close();
                    }
                    wait(60);
                    d[1]();
                    wait(200);
                    const popup = d[2] || findPopup();
                    verify(popup !== null, d[0]);
                    const p = audit.problems(audit.popupItems(popup), root, root.width);
                    if (p.length > 0) {
                        fail(d[0] + " [" + lang + "]:\n  " + p.join("\n  "));
                    }
                }
            }
        }

        // The dialog of the host that is open (the hosts keep their WDialog inside).
        function findPopup() {
            for (const h of [maint, manualAlign, setting, links, firmware]) {
                for (const c of h.data) {
                    if (c.visible !== undefined && c.opened) {
                        return c;
                    }
                }
            }
            return null;
        }
    }
}
