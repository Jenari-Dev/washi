import QtQuick

// The sheet of paper in the print previews: white on the grey area, with a thin outline and a
// soft shadow, sized to fit the free space at the sheet's aspect ratio (width / height).
Item {
    id: sheet

    property real aspect: 0.707
    property string imagePath: ""       // a rendered sheet (PNG); "" shows the empty paper
    property bool busy: false
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
            anchors.fill: parent
            visible: sheet.imagePath !== ""
            source: sheet.imagePath !== "" ? "file://" + sheet.imagePath : ""
            fillMode: Image.PreserveAspectFit
            asynchronous: true
            cache: false
            smooth: true
        }
    }
    Spinner {
        visible: sheet.busy
        anchors.centerIn: parent
        size: 28
    }
}
