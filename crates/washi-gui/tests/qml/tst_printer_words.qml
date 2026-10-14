import QtQuick
import QtTest 1.2

// The printer's own words and picture: paper names from the printer in the language shown (and
// following the switch), Washi's own names when the printer says nothing, and the picture of
// the printer in the order local pack > the printer's own > Washi's drawing.
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

    Loader {
        id: loader
        anchors.fill: parent
    }
    Component {
        id: photoComp
        PrintPage { documentMode: false }
    }
    Component {
        id: listComp
        PrinterListPage {}
    }
    Component {
        id: bannerComp
        PrinterBanner { width: 1000 }
    }

    // Names written for these tests (not a printer's).
    readonly property var words: ({
        en: { "media-type.com.canon-mtglossy": "Test Glossy Paper", "media-type.com.canon-mtluster": "Test Luster <b>Paper</b>", "media-source.rear": "Test Rear Tray" },
        ja: { "media-type.com.canon-mtglossy": "テスト光沢紙", "media-type.com.canon-mtluster": "テストラスター紙" }
    })

    TestCase {
        name: "PrinterWords"
        when: windowShown

        function boot(words, icon) {
            loader.sourceComponent = null;
            Store.reset();
            Store.transport = null;
            Img.reset();
            Img.engine = imaging;
            Prefs.hideLoadPaperHint = true;
            fake.connected = false;
            fake.chooseLanguage("en");
            fake.words = words === undefined ? root.words : words;
            fake.iconPath = icon || "";
            fake.pack = "";
            fake.overrides = ({});
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
        }

        function cleanupTestCase() {
            fake.chooseLanguage("en");
        }

        function ownPicture() {
            return Qt.resolvedUrl("own-picture.png").toString().replace(/^file:\/\//, "");
        }

        function asked(lang) {
            return fake.called("GetPrinterStrings").filter(c => c.args[1] === lang);
        }

        function test_names_come_from_the_printer_in_the_language_shown() {
            boot();
            tryVerify(() => Object.keys(Store.printerStrings).length > 0);
            compare(PrintModel.mediaLabel("com.canon-mtglossy"), "Test Glossy Paper");
            compare(Store.printerText("media-source", "rear", "x"), "Test Rear Tray");
            // A keyword the printer does not name keeps Washi's own name; so do the standard ones.
            compare(PrintModel.mediaLabel("com.canon-mtmat"), "Matte Photo Paper");
            compare(PrintModel.mediaLabel("stationery"), "Plain paper");
            compare(Store.printerText("media-type", "com.canon-mtmat", "fallback"), "fallback");
            // The paper types of the disc page too.
            compare(DiscMath.mediaLabel("a"), "Printable disc (recommended)");

            // The switch changes the language of the names at once, in both directions.
            Lang.choose("ja");
            tryVerify(() => PrintModel.mediaLabel("com.canon-mtglossy") === "テスト光沢紙");
            compare(PrintModel.mediaLabel("com.canon-mtluster"), "テストラスター紙");
            compare(Store.printerText("media-source", "rear", "x"), "x", "the Japanese file has no entry for it");
            Lang.choose("en");
            tryVerify(() => PrintModel.mediaLabel("com.canon-mtglossy") === "Test Glossy Paper");
        }

        function test_each_language_is_asked_for_once_and_nothing_else_is_sent() {
            boot();
            tryVerify(() => asked("en").length === 1);
            compare(asked("ja").length, 0, "the other language is not asked for before it is needed");
            Lang.choose("ja");
            tryVerify(() => asked("ja").length === 1);
            Lang.choose("en");
            Lang.choose("ja");
            wait(100);
            compare(asked("en").length, 1);
            compare(asked("ja").length, 1);
            compare(asked("en")[0].args[0], "printer.test");
            // Reads only: nothing that changes the printer was called.
            const methods = fake.calls.map(c => c.method);
            for (const w of ["RunMaintenance", "ApplySettings", "StartScan", "PrintFile", "PrintFiles", "PrintDisc", "Wake", "UpdateFirmware"]) {
                verify(methods.indexOf(w) < 0, w);
            }
        }

        function test_the_media_list_of_the_photo_page_follows_the_names_and_the_switch() {
            boot();
            loader.sourceComponent = photoComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            const page = loader.item;
            page.setFiles(["/p/beach.jpg"]);
            const combo = findChild(page, "mediaCombo");
            tryVerify(() => combo.count > 0);
            tryVerify(() => combo.displayText === "Test Glossy Paper");
            const texts = () => { const t = []; for (let i = 0; i < combo.count; i++) { t.push(combo.textAt(i)); } return t; };
            verify(texts().indexOf("Test Luster <b>Paper</b>") >= 0, texts().join("|"));
            compare(combo.contentItem.textFormat, Text.PlainText, "a name is never read as markup");
            const chosen = page.mediaType;
            Lang.choose("ja");
            tryVerify(() => combo.displayText === "テスト光沢紙");
            verify(texts().indexOf("テストラスター紙") >= 0, texts().join("|"));
            compare(page.mediaType, chosen, "the choice stays while the names change");
            Lang.choose("en");
            tryVerify(() => combo.displayText === "Test Glossy Paper");
            // The sentence that asks before printing uses the printer's name for the paper.
            page.askToPrint();
            const confirm = findChild(page, "printConfirm");
            tryVerify(() => confirm.visible);
            verify(confirm.text.indexOf("Test Glossy Paper") >= 0, confirm.text);
            confirm.close();
        }

        function test_a_printer_that_says_nothing_keeps_washis_names() {
            boot({ en: {}, ja: {} });
            tryVerify(() => asked("en").length === 1);
            wait(50);
            compare(PrintModel.mediaLabel("com.canon-mtglossy"), "Glossy Photo Paper");
            Lang.choose("ja");
            tryVerify(() => asked("ja").length === 1);
            compare(PrintModel.mediaLabel("com.canon-mtglossy"), "Glossy Photo Paper");
            compare(PrintModel.mediaLabel("com.canon-mthagaki"), "はがき");
            Lang.choose("en");
        }

        function test_a_failed_read_is_asked_again_with_the_next_reading_of_the_printer() {
            boot();
            Store.reset();
            Store.transport = null;
            fake.overrides = ({ GetPrinterStrings: [false, Store.errorBody("Printer", "away")] });
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = false;
            fake.connected = true;
            tryVerify(() => asked("en").length >= 1);
            wait(150);
            compare(PrintModel.mediaLabel("com.canon-mtglossy"), "Glossy Photo Paper", "Washi's own name meanwhile");
            // The printer is back: the next reading of it asks again, and the names arrive.
            const failed = asked("en").length;
            fake.overrides = ({});
            Store.applySnapshot(fake.snapshot);
            tryVerify(() => PrintModel.mediaLabel("com.canon-mtglossy") === "Test Glossy Paper");
            compare(asked("en").length, failed + 1);
            // Once the names are there, a reading does not ask again.
            Store.applySnapshot(fake.snapshot);
            wait(100);
            compare(asked("en").length, failed + 1);
        }

        function test_the_names_of_one_printer_are_not_shown_for_another() {
            boot();
            tryVerify(() => Store.printerText("media-source", "rear", "") !== "");
            const other = JSON.parse(JSON.stringify(fake.printers));
            other[0].active = false;
            other.push({ host: "other.test", name: null, source: "flag", active: true, power: "on", last_updated: 1 });
            Store.applyPrinters(other);
            compare(Store.printerText("media-source", "rear", "x"), "x", "cleared with the printer");
            tryVerify(() => fake.called("GetPrinterStrings").some(c => c.args[0] === "other.test"));
        }

        function test_the_picture_is_the_printers_own_unless_a_pack_image_is_installed() {
            boot(undefined, ownPicture());
            tryVerify(() => Store.printerIcon !== "");
            verify(Store.printerIcon.indexOf("file://") === 0 && Store.printerIcon.indexOf("own-picture.png") > 0, Store.printerIcon);
            const pic = createTemporaryObject(picComp, root, { model: "XK140 series" });
            const own = findChild(pic, "ownPicture");
            const pack = findChild(pic, "packPicture");
            const art = findChild(pic, "printerArt");
            tryVerify(() => own.status === Image.Ready);
            verify(own.visible && !art.visible && !pack.visible, "the printer's picture");
            compare(own.fillMode, Image.PreserveAspectFit);
            // A pack image installed by the person wins.
            fake.pack = Qt.resolvedUrl("pack-picture.png").toString();
            pic.model = "";
            pic.model = "XK140 series";
            tryVerify(() => pack.status === Image.Ready);
            verify(pack.visible && !own.visible && !art.visible, "the pack image");
        }

        function test_without_any_picture_washis_drawing_is_shown() {
            boot(undefined, "");
            tryVerify(() => fake.called("GetPrinterIcon").length > 0);
            wait(60);
            compare(Store.printerIcon, "");
            const pic = createTemporaryObject(picComp, root, { model: "XK140 series" });
            const art = findChild(pic, "printerArt");
            tryVerify(() => art.visible);
            verify(!findChild(pic, "ownPicture").visible);
            // A file that is gone or is no picture is not shown either.
            pic.iconSource = "file:///does/not/exist.png";
            tryVerify(() => findChild(pic, "ownPicture").status === Image.Error);
            verify(art.visible);
        }

        function test_a_path_with_special_characters_becomes_a_working_url() {
            boot();
            compare(Store._fileUrl("/home/a b/.cache/washi/x#1.png"), "file:///home/a%20b/.cache/washi/x%231.png");
        }

        function test_the_banner_and_the_list_show_it() {
            boot(undefined, ownPicture());
            tryVerify(() => Store.printerIcon !== "");
            loader.sourceComponent = bannerComp;
            tryVerify(() => loader.item !== null);
            const pic = loader.item.children.length > 0 ? findChild(loader.item, "ownPicture") : null;
            verify(pic !== null);
            tryVerify(() => pic.status === Image.Ready && pic.visible);
            verify(!findChild(loader.item, "printerArt").visible);
            // The picture keeps its proportions inside the banner's 170 x 86 slot.
            verify(pic.paintedWidth / pic.paintedHeight > 1.45 && pic.paintedWidth / pic.paintedHeight < 1.55, pic.paintedWidth + "x" + pic.paintedHeight);
            verify(pic.paintedHeight <= 86 && pic.paintedWidth <= 170);

            loader.sourceComponent = listComp;
            tryVerify(() => loader.item !== null);
            const row = findChild(loader.item, "listPicture-0");
            tryVerify(() => row !== null && row.hasPicture);
        }

        Component {
            id: picComp
            PrinterImage {
                width: 170
                height: 86
            }
        }
    }
}
