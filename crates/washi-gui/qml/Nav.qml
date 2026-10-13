pragma Singleton

import QtQuick

// Page navigation with a back stack.
QtObject {
    id: nav

    property string page: "home"
    property var history: []
    readonly property bool canGoBack: history.length > 0
    readonly property var managementPages: ["consumables", "status", "device-info", "utilities"]
    readonly property bool inManagement: managementPages.indexOf(page) >= 0

    // Open a page and remember where we came from.
    function go(id) {
        if (id === page) {
            return;
        }
        history = history.concat([page]);
        page = id;
    }

    // Switch without adding a history entry (tabs inside Printer Management).
    function replace(id) {
        page = id;
    }

    function back() {
        if (history.length === 0) {
            return;
        }
        const h = history.slice();
        page = h.pop();
        history = h;
    }

    // Ask Main.qml to open a dialog: "discover", "order-ink", "ink-models", "license", "maintenance",
    // "manual-align", "setting", "open-link", "firmware". `args` carries the details.
    signal dialogRequested(string name, var args)

    function dialog(name, args) {
        dialogRequested(name, args || ({}));
    }

    // Ask first, then open a web page of the printer or of the maker: `kind` is ink, help, error
    // (help for the current support code, or `args.code`), support, printer or maintenance.
    function openLink(kind, args) {
        dialogRequested("open-link", Object.assign({ kind: kind }, args || {}));
    }

    // Hand an address to the desktop (the web browser). Callers have asked the person first.
    // Headless checks set `openBrowser` to false: the address is announced, no browser starts.
    signal urlOpened(string url)
    property bool openBrowser: true

    function openUrl(url) {
        urlOpened(url);
        if (openBrowser) {
            Qt.openUrlExternally(url);
        }
    }
}
