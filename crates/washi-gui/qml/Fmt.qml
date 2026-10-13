pragma Singleton

import QtQuick

// Small pure helpers that turn printer codes into text (all user text goes through i18n).
//
// Text that is stored and shown later (a progress line, an error, a notice) is kept as a message
// source, not as a string: either a plain string (words of the service, which already speak the
// language it was told) or a function that builds the text when it is shown, so that its i18n()
// calls follow a switch of the language. `text()` turns either into the string to show.
QtObject {
    readonly property var inkOrder: ["BK", "C", "M", "Y", "PGBK"]

    function text(source) {
        if (typeof source === "function") {
            return source();
        }
        return source === undefined || source === null ? "" : String(source);
    }

    // Two sentences one after the other (the second may be empty).
    function sentences(first, second) {
        return second === "" ? first : i18nc("two sentences, one after the other", "%1 %2", first, second);
    }

    // The parts of a short summary, joined the way the language writes a list.
    function list(parts) {
        return parts.filter(p => p !== "").join(i18nc("separator between the parts of a summary", ", "));
    }

    function inkName(code) {
        switch (code) {
        case "BK": return i18nc("ink color", "Black");
        case "PGBK": return i18n("Pigment black");
        case "C": return i18nc("ink color", "Cyan");
        case "M": return i18nc("ink color", "Magenta");
        case "Y": return i18nc("ink color", "Yellow");
        case "GY": return i18nc("ink color", "Grey");
        default: return code;
        }
    }

    // Banner order: BK, C, M, Y, PGBK first, then anything else by the printer's own order.
    function sortInks(list) {
        const rank = ink => {
            const i = inkOrder.indexOf(ink.color);
            return i >= 0 ? i : 100 + (ink.order || 0);
        };
        return (list || []).slice().sort((a, b) => rank(a) - rank(b));
    }

    function inkColor(ink) {
        return Theme.inkColors[ink.color] || ink.rgb || "#8E8E8E";
    }

    // "XKI-N21<M>SETUP" -> "XKI-N21 (M) SETUP"
    function tankModel(model) {
        if (!model) {
            return "";
        }
        return model.replace(/<(.+?)>/, " ($1) ").replace(/\s+/g, " ").trim();
    }

    function percent(level) {
        return level === null || level === undefined ? "–" : level + "%";
    }

    function trayName(bin) {
        switch (bin) {
        case "inputbin_p1": return i18n("Rear tray");
        case "inputbin_p2": return i18n("Cassette");
        default: return bin;
        }
    }

    // "iso_a4_210x297mm" -> "A4"
    function paperSize(s) {
        if (!s) {
            return "";
        }
        let m = /^iso_a(\d)_/.exec(s);
        if (m) {
            return "A" + m[1];
        }
        m = /^iso_b(\d)_/.exec(s);
        if (m) {
            return "B" + m[1];
        }
        if (s.indexOf("na_letter") === 0) {
            return i18n("Letter");
        }
        if (s.indexOf("na_legal") === 0) {
            return i18n("Legal");
        }
        if (s.indexOf("hagaki") >= 0) {
            return i18n("Postcard");
        }
        return s.replace(/_\d.*$/, "").replace(/^[a-z]+_/, "").replace(/_/g, " ");
    }

    function paperType(s) {
        switch (s) {
        case "stationery": return i18nc("a kind of paper, in the middle of a sentence", "plain paper");
        case "photographic": return i18nc("a kind of paper, in the middle of a sentence", "photo paper");
        case "envelope": return i18nc("a kind of paper, in the middle of a sentence", "envelope");
        default: return s ? s.replace(/_/g, " ") : "";
        }
    }

    function sheets(n) {
        return i18np("%1 sheet", "%1 sheets", n);
    }

    function sum(obj) {
        let t = 0;
        for (const k in (obj || {})) {
            t += obj[k] || 0;
        }
        return t;
    }

    // "2100" -> "21:00"
    function hhmm(s) {
        return s && s.length === 4 ? s.substring(0, 2) + ":" + s.substring(2) : (s || "");
    }

    function unhhmm(s) {
        return (s || "").replace(":", "");
    }

    function clockValid(s) {
        return /^([01]\d|2[0-3]):[0-5]\d$/.test(s || "");
    }

    function errorText(body) {
        let e = null;
        try {
            e = JSON.parse(body);
        } catch (x) {
            return body || i18n("Unknown error");
        }
        switch (e.name) {
        case "NotRunning": return i18n("The Washi service is not running.");
        case "ReadOnly": return i18n("The Washi service is in read-only mode, so nothing can be changed.");
        case "Busy": return e.message || i18n("The printer is working on something else.");
        case "Timeout": return i18n("The service did not answer in time.");
        case "Bus": return i18n("Washi could not reach the desktop's message bus: %1", e.message);
        case "Internal": return i18n("Washi ran into an internal problem: %1", e.message);
        default: return e.message || e.name || i18n("Unknown error");
        }
    }

    // The text for an answer of the service that failed (`body` is what the Backend delivered).
    // A message source: a failure shown later still follows the language.
    function errorSource(body) {
        return () => errorText(body);
    }

    // What the printer says when it reports a support code: the words of the code's family. The
    // service sends the same words in its notifications, and `po/check.py` keeps both in step
    // with the table in `washi-core`. A family this app does not know (a service newer than the
    // app) gets a line that names the code.
    function supportMessage(support) {
        if (!support) {
            return "";
        }
        switch (String(support.message_key || "")) {
        case "support.no-paper-rear": return i18n("The rear tray is out of paper. Load some, then press OK on the printer.");
        case "support.no-paper-cassette": return i18n("The cassette is out of paper. Load some, then press OK on the printer.");
        case "support.paper-feed": return i18n("The paper could not be fed, or it does not match the paper settings. Check the paper and the paper settings on the printer.");
        case "support.cover": return i18n("A cover or tray is in the wrong position. Close the cover, or open the output tray, as the printer shows.");
        case "support.paper-jam": return i18n("Paper is jammed. Remove it carefully; switch the printer off only if its display tells you to.");
        case "support.duplex-size": return i18n("This paper size cannot be printed on both sides. Print on one side or choose another size.");
        case "support.head-missing": return i18n("The print head is not recognized. Reseat it, then restart the printer.");
        case "support.tank-unrecognized": return i18n("An ink tank is not recognized. Press it in until it clicks.");
        case "support.tank-wrong-slot": return i18n("An ink tank is in the wrong slot. Match the tanks to the colour labels.");
        case "support.ink-empty": return i18n("An ink tank is empty. Replace it to keep printing.");
        case "support.tank-missing": return i18n("An ink tank is missing. Install it.");
        case "support.ink-level-unknown": return i18n("The level of an ink tank cannot be determined, or it may have run out. Check the tanks and the printer's display; printing may still be possible.");
        case "support.absorber-nearly-full": return i18n("The ink absorber is nearly full. Printing continues for now; plan to have the printer serviced.");
        case "support.head-protective-tape": return i18n("Packing material may still be on the print head holder. Remove it.");
        case "support.auto-align-failed": return i18n("Automatic print head alignment did not succeed. Run a nozzle check, clean the print head if needed, then try again.");
        case "support.align-sheet-unreadable": return i18n("The alignment sheet could not be read. Make sure it lies correctly on the glass, then try again.");
        case "support.print-data": return i18n("This job cannot be printed. Cancel it and try again.");
        case "support.hardware-error": return i18n("The printer reported a hardware error. Switch it off and on again; if the code comes back, contact the maker's support.");
        case "support.absorber-full": return i18n("The ink absorber has filled up. The printer needs service by the maker.");
        case "support.notice": return i18n("The printer has a notice for you. See its display, or use the help link for details.");
        default: return i18n("The printer reports support code %1. See the printer's display, or use the help link.", support.code);
        }
    }

    // The host's image work (cropping photos out of a scan, ...) reports in English. What a
    // person can act on is translated here; the rest keeps its technical detail after a
    // translated headline.
    function imagingNotice(message) {
        return () => {
            if (message === "no photo was found on the glass") {
                return i18n("No photo was found on the glass. Check that the photos lie face down on it.");
            }
            return i18n("The photos could not be cut out: %1", message);
        };
    }

    // Numbers and times as the language of the interface writes them (a binding that calls these
    // follows a switch of the language: Lang.locale is a property of the Lang singleton).
    function number(n) {
        return Number(n).toLocaleString(Lang.locale, "f", 0);
    }

    function clock(date) {
        return date.toLocaleTimeString(Lang.locale, "hh:mm:ss");
    }

    // A value as the printer reports it in a settings read-back.
    function settingValue(v) {
        const s = String(v);
        let m = /^ON (\d+) min$/.exec(s);
        if (m) {
            return i18np("On, after %1 minute", "On, after %1 minutes", parseInt(m[1]));
        }
        m = /^time (\d{4})-(\d{4})$/.exec(s);
        if (m) {
            return i18n("Scheduled %1 – %2", hhmm(m[1]), hhmm(m[2]));
        }
        switch (s) {
        case "ON": return i18n("On");
        case "OFF": return i18n("Off");
        case "auto": return i18n("Automatic");
        case "wide": return i18n("Wide");
        case "(not reported)": return i18n("(not reported)");
        default: return s;
        }
    }

    function settingLabel(field) {
        switch (field) {
        case "autopoweron": return i18n("Auto power on");
        case "autopoweroff": return i18n("Auto power off");
        case "autopoweroff_standalone": return i18n("Auto power off (not connected)");
        case "silentmode": return i18n("Quiet mode");
        case "papergap": return i18n("Paper gap");
        case "drylevel": return i18n("Ink drying time");
        case "auto_printer_rom_update": return i18n("Automatic firmware update");
        case "mqtt_connection": return i18n("Cloud link");
        default: return field;
        }
    }
}
