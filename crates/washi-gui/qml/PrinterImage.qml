import QtQuick

// The picture of a printer, in this order: a local picture pack image
// (~/.local/share/washi/printers/<model>.png), the printer's own picture (read from the printer
// by the service, see Store.printerIcon), Washi's own line drawing. Whichever is there is fitted
// into the item without changing its proportions.
Item {
    id: pic

    property string model: ""
    // The printer's own picture, a file URL ("" = none).
    property string iconSource: Store.printerIcon
    // Draw Washi's own printer when there is no picture; a list that has a glyph of its own
    // turns this off and looks at `hasPicture`.
    property bool art: true

    readonly property string packSource: {
        const t = Store.transport;
        return t && t.packImage && model !== "" ? t.packImage(model) : "";
    }
    readonly property bool hasPicture: pack.status === Image.Ready || own.status === Image.Ready
    // A picture that is on its way is waited for rather than shown after the drawing.
    readonly property bool loading: pack.status === Image.Loading || own.status === Image.Loading

    implicitWidth: 160
    implicitHeight: 100

    Image {
        id: pack
        objectName: "packPicture"
        anchors.fill: parent
        source: pic.packSource
        visible: status === Image.Ready
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        smooth: true
        mipmap: true
    }
    Image {
        id: own
        objectName: "ownPicture"
        anchors.fill: parent
        source: pic.iconSource
        visible: pack.status !== Image.Ready && status === Image.Ready
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        smooth: true
        mipmap: true
    }
    PrinterArt {
        objectName: "printerArt"
        anchors.fill: parent
        visible: pic.art && !pic.hasPicture && !pic.loading
    }
}
