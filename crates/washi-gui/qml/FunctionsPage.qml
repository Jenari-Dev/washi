import QtQuick
import QtQuick.Layouts

PageScroll {
    WText {
        kind: "title"
        text: i18n("Functions")
    }
    WText {
        kind: "dim"
        text: i18n("Everything Washi can do with your printer.")
    }
    TileGrid {
        all: true
    }
}
