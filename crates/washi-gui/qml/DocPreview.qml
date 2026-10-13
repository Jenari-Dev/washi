import QtQuick

// The sheet of a PDF in Document Print: QtPdf (DocSheet) when that Qt module is installed, else
// poppler through the host (RasterSheet). Both are loaded by name at run time, so a system
// without QtPdf still starts the app.
Item {
    id: preview

    property url source
    property var pages: []
    property int sheetIndex: 0
    property int layout: 1
    property real aspect: 0.707

    readonly property int pageCount: sheet.item ? sheet.item.pageCount : 0
    readonly property bool firstLandscape: sheet.item ? sheet.item.firstLandscape : false
    readonly property bool failed: sheet.item ? sheet.item.failed : false
    property bool usingFallback: false

    Loader {
        id: sheet
        anchors.fill: parent
        source: preview.usingFallback ? "RasterSheet.qml" : "DocSheet.qml"
        onStatusChanged: {
            if (status === Loader.Error && !preview.usingFallback) {
                console.warn("QtPdf is not available; previewing documents with poppler instead");
                preview.usingFallback = true;
            }
        }
    }
    Binding { target: sheet.item; property: "source"; value: preview.source; when: sheet.item !== null }
    Binding { target: sheet.item; property: "pages"; value: preview.pages; when: sheet.item !== null }
    Binding { target: sheet.item; property: "sheetIndex"; value: preview.sheetIndex; when: sheet.item !== null }
    Binding { target: sheet.item; property: "layout"; value: preview.layout; when: sheet.item !== null }
    Binding { target: sheet.item; property: "aspect"; value: preview.aspect; when: sheet.item !== null }
}
