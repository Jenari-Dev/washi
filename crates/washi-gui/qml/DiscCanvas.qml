import QtQuick
import QtQuick.Shapes

// The disc-label canvas: the 120 mm label square with the picture, the texts, the printable
// ring (everything else prints white) and handles to move, scale and turn the selected item.
// `design` is the data of DiscMath.newDesign(); every edit is reported as `edited(design)` with
// a new object, so a binding on the owner's `design` keeps working.
Item {
    id: canvas

    property var design: DiscMath.newDesign()
    // -2 nothing, -1 the picture, 0.. a text
    property int selected: -2
    readonly property real size: Math.max(120, Math.min(width, height))
    readonly property real mm: DiscMath.pxPerMm(size)

    signal edited(var design)
    signal selectionChanged2(int index)

    ListModel {
        id: tm
    }

    function syncTexts() {
        const list = canvas.design.texts;
        while (tm.count > list.length) {
            tm.remove(tm.count - 1);
        }
        for (let i = 0; i < list.length; i++) {
            const t = list[i];
            const rg = t.ring;
            const row = {
                t_text: t.text, t_x: t.x_mm, t_y: t.y_mm, t_pt: t.size_pt, t_color: t.color, t_rot: t.rotation, t_align: t.align, t_family: t.family, t_bold: !!t.bold, t_italic: !!t.italic,
                t_ring_on: !!rg, t_ring_r: rg ? rg.radius_mm : DiscMath.defaultRingRadius, t_ring_start: rg ? rg.start_deg : 0,
                t_ring_cw: rg ? rg.clockwise : true, t_ring_sp: rg ? rg.letter_spacing_pt : 0
            };
            if (i < tm.count) {
                tm.set(i, row);
            } else {
                tm.append(row);
            }
        }
    }
    onDesignChanged: syncTexts()
    Component.onCompleted: syncTexts()

    function copy() {
        return JSON.parse(JSON.stringify(canvas.design));
    }

    function select(i) {
        selected = i;
        selectionChanged2(i);
    }

    // The picture's natural size, once it has loaded.
    Image {
        id: probe
        visible: false
        source: canvas.design.background ? "file://" + canvas.design.background.path : ""
        autoTransform: true
        asynchronous: false
    }
    readonly property bool havePicture: !!design.background && probe.status === Image.Ready && probe.implicitWidth > 0
    readonly property var pictureMm: havePicture ? DiscMath.pictureSizeMm(probe.implicitWidth, probe.implicitHeight, design.background.fit, design.background.scale, design.outer_mm) : ({ w: 0, h: 0 })

    Item {
        id: disc
        objectName: "discSquare"
        width: canvas.size
        height: canvas.size
        anchors.centerIn: parent
        clip: true

        Rectangle {
            anchors.fill: parent
            color: canvas.design.background_color
        }

        // ---- the picture
        Item {
            id: pic
            visible: canvas.havePicture
            width: DiscMath.mmToPx(canvas.pictureMm.w, canvas.size)
            height: DiscMath.mmToPx(canvas.pictureMm.h, canvas.size)
            x: canvas.size / 2 + DiscMath.mmToPx(canvas.design.background ? canvas.design.background.offset_mm[0] : 0, canvas.size) - width / 2
            y: canvas.size / 2 + DiscMath.mmToPx(canvas.design.background ? canvas.design.background.offset_mm[1] : 0, canvas.size) - height / 2
            rotation: canvas.design.background ? canvas.design.background.rotation : 0
            Image {
                anchors.fill: parent
                source: probe.source
                autoTransform: true
                smooth: true
                mipmap: true
                fillMode: Image.Stretch
            }
        }

        // ---- the texts (a ListModel, so that the items live on while a drag edits them)
        Repeater {
            id: texts
            model: tm
            delegate: Item {
                id: anchor
                required property int index
                required property string t_text
                required property real t_x
                required property real t_y
                required property real t_pt
                required property string t_color
                required property real t_rot
                required property string t_align
                required property string t_family
                required property bool t_bold
                required property bool t_italic
                required property bool t_ring_on
                required property real t_ring_r
                required property real t_ring_start
                required property bool t_ring_cw
                required property real t_ring_sp
                // The anchor is (x_mm, y_mm); the block turns about it. A text that follows the
                // ring is anchored at the disc centre instead, and its letters are placed on the ring.
                x: canvas.size / 2 + (t_ring_on ? 0 : DiscMath.mmToPx(t_x, canvas.size))
                y: canvas.size / 2 + (t_ring_on ? 0 : DiscMath.mmToPx(t_y, canvas.size))
                rotation: t_ring_on ? 0 : t_rot
                readonly property real fontPx: DiscMath.ptToPx(t_pt, canvas.size)
                readonly property real bw: label.contentWidth
                readonly property real bh: label.contentHeight
                readonly property real textLeft: t_align === "left" ? 0 : (t_align === "right" ? -bw : -bw / 2)
                readonly property bool ringOn: t_ring_on
                // The ring: its letters, their widths in mm (as the font draws them) and where they go.
                readonly property var ring: ({ radius_mm: t_ring_r, start_deg: t_ring_start, clockwise: t_ring_cw, letter_spacing_pt: t_ring_sp })
                readonly property var glyphChars: Array.from(t_text.replace(/[\r\n]+/g, " ")).map(c => c.charCodeAt(0) < 32 ? " " : c)
                readonly property var advancesMm: {
                    fontPx; t_family; t_bold; t_italic;       // widths change with the font
                    return glyphChars.map(c => metrics.advanceWidth(c) / canvas.mm);
                }
                readonly property var glyphs: t_ring_on ? DiscMath.ringLayout(ring, advancesMm, t_align) : []
                FontMetrics {
                    id: metrics
                    font.family: anchor.t_family !== "" ? anchor.t_family : Qt.application.font.family
                    font.bold: anchor.t_bold
                    font.italic: anchor.t_italic
                    font.pixelSize: Math.max(1, anchor.fontPx)
                }
                Repeater {
                    model: anchor.t_ring_on ? anchor.glyphs : []
                    delegate: Item {
                        required property var modelData
                        required property int index
                        // The point of the baseline in the middle of the letter sits on the ring.
                        x: DiscMath.mmToPx(modelData.x_mm, canvas.size)
                        y: DiscMath.mmToPx(modelData.y_mm, canvas.size)
                        rotation: modelData.rotation_deg
                        Text {
                            visible: anchor.glyphChars[index] !== " "
                            x: -width / 2
                            y: -baselineOffset
                            text: anchor.glyphChars[index] || ""
                            color: anchor.t_color
                            font.family: anchor.t_family !== "" ? anchor.t_family : Qt.application.font.family
                            font.bold: anchor.t_bold
                            font.italic: anchor.t_italic
                            font.pixelSize: Math.max(1, anchor.fontPx)
                            textFormat: Text.PlainText
                            wrapMode: Text.NoWrap
                        }
                    }
                }
                Text {
                    id: label
                    visible: !anchor.t_ring_on
                    x: anchor.textLeft
                    y: -height / 2
                    text: anchor.t_text
                    color: anchor.t_color
                    font.family: anchor.t_family !== "" ? anchor.t_family : Qt.application.font.family
                    font.bold: anchor.t_bold
                    font.italic: anchor.t_italic
                    font.pixelSize: Math.max(1, anchor.fontPx)
                    lineHeightMode: Text.FixedHeight
                    lineHeight: anchor.fontPx * 1.25
                    horizontalAlignment: anchor.t_align === "left" ? Text.AlignLeft : (anchor.t_align === "right" ? Text.AlignRight : Text.AlignHCenter)
                    textFormat: Text.PlainText
                    wrapMode: Text.NoWrap
                }
            }
        }

        // ---- what stays white: outside the ring (and the corners beyond the disc, in grey)
        Shape {
            anchors.fill: parent
            ShapePath {
                fillRule: ShapePath.OddEvenFill
                strokeColor: "transparent"
                fillColor: "#FFFFFF"
                PathSvg { path: DiscMath.maskPath(canvas.size, canvas.design.inner_mm, canvas.design.outer_mm) }
            }
        }
        Shape {
            anchors.fill: parent
            ShapePath {
                fillRule: ShapePath.OddEvenFill
                strokeColor: "transparent"
                fillColor: "#E6E6E6"
                PathSvg { path: DiscMath.cornersPath(canvas.size) }
            }
        }
        // Outlines of the disc, the printable ring and the hub
        Shape {
            anchors.fill: parent
            ShapePath {
                strokeColor: "#9AA3B2"
                strokeWidth: 1.2
                fillColor: "transparent"
                PathSvg { path: DiscMath.circlePath(canvas.size / 2, canvas.size / 2, canvas.size / 2 - 0.6) }
            }
            ShapePath {
                strokeColor: "#6C86B8"
                strokeWidth: 1
                strokeStyle: ShapePath.DashLine
                dashPattern: [3, 3]
                fillColor: "transparent"
                PathSvg { path: DiscMath.circlePath(canvas.size / 2, canvas.size / 2, DiscMath.mmToPx(canvas.design.outer_mm, canvas.size) / 2) }
            }
            ShapePath {
                strokeColor: "#6C86B8"
                strokeWidth: 1
                strokeStyle: ShapePath.DashLine
                dashPattern: [3, 3]
                fillColor: "transparent"
                PathSvg { path: DiscMath.circlePath(canvas.size / 2, canvas.size / 2, DiscMath.mmToPx(canvas.design.inner_mm, canvas.size) / 2) }
            }
        }
    }

    // A canvas point as an angle (degrees clockwise from the top) and a distance (mm) from the centre.
    function polar(px, py) {
        const c = canvas.size / 2;
        const dx = px - c;
        const dy = py - c;
        return { angle: Math.atan2(dx, -dy) * 180 / Math.PI, r: DiscMath.pxToMm(Math.hypot(dx, dy), canvas.size) };
    }

    // ---- selecting and moving ------------------------------------------------------------------------
    // Hit test in canvas pixels: the topmost text, else the picture.
    function hit(px, py) {
        for (let i = canvas.design.texts.length - 1; i >= 0; i--) {
            const a = texts.itemAt(i);
            if (!a) {
                continue;
            }
            if (a.ringOn) {
                const c = canvas.size / 2;
                if (DiscMath.ringHit(a.ring, a.advancesMm, a.t_align, a.t_pt, DiscMath.pxToMm(px - c, canvas.size), DiscMath.pxToMm(py - c, canvas.size))) {
                    return i;
                }
                continue;
            }
            const p = DiscMath.intoItem(px, py, a.x, a.y, a.rotation);
            if (p.x >= a.textLeft - 4 && p.x <= a.textLeft + a.bw + 4 && p.y >= -a.bh / 2 - 4 && p.y <= a.bh / 2 + 4) {
                return i;
            }
        }
        if (canvas.havePicture) {
            const cx = pic.x + pic.width / 2;
            const cy = pic.y + pic.height / 2;
            const p = DiscMath.intoItem(px, py, cx, cy, pic.rotation);
            if (Math.abs(p.x) <= pic.width / 2 && Math.abs(p.y) <= pic.height / 2) {
                return -1;
            }
        }
        return -2;
    }

    MouseArea {
        id: area
        objectName: "discMouse"
        x: disc.x
        y: disc.y
        width: disc.width
        height: disc.height
        property var start: null
        property point from
        onPressed: mouse => {
            const h = canvas.hit(mouse.x, mouse.y);
            canvas.select(h);
            const d = canvas.copy();
            if (h >= 0 && d.texts[h].ring) {
                // A ring text is dragged round the disc (the start angle) and in and out (the radius).
                const g = canvas.polar(mouse.x, mouse.y);
                start = { ring: true, angle: g.angle, r: g.r, start: d.texts[h].ring.start_deg, radius: d.texts[h].ring.radius_mm };
            } else {
                start = h === -1 && d.background ? { ox: d.background.offset_mm[0], oy: d.background.offset_mm[1] } : (h >= 0 ? { ox: d.texts[h].x_mm, oy: d.texts[h].y_mm } : null);
            }
            from = Qt.point(mouse.x, mouse.y);
        }
        onPositionChanged: mouse => {
            if (!pressed || !start || canvas.selected === -2) {
                return;
            }
            const dx = DiscMath.pxToMm(mouse.x - from.x, canvas.size);
            const dy = DiscMath.pxToMm(mouse.y - from.y, canvas.size);
            const d = canvas.copy();
            if (start.ring) {
                const g = canvas.polar(mouse.x, mouse.y);
                const ring = d.texts[canvas.selected].ring;
                ring.start_deg = DiscMath.clamp(Math.round(start.start + DiscMath.normalizeDeg(g.angle - start.angle)), -360, 360);
                ring.radius_mm = DiscMath.clamp(Math.round((start.radius + g.r - start.r) * 2) / 2, DiscMath.ringRadiusMin, DiscMath.ringRadiusMax);
            } else if (canvas.selected === -1 && d.background) {
                d.background.offset_mm = [DiscMath.clamp(start.ox + dx, -120, 120), DiscMath.clamp(start.oy + dy, -120, 120)];
            } else if (canvas.selected >= 0) {
                d.texts[canvas.selected].x_mm = DiscMath.clamp(start.ox + dx, -120, 120);
                d.texts[canvas.selected].y_mm = DiscMath.clamp(start.oy + dy, -120, 120);
            }
            canvas.edited(d);
        }
    }

    // ---- the selection frame with its handles --------------------------------------------------------------
    // One frame for either kind of item: `fx, fy` is the pivot in canvas pixels, `fr` its rotation,
    // and the box (bx, by, bw, bh) is relative to the pivot.
    readonly property bool textSelected: selected >= 0 && selected < tm.count && texts.count === tm.count && !!texts.itemAt(selected)
    // A text that follows the ring has no frame: an arc along its baseline shows what is selected.
    readonly property bool ringSelected: textSelected && texts.itemAt(selected).ringOn
    readonly property bool pictureSelected: selected === -1 && havePicture
    readonly property bool framed: (textSelected && !ringSelected) || pictureSelected
    readonly property real fx: disc.x + (textSelected ? texts.itemAt(selected).x : pic.x + pic.width / 2)
    readonly property real fy: disc.y + (textSelected ? texts.itemAt(selected).y : pic.y + pic.height / 2)
    readonly property real fr: textSelected ? texts.itemAt(selected).rotation : pic.rotation
    readonly property real fbx: textSelected ? texts.itemAt(selected).textLeft - 3 : -pic.width / 2
    readonly property real fby: textSelected ? -texts.itemAt(selected).bh / 2 - 2 : -pic.height / 2
    readonly property real fbw: textSelected ? texts.itemAt(selected).bw + 6 : pic.width
    readonly property real fbh: textSelected ? texts.itemAt(selected).bh + 4 : pic.height

    Shape {
        id: ringArc
        objectName: "ringSelection"
        visible: canvas.ringSelected
        x: disc.x
        y: disc.y
        width: canvas.size
        height: canvas.size
        ShapePath {
            strokeColor: Theme.accent
            strokeWidth: 2
            strokeStyle: ShapePath.DashLine
            dashPattern: [4, 3]
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathSvg {
                path: {
                    if (!canvas.ringSelected) {
                        return "M0 0";
                    }
                    const a = texts.itemAt(canvas.selected);
                    const span = DiscMath.ringSpan(a.ring, a.advancesMm, a.t_align);
                    return DiscMath.arcPath(canvas.size / 2, canvas.size / 2, DiscMath.mmToPx(a.ring.radius_mm, canvas.size), span.from, Math.max(span.to, span.from + 0.5));
                }
            }
        }
    }

    Item {
        id: frame
        objectName: "selectionFrame"
        visible: canvas.framed
        x: canvas.fx
        y: canvas.fy
        rotation: canvas.fr
        width: 0
        height: 0

        Rectangle {
            x: canvas.fbx
            y: canvas.fby
            width: canvas.fbw
            height: canvas.fbh
            color: "transparent"
            border.width: 1.5
            border.color: Theme.accent
        }
        // Scale: the bottom-right corner
        Rectangle {
            id: scaleHandle
            objectName: "scaleHandle"
            x: canvas.fbx + canvas.fbw - width / 2
            y: canvas.fby + canvas.fbh - height / 2
            width: 14
            height: 14
            radius: 2
            color: "#FFFFFF"
            border.width: 1.5
            border.color: Theme.accent
            MouseArea {
                anchors.fill: parent
                anchors.margins: -6
                cursorShape: Qt.SizeFDiagCursor
                property real startDist: 1
                property real startValue: 1
                onPressed: mouse => {
                    const p = mapToItem(canvas, mouse.x, mouse.y);
                    startDist = Math.max(8, Math.hypot(p.x - canvas.fx, p.y - canvas.fy));
                    const d = canvas.copy();
                    startValue = canvas.textSelected ? d.texts[canvas.selected].size_pt : d.background.scale;
                }
                onPositionChanged: mouse => {
                    if (!pressed) return;
                    const p = mapToItem(canvas, mouse.x, mouse.y);
                    const f = Math.hypot(p.x - canvas.fx, p.y - canvas.fy) / startDist;
                    const d = canvas.copy();
                    if (canvas.textSelected) {
                        d.texts[canvas.selected].size_pt = DiscMath.clamp(Math.round(startValue * f * 2) / 2, DiscMath.minPt, DiscMath.maxPt);
                    } else {
                        d.background.scale = DiscMath.clamp(Math.round(startValue * f * 100) / 100, 0.1, 8);
                    }
                    canvas.edited(d);
                }
            }
        }
        // Turn: a handle above the top edge
        Rectangle {
            x: canvas.fbx + canvas.fbw / 2 - 1
            y: canvas.fby - 22
            width: 2
            height: 22
            color: Theme.accent
        }
        Rectangle {
            id: turnHandle
            objectName: "turnHandle"
            x: canvas.fbx + canvas.fbw / 2 - width / 2
            y: canvas.fby - 22 - height / 2
            width: 14
            height: 14
            radius: 7
            color: "#FFFFFF"
            border.width: 1.5
            border.color: Theme.accent
            MouseArea {
                anchors.fill: parent
                anchors.margins: -6
                cursorShape: Qt.CrossCursor
                onPositionChanged: mouse => {
                    if (!pressed) return;
                    const p = mapToItem(canvas, mouse.x, mouse.y);
                    let a = DiscMath.handleAngle(p.x, p.y, canvas.fx, canvas.fy);
                    a = Math.round(a);
                    // Snap to the quarter turns when close.
                    for (const q of [-180, -90, 0, 90, 180]) {
                        if (Math.abs(a - q) <= 3) a = q === -180 ? 180 : q;
                    }
                    const d = canvas.copy();
                    if (canvas.textSelected) {
                        d.texts[canvas.selected].rotation = a;
                    } else {
                        d.background.rotation = a;
                    }
                    canvas.edited(d);
                }
            }
        }
    }
}
