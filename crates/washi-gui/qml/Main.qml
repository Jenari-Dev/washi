import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import io.github.jenari_dev.washi

QQC2.ApplicationWindow {
    id: root

    title: "Washi"
    visible: true
    // WASHI_SIZE=WxH sets the first window size (headless checks of narrow layouts).
    readonly property var envSize: (backend.envVar("WASHI_SIZE") || "1040x760").split("x").map(v => parseInt(v))
    width: envSize.length === 2 && envSize[0] >= 860 ? envSize[0] : 1040
    height: envSize.length === 2 && envSize[1] >= 600 ? envSize[1] : 760
    minimumWidth: 860
    minimumHeight: 600
    color: Theme.bg

    // The controls are styled in QML; keep the stock palette light whatever the desktop theme is.
    palette.window: Theme.bg
    palette.windowText: Theme.text
    palette.base: "#FFFFFF"
    palette.text: Theme.text
    palette.button: "#FFFFFF"
    palette.buttonText: Theme.text
    palette.highlight: Theme.accent
    palette.highlightedText: "#FFFFFF"
    palette.placeholderText: Theme.textMuted
    palette.toolTipBase: "#2B2B2B"
    palette.toolTipText: "#FFFFFF"

    property bool navExpanded: backend.envVar("WASHI_NAV") === "expanded" ? true : Prefs.navExpanded
    readonly property string shotPath: backend.envVar("WASHI_SCREENSHOT")

    Backend {
        id: backend
    }

    Imaging {
        id: imaging
    }

    Component.onCompleted: {
        Store.transport = backend;
        Img.engine = imaging;
        backend.start();
        const page = backend.envVar("WASHI_PAGE");
        if (page !== "") {
            Nav.replace(page);
        }
    }

    // ---- pages -----------------------------------------------------------------------------------
    Component { id: cHome; HomePage {} }
    Component { id: cFunctions; FunctionsPage {} }
    Component { id: cPhoto; PrintPage { documentMode: false } }
    Component { id: cDocument; PrintPage { documentMode: true } }
    Component { id: cScan; ScanPage {} }
    Component { id: cManagement; ManagementPage {} }
    Component { id: cRecommended; RecommendedPage {} }
    Component { id: cMore; MorePage {} }
    Component { id: cList; PrinterListPage {} }
    Component { id: cSettings; AppSettingsPage {} }
    Component { id: cDisc; DiscLabelPage {} }
    Component { id: cDown; ServiceDown {} }

    function pageFor(id) {
        switch (id) {
        case "functions": return cFunctions;
        case "photo-print": return cPhoto;
        case "document-print": return cDocument;
        case "scan": return cScan;
        case "consumables":
        case "status":
        case "device-info":
        case "utilities": return cManagement;
        case "recommended-apps": return cRecommended;
        case "more": return cMore;
        case "printer-list": return cList;
        case "app-settings": return cSettings;
        case "disc-label": return cDisc;
        default: return cHome;
        }
    }

    RowLayout {
        id: shell
        anchors.fill: parent
        spacing: 0

        NavRail {
            Layout.fillHeight: true
            expanded: root.navExpanded
            onToggleExpanded: {
                root.navExpanded = !root.navExpanded;
                Prefs.navExpanded = root.navExpanded;
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            TopBar {
                Layout.fillWidth: true
            }

            Loader {
                id: page
                objectName: "pageLoader"
                Layout.fillWidth: true
                Layout.fillHeight: true
                sourceComponent: !Store.connected && Nav.page !== "app-settings" ? cDown : root.pageFor(Nav.page)
            }
        }
    }

    // ---- dialogs ---------------------------------------------------------------------------------
    DiscoverDialog { id: discoverDlg }
    InkModelsDialog { id: inkDlg }
    LicenseDialog { id: licenseDlg }
    MaintenanceHost { id: maint }
    ManualAlignHost { id: manualAlign }
    SettingDialog { id: setting }
    LinkHost { id: links }
    FirmwareHost { id: firmware }

    Connections {
        target: Nav
        function onDialogRequested(name, args) {
            switch (name) {
            case "discover": discoverDlg.open(); break;
            case "order-ink": links.open({ kind: "ink" }); break;
            case "open-link": links.open(args); break;
            case "firmware": firmware.open(); break;
            case "ink-models": inkDlg.open(); break;
            case "license": licenseDlg.open(); break;
            case "maintenance": maint.open(args); break;
            case "manual-align": manualAlign.open(); break;
            case "setting": setting.open(args); break;
            }
        }
    }

    // ---- headless screenshots ----------------------------------------------------------------------
    // WASHI_SCREENSHOT=/path.png [WASHI_PAGE=<id>] [WASHI_NAV=expanded] [WASHI_DIALOG=<name>]
    // waits for the first data, opens the dialog if asked, saves the window and quits.
    // WASHI_SWITCH_LANG=en|ja|system then switches the language like a click on the switch does
    // (live, saving [ui] language) once everything is open, and waits before it saves the window.
    Timer {
        id: shot
        interval: 250
        repeat: true
        running: root.shotPath !== ""
        property int ticks: 0
        property int readySince: -1
        property bool opened: false
        property bool switched: false
        property int switchedAt: -1
        readonly property string switchTo: backend.envVar("WASHI_SWITCH_LANG")
        readonly property string dialogName: backend.envVar("WASHI_DIALOG")
        readonly property int minTicksNow: parseInt(backend.envVar("WASHI_SHOT_DELAY_MS") || "0") / 250
        onTriggered: {
            ticks++;
            const needCaps = dialogName.indexOf("cleaning") === 0 || Nav.page === "utilities" || dialogName === "manual-align" || dialogName === "link-maintenance";
            const needPrint = Nav.page === "photo-print" || Nav.page === "document-print";
            const needScan = Nav.page === "scan";
            const ready = !Store.connected ? ticks > 16 + minTicksNow : (Store.snapLoaded && (!needCaps || (Store.maintCap !== null && Store.settings !== null)) && (!needPrint || Store.printCaps !== null) && (!needScan || Store.scanCaps !== null));
            if (ready && !opened) {
                opened = true;
                readySince = ticks;
                switch (dialogName) {
                case "cleaning": maint.open({ op: "clean" }); break;
                case "cleaning-confirm": maint.open({ op: "clean" }); maint.nextFromOptions(); break;
                case "cleaning-roller": maint.open({ op: "clean", kind: "roller" }); break;
                case "cleaning-platen": maint.open({ op: "clean", kind: "platen" }); break;
                case "nozzle-confirm": maint.open({ op: "nozzle_check" }); break;
                case "nozzle-run":
                case "nozzle-done": maint.open({ op: "nozzle_check" }); maint.accept(); break;
                case "manual-align": manualAlign.open(); break;
                case "manual-align-enter": manualAlign.open(); manualAlign.begin(); manualAlign.patternDone(); break;
                case "quiet": setting.open({ field: "quiet" }); break;
                case "autopoweroff": setting.open({ field: "autopoweroff" }); break;
                case "abrasion": setting.open({ field: "paper_gap", value: true }); break;
                case "discover": discoverDlg.open(); break;
                case "ink-models": inkDlg.open(); break;
                case "license": licenseDlg.open(); break;
                case "order-ink": links.open({ kind: "ink" }); break;
                case "link-help": links.open({ kind: "help" }); break;
                case "link-error": links.open({ kind: "error" }); break;
                case "link-maintenance": links.open({ kind: "maintenance" }); break;
                case "firmware": firmware.open(); break;
                case "firmware-running": firmware.open(); firmware.understood = true; firmware.accept(); break;
                default:
                    if (dialogName.indexOf("nav-") === 0) {
                        Nav.dialog(dialogName.substring(4), {});
                    }
                }
            }
            if (opened && switchTo !== "" && !switched && ticks - readySince >= 6) {
                switched = true;
                switchedAt = ticks;
                Lang.choose(switchTo);
            }
            // "nozzle-done" runs a real job on the fake printer and waits for its result.
            const settled = (dialogName === "nozzle-done" ? maint.stage === "done" : true) && (switchTo === "" || (switched && ticks - switchedAt >= 6));
            const minTicks = parseInt(backend.envVar("WASHI_SHOT_DELAY_MS") || "0") / 250;
            if ((opened && settled && ticks - readySince >= (dialogName === "nozzle-run" ? 8 : 6) && ticks >= minTicks) || ticks > 120 + minTicks) {
                shot.stop();
                if (!backend.saveScreenshot(root.shotPath)) {
                    console.warn("screenshot: could not save the window");
                }
                Qt.quit();
            }
        }
    }
}
