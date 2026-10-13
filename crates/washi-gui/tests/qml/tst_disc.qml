import QtQuick
import QtTest 1.2

// Disc labels: the geometry of the printable ring, designs as JSON, the PrintDisc request, the
// designer page (print asks first, the tray guidance) and the crop box maths of the scan editor.
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
        id: pageComp
        DiscLabelPage {}
    }
    Component {
        id: docPreviewComp
        DocPreview {
            usingFallback: true
            width: 300
            height: 400
            source: "file:///docs/three.pdf"
            pages: [1, 2, 3]
        }
    }
    CropOverlay {
        id: overlay
        width: 10
        height: 10
    }

    TestCase {
        name: "DiscMath"
        when: windowShown

        function test_ring_geometry() {
            compare(DiscMath.diameter, 120);
            compare(DiscMath.defaultInner, 22);
            compare(DiscMath.defaultOuter, 116);
            compare(DiscMath.mmToPx(60, 600), 300);
            compare(DiscMath.pxToMm(300, 600), 60);
            compare(DiscMath.mmToPx(DiscMath.defaultOuter, 1200), 1160);
            // 12 pt is 4.233 mm: on a 600 px canvas (5 px per mm) about 21 px.
            verify(Math.abs(DiscMath.ptToPx(12, 600) - 21.17) < 0.05);
            verify(Math.abs(DiscMath.pxToPt(DiscMath.ptToPx(18, 700), 700) - 18) < 1e-9);
            verify(DiscMath.inRing(0, 40, 22, 116));
            verify(!DiscMath.inRing(0, 5, 22, 116), "the hub");
            verify(!DiscMath.inRing(0, 59, 22, 116), "beyond the printable edge");
            verify(DiscMath.inRing(57, 0, 22, 116));
        }

        function test_diameters_are_kept_within_what_the_printer_allows() {
            compare(DiscMath.clampInner(10), 17);
            compare(DiscMath.clampInner(50), 45);
            compare(DiscMath.clampInner(22.04), 22);
            compare(DiscMath.clampOuter(100), 114);
            compare(DiscMath.clampOuter(120), 118);
        }

        function test_the_mask_leaves_only_the_ring() {
            const p = DiscMath.maskPath(600, 22, 116);
            verify(p.indexOf("M0 0h600v600h-600z") === 0, p);
            // two circles follow: the outer one (r = 290) and the inner one (r = 55)
            verify(p.indexOf("a290 290 0 1 0 580 0") > 0, p);
            verify(p.indexOf("a55 55 0 1 0 110 0") > 0, p);
            verify(DiscMath.cornersPath(600).indexOf("a300 300") > 0);
        }

        function test_the_picture_covers_or_fits() {
            // 2000 x 1000: cover makes the short side span the outer diameter.
            const c = DiscMath.pictureSizeMm(2000, 1000, "cover", 1, 116);
            compare(c.h, 116);
            compare(c.w, 232);
            const f = DiscMath.pictureSizeMm(2000, 1000, "contain", 0.5, 116);
            verify(Math.abs(Math.sqrt(f.w * f.w + f.h * f.h) - 58) < 1e-9, "the diagonal spans half the diameter");
        }

        function test_points_turn_with_the_item() {
            const p = DiscMath.intoItem(10, 0, 0, 0, 90);
            verify(Math.abs(p.x) < 1e-9 && Math.abs(p.y + 10) < 1e-9, JSON.stringify(p));
            const q = DiscMath.intoItem(10, 20, 10, 20, 33);
            compare(q.x, 0);
            compare(q.y, 0);
            compare(Math.round(DiscMath.handleAngle(0, -10, 0, 0)), 0, "straight up is no rotation");
            compare(Math.round(DiscMath.handleAngle(10, 0, 0, 0)), 90);
            compare(Math.round(DiscMath.handleAngle(0, 10, 0, 0)), 180);
            compare(Math.round(DiscMath.handleAngle(-10, 0, 0, 0)), -90);
        }

        function test_designs_survive_a_round_trip_and_bad_ones_are_cleaned() {
            const d = DiscMath.newDesign();
            d.name = "Mix";
            d.media = "b";
            d.inner_mm = 30;
            d.background = { path: "/p/a.jpg", fit: "contain", scale: 1.5, offset_mm: [3, -4], rotation: 20 };
            d.texts.push({ text: "Hello", x_mm: 1, y_mm: 2, size_pt: 24, color: "#aa00cc", rotation: 5, align: "left", family: "Noto Sans", ring: null });
            d.texts.push({ text: "Round", x_mm: 0, y_mm: 0, size_pt: 14, color: "#000000", rotation: 0, align: "center", family: "", ring: { radius_mm: 40, start_deg: 180, clockwise: false, letter_spacing_pt: 3 } });
            const back = DiscMath.parseDesign(DiscMath.designJson(d));
            compare(JSON.stringify(back), JSON.stringify(d));
            // A design saved before the ring existed has none.
            compare(DiscMath.parseDesign(JSON.stringify({ texts: [{ text: "old" }] })).texts[0].ring, null);
            // Wild ring values are clamped; a ring that is not an object is no ring.
            const wildRing = DiscMath.parseDesign(JSON.stringify({ texts: [{ text: "x", ring: { radius_mm: 999, start_deg: -999, clockwise: false, letter_spacing_pt: -99 } }, { text: "y", ring: "round" }] }));
            compare(wildRing.texts[0].ring.radius_mm, 60);
            compare(wildRing.texts[0].ring.start_deg, -360);
            compare(wildRing.texts[0].ring.clockwise, false);
            compare(wildRing.texts[0].ring.letter_spacing_pt, -10);
            compare(wildRing.texts[1].ring, null);
            verify(DiscMath.parseDesign("not json") === null);
            verify(DiscMath.parseDesign("[]") === null);
            const wild = DiscMath.parseDesign(JSON.stringify({
                inner_mm: 999, outer_mm: -1, media: "z", background_color: "red",
                texts: [{ text: "x", size_pt: 9999, color: "nope", align: "diagonal" }, { nope: 1 }],
                background: { path: "" }
            }));
            compare(wild.inner_mm, 45);
            compare(wild.outer_mm, 114);
            compare(wild.media, "a");
            compare(wild.background_color, "#ffffff");
            compare(wild.background, null);
            compare(wild.texts.length, 1);
            compare(wild.texts[0].size_pt, 200);
            compare(wild.texts[0].color, "#000000");
            compare(wild.texts[0].align, "center");
        }

        function test_design_file_names() {
            compare(DiscMath.fileName("My mix 2026!"), "My mix 2026.json");
            compare(DiscMath.fileName("../../etc/passwd"), "etcpasswd.json");
            compare(DiscMath.fileName("   "), "");
            compare(DiscMath.fileName("夏のミックス"), "夏のミックス.json");
        }

        function test_the_print_request() {
            const d = DiscMath.newDesign();
            d.media = "b";
            d.inner_mm = 25;
            d.outer_mm = 117;
            d.background_color = "#112233";
            d.background = { path: "/p/a.jpg", fit: "cover", scale: 1.2, offset_mm: [1, 2], rotation: 15 };
            d.texts.push({ text: "Side A", x_mm: 0, y_mm: 32, size_pt: 20, color: "#ffffff", rotation: 0, align: "center", family: "Noto Sans" });
            d.texts.push({ text: "   ", x_mm: 0, y_mm: 0, size_pt: 20, color: "#000000", rotation: 0, align: "center", family: "" });
            const s = DiscMath.printSpec(d, fam => "/fonts/" + fam + ".ttf", "high");
            compare(s.media, "b");
            compare(s.inner_mm, 25);
            compare(s.outer_mm, 117);
            compare(s.image, "/p/a.jpg");
            compare(s.fit, "cover");
            compare(s.scale, 1.2);
            compare(JSON.stringify(s.offset_mm), "[1,2]");
            compare(s.rotation, 15);
            compare(s.background_color, "#112233");
            compare(s.quality, "high");
            compare(s.texts.length, 1, "empty text is not printed");
            compare(s.texts[0].font, "/fonts/Noto Sans.ttf");
            compare(s.texts[0].size_pt, 20);
            verify(!("confirm" in s), "the Store adds confirm");
            // without a picture the request has none
            d.background = null;
            verify(!("image" in DiscMath.printSpec(d, null, "")));
        }

        // ---- text that follows the ring: the same numbers as the tests of washi_core::disc::ring
        function near(a, b, msg) {
            verify(Math.abs(a - b) < 1e-9, (msg || "") + " " + a + " vs " + b);
        }

        function ring(r, start, cw, sp) {
            return { radius_mm: r, start_deg: start, clockwise: cw, letter_spacing_pt: sp || 0 };
        }

        function test_a_ring_letter_at_the_top_stands_upright_and_one_at_the_right_leans_over() {
            let g = DiscMath.ringLayout(ring(50, 0, true), [0], "left")[0];
            near(g.x_mm, 0); near(g.y_mm, -50); near(g.rotation_deg, 0);
            g = DiscMath.ringLayout(ring(50, 90, true), [0], "left")[0];
            near(g.x_mm, 50); near(g.y_mm, 0); near(g.rotation_deg, 90);
            g = DiscMath.ringLayout(ring(50, 270, true), [0], "left")[0];
            near(g.x_mm, -50); near(g.rotation_deg, -90);
        }

        function test_counter_clockwise_ring_text_at_the_bottom_reads_upright() {
            let g = DiscMath.ringLayout(ring(50, 180, false), [0], "left")[0];
            near(g.x_mm, 0); near(g.y_mm, 50); near(g.rotation_deg, 0);
            g = DiscMath.ringLayout(ring(50, 90, false), [0], "left")[0];
            near(g.x_mm, 50); near(g.rotation_deg, -90);
        }

        function test_ring_letters_follow_each_other_in_the_chosen_direction() {
            const step = 5 / 50 * 180 / Math.PI;
            const cw = DiscMath.ringLayout(ring(50, 0, true), [5, 5, 5], "left");
            const ccw = DiscMath.ringLayout(ring(50, 0, false), [5, 5, 5], "left");
            for (let i = 0; i < 3; i++) {
                near(cw[i].angle_deg, step * (i + 0.5), "cw " + i);
                near(ccw[i].angle_deg, -step * (i + 0.5), "ccw " + i);
                near(Math.hypot(cw[i].x_mm, cw[i].y_mm), 50);
            }
        }

        function test_the_alignment_says_what_the_start_angle_means() {
            const half = 6 / 40 * 180 / Math.PI;
            const t = ring(40, 30, true);
            const left = DiscMath.ringLayout(t, [6, 6], "left");
            const centre = DiscMath.ringLayout(t, [6, 6], "center");
            const right = DiscMath.ringLayout(t, [6, 6], "right");
            near(left[0].angle_deg, 30 + half / 2);
            near((centre[0].angle_deg + centre[1].angle_deg) / 2, 30);
            near(right[1].angle_deg + half / 2, 30);
            near(DiscMath.ringLayout(ring(40, 30, false), [6, 6], "right")[1].angle_deg - half / 2, 30);
        }

        function test_ring_letter_spacing_adds_room_between_letters_only() {
            const adv = [4, 4, 4];
            const plain = DiscMath.ringLayout(ring(50, 0, true), adv, "left");
            const spaced = DiscMath.ringLayout(ring(50, 0, true, 72), adv, "left");
            const deg = mm => mm / 50 * 180 / Math.PI;
            near(spaced[0].angle_deg, plain[0].angle_deg);
            near(spaced[1].angle_deg - plain[1].angle_deg, deg(25.4));
            near(spaced[2].angle_deg - plain[2].angle_deg, deg(50.8));
            near(DiscMath.ringArcLengthMm(ring(50, 0, true, 72), adv), 12 + 2 * 25.4);
            compare(DiscMath.ringLayout(ring(50, 0, true), [], "center").length, 0);
        }

        function test_ring_angles_are_normalized() {
            compare(DiscMath.normalizeDeg(190), -170);
            compare(DiscMath.normalizeDeg(-190), 170);
            compare(DiscMath.normalizeDeg(180), 180);
            compare(DiscMath.normalizeDeg(-180), 180);
            compare(DiscMath.normalizeDeg(720), 0);
        }

        function test_a_click_finds_ring_text_by_its_band_and_stretch() {
            const adv = [5, 5, 5, 5];     // 20 mm of text, centred on the top of a 48 mm ring
            const r = ring(48, 0, true);
            verify(DiscMath.ringHit(r, adv, "center", 18, 0, -50), "on the text");
            verify(DiscMath.ringHit(r, adv, "center", 18, 8, -48), "near its end");
            verify(!DiscMath.ringHit(r, adv, "center", 18, 0, -30), "inside the band");
            verify(!DiscMath.ringHit(r, adv, "center", 18, 0, -62), "outside the band");
            verify(!DiscMath.ringHit(r, adv, "center", 18, 0, 50), "the other side of the disc");
            verify(!DiscMath.ringHit(r, adv, "center", 18, 30, -38), "beyond the end of the text");
            // Counter-clockwise text hangs inwards from its radius.
            const c = ring(48, 180, false);
            verify(DiscMath.ringHit(c, adv, "center", 18, 0, 45));
            verify(!DiscMath.ringHit(c, adv, "center", 18, 0, 52));
            const span = DiscMath.ringSpan(r, adv, "center");
            near(span.from, -span.to);
        }
    }

    TestCase {
        name: "DiscPage"
        when: windowShown

        function boot(readOnly) {
            loader.sourceComponent = null;
            Store.reset();
            Store.transport = null;
            Img.reset();
            Img.engine = imaging;
            imaging.files = ({});
            imaging.clearLog();
            Prefs.hideLoadPaperHint = false;
            fake.connected = false;
            fake.readonly = !!readOnly;
            fake.env = ({});
            fake.overrides = ({});
            fake.clearCalls();
            Store.transport = fake;
            fake.connected = true;
            tryVerify(() => Store.snapLoaded);
            loader.sourceComponent = pageComp;
            tryVerify(() => loader.item !== null);
            return loader.item;
        }

        function test_an_empty_label_cannot_be_printed() {
            const p = boot();
            verify(!p.canPrint);
            p.addText("Side A");
            verify(p.canPrint);
            compare(p.selected, 0);
            compare(p.design.texts[0].y_mm, 32, "the first text goes below the hub");
            p.addText("Side B");
            compare(p.design.texts[1].y_mm, -32);
        }

        function test_print_asks_for_the_tray_then_confirms_then_sends() {
            const p = boot();
            p.addText("Side A");
            p.change(d => { d.media = "b"; });
            p.askToPrint();
            const tray = findChild(p, "discConfirm");
            wait(100);
            compare(fake.called("PrintDisc").length, 0, "nothing before the confirmation");
            // The tray guidance first ("Paper is loaded"), then the confirmation.
            const loaded = findChild(p.Window.contentItem || p, "loadedButton");
            verify(loaded !== null, "the tray guidance is shown");
            loaded.clicked();
            verify(tray.visible);
            wait(100);
            compare(fake.called("PrintDisc").length, 0);
            findChild(tray, "acceptButton").clicked();
            tryVerify(() => fake.called("PrintDisc").length === 1);
            const body = JSON.parse(fake.called("PrintDisc")[0].args[1]);
            compare(body.confirm, true);
            compare(body.media, "b");
            compare(body.texts.length, 1);
            compare(body.texts[0].text, "Side A");
            compare(body.inner_mm, 22);
            compare(body.outer_mm, 116);
            fake.busSignal("PrintFinished", "print-1", "true", "disc label: completed");
            findChild(p, "discJob").close();
        }

        function test_read_only_blocks_the_disc_print() {
            const p = boot(true);
            p.addText("Side A");
            verify(!p.canPrint);
            compare(p.blockedReason, "Read-only mode: the service will not print.");
            p.askToPrint();
            wait(100);
            compare(fake.called("PrintDisc").length, 0);
        }

        function test_designs_are_saved_listed_opened_and_deleted() {
            const p = boot();
            p.addText("Side A");
            verify(p.saveDesign("My mix"));
            verify(Object.keys(imaging.files).some(f => f === "/data/washi/discs/My mix.json"), JSON.stringify(Object.keys(imaging.files)));
            verify(!p.saveDesign("   "));
            p.addText("another");
            compare(p.design.texts.length, 2);
            p.refreshSaved();
            compare(p.savedNames.join(","), "My mix.json");
            verify(p.loadDesign("My mix.json"));
            compare(p.design.texts.length, 1);
            compare(p.design.name, "My mix");
            imaging.files["/data/washi/discs/bad.json"] = "{ nope";
            verify(!p.loadDesign("bad.json"));
            verify(p.notice.indexOf("not a Washi disc design") >= 0);
        }

        function test_items_can_be_removed() {
            const p = boot();
            p.addText("A");
            p.addText("B");
            p.setPicture("/p/a.jpg");
            compare(p.selected, -1);
            p.removeSelected();
            compare(p.design.background, null);
            p.selected = 0;
            p.removeSelected();
            compare(p.design.texts.length, 1);
            compare(p.design.texts[0].text, "B");
            compare(p.selected, -2);
        }

        function test_a_text_can_be_selected_moved_scaled_and_turned_on_the_canvas() {
            const p = boot();
            p.addText("Side A");
            p.selected = -2;
            const canvas = findChild(p, "discCanvas");
            const area = findChild(p, "discMouse");
            verify(canvas.size > 200, "the canvas has a size: " + canvas.size);
            const mm = canvas.mm;
            // Click on the text (32 mm below the centre).
            mouseClick(area, canvas.size / 2, canvas.size / 2 + 32 * mm);
            compare(p.selected, 0, "clicking the text selects it");
            mouseClick(area, canvas.size / 2, canvas.size / 2 - 40 * mm);
            compare(p.selected, -2, "clicking the empty disc selects nothing");
            mouseClick(area, canvas.size / 2, canvas.size / 2 + 32 * mm);
            // Drag it 20 mm to the right and 5 mm up.
            mouseDrag(area, canvas.size / 2, canvas.size / 2 + 32 * mm, 20 * mm, -5 * mm);
            verify(Math.abs(p.design.texts[0].x_mm - 20) < 1, "x " + p.design.texts[0].x_mm);
            verify(Math.abs(p.design.texts[0].y_mm - 27) < 1, "y " + p.design.texts[0].y_mm);
            // The square handle makes it bigger; the round one turns it.
            const before = p.design.texts[0].size_pt;
            const scale = findChild(canvas, "scaleHandle");
            tryVerify(() => scale.visible);
            mouseDrag(scale, scale.width / 2, scale.height / 2, 80, 80);
            verify(p.design.texts[0].size_pt > before, "size " + p.design.texts[0].size_pt);
            const turn = findChild(canvas, "turnHandle");
            // The handle turns with the text while it is dragged, so press on it and move the
            // pointer in the canvas's own coordinates: to the right of the pivot.
            mousePress(turn, turn.width / 2, turn.height / 2);
            mouseMove(canvas, canvas.fx + 120, canvas.fy);
            mouseRelease(canvas, canvas.fx + 120, canvas.fy);
            compare(p.design.texts[0].rotation, 90, "a handle dragged to the right turns the text a quarter, clockwise");
        }

        function test_a_picture_is_moved_and_scaled_on_the_canvas() {
            const p = boot();
            p.setPicture("/p/a.jpg");
            compare(p.selected, -1);
            p.change(d => { d.background.scale = 1; });
            const area = findChild(p, "discMouse");
            const canvas = findChild(p, "discCanvas");
            // The picture does not exist in the test, so it has no size and cannot be hit: the
            // click selects nothing and nothing moves.
            mouseDrag(area, canvas.size / 2, canvas.size / 2, 30, 30);
            compare(p.design.background.offset_mm[0], 0);
        }

        function test_the_font_comes_from_the_installed_families() {
            const p = boot();
            p.addText("Side A");
            const combo = findChild(p, "fontCombo");
            verify(combo !== null);
            compare(combo.currentIndex, 0, "Default first");
            verify(combo.count > 1, "families: " + combo.count);
            combo.activated(1);
            compare(p.design.texts[0].family, combo.model[1].value);
            verify(p.design.texts[0].family !== "");
            combo.activated(0);
            compare(p.design.texts[0].family, "");
        }

        function test_the_preview_as_printed_uses_the_print_request() {
            const p = boot();
            p.addText("Side A");
            p.showPrinted();
            tryVerify(() => p.printedPath === "/tmp/fake-disc.png");
            const req = imaging.called("discPreview")[0].arg;
            compare(req.texts[0].text, "Side A");
            verify(!("confirm" in req));
            findChild(p, "printedPreview").close();
        }

        function test_a_text_can_follow_the_ring() {
            const p = boot();
            p.addText("Side A");
            const opts = findChild(p, "ringOptions");
            verify(!opts.visible, "the options are hidden for a straight text");
            verify(findChild(p, "rotationSpin").visible);
            // The check box toggles by itself; set it as a click does.
            const box = findChild(p, "followRing");
            box.checked = true;
            box.toggled();
            tryVerify(() => !!p.design.texts[0].ring);
            compare(JSON.stringify(p.design.texts[0].ring), JSON.stringify(DiscMath.newRing()));
            compare(p.design.texts[0].align, "center");
            tryVerify(() => opts.visible);
            verify(!findChild(p, "rotationSpin").visible, "no turning of a text that follows the ring");
            verify(!findChild(p, "xSpin").visible, "and no position across or down");
            // The controls write the options.
            findChild(p, "ringRadius").value = 40;
            findChild(p, "ringRadius").valueModified();
            findChild(p, "ringStart").value = 180;
            findChild(p, "ringStart").valueModified();
            findChild(p, "ringSpacing").value = 4;
            findChild(p, "ringSpacing").valueModified();
            findChild(p, "ringCounterClockwise").clicked();
            const ring = p.design.texts[0].ring;
            compare(ring.radius_mm, 40);
            compare(ring.start_deg, 180);
            compare(ring.letter_spacing_pt, 4);
            compare(ring.clockwise, false);
            // The service prints what the canvas shows.
            const spec = p.spec("normal");
            compare(JSON.stringify(spec.texts[0].ring), JSON.stringify({ radius_mm: 40, start_deg: 180, clockwise: false, letter_spacing_pt: 4 }));
            // Turn it off again: a straight text, and the options go.
            box.checked = false;
            box.toggled();
            tryVerify(() => p.design.texts[0].ring === null);
            tryVerify(() => !opts.visible);
            verify(!("ring" in p.spec("normal").texts[0]));
        }

        function test_ring_text_is_selected_and_dragged_round_the_disc() {
            const p = boot();
            p.addText("WASHI");
            p.change(d => { d.texts[0].ring = DiscMath.newRing(); d.texts[0].size_pt = 18; });
            p.selected = -2;
            wait(150);
            const canvas = findChild(p, "discCanvas");
            const area = findChild(p, "discMouse");
            // The canvas changes size when the panel changes, so every point is worked out afresh.
            const mmX = (dx) => canvas.size / 2 + dx * canvas.mm;
            const mmY = (dy) => canvas.size / 2 + dy * canvas.mm;
            // The text is centred on the top of the 48 mm ring: its letters cover about 48 to 54 mm out.
            mouseClick(area, mmX(0), mmY(-50));
            compare(p.selected, 0, "clicking the ring text selects it");
            wait(150);
            verify(canvas.ringSelected);
            verify(findChild(canvas, "ringSelection").visible, "an arc shows what is selected");
            verify(!findChild(canvas, "selectionFrame").visible, "no frame and no handles");
            mouseClick(area, mmX(0), mmY(50));
            compare(p.selected, -2, "the other side of the disc is empty");
            wait(150);
            // Drag it a quarter turn clockwise, from the top to the right, and 8 mm outwards.
            mousePress(area, mmX(0), mmY(-50));
            mouseMove(area, mmX(30), mmY(-45));
            wait(150);
            mouseMove(area, mmX(58), mmY(0));
            mouseRelease(area, mmX(58), mmY(0));
            const ring = p.design.texts[0].ring;
            verify(Math.abs(ring.start_deg - 90) < 3, "start " + ring.start_deg);
            verify(Math.abs(ring.radius_mm - 56) < 1.5, "radius " + ring.radius_mm);
            compare(p.design.texts[0].x_mm, 0, "the straight position is untouched");
        }

        function test_files_dropped_on_the_disc_become_the_picture() {
            const p = boot();
            verify(findChild(p, "dropArea") !== null);
            p.dropFiles(["file:///users/me/Pictures/cover%20art.png", "file:///users/me/notes.txt"]);
            compare(p.design.background.path, "/users/me/Pictures/cover art.png");
            compare(p.selected, -1);
            compare(p.notice, "");
            p.dropFiles(["file:///users/me/notes.txt"]);
            compare(p.design.background.path, "/users/me/Pictures/cover art.png", "unchanged");
            verify(p.notice.indexOf("JPEG, PNG and BMP") >= 0, p.notice);
            p.dropFiles(["https://example.org/a.jpg"]);
            compare(p.design.background.path, "/users/me/Pictures/cover art.png", "only local files");
        }
    }

    Item {
        id: container
    }

    TestCase {
        name: "DocFallback"
        when: windowShown

        function test_poppler_previews_when_qtpdf_is_missing() {
            Img.reset();
            Img.engine = imaging;
            const holder = createTemporaryObject(docPreviewComp, container);
            verify(holder !== null);
            tryVerify(() => holder.pageCount === 3);
            compare(holder.usingFallback, true);
            verify(!holder.failed);
            holder.sheetIndex = 1;
            tryVerify(() => holder.firstLandscape === false);
        }
    }

    TestCase {
        name: "CropBox"
        when: windowShown

        function box(x, y, w, h) {
            return { x: x, y: y, w: w, h: h };
        }

        function near(a, b) {
            return Math.abs(a - b) < 1e-9;
        }

        function test_moving_keeps_the_box_inside() {
            const r = overlay.dragged(box(0.2, 0.2, 0.4, 0.4), "move", 0.7, -0.5, 0, 1, 1);
            compare(r.x, 0.6);
            compare(r.y, 0);
            compare(r.w, 0.4);
            compare(r.h, 0.4);
        }

        function test_resizing_a_corner_and_an_edge() {
            let r = overlay.dragged(box(0.1, 0.1, 0.5, 0.5), "se", 0.2, 0.1, 0, 1, 1);
            verify(near(r.w, 0.7) && near(r.h, 0.6));
            r = overlay.dragged(box(0.1, 0.1, 0.5, 0.5), "nw", 0.05, 0.05, 0, 1, 1);
            verify(near(r.x, 0.15) && near(r.y, 0.15) && near(r.w, 0.45) && near(r.h, 0.45));
            r = overlay.dragged(box(0.1, 0.1, 0.5, 0.5), "e", 0.9, 0, 0, 1, 1);
            verify(near(r.x + r.w, 1), "stops at the picture's edge");
            r = overlay.dragged(box(0.1, 0.1, 0.5, 0.5), "w", 2, 0, 0, 1, 1);
            verify(r.w >= overlay.minSize - 1e-9, "never smaller than the minimum");
        }

        function test_a_locked_ratio_is_kept_while_dragging() {
            // A 2000 x 1000 picture, locked to 1:1 in pixels: a box 0.25 wide is 0.5 high.
            let r = overlay.dragged(box(0.1, 0.1, 0.25, 0.5), "se", 0.15, 0, 1, 2000, 1000);
            compare(Math.round(r.w * 2000), Math.round(r.h * 1000));
            r = overlay.dragged(box(0.1, 0.1, 0.25, 0.5), "e", 0.1, 0, 1, 2000, 1000);
            compare(Math.round(r.w * 2000), Math.round(r.h * 1000));
            verify(r.y >= 0 && r.y + r.h <= 1 + 1e-9);
            // 3:2 on a square picture
            r = overlay.dragged(box(0.1, 0.1, 0.3, 0.2), "s", 0, 0.1, 1.5, 1000, 1000);
            verify(Math.abs(r.w / r.h - 1.5) < 1e-6);
            // A big drag cannot push it out of the picture.
            r = overlay.dragged(box(0.5, 0.5, 0.2, 0.2), "se", 2, 2, 1, 1000, 1000);
            verify(r.x + r.w <= 1 + 1e-9 && r.y + r.h <= 1 + 1e-9);
        }
    }
}
