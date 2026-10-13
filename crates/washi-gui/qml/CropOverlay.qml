import QtQuick

// Draggable trimming box over a picture. `imageRect` is where the picture is drawn (in this
// item's coordinates); `crop` is the box as fractions of the picture ({x, y, w, h}, or null for
// the whole picture). `lockRatio` (width / height in picture pixels, 0 = free) keeps the shape
// while dragging. Emits `edited(x, y, w, h)` (fractions) as the box changes.
Item {
    id: ov

    property rect imageRect: Qt.rect(0, 0, 0, 0)
    property var crop: null
    property real lockRatio: 0
    property real pictureWidth: 1       // picture size in pixels (for the ratio lock)
    property real pictureHeight: 1

    signal edited(real x, real y, real w, real h)

    readonly property real minSize: 0.04
    readonly property real cx: crop ? crop.x : 0
    readonly property real cy: crop ? crop.y : 0
    readonly property real cw: crop ? crop.w : 1
    readonly property real ch: crop ? crop.h : 1
    // The box in this item's coordinates.
    readonly property real bx: imageRect.x + cx * imageRect.width
    readonly property real by: imageRect.y + cy * imageRect.height
    readonly property real bw: cw * imageRect.width
    readonly property real bh: ch * imageRect.height

    function clamp(v, lo, hi) {
        return Math.max(lo, Math.min(hi, v));
    }

    // The box that results from dragging `kind` ("move", "nw", "n", ...) by (dx, dy) fractions
    // from the box `s` = {x, y, w, h}. Pure: the QML test calls it directly.
    function dragged(s, kind, dx, dy, lock, picW, picH) {
        let x = s.x, y = s.y, x2 = s.x + s.w, y2 = s.y + s.h;
        const m = minSize;
        if (kind === "move") {
            x = clamp(s.x + dx, 0, 1 - s.w);
            y = clamp(s.y + dy, 0, 1 - s.h);
            return { x: x, y: y, w: s.w, h: s.h };
        }
        if (kind.indexOf("w") >= 0) x = clamp(s.x + dx, 0, x2 - m);
        if (kind.indexOf("e") >= 0) x2 = clamp(s.x + s.w + dx, x + m, 1);
        if (kind.indexOf("n") >= 0) y = clamp(s.y + dy, 0, y2 - m);
        if (kind.indexOf("s") >= 0) y2 = clamp(s.y + s.h + dy, y + m, 1);
        if (lock > 0) {
            // w / h in fractions of the picture
            const k = lock * picH / picW;
            const horiz = kind.indexOf("w") >= 0 || kind.indexOf("e") >= 0;
            const vert = kind.indexOf("n") >= 0 || kind.indexOf("s") >= 0;
            let w = x2 - x, h = y2 - y;
            if (horiz && !vert) {
                h = w / k;
                const mid = s.y + s.h / 2;
                y = mid - h / 2;
                y2 = mid + h / 2;
            } else if (vert && !horiz) {
                w = h * k;
                const mid = s.x + s.w / 2;
                x = mid - w / 2;
                x2 = mid + w / 2;
            } else {
                // A corner: the width leads, the opposite corner stays.
                h = w / k;
                if (kind.indexOf("n") >= 0) y = y2 - h; else y2 = y + h;
            }
            // Keep it inside the picture, shrinking around the fixed side.
            w = x2 - x;
            h = y2 - y;
            let over = Math.max(0, -x, x2 - 1, (-y) * k, (y2 - 1) * k);
            if (over > 0) {
                const f = Math.max(m, w - over) / w;
                if (kind.indexOf("w") >= 0) x = x2 - w * f; else if (kind.indexOf("e") >= 0) x2 = x + w * f; else { const c = (x + x2) / 2; x = c - w * f / 2; x2 = c + w * f / 2; }
                if (kind.indexOf("n") >= 0) y = y2 - h * f; else if (kind.indexOf("s") >= 0) y2 = y + h * f; else { const c2 = (y + y2) / 2; y = c2 - h * f / 2; y2 = c2 + h * f / 2; }
            }
            x = clamp(x, 0, 1);
            x2 = clamp(x2, 0, 1);
            y = clamp(y, 0, 1);
            y2 = clamp(y2, 0, 1);
        }
        return { x: x, y: y, w: x2 - x, h: y2 - y };
    }

    // ---- drawing: the dimmed outside, the frame, thirds, handles -------------------------------
    Rectangle { x: ov.imageRect.x; y: ov.imageRect.y; width: ov.imageRect.width; height: Math.max(0, ov.by - ov.imageRect.y); color: "#99000000" }
    Rectangle { x: ov.imageRect.x; y: ov.by + ov.bh; width: ov.imageRect.width; height: Math.max(0, ov.imageRect.y + ov.imageRect.height - ov.by - ov.bh); color: "#99000000" }
    Rectangle { x: ov.imageRect.x; y: ov.by; width: Math.max(0, ov.bx - ov.imageRect.x); height: ov.bh; color: "#99000000" }
    Rectangle { x: ov.bx + ov.bw; y: ov.by; width: Math.max(0, ov.imageRect.x + ov.imageRect.width - ov.bx - ov.bw); height: ov.bh; color: "#99000000" }

    Rectangle {
        x: ov.bx
        y: ov.by
        width: ov.bw
        height: ov.bh
        color: "transparent"
        border.width: 2
        border.color: "#FFFFFF"
        Repeater {
            model: 2
            Rectangle { x: parent.width * (index + 1) / 3; width: 1; height: parent.height; color: "#88FFFFFF" }
        }
        Repeater {
            model: 2
            Rectangle { y: parent.height * (index + 1) / 3; height: 1; width: parent.width; color: "#88FFFFFF" }
        }
    }

    // The area inside moves the box.
    MouseArea {
        x: ov.bx
        y: ov.by
        width: ov.bw
        height: ov.bh
        cursorShape: Qt.SizeAllCursor
        property var start: null
        property point from
        onPressed: mouse => {
            start = { x: ov.cx, y: ov.cy, w: ov.cw, h: ov.ch };
            from = mapToItem(ov, mouse.x, mouse.y);
        }
        onPositionChanged: mouse => {
            if (!pressed || !start) return;
            const p = mapToItem(ov, mouse.x, mouse.y);
            const r = ov.dragged(start, "move", (p.x - from.x) / ov.imageRect.width, (p.y - from.y) / ov.imageRect.height, 0, 1, 1);
            ov.edited(r.x, r.y, r.w, r.h);
        }
    }

    Repeater {
        model: ["nw", "n", "ne", "e", "se", "s", "sw", "w"]
        delegate: Rectangle {
            id: handle
            required property string modelData
            readonly property string k: modelData
            width: 14
            height: 14
            radius: 2
            color: "#FFFFFF"
            border.width: 1.5
            border.color: Theme.accent
            x: (k.indexOf("w") >= 0 ? ov.bx : (k.indexOf("e") >= 0 ? ov.bx + ov.bw : ov.bx + ov.bw / 2)) - width / 2
            y: (k.indexOf("n") >= 0 ? ov.by : (k.indexOf("s") >= 0 ? ov.by + ov.bh : ov.by + ov.bh / 2)) - height / 2
            MouseArea {
                anchors.fill: parent
                anchors.margins: -8
                cursorShape: handle.k === "n" || handle.k === "s" ? Qt.SizeVerCursor : (handle.k === "e" || handle.k === "w" ? Qt.SizeHorCursor : (handle.k === "nw" || handle.k === "se" ? Qt.SizeFDiagCursor : Qt.SizeBDiagCursor))
                property var start: null
                property point from
                onPressed: mouse => {
                    start = { x: ov.cx, y: ov.cy, w: ov.cw, h: ov.ch };
                    from = mapToItem(ov, mouse.x, mouse.y);
                }
                onPositionChanged: mouse => {
                    if (!pressed || !start) return;
                    const p = mapToItem(ov, mouse.x, mouse.y);
                    const r = ov.dragged(start, handle.k, (p.x - from.x) / ov.imageRect.width, (p.y - from.y) / ov.imageRect.height, ov.lockRatio, ov.pictureWidth, ov.pictureHeight);
                    ov.edited(r.x, r.y, r.w, r.h);
                }
            }
        }
    }
}
