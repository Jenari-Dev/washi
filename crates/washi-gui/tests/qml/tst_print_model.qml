import QtQuick
import QtTest 1.2

// Photo Print / Document Print choices: media names, what the printer offers per size, the
// settings -> PrintFile options mapping, page ranges, n-up grids.
Item {
    width: 200
    height: 200

    FakeTransport {
        id: fake
    }

    TestCase {
        name: "PrintModel"
        when: windowShown

        readonly property var caps: fake.printCaps

        function test_media_names() {
            compare(PrintModel.mediaLabel("stationery"), "Plain paper");
            compare(PrintModel.mediaLabel("com.canon-mtgr"), "Photo Paper Plus Glossy II");
            compare(PrintModel.mediaLabel("com.canon-mtglossy"), "Glossy Photo Paper");
            compare(PrintModel.mediaLabel("com.canon-mtluster"), "Photo Paper Pro Luster");
            compare(PrintModel.mediaLabel("com.canon-mtmat"), "Matte Photo Paper");
            compare(PrintModel.mediaLabel("com.canon-mtunknown"), "Canon paper (unknown)");
            // Postcards: the two faces are told apart; nothing is claimed about the finish.
            compare(PrintModel.mediaLabel("com.canon-mthagaki"), "Postcard (Hagaki)");
            compare(PrintModel.mediaLabel("com.canon-mthagakia"), "Postcard (Hagaki), address side");
            compare(PrintModel.mediaLabel("com.canon-mthagakiglossy"), "Photo postcard (Hagaki)");
            compare(PrintModel.mediaLabel("com.canon-mtfineart"), "Fine art paper (other brands)");
            compare(PrintModel.mediaLabel("some-new_type"), "some new type");
            verify(PrintModel.isPhotoMedia("com.canon-mtglossy"));
            verify(!PrintModel.isPhotoMedia("stationery"));
            verify(!PrintModel.isPhotoMedia("envelope"));
        }

        function test_what_the_printer_offers() {
            const sizes = PrintModel.sizesFor(caps, "auto", "").map(z => z.key);
            compare(sizes.join(","), "a4,l,card");
            compare(PrintModel.sizesFor(caps, "cassette", "").map(z => z.key).join(","), "a4,a5,b5");
            compare(PrintModel.sizesFor(caps, "auto", "com.canon-mtluster").map(z => z.key).join(","), "a4,l");
            // Photo papers first on the photo page, the printer's order otherwise.
            compare(PrintModel.typesFor(caps, "auto", "l", true).join(","), "photographic,com.canon-mtglossy,com.canon-mtluster,stationery,envelope");
            compare(PrintModel.typesFor(caps, "auto", "l", false).join(","), "stationery,photographic,envelope,com.canon-mtglossy,com.canon-mtluster");
            compare(PrintModel.typesFor(caps, "cassette", "a4", false).join(","), "stationery");
            compare(PrintModel.defaultMedia(PrintModel.typesFor(caps, "auto", "l", true), true), "com.canon-mtglossy");
            compare(PrintModel.defaultMedia(PrintModel.typesFor(caps, "auto", "a4", false), false), "stationery");
            // Photo Paper Plus Glossy II is preferred when the printer lists it.
            compare(PrintModel.defaultMedia(["photographic", "com.canon-mtglossy", "com.canon-mtgr"], true), "com.canon-mtgr");
            compare(PrintModel.defaultMedia(["photographic", "com.canon-mtluster"], true), "photographic");
        }

        function test_the_unprintable_edge_follows_the_media_database_and_the_turn_of_the_sheet() {
            // Plain A4 from the front tray: [top, bottom, left, right] = 3, 12, 3.4, 5.6 mm.
            const portrait = PrintModel.unprintable(caps, "auto", "stationery", "a4", false);
            compare(portrait.top, 3);
            compare(portrait.bottom, 12);
            compare(portrait.left, 3.4);
            compare(portrait.right, 5.6);
            // A landscape sheet is the portrait one turned a quarter anticlockwise: the portrait
            // sheet's top edge is the left one, its left edge the bottom one, and so on.
            const land = PrintModel.unprintable(caps, "auto", "stationery", "a4", true);
            compare(land.left, 3);
            compare(land.bottom, 3.4);
            compare(land.right, 12);
            compare(land.top, 5.6);
            // The cassette is the "main" source of the database.
            compare(PrintModel.unprintable(caps, "cassette", "stationery", "a4", false).top, 5);
            // Nothing to show without an entry, without the figures, or when all are zero.
            verify(PrintModel.unprintable(caps, "auto", "stationery", "a5", false) === null);
            verify(PrintModel.unprintable(caps, "auto", "no-such-type", "a4", false) === null);
            verify(PrintModel.unprintable(null, "auto", "stationery", "a4", false) === null);
            verify(PrintModel.unprintable({ media: {} }, "auto", "stationery", "a4", false) === null, "an older service sends no margins");
            verify(PrintModel.unprintable({ margins: { auto: { x: { a4: [0, 0, 0, 0] } } } }, "auto", "x", "a4", false) === null);
            verify(PrintModel.unprintable({ margins: { auto: { x: { a4: [1, "?", 1, 1] } } } }, "auto", "x", "a4", false) === null);
            // The sheet as drawn, in mm.
            const a4 = caps.sizes.find(z => z.key === "a4");
            compare(PrintModel.sheetMm(a4, "portrait").w, 210);
            compare(PrintModel.sheetMm(a4, "landscape").w, 297);
            compare(PrintModel.sheetMm(a4, "landscape").h, 210);
            compare(PrintModel.sheetMm(null, "portrait").h, 297);
        }

        function test_borderless_follows_the_media_database() {
            verify(PrintModel.borderlessAvailable(caps, "auto", "com.canon-mtglossy", "l"));
            verify(!PrintModel.borderlessAvailable(caps, "auto", "com.canon-mtglossy", "card"), "card has margins only");
            verify(!PrintModel.borderlessAvailable(caps, "auto", "stationery", "l"));
            verify(PrintModel.borderlessAvailable(caps, "cassette", "stationery", "a4"), "plain A4 borderless from the cassette");
            verify(!PrintModel.borderlessAvailable(null, "auto", "stationery", "a4"));
            compare(PrintModel.flags(caps, "auto", "com.canon-mtglossy", "a4"), 3);
        }

        function test_settings_become_print_options() {
            const o = PrintModel.buildOptions({
                size: "l", mediaType: "com.canon-mtglossy", source: "rear", borderless: true, quality: "high",
                color: false, copies: 3, pagesText: "", layout: 1, duplex: "", orientation: "landscape", jobName: "beach.jpg"
            });
            compare(JSON.stringify(o), JSON.stringify({
                size: "l", media_type: "com.canon-mtglossy", source: "rear", quality: "high", color: false,
                copies: 3, layout: 1, orientation: "landscape", borderless: true, job_name: "beach.jpg"
            }));
            // A document: pages, duplex, layout; whitespace in the range goes; no borderless with n-up.
            const d = PrintModel.buildOptions({
                size: "a4", mediaType: "stationery", source: "cassette", borderless: true, quality: "normal",
                color: true, copies: 1, pagesText: " 1-6, 3 , 12-14", layout: 4, duplex: "long", orientation: "portrait"
            });
            compare(d.pages, "1-6,3,12-14");
            compare(d.layout, 4);
            compare(d.duplex, "long");
            compare(d.source, "cassette");
            verify(!("borderless" in d), "borderless needs one per sheet");
            verify(!("job_name" in d));
            // The defaults the service would apply anyway.
            const m = PrintModel.buildOptions({ size: "a4", mediaType: "stationery" });
            compare(m.source, "auto");
            compare(m.quality, "normal");
            compare(m.color, true);
            compare(m.copies, 1);
            compare(m.orientation, "auto");
        }

        function test_source_names() {
            compare(PrintModel.sourceKey("cassette"), "main");
            compare(PrintModel.sourceKey("rear"), "rear");
            compare(PrintModel.sourceOptions(caps).map(o => o.value).join(","), "auto,rear,cassette");
            compare(PrintModel.sourceOptions({ sources: ["auto", "rear"] }).map(o => o.value).join(","), "auto,rear");
        }

        function test_page_ranges() {
            compare(PrintModel.parsePages("1-6, 3, 12-14", 14).pages.join(","), "1,2,3,4,5,6,12,13,14");
            compare(PrintModel.parsePages("8-", 10).pages.join(","), "8,9,10");
            compare(PrintModel.parsePages("-2", 10).pages.join(","), "1,2");
            compare(PrintModel.parsePages("5,3,5", 5).pages.join(","), "3,5");
            compare(PrintModel.parsePages("2", 0).pages.join(","), "2", "an unknown page count checks the syntax only");
            compare(PrintModel.parsePages("", 5).error, "empty");
            compare(PrintModel.parsePages("a", 5).error, "syntax");
            compare(PrintModel.parsePages("3-1", 5).error, "backwards");
            compare(PrintModel.parsePages("2-9", 5).error, "range");
            compare(PrintModel.parsePages("0", 5).error, "syntax");
            compare(PrintModel.parsePages("1--2", 5).error, "syntax");
            verify(PrintModel.pagesError("range").indexOf("does not have") >= 0);
        }

        function test_grids_follow_the_core() {
            compare(JSON.stringify(PrintModel.grid(2, true)), JSON.stringify({ cols: 1, rows: 2 }));
            compare(JSON.stringify(PrintModel.grid(2, false)), JSON.stringify({ cols: 2, rows: 1 }));
            compare(JSON.stringify(PrintModel.grid(6, true)), JSON.stringify({ cols: 2, rows: 3 }));
            compare(JSON.stringify(PrintModel.grid(6, false)), JSON.stringify({ cols: 3, rows: 2 }));
            for (const n of PrintModel.layouts) {
                const g = PrintModel.grid(n, true);
                compare(g.cols * g.rows, n);
            }
        }

        function test_file_kinds() {
            compare(PrintModel.kindOf("/a/b.JPG"), "photo");
            compare(PrintModel.kindOf("/a/b.png"), "photo");
            compare(PrintModel.kindOf("/a/b.bmp"), "photo");
            compare(PrintModel.kindOf("/a/b.pdf"), "pdf");
            compare(PrintModel.kindOf("/a/b.docx"), "office");
            compare(PrintModel.kindOf("/a/b.gif"), "other");
            compare(PrintModel.documentFilters(false).length, 1);
            verify(PrintModel.documentFilters(true).length > 1);
        }

        function test_description() {
            const t = PrintModel.describe({ copies: 2, mediaType: "com.canon-mtglossy", size: "l", sizeLabel: "L photo", borderless: true, color: true, layout: 1 }, "beach.jpg");
            compare(t, "2 copies of beach.jpg on Glossy Photo Paper (L photo), borderless");
        }
    }
}
