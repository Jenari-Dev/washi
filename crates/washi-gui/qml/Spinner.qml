import QtQuick

// A ring of dots that turns while something is going on.
Item {
    id: spin

    property real size: 40
    property color color: Theme.accent
    property bool running: true

    implicitWidth: size
    implicitHeight: size

    Item {
        anchors.fill: parent
        RotationAnimator on rotation {
            from: 0
            to: 360
            duration: 1100
            loops: Animation.Infinite
            running: spin.running && spin.visible
        }
        Repeater {
            model: 8
            delegate: Rectangle {
                required property int index
                width: spin.size * 0.15
                height: width
                radius: width / 2
                color: spin.color
                opacity: 0.15 + 0.85 * (index / 7)
                x: spin.size / 2 - width / 2 + Math.cos(index * Math.PI / 4) * (spin.size / 2 - width / 2)
                y: spin.size / 2 - height / 2 + Math.sin(index * Math.PI / 4) * (spin.size / 2 - height / 2)
            }
        }
    }
}
