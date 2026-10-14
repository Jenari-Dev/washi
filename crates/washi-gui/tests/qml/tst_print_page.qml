import QtQuick
import QtTest 1.2

// Photo Print and Document Print pages: what they offer from the printer's media summary, how
// the choices stay consistent, and that nothing is sent before the user confirms.
Item {
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
        id: docComp
        PrintPage { documentMode: true }
    }

    TestCase {
        name: "PrintPage"
        when: windowShown

        function boot(readOnly, power) {
            loader.sourceComponent = null;
            Store.reset();
            Store.transport = null;
            Img.reset();
            Img.engine = imaging;
            Prefs.hideLoadPaperHint = true;
            fake.connected = false;
            fake.readonly = !!readOnly;
            fake.env = ({});
            const snap = JSON.parse(JSON.stringify(fake.snapshot));
            snap.power = power || "on";
            fake.overrides = ({ GetSnapshot: [true, JSON.stringify(snap)] });
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
        }

        function photoPage(files) {
            boot(false);
            loader.sourceComponent = photoComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            if (files) {
                loader.item.setFiles(files);
            }
            return loader.item;
        }

        function printCalls() {
            return fake.called("PrintFile").concat(fake.called("PrintFiles"));
        }

        function test_dropped_pictures_are_selected_like_chosen_ones() {
            const p = photoPage();
            verify(findChild(p, "dropArea") !== null, "the preview takes drops");
            verify(!findChild(p, "dropHighlight").visible);
            p.dropFiles(["file:///users/me/Pictures/beach%20day.jpg", "file:///users/me/Pictures/b.png", "file:///users/me/notes.txt"]);
            compare(p.files.join("|"), "/users/me/Pictures/beach day.jpg|/users/me/Pictures/b.png", "the unsupported file is left out");
            compare(p.dropNotice, "");
            // Dropping something else changes nothing and says why.
            p.dropFiles(["file:///users/me/report.pdf"]);
            compare(p.files.length, 2);
            verify(p.dropNotice.indexOf("JPEG, PNG and BMP") >= 0, p.dropNotice);
            verify(findChild(p, "dropNotice").visible);
            // A good drop clears the notice.
            p.dropFiles(["file:///users/me/c.bmp"]);
            compare(p.files.join("|"), "/users/me/c.bmp");
            compare(p.dropNotice, "");
            // Only local files count.
            p.dropFiles(["https://example.org/x.jpg"]);
            compare(p.files.join("|"), "/users/me/c.bmp");
            // Nothing was sent to the printer by dropping.
            compare(printCalls().length, 0);
        }

        function test_a_dropped_document_replaces_the_current_one() {
            boot(false);
            loader.sourceComponent = docComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            const p = loader.item;
            verify(findChild(p, "dropArea") !== null);
            p.dropFiles(["file:///users/me/a.pdf", "file:///users/me/b.pdf"]);
            compare(p.files.join("|"), "/users/me/a.pdf", "one document at a time");
            p.dropFiles(["file:///users/me/photo.jpg"]);
            compare(p.files.join("|"), "/users/me/a.pdf");
            verify(p.dropNotice.indexOf("PDF and Office documents") >= 0, p.dropNotice);
            p.dropFiles(["file:///users/me/letter.docx"]);
            compare(p.files.join("|"), "/users/me/letter.docx");
        }

        function test_the_unprintable_edge_is_framed_on_the_preview_unless_borderless() {
            const p = photoPage(["/p/beach.jpg"]);
            const sheet = findChild(p, "photoSheet");
            const frame = findChild(sheet, "unprintableFrame");
            compare(p.borderless, true);
            verify(sheet.edge === null, "borderless prints to the edge: nothing to mark");
            verify(!frame.visible);
            verify(!findChild(p, "edgeHint").visible);
            // With margins the printer's own figures are drawn, where the sheet is.
            p.sizeKey = "a4";
            p.mediaType = "stationery";
            p.orientation = "portrait";
            verify(!p.borderless);
            verify(sheet.edge !== null);
            compare(sheet.edge.top, 3);
            compare(sheet.edge.bottom, 12);
            verify(frame.visible);
            verify(findChild(p, "edgeHint").visible);
            tryVerify(() => frame.width > 50);
            fuzzyCompare(frame.x0 / frame.width, 3.4 / 210, 0.001);
            fuzzyCompare(1 - frame.x1 / frame.width, 5.6 / 210, 0.001);
            fuzzyCompare(frame.y0 / frame.height, 3 / 297, 0.001);
            fuzzyCompare(1 - frame.y1 / frame.height, 12 / 297, 0.001);
            // Turned on its side, the same figures turn with the paper.
            p.orientation = "landscape";
            compare(sheet.edge.left, 3);
            compare(sheet.edge.right, 12);
            tryVerify(() => frame.width > frame.height);
            fuzzyCompare(frame.x0 / frame.width, 3 / 297, 0.001);
            fuzzyCompare(1 - frame.y1 / frame.height, 3.4 / 210, 0.001);
            // Another tray has its own figures. (Plain A4 from the cassette can go borderless, and
            // that turns itself on until the user chooses; asked for margins, the edge is back.)
            p.source = "cassette";
            verify(p.borderless && sheet.edge === null);
            p.borderlessTouched = true;
            p.borderless = false;
            // (the sheet is still on its side: the portrait top margin, 5 mm, is its left edge)
            compare(sheet.edge.left, 5);
            compare(sheet.edge.top, 3.4);
        }

        function test_the_document_preview_marks_the_unprintable_edge_too() {
            boot(false);
            loader.sourceComponent = docComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            const p = loader.item;
            p.setFiles(["/users/me/a.pdf"]);
            tryVerify(() => p.edge !== null);
            compare(p.edge.top, 3, "plain A4 from the front tray");
            verify(findChild(p, "edgeHint").visible);
            // Documents have no borderless setting, so the edge is always there.
            p.sizeKey = "a4";
            p.mediaType = "stationery";
            verify(p.edge !== null);
        }

        function test_defaults_from_the_printer() {
            const p = photoPage(["/p/beach.jpg"]);
            compare(p.sizeKey, "l", "photos default to an L print");
            compare(p.mediaType, "com.canon-mtglossy");
            compare(p.borderless, true, "borderless is on when the printer has it");
            compare(p.orientation, "landscape", "the photo is wider than tall");
            compare(p.quality, "normal");
            compare(p.copies, 1);
            verify(p.canPrint);
            compare(p.blockedReason, "");
        }

        function test_media_follows_the_size() {
            const p = photoPage(["/p/beach.jpg"]);
            p.mediaType = "com.canon-mtluster";
            p.sizeKey = "card";
            compare(p.mediaType, "com.canon-mtglossy", "luster has no card entry; the preferred photo paper is taken");
            compare(p.borderlessOk, false, "cards have margins only");
            compare(p.borderless, false);
            p.sizeKey = "l";
            compare(p.borderlessOk, true);
            p.mediaType = "stationery";
            compare(p.borderlessOk, false, "plain paper in L has no borderless entry");
            p.layout = 4;
            compare(p.borderless, false, "n photos per sheet are not borderless");
        }

        function test_a_cassette_offers_only_plain_paper() {
            const p = photoPage(["/p/beach.jpg"]);
            p.source = "cassette";
            compare(p.typeList.join(","), "stationery");
            compare(p.mediaType, "stationery");
            compare(p.sizeList.map(z => z.key).join(","), "a4,a5,b5");
            compare(p.sizeKey, "a4");
        }

        function test_nothing_is_sent_before_confirming() {
            const p = photoPage(["/p/beach.jpg"]);
            p.askToPrint();
            wait(150);
            compare(printCalls().length, 0);
            const confirm = findChild(p, "printConfirm");
            verify(confirm.visible, "the confirmation is asked");
            verify(confirm.text.indexOf("Glossy Photo Paper") >= 0, confirm.text);
            verify(confirm.text.indexOf("borderless") >= 0, confirm.text);
            confirm.close();
            wait(100);
            compare(printCalls().length, 0);
        }

        function test_print_sends_confirm_and_follows_the_job() {
            const p = photoPage(["/p/beach.jpg"]);
            p.copies = 2;
            p.startPrinting();
            tryVerify(() => printCalls().length === 1);
            const c = printCalls()[0];
            compare(c.method, "PrintFile");
            compare(c.args[0], "printer.test");
            compare(c.args[1], "/p/beach.jpg");
            const o = JSON.parse(c.args[2]);
            compare(o.confirm, true);
            compare(o.size, "l");
            compare(o.media_type, "com.canon-mtglossy");
            compare(o.copies, 2);
            compare(o.borderless, true);
            compare(o.orientation, "landscape");
            const job = findChild(p, "printJob");
            tryVerify(() => job.jobId === "print-1");
            fake.busSignal("PrintProgress", "print-1", "sending", "Sending 1234 bytes.");
            verify(job.progress.indexOf("Sending the job") >= 0, job.progress);
            fake.busSignal("PrintProgress", "print-9", "state", "someone else");
            verify(job.progress.indexOf("Sending the job") >= 0, "another job's progress is ignored");
            fake.busSignal("PrintFinished", "print-1", "true", "beach.jpg: completed");
            compare(job.stage, "done");
            compare(job.ok, true);
            job.close();
        }

        function test_several_photos_use_printfiles_and_layout() {
            const p = photoPage(["/p/a.jpg", "/p/b.jpg", "/p/c.jpg"]);
            p.layout = 2;
            compare(p.sheetCount, 2);
            p.startPrinting();
            tryVerify(() => fake.called("PrintFiles").length === 1);
            const c = fake.called("PrintFiles")[0];
            compare(JSON.parse(c.args[1]).join(","), "/p/a.jpg,/p/b.jpg,/p/c.jpg");
            compare(JSON.parse(c.args[2]).layout, 2);
            findChild(p, "printJob").close();
        }

        function test_a_failed_job_is_explained_and_can_be_cancelled() {
            const p = photoPage(["/p/beach.jpg"]);
            p.startPrinting();
            const job = findChild(p, "printJob");
            tryVerify(() => job.jobId === "print-1");
            job.cancel();
            tryVerify(() => fake.called("CancelJob").length === 1);
            compare(fake.called("CancelJob")[0].args[0], "print-1");
            fake.busSignal("PrintFinished", "print-1", "false", "job print-1 was cancelled");
            compare(job.stage, "done");
            compare(job.ok, false);
            compare(job.result, "The job was stopped.");
            job.close();
        }

        function test_read_only_and_busy_block_printing() {
            boot(true);
            loader.sourceComponent = photoComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            loader.item.setFiles(["/p/beach.jpg"]);
            verify(!loader.item.canPrint);
            verify(loader.item.blockedReason.indexOf("Read-only") >= 0, loader.item.blockedReason);
            loader.item.startPrinting();
            wait(150);
            compare(printCalls().length, 0, "the Store refuses before the bus");
            loader.sourceComponent = null;

            boot(false);
            const snap = JSON.parse(JSON.stringify(fake.snapshot));
            snap.activity = "maintenance: nozzle check";
            fake.overrides = ({ GetSnapshot: [true, JSON.stringify(snap)] });
            Store.loadSnapshot();
            tryVerify(() => Store.busy);
            loader.sourceComponent = photoComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            loader.item.setFiles(["/p/beach.jpg"]);
            verify(!loader.item.canPrint);
        }

        function test_a_sleeping_printer_can_be_printed_to() {
            boot(false, "sleeping");
            loader.sourceComponent = photoComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            loader.item.setFiles(["/p/beach.jpg"]);
            verify(loader.item.canPrint, "a job wakes the printer");
        }

        function test_the_preview_asks_the_host() {
            const p = photoPage(["/p/beach.jpg"]);
            p.layout = 2;
            p.orientation = "portrait";
            tryVerify(() => imaging.called("photoPreview").length > 0 && imaging.called("photoPreview").slice(-1)[0].arg.layout === 2);
            const last = imaging.called("photoPreview").slice(-1)[0].arg;
            compare(last.layout, 2);
            compare(last.orientation, "portrait");
            compare(last.size, "l");
            tryVerify(() => p.sheetPath === "/tmp/fake-sheet.png");
        }

        function test_document_page_order_is_offered_sent_and_shown_in_the_preview() {
            boot(false);
            loader.sourceComponent = docComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            const p = loader.item;
            verify(findChild(p, "pageOrderRow").visible);
            const combo = findChild(p, "pageOrderCombo");
            compare(combo.count, 2);
            compare(p.pageOrder, "normal");
            verify(!("page_order" in PrintModel.buildOptions(p.settingsObject())));
            combo.currentIndex = 1;
            combo.activated(1);
            compare(p.pageOrder, "reverse");
            compare(combo.currentValue, "reverse");
            compare(PrintModel.buildOptions(p.settingsObject()).page_order, "reverse");
            // The preview walks the sheets in the order they are printed (the pages are known from
            // the range here: there is no document to read).
            p.allPages = false;
            p.pagesText = "1-5";
            compare(p.sheetCount, 5);
            compare(p.sheetOrder.join(","), "4,3,2,1,0");
            p.layout = 2;
            compare(p.sheetOrder.join(","), "2,1,0", "pages keep their place on a sheet; the sheets turn round");
            p.layout = 1;
            p.duplex = "long";
            compare(p.sheetOrder.join(","), "4,2,3,0,1", "a two-sided sheet keeps its front and back");
            p.pageOrder = "normal";
            compare(p.sheetOrder.join(","), "0,1,2,3,4");
            p.pageOrder = "reverse";
            // It goes to the service when printing; the confirmation sentence says so.
            p.files = ["/d/doc.pdf"];
            tryVerify(() => p.canPrint);
            verify(PrintModel.describe(p.settingsObject(), "doc.pdf").indexOf("last page first") > 0);
            p.startPrinting();
            tryVerify(() => printCalls().length === 1);
            const o = JSON.parse(printCalls()[0].args[2]);
            compare(o.page_order, "reverse");
            compare(o.duplex, "long");
            loader.sourceComponent = null;
        }

        function test_photos_have_no_page_order() {
            const p = photoPage(["/p/beach.jpg"]);
            verify(!findChild(p, "pageOrderRow").visible);
            p.pageOrder = "reverse";
            verify(!("page_order" in PrintModel.buildOptions(p.settingsObject())), "never sent for photos");
        }

        function test_document_page_options() {
            boot(false);
            loader.sourceComponent = docComp;
            tryVerify(() => loader.item !== null);
            tryVerify(() => Store.printCaps !== null);
            const p = loader.item;
            compare(p.sizeKey, "a4");
            compare(p.mediaType, "stationery");
            compare(p.borderlessOk, false);
            // A page range needs the page count; the loaded PDF supplies it. Without a file the
            // syntax is still checked.
            p.allPages = false;
            p.pagesText = "1-6, 3, 12-14";
            verify(p.pageCheck.ok);
            p.pagesText = "3-1";
            verify(!p.pageCheck.ok);
            compare(p.pageCheck.error, "backwards");
            p.pagesText = "2";
            p.duplex = "short";
            p.layout = 4;
            p.files = ["/d/doc.pdf"];
            const o = PrintModel.buildOptions(p.settingsObject());
            compare(o.pages, "2");
            compare(o.duplex, "short");
            compare(o.layout, 4);
            compare(o.job_name, "doc.pdf");
            verify(!("borderless" in o));
            loader.sourceComponent = null;
        }
    }
}
