import QtQuick
import QtTest 1.2

// Home tiles: four per row, Customize hides and restores tiles.
Item {
    width: 900
    height: 600

    TileGrid {
        id: grid
        x: 24
        y: 24
        width: 700
    }

    TestCase {
        name: "HomeTiles"
        when: windowShown

        function init() {
            Prefs.hiddenTiles = "";
            grid.editing = false;
        }

        function test_default_tiles() {
            compare(grid.shown.length, 8);
            const ids = grid.shown.map(t => t.id);
            verify(ids.indexOf("document-print") >= 0 && ids.indexOf("remote-ui") >= 0 && ids.indexOf("manual") >= 0);
        }

        function test_hide_and_restore() {
            Prefs.setHidden("disc-label", true);
            compare(grid.shown.length, 7);
            grid.editing = true;
            compare(grid.shown.length, 8);       // hidden tiles show (dimmed) while customizing
            grid.editing = false;
            Prefs.setHidden("disc-label", false);
            compare(grid.shown.length, 8);
        }

        function test_functions_page_shows_everything() {
            Prefs.setHidden("scan", true);
            grid.all = true;
            verify(grid.shown.length >= 9);
            grid.all = false;
            Prefs.setHidden("scan", false);
        }
    }
}
