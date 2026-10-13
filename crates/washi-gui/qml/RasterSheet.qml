import QtQuick

// The fallback of DocSheet for systems without QtPdf: the page is rasterised by poppler through
// the host (Imaging.pdfPage) and shown on the sheet. One page per sheet.
PhotoSheet {
    id: sheet

    property url source
    property var pages: []              // 1-based pages in print order
    property int sheetIndex: 0
    property int layout: 1
    property int pageCount: 0
    property bool firstLandscape: false
    readonly property bool failed: error !== ""
    property string error: ""
    property int _token: 0

    function pageNo() {
        const i = sheetIndex * layout;
        return i < pages.length ? pages[i] : 0;
    }

    function refresh() {
        const n = pageNo();
        if (!Img.engine || n <= 0) {
            imagePath = "";
            return;
        }
        const t = ++_token;
        busy = true;
        Img.wait(Img.engine.pdfPage(Img.localPath(source), n, 900), (ok, r) => {
            if (t !== _token) {
                return;
            }
            busy = false;
            imagePath = ok ? r.path : "";
            error = ok ? "" : (r.message || "");
            if (ok && n === 1) {
                firstLandscape = r.width > r.height;
            }
        });
    }

    onSourceChanged: {
        pageCount = Img.engine ? Img.engine.pdfPages(Img.localPath(source)) : 0;
        refresh();
    }
    onPagesChanged: refresh()
    onSheetIndexChanged: refresh()
    onLayoutChanged: refresh()
}
