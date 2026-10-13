import QtQuick
import QtQuick.Pdf

// A sheet of a PDF document: the selected pages of the document drawn on the paper, the way
// the Page Layout (1 to 16 pages per sheet) will place them. Pages come from QtPdf.
PhotoSheet {
    id: sheet

    property url source
    property alias document: pdf
    readonly property int pageCount: pdf.pageCount
    property var pages: []              // 1-based pages in print order
    property int sheetIndex: 0
    property int layout: 1
    readonly property bool portraitSheet: aspect < 1
    readonly property int sheets: Math.max(1, Math.ceil(pages.length / layout))
    readonly property bool firstLandscape: pdf.pageCount > 0 && pdf.pagePointSize(0).width > pdf.pagePointSize(0).height
    readonly property bool failed: pdf.status === PdfDocument.Error
    readonly property bool ready: pdf.status === PdfDocument.Ready

    PdfDocument {
        id: pdf
        source: sheet.source
    }

    // Cells like washi_core::print::layout::cells: a gutter between and around the pages when
    // several share the sheet.
    readonly property var cols: layout === 1 ? 1 : (layout === 2 ? (portraitSheet ? 1 : 2) : (layout === 4 ? 2 : (layout === 6 ? (portraitSheet ? 2 : 3) : (layout === 9 ? 3 : 4))))
    readonly property int rows: Math.round(layout / cols)

    Repeater {
        model: sheet.ready ? sheet.layout : 0
        delegate: Item {
            id: cell
            required property int index
            readonly property real gutter: sheet.layout === 1 ? 0 : Math.min(sheet.paper.width, sheet.paper.height) / 60
            readonly property real cw: (sheet.paper.width - gutter * (sheet.cols + 1)) / sheet.cols
            readonly property real ch: (sheet.paper.height - gutter * (sheet.rows + 1)) / sheet.rows
            readonly property int pos: sheet.sheetIndex * sheet.layout + index
            readonly property int pageNo: pos < sheet.pages.length ? sheet.pages[pos] : 0
            x: gutter + (index % sheet.cols) * (cw + gutter)
            y: gutter + Math.floor(index / sheet.cols) * (ch + gutter)
            width: cw
            height: ch
            visible: pageNo > 0
            PdfPageImage {
                anchors.fill: parent
                document: pdf
                currentFrame: Math.max(0, cell.pageNo - 1)
                fillMode: Image.PreserveAspectFit
                sourceSize.width: Math.max(64, cell.cw * 2)
                asynchronous: true
                // A hairline shows where the page ends when it is smaller than the cell.
            }
        }
    }
}
