pragma Singleton

import QtQuick
import QtCore

// Per-user UI preferences, kept in ~/.config/washi/gui.ini.
Settings {
    location: StandardPaths.writableLocation(StandardPaths.ConfigLocation) + "/washi/gui.ini"
    category: "ui"

    // Comma-separated ids of Home tiles the user removed with "Customize".
    property string hiddenTiles: ""
    property bool navExpanded: false
    property bool autoDetect: true
    // "Do not show again" of the load-paper reminder before printing.
    property bool hideLoadPaperHint: false
    // Folders the file dialogs open in next time (file:// URLs; "" = the default).
    property string photoFolder: ""
    property string documentFolder: ""
    property string scanFolder: ""

    function isHidden(id) {
        return hiddenTiles.split(",").indexOf(id) >= 0;
    }

    function setHidden(id, hidden) {
        const list = hiddenTiles.split(",").filter(x => x !== "" && x !== id);
        if (hidden) {
            list.push(id);
        }
        hiddenTiles = list.join(",");
    }
}
