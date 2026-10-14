import QtQuick
import QtTest 1.2

// The Scan screens: setup -> preview/edit -> rotate/crop or image correction -> preview, what is
// sent to the service (StartScan with confirm), how pages arrive and what saving asks of the host.
Item {
    id: root
    readonly property var theFlow: flow
    width: 1040
    height: 760

    FakeTransport {
        id: fake
    }
    FakeImaging {
        id: imaging
    }

    ScanFlow {
        id: flow
    }
    Component {
        id: setupComp
        ScanSetupView {
            flow: root.theFlow
            width: 1040
            height: 760
        }
    }
    Component {
        id: previewComp
        ScanPreviewView {
            flow: root.theFlow
            width: 1040
            height: 760
        }
    }

    TestCase {
        name: "ScanFlow"
        when: windowShown

        function init() {
            Store.reset();
            Store.transport = null;
            Img.reset();
            Img.engine = imaging;
            imaging.pages = [];
            imaging.work = null;
            imaging.clearLog();
            imaging.ocr = false;
            fake.connected = false;
            fake.readonly = false;
            fake.overrides = ({});
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
            flow.newScan();
            flow.mode = "normal";
            flow.stitchDirection = "left-right";
            flow.kind = "document";
            flow.sizeKey = "a4";
            flow.resolution = 300;
            flow.colorMode = "color";
            flow.format = "pdf";
            flow.range = "all";
            flow.singleFile = true;
            flow.errorSource = "";
            flow.scanning = false;
            fake.clearCalls();
        }

        // Scan and deliver `n` pages the way the service does.
        function scanPages(n) {
            verify(flow.startScan());
            tryVerify(() => fake.called("StartScan").length === 1);
            tryVerify(() => flow.scanSession === "scan-1");
            for (let i = 0; i < n; i++) {
                fake.busSignal("ScanPageReady", "scan-1", String(i), "/run/washi/scan-1/page-" + (i + 1) + ".jpg");
            }
            fake.busSignal("ScanFinished", "scan-1", "true", n + " page(s) scanned.");
            tryVerify(() => flow.step === "preview" && flow.mainPath !== "");
        }

        function test_setup_sends_the_scan_with_confirm() {
            compare(flow.step, "setup");
            flow.kind = "photo";
            flow.sizeKey = "l";
            flow.resolution = 600;
            flow.colorMode = "gray";
            verify(flow.startScan());
            tryVerify(() => fake.called("StartScan").length === 1);
            const body = JSON.parse(fake.called("StartScan")[0].args[1]);
            compare(body.confirm, true);
            compare(body.size, "l");
            compare(body.resolution, 600);
            compare(body.mode, "gray");
            compare(body.type, "photo");
            verify(!("auto_crop" in body));
            verify(flow.scanning);
            verify(!flow.canScan, "no second scan while one runs");
        }

        function test_pages_arrive_and_the_preview_opens() {
            scanPages(2);
            compare(flow.count, 2);
            compare(flow.current, 1, "the newest page is shown");
            compare(flow.pages.count, 2);
            compare(imaging.called("scanAddPage").length, 2);
            const first = imaging.called("scanAddPage")[0].arg;
            compare(first.dpi, 300);
            compare(first.mode, "color");
            verify(first.path.indexOf("page-1.jpg") >= 0);
            tryVerify(() => flow.pages.get(0).thumb !== "" && flow.pages.get(1).thumb !== "");
            verify(!flow.scanning);
        }

        function test_other_sessions_are_ignored() {
            verify(flow.startScan());
            tryVerify(() => flow.scanSession === "scan-1");
            fake.busSignal("ScanPageReady", "scan-7", "0", "/x/y.jpg");
            wait(100);
            compare(imaging.called("scanAddPage").length, 0);
            fake.busSignal("ScanFinished", "scan-1", "false", "the scanner is busy");
            compare(flow.error, "the scanner is busy");
            compare(flow.step, "setup", "a failed scan with no pages stays on the setup screen");
            verify(!flow.scanning);
        }

        function test_rotate_delete_and_revert_act_on_the_current_page() {
            scanPages(2);
            flow.select(0);
            flow.rotate("right");
            compare(imaging.called("scanRotate")[0].arg.i, 0);
            compare(imaging.called("scanRotate")[0].arg.dir, "right");
            flow.revert();
            compare(imaging.called("scanRevert")[0].arg, 0);
            flow.deleteCurrent();
            compare(flow.count, 1);
            compare(imaging.called("scanDelete")[0].arg, 0);
            compare(flow.step, "preview");
            flow.deleteCurrent();
            compare(flow.count, 0);
            compare(flow.step, "setup", "deleting the last page goes back to the setup");
        }

        function test_next_scan_adds_a_page_with_the_same_settings() {
            scanPages(1);
            flow.nextScan();
            verify(flow.awaitingNext);
            wait(50);
            compare(fake.called("StartScan").length, 1, "waits for the user to put the original on the glass");
            fake.clearCalls();
            flow.scanNext();
            verify(!flow.awaitingNext);
            tryVerify(() => fake.called("StartScan").length === 1);
            compare(JSON.parse(fake.called("StartScan")[0].args[1]).size, "a4");
            tryVerify(() => flow.scanSession === "scan-1");
            fake.busSignal("ScanPageReady", "scan-1", "0", "/run/washi/scan-2/page-1.jpg");
            fake.busSignal("ScanFinished", "scan-1", "true", "1 page(s) scanned.");
            tryVerify(() => flow.count === 2);
        }

        function test_rotate_crop_screen_commits_on_ok_and_discards_on_cancel() {
            scanPages(1);
            flow.openCrop();
            compare(flow.step, "crop");
            compare(imaging.called("editBegin")[0].arg, 0);
            tryVerify(() => flow.editPath !== "");
            // The screen shows the page turned and de-slanted, without crop or colour work.
            verify(imaging.called("scanRender").some(r => r.arg.mode === "work-geometry"));
            flow.cropRotate("left");
            compare(flow.currentEdits.rotation, "Left");
            flow.setSlant(2.5);
            compare(flow.currentEdits.slant_degrees, 2.5);
            flow.setSlant(40);
            compare(flow.currentEdits.slant_degrees, 10, "the slant stays within +-10 degrees");
            flow.setAspect("square");
            verify(flow.currentEdits.crop !== null);
            flow.setCrop(0.1, 0.1, 0.5, 0.5);
            compare(flow.currentEdits.crop.w, 0.5);
            flow.cropCancel();
            compare(flow.step, "preview");
            compare(imaging.called("editCancel").length, 1);
            compare(imaging.called("editCommit").length, 0);
            compare(imaging.pages[0].edits.rotation, "None", "cancelling leaves the page alone");

            flow.openCrop();
            flow.setSlant(1.5);
            flow.cropOk();
            compare(flow.step, "preview");
            compare(imaging.called("editCommit").length, 1);
            compare(imaging.pages[0].edits.slant_degrees, 1.5);
        }

        function test_automatic_straightening_sets_the_slant() {
            scanPages(1);
            flow.openCrop();
            flow.autoStraighten();
            tryVerify(() => flow.currentEdits.slant_degrees === -1.5);
            compare(imaging.called("editAutoDeskew").length, 1);
            flow.cropCancel();
        }

        function test_image_correction_screen() {
            scanPages(1);
            flow.openCorrect();
            compare(flow.step, "correct");
            flow.setCorrection({ color_correction: "Document" });
            flow.setCorrection({ reduce_show_through: true });
            flow.setCorrection({ color_mode: "Grayscale", unsharp_amount: 0.4 });
            const c = flow.currentEdits.correction;
            compare(c.color_correction, "Document");
            compare(c.reduce_show_through, true);
            compare(c.color_mode, "Grayscale");
            compare(c.unsharp_amount, 0.4);
            compare(imaging.called("editCorrection").length, 3);
            tryVerify(() => imaging.called("scanRender").some(r => r.arg.mode === "work"));
            flow.correctOk();
            compare(flow.step, "preview");
            compare(imaging.pages[0].edits.correction.color_mode, "Grayscale");
        }

        function test_steps_do_not_leak_into_each_other() {
            scanPages(1);
            flow.openCorrect();
            flow.cropOk();                 // not the crop screen: ignored
            compare(flow.step, "correct");
            flow.back();                   // back from an edit screen cancels it
            compare(flow.step, "preview");
            compare(imaging.called("editCancel").length, 1);
            flow.back();
            compare(flow.step, "setup");
            compare(flow.count, 1, "the pages are kept when going back to the settings");
        }

        function test_save_asks_the_host_with_the_chosen_options() {
            scanPages(2);
            flow.format = "jpeg";
            flow.range = "current";
            flow.select(0);
            let result = null;
            flow.save("/users/me/Scan_1", (ok, r) => { result = r; });
            tryVerify(() => result !== null);
            const req = imaging.called("scanSave")[0].arg;
            compare(req.path, "/users/me/Scan_1");
            compare(req.format, "jpeg");
            compare(req.range, "current");
            compare(req.current, 0);
            compare(flow.savedFiles.length, 1);
            verify(flow.savedFiles[0].endsWith(".jpg"));
        }

        function test_auto_scan_and_multi_photo_scan_the_whole_glass() {
            flow.mode = "auto";
            verify(flow.startScan());
            tryVerify(() => fake.called("StartScan").length === 1);
            let body = JSON.parse(fake.called("StartScan")[0].args[1]);
            compare(body.size, "auto");
            compare(body.auto_crop, false);
            fake.busSignal("ScanPageReady", "scan-1", "0", "/run/washi/scan-1/page-1.jpg");
            fake.busSignal("ScanFinished", "scan-1", "true", "1 page(s) scanned.");
            tryVerify(() => flow.step === "preview");
            compare(imaging.called("scanExtra")[0].arg.op, "classify");
            compare(flow.detected, "document");
            compare(flow.format, "pdf");
            compare(imaging.pages[0].edits.correction.color_correction, "Document");

            flow.newScan();
            fake.clearCalls();
            imaging.clearLog();
            flow.mode = "multi-photo";
            flow.startScan();
            tryVerify(() => flow.scanSession === "scan-1");
            fake.busSignal("ScanPageReady", "scan-1", "0", "/run/washi/scan-1/page-1.jpg");
            fake.busSignal("ScanFinished", "scan-1", "true", "1 page(s) scanned.");
            tryVerify(() => flow.step === "preview");
            compare(imaging.called("scanExtra")[0].arg.op, "crop-photos");
            compare(flow.count, 2, "one page per photo");
        }

        function stitchRequests() {
            return imaging.called("scanExtra").filter(r => r.arg.op === "stitch").map(r => r.arg);
        }

        function test_the_stitch_direction_is_chosen_in_setup_and_used_to_join() {
            flow.mode = "stitch";
            const setup = createTemporaryObject(setupComp, root);
            verify(findChild(setup, "stitchDirectionRow").visible);
            verify(findChild(setup, "stitch-left-right").checked, "left and right first");
            verify(!findChild(setup, "stitch-top-bottom").checked);
            mouseClick(findChild(setup, "stitch-top-bottom"));
            compare(flow.stitchDirection, "top-bottom");
            verify(findChild(setup, "stitch-top-bottom").checked);
            verify(!findChild(setup, "stitch-left-right").checked);
            // The row is only for Stitch.
            flow.mode = "normal";
            verify(!findChild(setup, "stitchDirectionRow").visible);
            flow.mode = "stitch";
            scanPages(2);
            flow.stitchLastTwo();
            tryVerify(() => stitchRequests().length === 1);
            compare(stitchRequests()[0].direction, "top-bottom");
            compare(stitchRequests()[0].a, 0);
            compare(stitchRequests()[0].b, 1);
        }

        function test_the_join_button_of_the_preview_offers_both_directions() {
            flow.mode = "stitch";
            scanPages(2);
            const view = createTemporaryObject(previewComp, root);
            verify(findChild(view, "joinButton").visible);
            // Left and right is the default; the menu next to Join changes it.
            const menu = findChild(view, "joinMenu");
            verify(menu !== null);
            compare(menu.count, 2);
            verify(menu.itemAt(0).checked && !menu.itemAt(1).checked);
            menu.itemAt(1).triggered();
            verify(menu.itemAt(1).checked, "the menu shows the choice");
            compare(flow.stitchDirection, "top-bottom");
            mouseClick(findChild(view, "joinButton"));
            tryVerify(() => stitchRequests().length === 1);
            compare(stitchRequests()[0].direction, "top-bottom");
            // An explicit direction wins over the chosen one; anything else is the chosen one.
            flow.newScan();
            fake.clearCalls();
            flow.mode = "stitch";
            scanPages(2);
            compare(flow.count, 2);
            flow.stitchLastTwo("left-right");
            tryVerify(() => stitchRequests().length === 2);
            compare(stitchRequests()[1].direction, "left-right");
        }

        function test_stitching_needs_two_pages() {
            flow.mode = "stitch";
            scanPages(1);
            flow.stitchLastTwo();
            wait(50);
            compare(stitchRequests().length, 0);
        }

        function test_read_only_never_scans() {
            fake.readonly = true;
            verify(!flow.canScan);
            verify(!flow.startScan());
            wait(100);
            compare(fake.called("StartScan").length, 0);
        }

        function test_scan_model_mappings() {
            compare(ScanModel.resolutions(fake.scanCaps).join(","), "75,100,150,200,300,600,1200");
            compare(ScanModel.resolutions(null).length, 7);
            compare(ScanModel.defaultResolution("document", [75, 150, 300, 600]), 300);
            compare(ScanModel.defaultResolution("photo", [75, 150, 300, 600]), 600);
            compare(ScanModel.defaultResolution("photo", [75, 150, 300]), 150, "the middle one when the usual is missing");
            compare(ScanModel.scanModes({ ocr: false }).length, 4);
            compare(ScanModel.scanModes({ ocr: true }).length, 5);
            compare(ScanModel.formats({ ocr: false }).length, 2);
            compare(ScanModel.formats({ ocr: true }).length, 4);
            compare(ScanModel.suggestedName(new Date(2026, 9, 13, 15, 30, 12)), "Scan_20261013_153012");
            compare(ScanModel.aspectPresets().map(a => a.value).join(","), "custom,original,square,3:2,4:3,7:5,10:8,16:9");
            compare(ScanModel.extensionFor("jpeg"), ".jpg");
        }
    }
}
