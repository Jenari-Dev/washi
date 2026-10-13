import QtQuick
import QtQuick.Layouts

PageScroll {
    id: page

    readonly property var apps: [
        { title: i18n("Photo Print"), text: i18n("Print photos from Washi."), page: "photo-print", glyph: "photo" },
        { title: i18n("Document Print"), text: i18n("Print documents from Washi."), page: "document-print", glyph: "document" },
        { title: i18n("Scan"), text: i18n("Scan from the glass of your printer."), page: "scan", glyph: "scan" },
        { title: i18n("Simple Scan"), text: i18n("A free scanning app from the GNOME project."), url: "https://gitlab.gnome.org/GNOME/simple-scan", glyph: "scan" },
        { title: i18n("GIMP"), text: i18n("Free photo editor for retouching before you print."), url: "https://www.gimp.org/", glyph: "photo" },
        { title: i18n("Inkscape"), text: i18n("Free vector drawing, good for posters and labels."), url: "https://inkscape.org/", glyph: "star" },
        { title: i18n("Scribus"), text: i18n("Free page layout for flyers, booklets and posters."), url: "https://www.scribus.net/", glyph: "document" },
        { title: i18n("LibreOffice"), text: i18n("Free office suite that exports PDF for printing."), url: "https://www.libreoffice.org/", glyph: "document" }
    ]

    WText {
        kind: "title"
        text: i18n("Recommended Apps")
    }
    WText {
        Layout.fillWidth: true
        kind: "dim"
        wrapMode: Text.WordWrap
        text: i18n("Washi's own pages, and free programs that work well with your printer. Links open in your web browser.")
    }
    GridLayout {
        Layout.fillWidth: true
        columns: 2
        rowSpacing: 12
        columnSpacing: 12
        uniformCellWidths: true
        Repeater {
            model: page.apps
            delegate: WCard {
                required property var modelData
                Layout.fillWidth: true
                implicitHeight: 92
                RowLayout {
                    anchors.fill: parent
                    anchors.margins: 16
                    spacing: 14
                    Glyph {
                        name: modelData.glyph
                        size: 32
                        weight: 1.5
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        WText {
                            text: modelData.title
                            font.weight: Font.DemiBold
                        }
                        WText {
                            Layout.fillWidth: true
                            kind: "dim"
                            wrapMode: Text.WordWrap
                            text: modelData.text
                        }
                    }
                    WButton {
                        glyph: modelData.url ? "external" : ""
                        text: modelData.url ? i18n("Website") : i18n("Open")
                        onClicked: modelData.url ? Nav.openUrl(modelData.url) : Nav.go(modelData.page)
                    }
                }
            }
        }
    }
}
