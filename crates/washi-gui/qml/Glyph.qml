import QtQuick
import QtQuick.Shapes

// A monochrome line icon from the Icons singleton.
Item {
    id: g

    property string name: ""
    property color color: Theme.navy
    property real size: 24
    property real weight: 1.7

    implicitWidth: size
    implicitHeight: size
    width: size
    height: size

    // Shapes are not cut off at the edge of a Flickable or any other clipping parent (they would
    // draw over whatever lies beyond a scrolled view). Drawn into a small texture of its own, an
    // icon is a plain quad of the scene graph and is clipped like everything else.
    layer.enabled: true
    layer.samples: 4
    layer.smooth: true

    Shape {
        width: 24
        height: 24
        scale: g.size / 24
        transformOrigin: Item.TopLeft
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: g.color
            strokeWidth: g.weight
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: Icons.stroke(g.name) }
        }
        ShapePath {
            strokeColor: "transparent"
            strokeWidth: -1
            fillColor: g.color
            PathSvg { path: Icons.fill(g.name) }
        }
    }
}
