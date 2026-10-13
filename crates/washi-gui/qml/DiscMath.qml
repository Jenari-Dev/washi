pragma Singleton

import QtQuick

// The geometry and the data of the disc-label designer, as pure functions: the printable ring,
// millimetres to canvas pixels, the picture's size, the JSON a design is saved as and the JSON
// the service prints (`PrintDisc`). Mirrors washi_core::disc.
QtObject {
    id: math

    readonly property real diameter: 120        // the disc, mm
    readonly property real innerMin: 17
    readonly property real innerMax: 45
    readonly property real outerMin: 114
    readonly property real outerMax: 118
    readonly property real defaultInner: 22
    readonly property real defaultOuter: 116
    readonly property real minPt: 4
    readonly property real maxPt: 200

    function clamp(v, lo, hi) {
        return Math.max(lo, Math.min(hi, v));
    }

    function clampInner(v) {
        return clamp(Math.round(v * 10) / 10, innerMin, innerMax);
    }

    function clampOuter(v) {
        return clamp(Math.round(v * 10) / 10, outerMin, outerMax);
    }

    // ---- canvas scale --------------------------------------------------------------------------
    // A canvas of `px` pixels shows the 120 mm square of the label.
    function pxPerMm(px) {
        return px / diameter;
    }

    function mmToPx(mm, px) {
        return mm * px / diameter;
    }

    function pxToMm(v, px) {
        return v * diameter / px;
    }

    // Font size in canvas pixels for a size in points (1 pt = 1/72 inch).
    function ptToPx(pt, px) {
        return pt * 25.4 / 72 * pxPerMm(px);
    }

    function pxToPt(v, px) {
        return v / pxPerMm(px) * 72 / 25.4;
    }

    // ---- the ring ----------------------------------------------------------------------------------
    // SVG path of everything that stays white: the square minus the printable ring (use an
    // even-odd fill). `px` is the canvas size.
    function circlePath(cx, cy, r) {
        return "M" + (cx - r) + " " + cy + "a" + r + " " + r + " 0 1 0 " + (2 * r) + " 0a" + r + " " + r + " 0 1 0 " + (-2 * r) + " 0z";
    }

    function maskPath(px, innerMm, outerMm) {
        const c = px / 2;
        return "M0 0h" + px + "v" + px + "h" + (-px) + "z" + circlePath(c, c, mmToPx(outerMm, px) / 2) + circlePath(c, c, mmToPx(innerMm, px) / 2);
    }

    // The square minus the disc itself (the corners outside the 120 mm circle).
    function cornersPath(px) {
        const c = px / 2;
        return "M0 0h" + px + "v" + px + "h" + (-px) + "z" + circlePath(c, c, px / 2);
    }

    // True when a point (mm from the disc centre) lies in the printable ring.
    function inRing(xmm, ymm, innerMm, outerMm) {
        const d = Math.sqrt(xmm * xmm + ymm * ymm);
        return d >= innerMm / 2 && d <= outerMm / 2;
    }

    // ---- the picture ---------------------------------------------------------------------------------
    // Width and height of the background picture in mm, as washi_core renders it: `cover` makes
    // the shorter side span the outer diameter, `contain` makes the diagonal span it.
    function pictureSizeMm(imgW, imgH, fit, scale, outerMm) {
        const k = fit === "contain" ? outerMm / Math.sqrt(imgW * imgW + imgH * imgH) : outerMm / Math.min(imgW, imgH);
        return { w: imgW * k * scale, h: imgH * k * scale };
    }

    // A point relative to (cx, cy), turned back by `deg` clockwise: where a point of the canvas
    // lies in the coordinates of an item that is rotated by `deg` about (cx, cy).
    function intoItem(x, y, cx, cy, deg) {
        const r = -deg * Math.PI / 180;
        const dx = x - cx;
        const dy = y - cy;
        return { x: dx * Math.cos(r) - dy * Math.sin(r), y: dx * Math.sin(r) + dy * Math.cos(r) };
    }

    // The angle (degrees clockwise from "up") of the line from (cx, cy) to (x, y); the rotation
    // handle sits above the item, so dragging it to the right turns the item clockwise.
    function handleAngle(x, y, cx, cy) {
        let a = Math.atan2(y - cy, x - cx) * 180 / Math.PI + 90;
        while (a > 180) {
            a -= 360;
        }
        while (a <= -180) {
            a += 360;
        }
        return a;
    }

    // ---- text that follows the ring ---------------------------------------------------------------
    // The same formulas as washi_core::disc::RingText (the printed image): letters on a circle
    // about the disc centre, the start angle in degrees clockwise from the top, clockwise
    // (letter tops outwards) or counter-clockwise (tops inwards), the radius being that of the
    // baseline. Kerning pairs are not applied.
    readonly property real ringRadiusMin: 5
    readonly property real ringRadiusMax: 60
    readonly property real spacingMin: -10
    readonly property real spacingMax: 100
    readonly property real defaultRingRadius: 48

    function newRing() {
        return { radius_mm: defaultRingRadius, start_deg: 0, clockwise: true, letter_spacing_pt: 0 };
    }

    // An angle in degrees brought into (-180, 180].
    function normalizeDeg(a) {
        let v = a % 360;
        if (v > 180) {
            v -= 360;
        } else if (v <= -180) {
            v += 360;
        }
        return v === 0 ? 0 : v;
    }

    function ringSpacingMm(ring) {
        return ring.letter_spacing_pt * 25.4 / 72;
    }

    function ringArcLengthMm(ring, advancesMm) {
        let total = 0;
        for (const a of advancesMm) {
            total += a;
        }
        return total + ringSpacingMm(ring) * Math.max(0, advancesMm.length - 1);
    }

    // Where every letter goes: [{x_mm, y_mm, angle_deg, rotation_deg}] (mm from the disc centre,
    // right and down; rotation clockwise, in (-180, 180]). `align` says what the start angle is:
    // where the text begins (left), its middle (center) or its end (right).
    function ringLayout(ring, advancesMm, align) {
        const total = ringArcLengthMm(ring, advancesMm);
        const shift = align === "left" ? 0 : (align === "right" ? -total : -total / 2);
        const dir = ring.clockwise ? 1 : -1;
        const r = Math.max(ring.radius_mm, 1e-6);
        const out = [];
        let s = 0;
        for (const adv of advancesMm) {
            const along = s + adv / 2 + shift;
            const angle = ring.start_deg + dir * (along / r) * 180 / Math.PI;
            const rad = angle * Math.PI / 180;
            out.push({
                x_mm: r * Math.sin(rad),
                y_mm: -r * Math.cos(rad),
                angle_deg: angle,
                rotation_deg: normalizeDeg(ring.clockwise ? angle : angle + 180)
            });
            s += adv + ringSpacingMm(ring);
        }
        return out;
    }

    // The stretch of the ring a text covers, in degrees clockwise from the top: {from, to}, from
    // not more than to (angles are not normalised; a text can run past 360).
    function ringSpan(ring, advancesMm, align) {
        const total = ringArcLengthMm(ring, advancesMm);
        const shift = align === "left" ? 0 : (align === "right" ? -total : -total / 2);
        const dir = ring.clockwise ? 1 : -1;
        const a = ring.start_deg + dir * (shift / ring.radius_mm) * 180 / Math.PI;
        const b = ring.start_deg + dir * ((shift + total) / ring.radius_mm) * 180 / Math.PI;
        return { from: Math.min(a, b), to: Math.max(a, b) };
    }

    // The point at `deg` (clockwise from the top) on a circle of radius `r` about (cx, cy).
    function ringPoint(cx, cy, r, deg) {
        const rad = deg * Math.PI / 180;
        return { x: cx + r * Math.sin(rad), y: cy - r * Math.cos(rad) };
    }

    // SVG path of the arc from angle `a0` to `a1` (a0 < a1, clockwise) on a circle of radius r.
    function arcPath(cx, cy, r, a0, a1) {
        const sweep = Math.min(a1 - a0, 359.9);
        const p0 = ringPoint(cx, cy, r, a0);
        const p1 = ringPoint(cx, cy, r, a0 + sweep);
        return "M" + p0.x + " " + p0.y + "A" + r + " " + r + " 0 " + (sweep > 180 ? 1 : 0) + " 1 " + p1.x + " " + p1.y;
    }

    // True when a canvas point (mm from the disc centre) lies on a ring text: within its band of
    // radii (the letters stand on the baseline outwards, or hang from it inwards) and its stretch
    // of the ring.
    function ringHit(ring, advancesMm, align, sizePt, xmm, ymm) {
        const d = Math.sqrt(xmm * xmm + ymm * ymm);
        const em = sizePt * 25.4 / 72;
        const lo = ring.clockwise ? ring.radius_mm - 0.3 * em : ring.radius_mm - 1.05 * em;
        const hi = ring.clockwise ? ring.radius_mm + 1.05 * em : ring.radius_mm + 0.3 * em;
        if (d < lo - 1 || d > hi + 1) {
            return false;
        }
        const span = ringSpan(ring, advancesMm, align);
        const slack = 2 / Math.max(d, 1) * 180 / Math.PI;
        const at = Math.atan2(xmm, -ymm) * 180 / Math.PI;
        // Try the angle turned by whole circles around the span.
        const mid = (span.from + span.to) / 2;
        let a = at + 360 * Math.round((mid - at) / 360);
        return a >= span.from - slack && a <= span.to + slack;
    }

    // ---- designs ---------------------------------------------------------------------------------------
    function newText(text) {
        return { text: text || "", x_mm: 0, y_mm: 32, size_pt: 18, color: "#000000", rotation: 0, align: "center", family: "", ring: null };
    }

    // A ring read from JSON: every field checked and clamped; null when there is none.
    function parseRing(r) {
        if (!r || typeof r !== "object" || Array.isArray(r)) {
            return null;
        }
        return {
            radius_mm: clamp(Number(r.radius_mm) || defaultRingRadius, ringRadiusMin, ringRadiusMax),
            start_deg: clamp(Number(r.start_deg) || 0, -360, 360),
            clockwise: r.clockwise !== false,
            letter_spacing_pt: clamp(Number(r.letter_spacing_pt) || 0, spacingMin, spacingMax)
        };
    }

    function newDesign() {
        return {
            version: 1,
            name: "",
            media: "a",
            inner_mm: defaultInner,
            outer_mm: defaultOuter,
            background_color: "#ffffff",
            background: null,
            texts: []
        };
    }

    function isColor(s) {
        return /^#[0-9a-fA-F]{6}$/.test(String(s));
    }

    // A design read from JSON text: every field checked and clamped, unknown ones dropped.
    // Returns null when it is not a design.
    function parseDesign(text) {
        let v = null;
        try {
            v = JSON.parse(text);
        } catch (e) {
            return null;
        }
        if (!v || typeof v !== "object" || Array.isArray(v)) {
            return null;
        }
        const d = newDesign();
        d.name = typeof v.name === "string" ? v.name : "";
        d.media = v.media === "b" ? "b" : "a";
        d.inner_mm = clampInner(Number(v.inner_mm) || defaultInner);
        d.outer_mm = clampOuter(Number(v.outer_mm) || defaultOuter);
        d.background_color = isColor(v.background_color) ? String(v.background_color).toLowerCase() : "#ffffff";
        const b = v.background;
        if (b && typeof b === "object" && typeof b.path === "string" && b.path !== "") {
            const off = Array.isArray(b.offset_mm) && b.offset_mm.length === 2 ? b.offset_mm : [0, 0];
            d.background = {
                path: b.path,
                fit: b.fit === "contain" ? "contain" : "cover",
                scale: clamp(Number(b.scale) || 1, 0.1, 8),
                offset_mm: [clamp(Number(off[0]) || 0, -120, 120), clamp(Number(off[1]) || 0, -120, 120)],
                rotation: Number(b.rotation) || 0
            };
        }
        if (Array.isArray(v.texts)) {
            for (const t of v.texts.slice(0, 40)) {
                if (!t || typeof t.text !== "string") {
                    continue;
                }
                d.texts.push({
                    text: t.text,
                    x_mm: clamp(Number(t.x_mm) || 0, -120, 120),
                    y_mm: clamp(Number(t.y_mm) || 0, -120, 120),
                    size_pt: clamp(Number(t.size_pt) || 18, minPt, maxPt),
                    color: isColor(t.color) ? String(t.color).toLowerCase() : "#000000",
                    rotation: Number(t.rotation) || 0,
                    align: ["left", "center", "right"].indexOf(t.align) >= 0 ? t.align : "center",
                    family: typeof t.family === "string" ? t.family : "",
                    ring: parseRing(t.ring)
                });
            }
        }
        return d;
    }

    function designJson(d) {
        return JSON.stringify(d, null, 2);
    }

    // File name for a design: ASCII letters and digits, space, dash and underscore, and any
    // non-ASCII character (Japanese names are welcome). No dots or slashes, so a name can never
    // leave the designs folder.
    function fileName(name) {
        let clean = "";
        for (const ch of String(name)) {
            if (/[A-Za-z0-9 _-]/.test(ch) || ch.codePointAt(0) > 0x7f) {
                clean += ch;
            }
        }
        clean = clean.trim().replace(/\s+/g, " ");
        return clean === "" ? "" : clean + ".json";
    }

    // The JSON PrintDisc takes (confirm is added by the Store). `fontFile(family)` maps a font
    // family to a file ("" = let the host pick one).
    function printSpec(d, fontFile, quality) {
        const spec = {
            inner_mm: d.inner_mm,
            outer_mm: d.outer_mm,
            media: d.media,
            background_color: d.background_color,
            texts: d.texts.filter(t => t.text.trim() !== "").map(t => {
                const o = { text: t.text, x_mm: t.x_mm, y_mm: t.y_mm, size_pt: t.size_pt, color: t.color, rotation: t.rotation, align: t.align };
                const f = t.family !== "" && fontFile ? fontFile(t.family) : "";
                if (f !== "") {
                    o.font = f;
                }
                if (t.ring) {
                    o.ring = { radius_mm: t.ring.radius_mm, start_deg: t.ring.start_deg, clockwise: t.ring.clockwise, letter_spacing_pt: t.ring.letter_spacing_pt };
                }
                return o;
            })
        };
        if (d.background) {
            spec.image = d.background.path;
            spec.fit = d.background.fit;
            spec.scale = d.background.scale;
            spec.offset_mm = d.background.offset_mm;
            spec.rotation = d.background.rotation;
        }
        if (quality) {
            spec.quality = quality;
        }
        return spec;
    }

    readonly property var swatches: ["#000000", "#FFFFFF", "#C93A3A", "#E0702B", "#F2CB1D", "#2E9E4F", "#1E50B4", "#7B5CC9", "#C9437E", "#8C8C8C"]

    function mediaLabel(m) {
        return m === "b" ? i18n("Other printable disc") : i18n("Printable disc (recommended)");
    }
}
