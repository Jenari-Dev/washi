import QtQuick
import QtQuick.Shapes

// Line drawings (our own) for the three guidance steps of the Scan screen:
// 1 lay the original face down on the open glass, 2 line its corner up with the mark,
// 3 close the cover. `photo` draws a picture instead of a page of text.
Item {
    id: d

    property int step: 1
    property bool photo: false

    implicitWidth: 150
    implicitHeight: 92

    readonly property color line: Theme.navy
    readonly property color glass: "#DDE8F7"
    readonly property color paper: "#FFFFFF"

    // The scanner body (seen from above and in front)
    Rectangle {
        id: body
        x: 12
        y: 34
        width: 126
        height: 52
        radius: 5
        color: "#EEF3FB"
        border.width: 2
        border.color: d.line
    }
    // The glass
    Rectangle {
        id: glassPane
        x: 22
        y: 40
        width: 106
        height: 40
        radius: 2
        color: d.glass
        border.width: 1.5
        border.color: d.line
    }

    // Step 1: the cover stands open behind the glass; the original is lowered face down.
    Shape {
        visible: d.step === 1
        anchors.fill: parent
        ShapePath {
            strokeColor: d.line
            strokeWidth: 2
            fillColor: "#F4F7FC"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M16 36 L28 6 L134 6 L134 14 L124 36 Z" }
        }
    }
    // The original: a page (lines) or a photo (frame with a hill).
    Item {
        id: original
        visible: d.step !== 3
        x: d.step === 2 ? 26 : 50
        y: d.step === 2 ? 44 : 38
        width: 38
        height: 32
        rotation: d.step === 1 ? -6 : 0
        Rectangle {
            anchors.fill: parent
            color: d.paper
            border.width: 1.5
            border.color: d.line
        }
        // Text lines (dashed: they are on the underside) or the picture
        Column {
            visible: !d.photo
            x: 6
            y: 6
            spacing: 5
            Repeater {
                model: 4
                Rectangle {
                    width: 26 - (index === 3 ? 10 : 0)
                    height: 2
                    color: "#9FB3D6"
                }
            }
        }
        Item {
            visible: d.photo
            x: 5
            y: 5
            width: parent.width - 10
            height: parent.height - 10
            clip: true
            Rectangle {
                anchors.fill: parent
                color: "#CFE3F8"
            }
            Rectangle {
                x: -6
                y: parent.height * 0.55
                width: parent.width * 0.9
                height: parent.height
                radius: width / 2
                color: "#8FBF7F"
            }
            Rectangle {
                x: parent.width * 0.68
                y: 3
                width: 7
                height: 7
                radius: 4
                color: "#F2CB1D"
            }
        }
    }
    // Step 1: an arrow showing the original going face down onto the glass.
    Shape {
        visible: d.step === 1
        x: 100
        y: 14
        ShapePath {
            strokeColor: Theme.accent
            strokeWidth: 2.2
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M12 0 L12 22 M5 15 L12 23 L19 15" }
        }
    }
    // Step 2: the mark on the glass edge (a corner bracket with a red arrow) and an arrow to it.
    Shape {
        visible: d.step === 2
        anchors.fill: parent
        ShapePath {
            strokeColor: Theme.danger
            strokeWidth: 2.2
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M22 40 L22 52 M22 40 L34 40" }
        }
        ShapePath {
            strokeColor: Theme.accent
            strokeWidth: 2.2
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M62 22 L38 36 M38 28 L38 37 L47 36" }
        }
    }
    // Step 3: the cover is closed over the glass; a press arrow.
    Rectangle {
        visible: d.step === 3
        x: 20
        y: 38
        width: 110
        height: 44
        radius: 3
        color: "#C6D5EE"
        border.width: 2
        border.color: d.line
        Rectangle {
            x: 8
            y: 6
            width: 94
            height: 3
            radius: 1
            color: "#A9BCDD"
        }
    }
    Shape {
        visible: d.step === 3
        x: 62
        y: 4
        ShapePath {
            strokeColor: Theme.accent
            strokeWidth: 2.2
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M12 0 L12 24 M5 17 L12 25 L19 17" }
        }
    }
}
