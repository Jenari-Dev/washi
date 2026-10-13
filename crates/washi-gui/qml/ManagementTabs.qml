import QtQuick

// The four tabs of Printer Management, sitting in the bottom edge of the banner: a small coloured
// icon and a label each; the current tab is a plate in the colour of the page below, so that it
// joins the page, with a blue bar on top.
Item {
    id: strip

    // [{id, label, glyph, tint}]
    property var tabs: []
    property int current: 0

    signal selected(string id)

    implicitHeight: 44
    implicitWidth: row.implicitWidth

    Row {
        id: row
        height: strip.height
        spacing: 4
        Repeater {
            model: strip.tabs
            delegate: Item {
                id: pt
                required property var modelData
                required property int index
                objectName: "tab-" + modelData.id
                readonly property bool active: strip.current === index
                width: Math.max(124, content.implicitWidth + 40)
                height: row.height

                signal activated()
                onActivated: strip.selected(modelData.id)

                // The plate: rounded at the top, square at the bottom where it meets the page.
                Item {
                    anchors.fill: parent
                    anchors.topMargin: 4
                    visible: pt.active || area.containsMouse
                    Rectangle {
                        anchors.fill: parent
                        radius: 8
                        color: pt.active ? Theme.bg : "#66FFFFFF"
                    }
                    Rectangle {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        height: parent.height / 2
                        color: pt.active ? Theme.bg : "#66FFFFFF"
                    }
                }
                Rectangle {
                    visible: pt.active
                    anchors.top: parent.top
                    anchors.topMargin: 4
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: parent.width - 20
                    height: 3
                    radius: 1.5
                    color: Theme.accent
                }
                Row {
                    id: content
                    anchors.centerIn: parent
                    anchors.verticalCenterOffset: 2
                    spacing: 8
                    Glyph {
                        anchors.verticalCenter: parent.verticalCenter
                        name: pt.modelData.glyph
                        size: 18
                        weight: 1.9
                        color: pt.modelData.tint
                    }
                    WText {
                        anchors.verticalCenter: parent.verticalCenter
                        text: pt.modelData.label
                        font.weight: pt.active ? Font.DemiBold : Font.Normal
                        color: pt.active ? Theme.text : Theme.textDim
                    }
                }
                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    activeFocusOnTab: true
                    onClicked: pt.activated()
                    Keys.onReturnPressed: pt.activated()
                    Keys.onSpacePressed: pt.activated()
                }
            }
        }
    }
}
