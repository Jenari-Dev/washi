import QtQuick
import QtQuick.Controls as QQC2

// Banner ink marker: coloured square with the tank code, and a mini level bar below it.
Item {
    id: chip

    property var ink: ({})
    readonly property color swatch: Fmt.inkColor(ink)
    readonly property bool dark: (swatch.r * 0.299 + swatch.g * 0.587 + swatch.b * 0.114) < 0.62
    readonly property bool problem: ink.status === "low" || ink.status === "empty"

    implicitWidth: 40
    implicitHeight: 42

    Rectangle {
        id: square
        width: parent.width
        height: 24
        radius: 4
        color: chip.swatch
        border.width: chip.dark ? 0 : 1
        border.color: "#C9B43A"
        Text {
            anchors.centerIn: parent
            text: chip.ink.color || ""
            font.pixelSize: 11
            font.weight: Font.Bold
            color: chip.dark ? "#FFFFFF" : "#3A3200"
        }
    }
    Rectangle {
        y: 30
        width: parent.width
        height: 6
        radius: 3
        color: "#FFFFFF"
        border.width: 1
        border.color: "#B8C6DD"
        Rectangle {
            x: 1
            y: 1
            height: parent.height - 2
            radius: 2
            width: Math.max(0, (parent.width - 2) * Math.min(100, ink.level || 0) / 100)
            color: chip.problem ? (chip.ink.status === "empty" ? Theme.danger : Theme.warn) : chip.swatch
        }
    }
    Glyph {
        visible: chip.problem
        x: chip.width - 11
        y: -6
        name: chip.ink.status === "empty" ? "error" : "warning"
        size: 16
        weight: 2
        color: chip.ink.status === "empty" ? Theme.danger : Theme.warn
    }

    HoverHandler { id: hover }
    QQC2.ToolTip.visible: hover.hovered
    QQC2.ToolTip.delay: 300
    QQC2.ToolTip.text: Fmt.inkName(chip.ink.color) + " – " + Fmt.percent(chip.ink.level)
}
