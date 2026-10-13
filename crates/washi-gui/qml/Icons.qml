pragma Singleton

import QtQuick

// Line icons drawn for Washi on a 24x24 grid (stroke paths, plus optional filled dots).
// Every glyph is authored here as SVG path data; there are no image files.
QtObject {
    id: icons

    function j() {
        return Array.prototype.slice.call(arguments).join(" ");
    }

    function r2(v) {
        return Math.round(v * 100) / 100;
    }

    function circ(cx, cy, r) {
        return "M" + (cx - r) + " " + cy + "a" + r + " " + r + " 0 1 0 " + (2 * r) + " 0a" + r + " " + r + " 0 1 0 " + (-2 * r) + " 0z";
    }

    function rect(x, y, w, h, r) {
        r = r || 0;
        if (r === 0) {
            return "M" + x + " " + y + "h" + w + "v" + h + "h" + (-w) + "z";
        }
        const a = "a" + r + " " + r + " 0 0 1 ";
        return "M" + (x + r) + " " + y + "h" + (w - 2 * r) + a + r + " " + r + "v" + (h - 2 * r) + a + (-r) + " " + r
            + "h" + (-(w - 2 * r)) + a + (-r) + " " + (-r) + "v" + (-(h - 2 * r)) + a + r + " " + (-r) + "z";
    }

    // A gear outline: `teeth` teeth between radii rIn and rOut.
    function gear(cx, cy, rIn, rOut, teeth) {
        const pitch = 2 * Math.PI / teeth;
        const pts = [];
        for (let i = 0; i < teeth; i++) {
            const a = i * pitch;
            const spec = [[rIn, -0.32], [rOut, -0.17], [rOut, 0.17], [rIn, 0.32]];
            for (const s of spec) {
                const ang = a + s[1] * pitch;
                pts.push([r2(cx + s[0] * Math.cos(ang)), r2(cy + s[0] * Math.sin(ang))]);
            }
        }
        return "M" + pts.map(p => p[0] + " " + p[1]).join("L") + "z";
    }

    readonly property var defs: ({
        // ---- navigation
        "back": { s: "M19 12H5 M11 5.5L4.5 12l6.5 6.5" },
        "menu": { s: "M4 6.5h16 M4 12h16 M4 17.5h16" },
        "home": { s: "M3.5 11.2L12 4l8.5 7.2 M5.8 9.6V19.5h4.4v-5.2h3.6v5.2h4.4V9.6" },
        "functions": { s: j(rect(4, 4, 6.5, 6.5, 1.2), rect(13.5, 4, 6.5, 6.5, 1.2), rect(4, 13.5, 6.5, 6.5, 1.2), "M16.75 13.5v6.5 M13.5 16.75H20") },
        "photo": { s: j(rect(3.5, 5, 17, 14, 2), "M3.8 16.2l4.6-4.6 4 4 2.6-2.6 5.2 5.2"), f: circ(15.6, 9.4, 1.4) },
        "document": { s: "M6 3.5h8.2L19 8.3V20.5H6z M14 3.5V8.5H19 M9 13h7 M9 16.5h7" },
        "scan": { s: "M4 8V5.5A1.5 1.5 0 0 1 5.5 4H8 M16 4h2.5A1.5 1.5 0 0 1 20 5.5V8 M20 16v2.5a1.5 1.5 0 0 1-1.5 1.5H16 M8 20H5.5A1.5 1.5 0 0 1 4 18.5V16 M3.5 12h17" },
        "printer": { s: "M7 9V3.8h10V9 M7 17.5H4V9h16v8.5h-3 M7 14.5h10v6H7z", f: circ(16.8, 11.7, 0.8) },
        "star": { s: "M12 3.8l2.6 5.3 5.8.8-4.2 4.1 1 5.8L12 17.1 6.8 19.8l1-5.8-4.2-4.1 5.8-.8z" },
        "more": { f: j(circ(5, 12, 1.6), circ(12, 12, 1.6), circ(19, 12, 1.6)) },
        "plus": { s: "M12 5v14 M5 12h14" },
        "printerlist": { s: "M9.5 6.5H20 M9.5 12H20 M9.5 17.5H20", f: j(circ(5, 6.5, 1.3), circ(5, 12, 1.3), circ(5, 17.5, 1.3)) },
        "gear": { s: j(gear(12, 12, 6.6, 9.3, 8), circ(12, 12, 2.8)) },
        "chevdown": { s: "M6 9.5l6 6 6-6" },
        "chevup": { s: "M6 14.5l6-6 6 6" },
        "chevright": { s: "M9.5 6l6 6-6 6" },
        "chevleft": { s: "M14.5 6l-6 6 6 6" },
        "check": { s: "M5 12.5l4.5 4.5L19 7.5" },
        "close": { s: "M6 6l12 12 M18 6L6 18" },
        "external": { s: "M14 4h6v6 M20 4l-9 9 M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" },
        "refresh": { s: "M19.5 12a7.5 7.5 0 1 1-2.4-5.5 M19.8 4.2v4.6h-4.6" },
        "copy": { s: j(rect(8.5, 8.5, 11, 11, 1.5), "M5 15.5V6.5A1.5 1.5 0 0 1 6.5 5h9") },
        // ---- states
        "info": { s: j(circ(12, 12, 8.5), "M12 11v5.5"), f: circ(12, 7.9, 1.1) },
        "warning": { s: "M12 4.2L21 19.5H3z M12 10v4.5", f: circ(12, 17.1, 1) },
        "error": { s: j(circ(12, 12, 8.5), "M12 7.5v5.5"), f: circ(12, 16.4, 1.1) },
        "success": { s: j(circ(12, 12, 8.5), "M8 12.3l2.9 2.9L16.2 9.5") },
        "moon": { s: "M19 14.8A7.6 7.6 0 1 1 9.2 5a6.1 6.1 0 0 0 9.8 9.8z" },
        // ---- printer management tabs and tiles
        "drop": { s: "M12 3.5c3.4 4.2 6 7.2 6 10.6a6 6 0 0 1-12 0c0-3.4 2.6-6.4 6-10.6z" },
        "bell": { s: "M6 17.5v-6.5a6 6 0 0 1 12 0v6.5l1.5 1.5h-15z M10 21a2 2 0 0 0 4 0" },
        "wrench": { s: "M20.2 7.4a4.6 4.6 0 0 1-6.1 4.4L7.2 18.7a1.9 1.9 0 0 1-2.7-2.7l6.9-6.9a4.6 4.6 0 0 1 4.4-6.1l-2.6 2.6.5 2.4 2.4.5z" },
        "disc": { s: j(circ(12, 12, 8.5), circ(12, 12, 2.2), "M5.8 10.5A6.5 6.5 0 0 1 10.5 5.8") },
        "book": { s: "M4 5.8C6.6 4.7 9.6 4.7 12 6.3c2.4-1.6 5.4-1.6 8-.5v12.7c-2.6-1.1-5.6-1.1-8 .5-2.4-1.6-5.4-1.6-8-.5z M12 6.3v12.7" },
        "monitor": { s: j(rect(3.5, 4.5, 17, 12, 1.5), "M8.5 20.5h7 M12 16.5v4") },
        "laptop": { s: j(rect(5.5, 5.5, 13, 9, 1), "M3 18.5h18") },
        "globe": { s: j(circ(12, 12, 8.5), "M3.5 12h17 M12 3.5c-3 3-3 14 0 17 M12 3.5c3 3 3 14 0 17") },
        "wifi": { s: "M3.5 9.8a12.7 12.7 0 0 1 17 0 M6.5 13.2a8.3 8.3 0 0 1 11 0 M9.4 16.5a4 4 0 0 1 5.2 0", f: circ(12, 19.4, 1.3) },
        "lan": { s: j(rect(9, 3.5, 6, 5, 0.8), "M12 8.5v4 M5 15.5v-3h14v3 M12 12.5v3", rect(2.5, 15.5, 5, 5, 0.8), rect(9.5, 15.5, 5, 5, 0.8), rect(16.5, 15.5, 5, 5, 0.8)) },
        "cart": { s: "M3 4.5h2.4l2.1 10.2h10.3l1.9-7.6H6.3", f: j(circ(9.5, 19, 1.4), circ(17, 19, 1.4)) },
        "folder": { s: "M3.5 6.5a1 1 0 0 1 1-1h5l2 2.5h8a1 1 0 0 1 1 1V18a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1z" },
        // ---- utilities
        "cleaning": { s: "M10 4l1.6 4.4L16 10l-4.4 1.6L10 16l-1.6-4.4L4 10l4.4-1.6z M18 14l.9 2.1L21 17l-2.1.9L18 20l-.9-2.1L15 17l2.1-.9z" },
        "nozzle": { s: j(rect(4, 3.5, 16, 17, 1.5), "M7.5 8h9 M7.5 12h9 M7.5 16h5.5") },
        "align": { s: "M4 8h11 M9 16h11 M17 4.5l3 3.5-3 3.5 M7 12.5L4 16l3 3.5" },
        "roller": { s: j(rect(3.5, 8, 17, 8, 3), "M8 8v8 M16 8v8") },
        "platen": { s: j(rect(3.5, 14.5, 17, 5, 1.2), "M12 4v6.5 M9 8l3 3 3-3") },
        "power": { s: "M12 3.5v8.2 M7.1 6.6a7.5 7.5 0 1 0 9.8 0" },
        "timer": { s: j(circ(12, 13.5, 7.2), "M12 13.5V9.5 M9.6 3.5h4.8 M18.6 7.6l1.4-1.4") },
        "quiet": { s: "M4 9.5h3.5L12.5 6v12l-5-3.5H4z M16.5 9.5l5 5 M21.5 9.5l-5 5" },
        "hourglass": { s: "M7 3.5h10 M7 20.5h10 M8 3.5c0 4 4 5 4 8.5s-4 4.5-4 8.5 M16 3.5c0 4-4 5-4 8.5s4 4.5 4 8.5" },
        "gap": { s: "M4 6.5h16 M4 17.5h16 M12 9v6 M9.5 11l2.5-2.5 2.5 2.5 M9.5 13l2.5 2.5 2.5-2.5" },
        "firmware": { s: j(rect(6, 6, 12, 12, 2), "M12 9v5.5 M9.7 12.5l2.3 2.3 2.3-2.3 M9 3.5V6 M15 3.5V6 M9 18v2.5 M15 18v2.5 M3.5 9H6 M3.5 15H6 M18 9h2.5 M18 15h2.5") },
        // ---- print / scan panels
        "rotatel": { s: "M5.5 9.5a7.6 7.6 0 1 1-.9 5 M5.2 4.2v5.3h5.3" },
        "rotater": { s: "M18.5 9.5a7.6 7.6 0 1 0 .9 5 M18.8 4.2v5.3h-5.3" },
        "trash": { s: "M5 7h14 M9.5 7V4.5h5V7 M7 7l1 13h8l1-13 M10.5 10.5v6 M13.5 10.5v6" },
        "crop": { s: "M7 3v14h14 M3 7h14v14" },
        "wand": { s: "M4.5 19.5L14.5 9.5 M12.2 7.2l4.6 4.6 M17.5 3.5v3 M16 5h3 M20.5 10v2.4 M19.3 11.2h2.4" },
        "sliders": { s: "M4 7h9 M17 7h3 M4 17h3 M11 17h9 M15 4.5v5 M9 14.5v5" },
        "folderopen": { s: "M3.5 7a1 1 0 0 1 1-1h5l2 2.5h7a1 1 0 0 1 1 1v1.5 M3.5 7v11a1 1 0 0 0 1 1h13.3a1 1 0 0 0 1-.8l1.6-7a1 1 0 0 0-1-1.2H8a1 1 0 0 0-1 .8L5.2 19" },
        "text": { s: "M5 6.5h14 M12 6.5v12 M9 18.5h6" },
        "undo": { s: "M8.5 6L4.5 10l4 4 M5 10h8.5a5.5 5.5 0 0 1 0 11H9" },
        "portrait": { s: rect(7, 3.5, 10, 17, 1) },
        "landscape": { s: rect(3.5, 7, 17, 10, 1) }
    })

    function stroke(name) {
        const d = defs[name];
        return d && d.s ? d.s : "M0 0";
    }

    function fill(name) {
        const d = defs[name];
        return d && d.f ? d.f : "M0 0";
    }
}
