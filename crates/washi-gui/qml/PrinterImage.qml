import QtQuick

// The printer picture: a local picture pack image when one is installed
// (~/.local/share/washi/printers/<model>.png), else Washi's own drawing.
Item {
    id: pic

    property string model: ""
    readonly property string packSource: {
        const t = Store.transport;
        return t && t.packImage && model !== "" ? t.packImage(model) : "";
    }

    implicitWidth: 160
    implicitHeight: 100

    Image {
        id: img
        anchors.fill: parent
        source: pic.packSource
        visible: status === Image.Ready
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        smooth: true
        mipmap: true
    }
    PrinterArt {
        anchors.fill: parent
        visible: img.status !== Image.Ready
    }
}
