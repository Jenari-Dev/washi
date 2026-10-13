import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Scrolling page body: children are laid out in a column with the page margins.
Flickable {
    id: flick

    default property alias content: col.data
    property int margin: Theme.pageMargin
    property alias spacing: col.spacing

    contentWidth: width
    contentHeight: col.implicitHeight + 2 * margin
    clip: true
    boundsBehavior: Flickable.StopAtBounds

    QQC2.ScrollBar.vertical: QQC2.ScrollBar {
        policy: flick.contentHeight > flick.height ? QQC2.ScrollBar.AlwaysOn : QQC2.ScrollBar.AlwaysOff
        contentItem: Rectangle {
            implicitWidth: 6
            radius: 3
            color: "#B0B0B0"
            opacity: parent.pressed ? 1 : 0.7
        }
        background: Item {}
    }

    ColumnLayout {
        id: col
        x: flick.margin
        y: flick.margin
        width: flick.width - 2 * flick.margin
        spacing: 16
    }
}
