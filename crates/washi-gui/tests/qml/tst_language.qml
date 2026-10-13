import QtQuick
import QtTest 1.2

// The language switch: the segmented control, and that switching updates every visible string
// at once, in both directions, without recreating anything.
Item {
    id: root
    width: 1040
    height: 700

    FakeTransport {
        id: fake
    }
    FakeImaging {
        id: imaging
    }
    Audit {
        id: audit
    }

    LangSwitch {
        id: sw
        x: 8
        y: 8
    }
    Item {
        id: frame
        x: 0
        y: 40
        width: 1000
        height: 640
        Loader {
            id: loader
            anchors.fill: parent
        }
    }
    DiscoverDialog { id: discoverDlg }
    LicenseDialog { id: licenseDlg }
    MaintenanceHost { id: maint }
    ManualAlignHost { id: manualAlign }
    SettingDialog { id: setting }
    LinkHost { id: links }
    FirmwareHost { id: firmware }
    LoadPaperDialog { id: loadPaper }
    PrintJobDialog { id: printJob }
    WakeButton { id: wake; objectName: "wake" }
    Component { id: homeComp; HomePage {} }
    Component { id: managementComp; ManagementPage {} }
    Component { id: settingsComp; AppSettingsPage {} }
    Component { id: listComp; PrinterListPage {} }
    Component { id: photoComp; PrintPage { documentMode: false } }
    Component { id: docComp; PrintPage { documentMode: true } }
    Component { id: scanComp; ScanPage {} }
    Component { id: discComp; DiscLabelPage {} }

    TestCase {
        name: "Language"
        when: windowShown

        function init() {
            loader.sourceComponent = null;
            closeDialogs();
            Store.reset();
            Store.transport = null;
            Img.reset();
            Img.engine = imaging;
            imaging.pages = [];
            imaging.work = null;
            Prefs.hideLoadPaperHint = true;
            Nav.page = "home";
            Nav.history = [];
            fake.connected = true;
            fake.overrides = ({});
            fake.readonly = false;
            fake.savedLanguages = [];
            Store.transport = fake;
            fake.chooseLanguage("en");
            fake.savedLanguages = [];
            fake.clearCalls();
            tryVerify(() => Store.snapLoaded);
            Store.loadCaps(true);
            Store.loadPrintCaps(true);
            Store.loadScanCaps(true);
            tryVerify(() => Store.maintCap !== null && Store.settings !== null && Store.printCaps !== null && Store.scanCaps !== null);
        }

        // ---- helpers ---------------------------------------------------------------------------
        function closeDialogs() {
            discoverDlg.close();
            licenseDlg.close();
            loadPaper.close();
            printJob.close();
            maint.cancel();
            manualAlign.cancel();
            setting.cancel();
            firmware.cancel();
            for (const c of links.data) {
                if (c.close) {
                    c.close();
                }
            }
        }

        function show(comp) {
            loader.sourceComponent = comp;
            tryVerify(() => loader.item !== null);
            wait(150);
            return loader.item;
        }

        // The texts a person sees now, in order.
        function seen(popup) {
            return audit.texts(popup ? audit.popupItems(popup) : audit.items(loader.item));
        }

        // The same screen in English, then in Japanese, then in English again, without
        // rebuilding anything: the texts of the first and the last must be equal, those of the
        // second must be Japanese wherever the first was English.
        function followsTheLanguage(name, popup) {
            Lang.choose("en");
            wait(60);
            const en = seen(popup);
            verify(en.length > 0, name + ": nothing to look at");
            Lang.choose("ja");
            wait(60);
            const ja = seen(popup);
            compare(ja.length, en.length, name + ": the number of texts changed with the language");
            const same = [];
            const english = [];
            for (let i = 0; i < en.length; i++) {
                if (audit.englishWords(en[i]).length > 0 && ja[i] === en[i]) {
                    same.push(en[i]);
                }
                const left = audit.englishWords(ja[i]);
                if (left.length > 0) {
                    english.push(ja[i] + "  [" + left.join(", ") + "]");
                }
            }
            compare(same, [], name + ": still English in Japanese");
            compare(english, [], name + ": English words left in Japanese text");
            Lang.choose("en");
            wait(60);
            compare(seen(popup), en, name + ": back in English the texts are not what they were");
        }

        function cleanupTestCase() {
            fake.chooseLanguage("en");
        }

        function test_catalog_is_there() {
            verify(L10n.hasCatalog("ja"), "ja catalog not found");
        }

        function test_switch_shows_the_current_language() {
            const en = findChild(sw, "lang-en");
            const ja = findChild(sw, "lang-ja");
            verify(en && ja);
            compare(en.selected, true);
            compare(ja.selected, false);
            compare(Lang.current, "en");
            mouseClick(ja);
            compare(Lang.current, "ja");
            compare(ja.selected, true);
            compare(en.selected, false);
            compare(fake.savedLanguages, ["ja"]);
            mouseClick(en);
            compare(Lang.current, "en");
            compare(fake.savedLanguages, ["ja", "en"]);
        }

        function test_the_switch_is_reachable_from_the_keyboard() {
            const ja = findChild(sw, "lang-ja");
            ja.forceActiveFocus();
            verify(ja.activeFocus);
            keyClick(Qt.Key_Space);
            compare(Lang.current, "ja");
        }

        function test_the_daemon_is_told() {
            Lang.choose("ja");
            tryVerify(() => fake.called("SetLanguage").length === 1);
            compare(fake.called("SetLanguage")[0].args, ["ja"]);
            Lang.choose("en");
            tryVerify(() => fake.called("SetLanguage").length === 2);
            compare(fake.called("SetLanguage")[1].args, ["en"]);
            // and again when the service (re)starts
            fake.clearCalls();
            fake.connected = false;
            fake.connected = true;
            tryVerify(() => fake.called("SetLanguage").length === 1);
            compare(fake.called("SetLanguage")[0].args, ["en"]);
        }

        function test_a_plain_binding_follows_the_language() {
            const item = createTemporaryObject(textItem, root);
            compare(item.text, "Language");
            Lang.choose("ja");
            compare(item.text, "言語");
            Lang.choose("en");
            compare(item.text, "Language");
        }

        // ---- every visible string, live -------------------------------------------------------------
        function test_home() {
            show(homeComp);
            followsTheLanguage("home");
        }

        function test_printer_management_tabs() {
            for (const tab of ["consumables", "status", "device-info", "utilities"]) {
                Nav.page = tab;
                show(managementComp);
                followsTheLanguage(tab);
            }
        }

        function test_app_settings_and_printer_list() {
            show(settingsComp);
            followsTheLanguage("app settings");
            show(listComp);
            followsTheLanguage("printer list");
        }

        function test_print_pages() {
            show(photoComp).setFiles(["/p/beach.jpg", "/p/b.jpg"]);
            followsTheLanguage("photo print");
            const doc = show(docComp);
            doc.setFiles(["/d/doc.pdf"]);
            doc.allPages = false;
            doc.pagesText = "9-2";
            wait(100);
            followsTheLanguage("document print");
        }

        function test_scan_screens() {
            const page = show(scanComp);
            followsTheLanguage("scan setup");
            const flow = page.flow;
            flow.startScan();
            tryVerify(() => flow.scanSession !== "");
            fake.busSignal("ScanPageReady", flow.scanSession, "0", "/run/washi/p1.jpg");
            fake.busSignal("ScanFinished", flow.scanSession, "true", "1 page scanned.");
            tryVerify(() => flow.step === "preview" && flow.mainPath !== "");
            flow.noticeSource = () => i18n("This looks like a document: it is saved as PDF.");
            wait(100);
            followsTheLanguage("scan preview");
            flow.openCrop();
            wait(150);
            followsTheLanguage("rotate and crop");
            flow.back();
            flow.openCorrect();
            wait(150);
            followsTheLanguage("image correction");
        }

        function test_disc_label() {
            const page = show(discComp);
            page.addText("Summer mix");
            wait(100);
            followsTheLanguage("disc label");
            // The same text following the ring, and a picture that cannot be used.
            page.change(d => { d.texts[0].ring = DiscMath.newRing(); });
            wait(150);
            followsTheLanguage("disc label with ring text");
            page.dropFiles(["file:///users/me/notes.txt"]);
            wait(100);
            followsTheLanguage("disc label with a refused drop");
        }

        function test_a_refused_drop_on_the_print_pages() {
            const photo = show(photoComp);
            photo.dropFiles(["file:///users/me/notes.txt"]);
            wait(100);
            followsTheLanguage("photo print with a refused drop");
            const doc = show(docComp);
            doc.dropFiles(["file:///users/me/pic.jpg"]);
            wait(100);
            followsTheLanguage("document print with a refused drop");
        }

        // The Status tab with a support code: the code, its words and the help button.
        function test_support_codes_on_the_status_tab() {
            const cases = [
                ["error", { code: "1300", message_key: "support.paper-jam", message: "x", severity: "error" }],
                ["warning", { code: "1000", message_key: "support.no-paper-rear", message: "x", severity: "warning" }],
                ["notice", { code: "D109", message_key: "support.notice", message: "x", severity: "info" }],
                ["unknown", { code: "9999", message_key: "support.unknown", message: "x", severity: "warning" }]
            ];
            Nav.page = "status";
            for (const c of cases) {
                const s = JSON.parse(JSON.stringify(Store.snap));
                s.support = c[1];
                Store.applySnapshot(s);
                show(managementComp);
                followsTheLanguage("status with a " + c[0] + " code");
            }
            Lang.choose("ja");
            compare(Store.statusText, "サポート番号 9999");
            Lang.choose("en");
        }

        function test_the_link_questions() {
            for (const k of ["ink", "help", "error", "support", "printer", "maintenance"]) {
                links.open({ kind: k, code: "1300" });
                tryVerify(() => links.link !== null);
                wait(100);
                followsTheLanguage("link " + k, popupOf(links));
                popupOf(links).close();
            }
            // The printer had no address of its own for this.
            fake.overrides = ({ GetWebLinks: [true, JSON.stringify({ ink: { url: "http://printer.test/", source: "printer" }, help: { url: "http://printer.test/", source: "printer" }, printer_page: { url: "http://printer.test/", source: "printer" }, maintenance_page: { url: "http://printer.test/", source: "printer" }, support: { url: "http://printer.test/", source: "printer" }, from_printer: false })] });
            links.open({ kind: "help" });
            tryVerify(() => links.link !== null);
            wait(100);
            followsTheLanguage("link fallback", popupOf(links));
            popupOf(links).close();
        }

        function test_the_firmware_card_and_dialog() {
            const s = JSON.parse(JSON.stringify(Store.snap));
            Nav.page = "device-info";
            for (const state of ["available", "unavailable", "unknown"]) {
                s.firmware_update = state;
                Store.applySnapshot(s);
                show(managementComp);
                followsTheLanguage("firmware card (" + state + ")");
            }
            s.firmware_update = "available";
            Store.applySnapshot(s);
            firmware.open();
            wait(120);
            followsTheLanguage("firmware warning", popupOf(firmware));
            firmware.understood = true;
            firmware.accept();
            tryVerify(() => firmware.jobId === "job-1");
            fake.busSignal("MaintenanceProgress", "job-1", "waiting", "…");
            wait(100);
            followsTheLanguage("firmware running", popupOf(firmware));
            fake.busSignal("MaintenanceFinished", "job-1", "true", "完了しました");
            wait(100);
            followsTheLanguage("firmware done", popupOf(firmware));
            firmware.cancel();
        }

        function test_the_extra_maintenance_cards_and_dialogs() {
            const caps = JSON.parse(JSON.stringify(Store.maintCap));
            caps.cleaning.push({ kind: "roller", inkgroups: ["all"] }, { kind: "platen", inkgroups: ["all"] });
            Store.maintCap = caps;
            Nav.page = "utilities";
            show(managementComp);
            followsTheLanguage("utilities with the roller and bottom plate cleaning");
            for (const kind of ["roller", "platen"]) {
                maint.open({ op: "clean", kind: kind });
                wait(120);
                followsTheLanguage("cleaning " + kind, popupOf(maint));
                maint.cancel();
            }
        }

        function test_dialogs() {
            const cases = [
                ["add printer", () => discoverDlg.open(), discoverDlg],
                ["license", () => licenseDlg.open(), licenseDlg],
                ["load paper", () => loadPaper.openFor("rear", true, true), loadPaper],
                ["disc tray", () => loadPaper.openFor("disc", false, true), loadPaper],
                ["print progress", () => printJob.start(["/p/a.jpg"], {}, "a.jpg"), null]
            ];
            for (const c of cases) {
                Lang.choose("en");
                c[1]();
                wait(150);
                const popup = c[2] || popupOf(printJob);
                followsTheLanguage(c[0], popup);
                popup.close();
            }
        }

        function test_the_maintenance_dialog_through_its_stages() {
            const stages = [
                () => maint.open({ op: "clean" }),
                () => maint.nextFromOptions(),
                () => { maint.accept(); },
            ];
            for (const step of stages) {
                step();
                wait(120);
                followsTheLanguage("maintenance " + maint.stage, popupOf(maint));
            }
            // the job finishes: the result and the follow-up are words of the app, too
            fake.busSignal("MaintenanceFinished", "job-1", "true", "完了しました");
            tryVerify(() => maint.stage === "done");
            followsTheLanguage("maintenance done", popupOf(maint));
            maint.cancel();
        }

        function popupOf(host) {
            for (const c of host.data) {
                if (c.opened !== undefined && c.contentItem) {
                    return c;
                }
            }
            return null;
        }

        function test_settings_and_alignment_dialogs() {
            for (const f of ["autopoweroff", "quiet", "dry_level", "paper_gap"]) {
                setting.open({ field: f });
                wait(100);
                followsTheLanguage("setting " + f, popupOf(setting));
            }
            setting.open({ field: "auto_firmware", value: true });
            wait(100);
            followsTheLanguage("setting toggle", popupOf(setting));
            setting.apply();
            tryVerify(() => setting.stage === "done");
            followsTheLanguage("setting result", popupOf(setting));
            setting.cancel();
            manualAlign.open();
            wait(100);
            followsTheLanguage("manual alignment", popupOf(manualAlign));
            manualAlign.begin();
            wait(100);
            followsTheLanguage("manual alignment: confirm", popupOf(manualAlign));
            manualAlign.patternDone();
            wait(100);
            followsTheLanguage("manual alignment: values", popupOf(manualAlign));
            manualAlign.cancel();
        }

        // ---- text that is put together later ---------------------------------------------------------
        function test_a_progress_line_follows_the_language_while_it_is_shown() {
            maint.open({ op: "nozzle_check" });
            maint.accept();
            compare(maint.stage, "running");
            compare(maint.progress, "Sending the job to the printer…");
            Lang.choose("ja");
            compare(maint.progress, "ジョブをプリンターへ送信しています…");
            // what the service says is its own words, in the language it was told
            fake.busSignal("MaintenanceProgress", "job-1", "waiting", "ノズルチェックパターンを印刷中…（4秒）");
            compare(maint.progress, "ノズルチェックパターンを印刷中…（4秒）");
            Lang.choose("en");
            compare(maint.progress, "ノズルチェックパターンを印刷中…（4秒）", "the service's text is not retranslated");
            maint.cancel();
            fake.busSignal("MaintenanceFinished", "job-1", "true", "done");
        }

        function test_failures_follow_the_language() {
            Store.snapErrorSource = Fmt.errorSource(Store.errorBody("NotRunning", ""));
            compare(Store.snapError, "The Washi service is not running.");
            Lang.choose("ja");
            compare(Store.snapError, "Washi のサービスが動作していません。");
            Store.capsErrorSource = Fmt.errorSource(Store.errorBody("ReadOnly", ""));
            compare(Store.capsError, "Washi のサービスが読み取り専用モードのため、変更はできません。");
            Lang.choose("en");
            compare(Store.capsError, "The Washi service is in read-only mode, so nothing can be changed.");
            // a message of the printer or the service is shown as it came
            Store.capsErrorSource = Fmt.errorSource(Store.errorBody("Printer", "プリンターがスリープ中です"));
            compare(Store.capsError, "プリンターがスリープ中です");
        }

        function test_the_status_follows_the_language() {
            compare(Store.statusText, "Online");
            Lang.choose("ja");
            compare(Store.statusText, "オンライン");
            Lang.choose("en");
            compare(Store.statusText, "Online");
        }

        function test_values_read_back_from_the_printer() {
            compare(Fmt.settingValue("ON 30 min"), "On, after 30 minutes");
            compare(Fmt.settingValue("time 2230-0600"), "Scheduled 22:30 – 06:00");
            compare(Fmt.settingValue("OFF"), "Off");
            compare(Fmt.settingValue("(not reported)"), "(not reported)");
            compare(Fmt.settingValue(3), "3");
            Lang.choose("ja");
            compare(Fmt.settingValue("ON 30 min"), "オン（30分後）");
            compare(Fmt.settingValue("time 2230-0600"), "指定時間 22:30〜06:00");
            compare(Fmt.settingValue("OFF"), "オフ");
            compare(Fmt.settingValue("(not reported)"), "（取得できません）");
            compare(Fmt.settingValue("1: A=0 B=1"), "1: A=0 B=1", "numbers and letters of the alignment stay as they are");
        }

        function test_support_codes_have_words_in_both_languages() {
            const jam = { code: "1300", message_key: "support.paper-jam" };
            verify(Fmt.supportMessage(jam).indexOf("Paper is jammed.") === 0);
            Lang.choose("ja");
            compare(Fmt.supportMessage(jam), "用紙がつまっています。慎重に取り除いてください。プリンターの電源を切るのは、画面で指示されたときだけにしてください。");
            compare(Fmt.supportMessage({ code: "9999", message_key: "support.unknown" }), "プリンターがサポート番号 9999 を報告しています。プリンターの表示か、ヘルプのリンクを確認してください。");
            // A family this app does not know yet still names the code.
            compare(Fmt.supportMessage({ code: "7777", message_key: "support.from-the-future" }), "プリンターがサポート番号 7777 を報告しています。プリンターの表示か、ヘルプのリンクを確認してください。");
            Lang.choose("en");
            verify(Fmt.supportMessage({ code: "7777", message_key: "support.from-the-future" }).indexOf("7777") >= 0);
            compare(Fmt.supportMessage(null), "");
        }

        // ---- plurals, numbers, units -----------------------------------------------------------------
        function test_plural_forms_and_units() {
            compare(Fmt.sheets(1), "1 sheet");
            compare(Fmt.sheets(3), "3 sheets");
            Lang.choose("ja");
            compare(Fmt.sheets(1), "1枚");
            compare(Fmt.sheets(3), "3枚");
            compare(i18n("%1 dpi", 600), "600 dpi");
            compare(i18np("after %1 minute", "after %1 minutes", 30), "30分後");
            compare(i18n("Outer diameter: %1 mm", "116.0"), "外径：116.0 mm");
            compare(i18n("Level %1", 3), "レベル 3");
            Lang.choose("en");
            compare(i18np("after %1 minute", "after %1 minutes", 1), "after 1 minute");
            compare(i18np("after %1 minute", "after %1 minutes", 30), "after 30 minutes");
        }

        function test_dates_and_numbers_use_the_language() {
            const d = new Date(2026, 9, 13, 21, 5, 9);
            compare(Fmt.clock(d), "21:05:09");
            compare(Lang.locale.name, "en_US");
            compare(d.toLocaleDateString(Lang.locale, Locale.LongFormat), "Tuesday, October 13, 2026");
            compare(Fmt.number(1234567), "1,234,567");
            Lang.choose("ja");
            compare(Lang.locale.name, "ja_JP");
            compare(Fmt.clock(d), "21:05:09");
            compare(d.toLocaleDateString(Lang.locale, Locale.LongFormat), "2026年10月13日火曜日");
            compare(Fmt.number(1234567), "1,234,567");
            // a binding follows by itself
            const item = createTemporaryObject(clockItem, root);
            compare(item.text, "21:05:09");
            compare(item.day, "2026年10月13日火曜日");
            Lang.choose("en");
            compare(item.day, "Tuesday, October 13, 2026");
        }

        Component {
            id: clockItem
            Text {
                readonly property date when: new Date(2026, 9, 13, 21, 5, 9)
                readonly property string day: when.toLocaleDateString(Lang.locale, Locale.LongFormat)
                text: Fmt.clock(when)
            }
        }

        // ---- controls keep what the user chose ------------------------------------------------------
        function test_combo_boxes_keep_their_choice_while_the_language_changes() {
            const page = show(photoComp);
            page.setFiles(["/p/beach.jpg"]);
            wait(100);
            page.color = false;
            page.quality = "high";
            page.source = "rear";
            wait(100);
            const color = findChild(page, "colorCombo");
            const quality = findChild(page, "qualityCombo");
            const size = findChild(page, "sizeCombo");
            const size0 = page.sizeKey;
            compare(color.displayText, "Black and white");
            compare(quality.displayText, "High");
            Lang.choose("ja");
            wait(150);
            compare(color.displayText, "モノクロ");
            compare(quality.displayText, "高");
            compare(page.color, false);
            compare(page.quality, "high");
            compare(page.sizeKey, size0);
            verify(size.displayText.length > 0 && audit.englishWords(size.displayText).length === 0, size.displayText);
            Lang.choose("en");
            wait(150);
            compare(color.displayText, "Black and white");
            compare(color.currentValue, false);
            compare(quality.currentValue, "high");
        }

        function test_the_pages_range_message_follows() {
            const doc = show(docComp);
            doc.setFiles(["/d/doc.pdf"]);
            doc.allPages = false;
            doc.pagesText = "5-1";
            wait(100);
            compare(PrintModel.pagesError("backwards"), "A range must go from the lower page to the higher one.");
            Lang.choose("ja");
            compare(PrintModel.pagesError("backwards"), "範囲は、小さいページ番号から大きいページ番号の順に指定してください。");
        }

        // ---- persistence and the service ---------------------------------------------------------------
        function test_the_choice_is_handed_to_the_backend_to_be_saved() {
            Lang.choose("ja");
            Lang.choose("en");
            Lang.choose("system");
            compare(fake.savedLanguages, ["ja", "en", "system"]);
            compare(Lang.choose("klingon"), undefined);
            compare(fake.savedLanguages.length, 3, "an unknown choice is refused");
        }

        function test_the_app_starts_in_the_language_the_backend_reports() {
            // (what main() read from config.toml or the session's locale)
            const saved = fake.language;
            fake.language = "ja";
            compare(Lang.current, "ja");
            compare(Lang.ja, true);
            fake.language = saved;
            compare(Lang.current, "en");
        }

        Component {
            id: textItem
            Text {
                text: i18n("Language")
            }
        }
    }
}
