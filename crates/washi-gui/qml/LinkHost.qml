import QtQuick
import QtQuick.Layouts

// Every link that leaves the app goes through this question first. It asks the service for the
// addresses the printer itself offers (ink shop, manual, help for a support code, the printer's
// own web page), says whose page it is and where it is, and opens it in the web browser only
// after "Open". Nothing is fetched or sent to the web here: the address is only shown.
Item {
    id: host

    // ink | help | error | support | printer | maintenance
    property string kind: "help"
    property string code: ""
    property var link: null             // {url, source: "maker" | "printer"}
    property bool loading: false

    readonly property bool fromMaker: !!link && link.source === "maker"
    // The host name of the address, for the question ("rs.example.net").
    readonly property string where: link ? hostOf(link.url) : ""
    // The page was asked for as the printer's own, or the printer had no address of its own for it.
    readonly property bool asked: kind === "printer" || kind === "maintenance"

    width: 0
    height: 0

    function hostOf(url) {
        const m = /^https?:\/\/([^\/?#]+)/i.exec(url || "");
        return m ? m[1] : (url || "");
    }

    function titleFor(k) {
        switch (k) {
        case "ink": return i18n("Order Ink");
        case "help": return i18n("Manual");
        case "error": return host.code !== "" ? i18n("Help for Support Code %1", host.code) : i18n("Help");
        case "support": return i18n("Canon Support");
        case "printer": return i18n("Printer Web Page");
        case "maintenance": return i18n("Maintenance on the Printer");
        default: return i18n("Web Page");
        }
    }

    function pick(links, k) {
        switch (k) {
        case "ink": return links.ink;
        case "help": return links.help;
        case "error": return links.error || links.printer_page;
        case "support": return links.support;
        case "maintenance": return links.maintenance_page;
        default: return links.printer_page;
        }
    }

    // Ask the service for the address, then the question. Without an answer (the service is
    // gone), the printer's own page is the fallback: it is the one address that needs no lookup.
    function open(args) {
        kind = args.kind || "help";
        code = kind === "error" ? (args.code !== undefined ? String(args.code) : (Store.support ? String(Store.support.code) : "")) : "";
        link = null;
        loading = true;
        dlg.open();
        const k = kind;
        Store.getWebLinks(code, (ok, links) => {
            if (host.kind !== k || !dlg.visible) {
                return;
            }
            loading = false;
            link = ok && links ? pick(links, k) : { url: "http://" + Store.activeHost + "/", source: "printer" };
        });
    }

    // The question is answered with "Open": hand the address to the desktop.
    function accept() {
        if (!link) {
            return;
        }
        const url = link.url;
        dlg.close();
        Nav.openUrl(url);
    }

    function tankModels() {
        return Store.inks.map(i => Fmt.tankModel(i.model)).filter(m => m !== "").join(", ") || i18n("the tank models listed under Consumables");
    }

    WDialog {
        id: dlg
        objectName: "linkDialog"
        dialogWidth: 500
        title: host.titleFor(host.kind)

        Spinner {
            visible: host.loading
            Layout.alignment: Qt.AlignHCenter
            size: 32
            running: host.loading
        }

        ColumnLayout {
            visible: !host.loading && host.link !== null
            Layout.fillWidth: true
            spacing: 10

            WText {
                objectName: "linkQuestion"
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: host.fromMaker
                    ? i18n("This opens a Canon web page in your web browser (%1). The address names your printer model; Washi sends nothing else.", host.where)
                    : (host.asked
                        ? i18n("This opens the printer's own web page in your web browser (%1).", host.where)
                        : i18n("The printer gave no address of its own for this, so this opens the printer's own web page in your web browser (%1).", host.where))
            }
            WText {
                objectName: "linkInkHint"
                visible: host.kind === "ink" && host.fromMaker
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: i18n("Look for: %1.", host.tankModels())
            }
            WText {
                objectName: "linkPasswordHint"
                visible: host.kind === "maintenance"
                Layout.fillWidth: true
                kind: "dim"
                wrapMode: Text.WordWrap
                text: i18n("The printer asks for its administrator password there. Washi does not see or store it.")
            }
        }

        buttons: [
            WButton {
                text: i18n("Cancel")
                onClicked: dlg.close()
            },
            WButton {
                objectName: "acceptButton"
                kind: "primary"
                glyph: "external"
                enabled: !host.loading && host.link !== null
                text: host.fromMaker ? i18n("Open Web Page") : i18n("Open Printer Page")
                onClicked: host.accept()
            }
        ]
    }
}
