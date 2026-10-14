import QtQuick
import QtQuick.Shapes

// The sheet of paper in the print previews: white on the grey area, with a thin outline and a
// soft shadow, sized to fit the free space at the sheet's aspect ratio (width / height). When the
// printer's unprintable edge is known (`edge`, in mm: PrintModel.unprintable) a dashed frame shows
// where printing stops; `sheetMm` is the sheet's size in mm, to put it in the right place.
Item {
    id: sheet

    property real aspect: 0.707
    property string imagePath: ""       // a rendered sheet (PNG); "" shows the empty paper
    property bool busy: false
    property var edge: null             // {top, right, bottom, left} in mm, or null: no frame
    // The picture is a whole sheet that the printer is asked to fit into what it can print (photos
    // with margins): drawn inside the frame, scaled the same way.
    property bool fitToEdge: false
    property var sheetMm: ({ w: 210, h: 297 })
    default property alias content: paper.data
    readonly property alias paper: paper

    Rectangle {
        id: paperShadow
        x: paper.x + 2
        y: paper.y + 4
        width: paper.width
        height: paper.height
        color: "#22000000"
        radius: 2
    }
    Rectangle {
        id: paper
        readonly property real fitW: Math.max(40, Math.min(sheet.width - 8, (sheet.height - 8) * sheet.aspect))
        width: fitW
        height: fitW / sheet.aspect
        anchors.centerIn: parent
        color: "#FFFFFF"
        border.width: 1
        border.color: "#C8C8C8"
        clip: true

        Image {
            readonly property bool inFrame: sheet.fitToEdge && frame.visible
            x: inFrame ? frame.x0 : 0
            y: inFrame ? frame.y0 : 0
            width: inFrame ? frame.x1 - frame.x0 : parent.width
            height: inFrame ? frame.y1 - frame.y0 : parent.height
            visible: sheet.imagePath !== ""
            source: sheet.imagePath !== "" ? "file://" + sheet.imagePath : ""
            fillMode: Image.PreserveAspectFit
            asynchronous: true
            cache: false
            smooth: true
        }
    }
    // The edge the printer cannot print on, over whatever is on the paper.
    Shape {
        id: frame
        objectName: "unprintableFrame"
        visible: sheet.edge !== null
        x: paper.x
        y: paper.y
        width: paper.width
        height: paper.height
        z: 10
        readonly property real x0: sheet.edge ? sheet.edge.left / sheet.sheetMm.w * width : 0
        readonly property real x1: sheet.edge ? width - sheet.edge.right / sheet.sheetMm.w * width : 0
        readonly property real y0: sheet.edge ? sheet.edge.top / sheet.sheetMm.h * height : 0
        readonly property real y1: sheet.edge ? height - sheet.edge.bottom / sheet.sheetMm.h * height : 0
        ShapePath {
            strokeColor: "#8896B3"
            strokeWidth: 1
            strokeStyle: ShapePath.DashLine
            dashPattern: [4, 3]
            fillColor: "transparent"
            startX: frame.x0
            startY: frame.y0
            PathLine { x: frame.x1; y: frame.y0 }
            PathLine { x: frame.x1; y: frame.y1 }
            PathLine { x: frame.x0; y: frame.y1 }
            PathLine { x: frame.x0; y: frame.y0 }
        }
    }
    Spinner {
        visible: sheet.busy
        anchors.centerIn: parent
        size: 28
    }
}
