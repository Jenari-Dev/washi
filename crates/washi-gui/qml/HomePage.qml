import QtQuick
import QtQuick.Layouts

ColumnLayout {
    spacing: 0

    PrinterBanner {
        Layout.fillWidth: true
    }
    PageScroll {
        Layout.fillWidth: true
        Layout.fillHeight: true
        TileGrid {}
    }
}
