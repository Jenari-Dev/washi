import QtQuick
import QtQuick.Shapes

// Washi's own line drawing of an inkjet printer (used when no picture pack is installed).
Item {
    id: art

    property color stroke: Theme.navy
    property color body: "#FFFFFF"
    property color lid: "#E3EDFA"
    property real weight: 2

    implicitWidth: 220
    implicitHeight: 140

    Shape {
        width: 220
        height: 140
        scale: Math.min(art.width / 220, art.height / 140)
        transformOrigin: Item.TopLeft
        x: (art.width - 220 * scale) / 2
        y: (art.height - 140 * scale) / 2
        preferredRendererType: Shape.CurveRenderer

        // Paper standing in the rear tray
        ShapePath {
            strokeColor: art.stroke
            strokeWidth: art.weight
            fillColor: "#FFFFFF"
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M70 10 L150 10 L157 50 L63 50 Z" }
        }
        ShapePath {
            strokeColor: "#9FB3D6"
            strokeWidth: 1.5
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathSvg { path: "M80 20 H140 M82 28 H138 M84 36 H136" }
        }
        // Body
        ShapePath {
            strokeColor: art.stroke
            strokeWidth: art.weight
            fillColor: art.body
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M32 48 H188 A12 12 0 0 1 200 60 V106 A8 8 0 0 1 192 114 H28 A8 8 0 0 1 20 106 V60 A12 12 0 0 1 32 48 Z" }
        }
        // Lid / scanner cover
        ShapePath {
            strokeColor: art.stroke
            strokeWidth: art.weight
            fillColor: art.lid
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M32 48 H188 A12 12 0 0 1 200 60 V72 H20 V60 A12 12 0 0 1 32 48 Z" }
        }
        // Control panel
        ShapePath {
            strokeColor: art.stroke
            strokeWidth: 1.6
            fillColor: "#FFFFFF"
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M142 54 H186 A3 3 0 0 1 189 57 V65 A3 3 0 0 1 186 68 H142 A3 3 0 0 1 139 65 V57 A3 3 0 0 1 142 54 Z" }
        }
        ShapePath {
            strokeColor: "transparent"
            strokeWidth: -1
            fillColor: Theme.accent
            PathSvg { path: "M149 61 a2.2 2.2 0 1 0 4.4 0 a2.2 2.2 0 1 0 -4.4 0 Z" }
        }
        ShapePath {
            strokeColor: "transparent"
            strokeWidth: -1
            fillColor: Theme.ok
            PathSvg { path: "M175 61 a2.2 2.2 0 1 0 4.4 0 a2.2 2.2 0 1 0 -4.4 0 Z" }
        }
        // Output slot
        ShapePath {
            strokeColor: art.stroke
            strokeWidth: 1.8
            fillColor: "#C9D6EC"
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M44 86 H176 A4 4 0 0 1 180 90 V94 H40 V90 A4 4 0 0 1 44 86 Z" }
        }
        // Printed sheet coming out
        ShapePath {
            strokeColor: art.stroke
            strokeWidth: art.weight
            fillColor: "#FFFFFF"
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: "M52 94 H168 L176 126 H44 Z" }
        }
        ShapePath {
            strokeColor: "#9FB3D6"
            strokeWidth: 1.5
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathSvg { path: "M64 104 H156 M62 112 H158" }
        }
        // Feet
        ShapePath {
            strokeColor: art.stroke
            strokeWidth: art.weight
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathSvg { path: "M34 114 V120 M186 114 V120" }
        }
    }
}
