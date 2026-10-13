import QtQuick
import QtTest 1.2

// Printer Management: the pivot switches tabs, keeps the left navigation in step, and does not
// pile up history entries.
Item {
    width: 1000
    height: 760

    FakeTransport {
        id: fake
    }

    ManagementPage {
        id: page
        anchors.fill: parent
    }

    TestCase {
        name: "ManagementTabs"
        when: windowShown

        function init() {
            Store.reset();
            Store.transport = null;
            Nav.page = "consumables";
            Nav.history = [];
            fake.clearCalls();
            fake.connected = false;
            Store.transport = fake;
            fake.connected = true;      // bootstrap: ListPrinters, then GetSnapshot
            tryVerify(() => Store.snapLoaded);
        }

        function test_loads_data() {
            compare(Store.activeHost, "printer.test");
            compare(Store.power, "on");
            compare(Store.displayName, "Canon XK140 series");
            compare(Store.statusKind, "online");
        }

        function test_tabs_switch() {
            compare(page.tabIndex, 0);
            mouseClick(findChild(page, "tab-status"));
            compare(Nav.page, "status");
            compare(page.tabIndex, 1);
            tryVerify(() => findChild(page, "tab-status") !== null);
            mouseClick(findChild(page, "tab-device-info"));
            compare(page.tabIndex, 2);
            mouseClick(findChild(page, "tab-utilities"));
            compare(page.tabIndex, 3);
            mouseClick(findChild(page, "tab-consumables"));
            compare(page.tabIndex, 0);
            compare(Nav.page, "consumables");
        }

        function test_tabs_do_not_add_history() {
            Nav.go("home");
            Nav.go("consumables");
            const depth = Nav.history.length;
            mouseClick(findChild(page, "tab-utilities"));
            mouseClick(findChild(page, "tab-status"));
            compare(Nav.history.length, depth);
            Nav.back();
            compare(Nav.page, "home");
        }

        function test_utilities_tab_asks_the_service() {
            mouseClick(findChild(page, "tab-utilities"));
            tryVerify(() => fake.called("GetSettings").length > 0 && fake.called("GetMaintenanceCapability").length > 0);
            tryVerify(() => findChild(page, "card-cleaning") !== null);
            verify(findChild(page, "card-nozzle") !== null);
            verify(findChild(page, "card-quiet") !== null);
        }
    }
}
