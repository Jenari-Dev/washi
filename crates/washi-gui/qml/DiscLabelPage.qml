import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import QtCore

// Disc Label: design a label for a printable CD or DVD (picture, texts, colour) on the printable
// ring and print it through the disc tray. Designs are saved as JSON in
// ~/.local/share/washi/discs/. The look follows the rest of the app; the designer itself is
// Washi's own.
RowLayout {
    id: page

    spacing: 0

    property var design: DiscMath.newDesign()
    property int selected: -2           // -2 nothing, -1 the picture, 0.. a text
    // What the page says about saving and opening designs: a message source (see Fmt.text()).
    property var noticeSource: ""
    readonly property string notice: Fmt.text(noticeSource)

    readonly property string designsDir: Img.engine ? Img.engine.dataDir() + "/discs" : ""
    readonly property bool hasContent: !!design.background || design.texts.some(t => t.text.trim() !== "")
    readonly property bool canPrint: Store.canJob && hasContent
    readonly property var sel: selected >= 0 && selected < design.texts.length ? design.texts[selected] : null
    readonly property string blockedReason: {
        if (!Store.connected) return i18n("The Washi service is not running.");
        if (!Store.hasPrinter) return i18n("Add a printer first.");
        if (Store.readOnly) return i18n("Read-only mode: the service will not print.");
        if (Store.power === "offline") return i18n("The printer cannot be reached.");
        if (Store.busy) return i18n("The printer is busy: %1", Store.statusTip);
        if (!hasContent) return i18n("Add a picture or some text first.");
        return "";
    }

    function copy() {
        return JSON.parse(JSON.stringify(page.design));
    }

    function change(mutator) {
        const d = copy();
        mutator(d);
        design = d;
    }

    function addText(txt) {
        const d = copy();
        const t = DiscMath.newText(txt || i18n("Your text"));
        // The first text goes below the hub; the next ones stack above it.
        t.y_mm = d.texts.length === 0 ? 32 : (d.texts.length === 1 ? -32 : 0);
        d.texts.push(t);
        design = d;
        selected = d.texts.length - 1;
    }

    function removeSelected() {
        const d = copy();
        if (selected === -1) {
            d.background = null;
        } else if (selected >= 0) {
            d.texts.splice(selected, 1);
        }
        design = d;
        selected = -2;
    }

    // A picture dropped from a file manager becomes the background.
    function dropFiles(urls) {
        const paths = urls.map(u => Img.localPath(u)).filter(p => String(p).charAt(0) === "/" && PrintModel.kindOf(p) === "photo");
        if (paths.length === 0) {
            noticeSource = () => i18n("Only JPEG, PNG and BMP pictures can be used here.");
            return;
        }
        noticeSource = "";
        setPicture(paths[0]);
    }

    function setPicture(path) {
        change(d => {
            d.background = { path: path, fit: "cover", scale: 1, offset_mm: [0, 0], rotation: 0 };
        });
        selected = -1;
    }

    Component.onCompleted: {
        // The screenshot checks preload a design.
        const img = Store.env("WASHI_DISC_IMAGE");
        if (img !== "") {
            setPicture(img);
        }
        const txt = Store.env("WASHI_DISC_TEXT");
        if (txt !== "") {
            for (const line of txt.split("|")) {
                addText(line.replace(/\\n/g, "\n"));
            }
            // WASHI_DISC_RING=index:radius:start:cw|ccw:spacing;... lets texts follow the ring.
            const rings = Store.env("WASHI_DISC_RING");
            for (const r of rings === "" ? [] : rings.split(";")) {
                const f = r.split(":");
                const i = parseInt(f[0]);
                if (i >= 0 && i < design.texts.length) {
                    change(d => {
                        d.texts[i].ring = { radius_mm: parseFloat(f[1]), start_deg: parseFloat(f[2]), clockwise: f[3] !== "ccw", letter_spacing_pt: parseFloat(f[4] || "0") };
                        d.texts[i].size_pt = 20;
                    });
                }
            }
            const pick = Store.env("WASHI_DISC_SELECT");
            selected = pick === "picture" ? -1 : (pick === "none" ? -2 : 0);
        }
    }

    // ---- printing --------------------------------------------------------------------------------------
    function spec(quality) {
        return DiscMath.printSpec(design, fam => Img.engine ? Img.engine.fontFile(fam) : "", quality);
    }

    function askToPrint() {
        if (!canPrint) {
            return;
        }
        loadTray.openFor("disc", false, true);
    }

    // The headless screenshot checks open the dialogs through Nav (WASHI_DIALOG=nav-disc-tray).
    Connections {
        target: Nav
        function onDialogRequested(name, args) {
            if (name === "disc-tray") {
                loadTray.openFor("disc", false, true);
            } else if (name === "disc-confirm") {
                confirmDlg.open();
            } else if (name === "disc-start") {
                job.startDisc(page.spec("normal"), i18n("disc label"));
            } else if (name === "disc-preview") {
                page.showPrinted();
            } else if (name === "disc-save") {
                saveDlg.open();
            }
        }
    }

    LoadPaperDialog {
        id: loadTray
        onProceed: confirmDlg.open()
    }

    ConfirmDialog {
        id: confirmDlg
        objectName: "discConfirm"
        title: i18n("Print Disc Label")
        acceptText: i18n("Print")
        glyph: "printer"
        text: i18n("Print this label in %1? Disc type: %2. The disc tray must be loaded.", Store.displayName, DiscMath.mediaLabel(page.design.media))
        onAccepted: job.startDisc(page.spec("normal"), i18n("disc label"))
    }

    PrintJobDialog {
        id: job
        objectName: "discJob"
    }

    // ---- the picture ----------------------------------------------------------------------------------------
    FileDialog {
        id: picturePicker
        title: i18n("Select Picture")
        fileMode: FileDialog.OpenFile
        nameFilters: PrintModel.photoFilters
        currentFolder: Prefs.photoFolder || StandardPaths.writableLocation(StandardPaths.PicturesLocation)
        onAccepted: {
            Prefs.photoFolder = String(currentFolder);
            page.setPicture(Img.localPath(selectedFile));
        }
    }

    // ---- saved designs ----------------------------------------------------------------------------------------
    property var savedNames: []

    function refreshSaved() {
        savedNames = Img.engine ? (Img.parse(Img.engine.listFiles(designsDir, ".json")) || []) : [];
    }

    function saveDesign(name) {
        const file = DiscMath.fileName(name);
        if (file === "") {
            noticeSource = () => i18n("Give the design a name made of letters and digits.");
            return false;
        }
        const d = copy();
        d.name = name.trim();
        if (!Img.engine.writeText(designsDir + "/" + file, DiscMath.designJson(d))) {
            noticeSource = () => i18n("The design could not be saved.");
            return false;
        }
        design = d;
        noticeSource = () => i18n("Saved “%1”.", d.name);
        return true;
    }

    function loadDesign(file) {
        const d = DiscMath.parseDesign(Img.engine.readText(designsDir + "/" + file));
        if (!d) {
            noticeSource = () => i18n("That file is not a Washi disc design.");
            return false;
        }
        design = d;
        selected = -2;
        noticeSource = () => i18n("Opened “%1”.", d.name || file.replace(/\.json$/, ""));
        return true;
    }

    WDialog {
        id: saveDlg
        objectName: "saveDesignDialog"
        title: i18n("Save Design")
        onOpened: nameField.forceActiveFocus()
        WText {
            Layout.fillWidth: true
            kind: "dim"
            wrapMode: Text.WordWrap
            text: i18n("Designs are kept in %1.", page.designsDir)
        }
        WField {
            id: nameField
            objectName: "designName"
            Layout.fillWidth: true
            placeholderText: i18n("Name of the design")
            text: page.design.name
            onAccepted: saveBtn.clicked()
        }
        buttons: [
            WButton {
                text: i18n("Cancel")
                onClicked: saveDlg.close()
            },
            WButton {
                id: saveBtn
                objectName: "saveDesignButton"
                kind: "primary"
                enabled: DiscMath.fileName(nameField.text) !== ""
                text: i18n("Save")
                onClicked: {
                    if (page.saveDesign(nameField.text)) {
                        saveDlg.close();
                    }
                }
            }
        ]
    }

    WDialog {
        id: openDlg
        objectName: "openDesignDialog"
        title: i18n("Open Design")
        onAboutToShow: page.refreshSaved()
        WText {
            visible: page.savedNames.length === 0
            Layout.fillWidth: true
            kind: "dim"
            wrapMode: Text.WordWrap
            text: i18n("No saved designs yet. They appear here after you save one.")
        }
        Repeater {
            model: page.savedNames
            delegate: RowLayout {
                required property string modelData
                Layout.fillWidth: true
                spacing: 8
                WButton {
                    Layout.fillWidth: true
                    text: modelData.replace(/\.json$/, "")
                    onClicked: {
                        if (page.loadDesign(modelData)) {
                            openDlg.close();
                        }
                    }
                }
                WButton {
                    kind: "quiet"
                    glyph: "trash"
                    implicitWidth: 36
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: i18n("Delete this design")
                    onClicked: {
                        Img.engine.removeDataFile(page.designsDir + "/" + modelData);
                        page.refreshSaved();
                    }
                }
            }
        }
        buttons: [
            WButton {
                text: i18n("Close")
                onClicked: openDlg.close()
            }
        ]
    }

    // ---- "Preview as printed" -------------------------------------------------------------------------------
    property string printedPath: ""
    property bool printedBusy: false
    property var printedErrorSource: ""
    readonly property string printedError: Fmt.text(printedErrorSource)

    function showPrinted() {
        printedBusy = true;
        printedErrorSource = "";
        printedPath = "";
        previewDlg.open();
        Img.discPreview(spec("normal"), 720, (ok, r) => {
            printedBusy = false;
            if (ok) {
                printedPath = r.path;
            } else {
                printedErrorSource = () => i18n("The preview could not be made: %1", r.message);
            }
        });
    }

    WDialog {
        id: previewDlg
        objectName: "printedPreview"
        title: i18n("Preview as printed")
        dialogWidth: 520
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 440
            Image {
                anchors.centerIn: parent
                width: Math.min(parent.width, parent.height)
                height: width
                visible: page.printedPath !== ""
                source: page.printedPath !== "" ? "file://" + page.printedPath : ""
                fillMode: Image.PreserveAspectFit
                cache: false
                smooth: true
            }
            Spinner {
                visible: page.printedBusy
                anchors.centerIn: parent
                size: 36
            }
            WText {
                visible: page.printedError !== ""
                anchors.centerIn: parent
                width: parent.width - 20
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                color: Theme.danger
                text: page.printedError
            }
        }
        WText {
            Layout.fillWidth: true
            kind: "muted"
            wrapMode: Text.WordWrap
            text: i18n("This is the image that is sent to the printer, drawn the way the printer driver draws it. White stays unprinted.")
        }
        buttons: [
            WButton {
                kind: "primary"
                text: i18n("Close")
                onClicked: previewDlg.close()
            }
        ]
    }

    // ---- the canvas ------------------------------------------------------------------------------------------
    Rectangle {
        Layout.fillWidth: true
        Layout.fillHeight: true
        color: "#E6E6E6"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 10

            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                WText {
                    kind: "title"
                    text: i18n("Disc Label")
                }
                Item {
                    Layout.fillWidth: true
                }
                WButton {
                    objectName: "addTextButton"
                    glyph: "text"
                    text: i18n("Add Text")
                    onClicked: page.addText("")
                }
                WButton {
                    objectName: "addPictureButton"
                    glyph: "photo"
                    text: page.design.background ? i18n("Change Picture") : i18n("Add Picture")
                    onClicked: picturePicker.open()
                }
                WButton {
                    objectName: "previewButton"
                    enabled: page.hasContent
                    text: i18n("Preview as printed")
                    onClicked: page.showPrinted()
                }
            }

            DiscCanvas {
                id: discCanvas
                objectName: "discCanvas"
                Layout.fillWidth: true
                Layout.fillHeight: true
                design: page.design
                selected: page.selected
                onEdited: d => page.design = d
                onSelectionChanged2: i => page.selected = i

                DropArea {
                    id: discDrop
                    objectName: "dropArea"
                    anchors.fill: parent
                    onDropped: drop => {
                        if (drop.hasUrls) {
                            drop.acceptProposedAction();
                            page.dropFiles(drop.urls);
                        }
                    }
                }
                Rectangle {
                    objectName: "dropHighlight"
                    visible: discDrop.containsDrag
                    anchors.fill: parent
                    radius: 8
                    color: "#261354BF"
                    border.width: 2
                    border.color: Theme.accent
                    WText {
                        anchors.centerIn: parent
                        font.weight: Font.DemiBold
                        font.pixelSize: 18
                        color: Theme.accent
                        text: i18n("Drop a picture here")
                    }
                }
            }

            WText {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                kind: "muted"
                wrapMode: Text.WordWrap
                text: page.notice !== "" ? page.notice : i18n("Click an item to select it. Drag it to move, the square corner to resize, the round handle to turn. Only the ring between the dashed circles is printed. You can drop a picture onto the disc.")
            }
        }
    }

    // ---- the panel ---------------------------------------------------------------------------------------------
    SidePanel {
        showPrinter: true
        panelWidth: 310
        footer: [
            WButton {
                objectName: "printButton"
                Layout.fillWidth: true
                kind: "primary"
                enabled: page.canPrint
                text: i18n("Print")
                onClicked: page.askToPrint()
            }
        ]

        FormRow {
            label: i18n("Disc Type")
            WRadio {
                objectName: "mediaA"
                text: DiscMath.mediaLabel("a")
                checked: page.design.media === "a"
                onClicked: page.change(d => { d.media = "a"; })
            }
            WRadio {
                objectName: "mediaB"
                text: DiscMath.mediaLabel("b")
                checked: page.design.media === "b"
                onClicked: page.change(d => { d.media = "b"; })
            }
        }
        FormRow {
            label: i18n("Outer diameter: %1 mm", page.design.outer_mm.toFixed(1))
            WSlider {
                objectName: "outerSlider"
                Layout.fillWidth: true
                from: DiscMath.outerMin
                to: DiscMath.outerMax
                stepSize: 0.5
                value: page.design.outer_mm
                onMoved: page.change(d => { d.outer_mm = DiscMath.clampOuter(value); })
            }
        }
        FormRow {
            label: i18n("Inner diameter: %1 mm", page.design.inner_mm.toFixed(1))
            WSlider {
                objectName: "innerSlider"
                Layout.fillWidth: true
                from: DiscMath.innerMin
                to: DiscMath.innerMax
                stepSize: 0.5
                value: page.design.inner_mm
                onMoved: page.change(d => { d.inner_mm = DiscMath.clampInner(value); })
            }
        }
        FormRow {
            label: i18n("Background Color")
            ColorPicker {
                Layout.fillWidth: true
                value: page.design.background_color
                onPicked: c => page.change(d => { d.background_color = c; })
            }
        }

        // ---- items
        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Theme.line
        }
        WText {
            font.weight: Font.DemiBold
            text: i18n("Items")
        }
        Repeater {
            model: [{ idx: -1, label: i18n("Picture"), glyph: "photo", on: !!page.design.background }].concat(page.design.texts.map((t, i) => ({ idx: i, label: t.text.trim() === "" ? i18n("Text %1", i + 1) : t.text.split("\n")[0], glyph: "text", on: true }))).filter(r => r.on)
            delegate: Rectangle {
                id: row
                required property var modelData
                objectName: "item-" + modelData.idx
                Layout.fillWidth: true
                height: 34
                radius: 4
                color: page.selected === modelData.idx ? Theme.accentTint : "transparent"
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    anchors.rightMargin: 4
                    spacing: 8
                    Glyph {
                        name: row.modelData.glyph
                        size: 18
                    }
                    WText {
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        text: row.modelData.label
                    }
                    WButton {
                        kind: "quiet"
                        glyph: "close"
                        implicitWidth: 28
                        implicitHeight: 26
                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.text: i18n("Remove")
                        onClicked: {
                            page.selected = row.modelData.idx;
                            page.removeSelected();
                        }
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    anchors.rightMargin: 36
                    cursorShape: Qt.PointingHandCursor
                    onClicked: page.selected = row.modelData.idx
                }
            }
        }
        WText {
            visible: !page.design.background && page.design.texts.length === 0
            Layout.fillWidth: true
            kind: "dim"
            wrapMode: Text.WordWrap
            text: i18n("Nothing on the label yet. Add a picture or some text.")
        }

        // ---- the selected text
        ColumnLayout {
            visible: page.sel !== null
            Layout.fillWidth: true
            spacing: 10
            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: Theme.line
            }
            WText {
                font.weight: Font.DemiBold
                text: i18n("Text")
            }
            QQC2.TextArea {
                id: textArea
                objectName: "textArea"
                Layout.fillWidth: true
                Layout.preferredHeight: 64
                font.pixelSize: Theme.fontSize
                wrapMode: TextEdit.NoWrap
                selectByMouse: true
                text: page.sel ? page.sel.text : ""
                onTextChanged: {
                    if (page.sel && text !== page.sel.text) {
                        const i = page.selected;
                        page.change(d => { d.texts[i].text = text; });
                    }
                }
                background: Rectangle {
                    radius: 4
                    border.width: textArea.activeFocus ? 2 : 1
                    border.color: textArea.activeFocus ? Theme.accent : Theme.fieldBorder
                    color: "#FFFFFF"
                }
            }
            FormRow {
                label: i18n("Font")
                WCombo {
                    objectName: "fontCombo"
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: [{ text: i18n("Default"), value: "" }].concat(Qt.fontFamilies().map(f => ({ text: f, value: f })))
                    selectedValue: page.sel ? page.sel.family : ""
                    onActivated: i => page.change(d => { d.texts[page.selected].family = valueAt(i); })
                }
            }
            WCheck {
                objectName: "followRing"
                Layout.fillWidth: true
                text: i18n("Follow the ring")
                checked: !!page.sel && !!page.sel.ring
                onToggled: page.change(d => {
                    d.texts[page.selected].ring = checked ? DiscMath.newRing() : null;
                    if (checked) {
                        d.texts[page.selected].align = "center";
                    }
                })
            }
            ColumnLayout {
                objectName: "ringOptions"
                visible: !!page.sel && !!page.sel.ring
                Layout.fillWidth: true
                spacing: 10
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    FormRow {
                        label: i18n("Radius (mm)")
                        WSpin {
                            objectName: "ringRadius"
                            Layout.fillWidth: true
                            from: DiscMath.ringRadiusMin
                            to: DiscMath.ringRadiusMax
                            value: page.sel && page.sel.ring ? Math.round(page.sel.ring.radius_mm) : 48
                            onValueModified: page.change(d => { d.texts[page.selected].ring.radius_mm = value; })
                        }
                    }
                    FormRow {
                        label: i18n("Start angle (°)")
                        WSpin {
                            objectName: "ringStart"
                            Layout.fillWidth: true
                            from: -360
                            to: 360
                            value: page.sel && page.sel.ring ? Math.round(page.sel.ring.start_deg) : 0
                            onValueModified: page.change(d => { d.texts[page.selected].ring.start_deg = value; })
                        }
                    }
                }
                FormRow {
                    label: i18n("Direction")
                    RowLayout {
                        spacing: 12
                        WRadio {
                            objectName: "ringClockwise"
                            text: i18n("Clockwise")
                            checked: !!page.sel && !!page.sel.ring && page.sel.ring.clockwise
                            onClicked: page.change(d => { d.texts[page.selected].ring.clockwise = true; })
                        }
                        WRadio {
                            objectName: "ringCounterClockwise"
                            text: i18n("Counter-clockwise")
                            checked: !!page.sel && !!page.sel.ring && !page.sel.ring.clockwise
                            onClicked: page.change(d => { d.texts[page.selected].ring.clockwise = false; })
                        }
                    }
                }
                FormRow {
                    label: i18n("Letter spacing (pt)")
                    WSpin {
                        objectName: "ringSpacing"
                        Layout.fillWidth: true
                        from: DiscMath.spacingMin
                        to: DiscMath.spacingMax
                        value: page.sel && page.sel.ring ? Math.round(page.sel.ring.letter_spacing_pt) : 0
                        onValueModified: page.change(d => { d.texts[page.selected].ring.letter_spacing_pt = value; })
                    }
                }
                WText {
                    Layout.fillWidth: true
                    kind: "muted"
                    wrapMode: Text.WordWrap
                    text: i18n("Angles count clockwise from the top of the disc. The alignment below says whether the text starts, is centered or ends at the start angle. Clockwise letters stand on the circle; counter-clockwise ones hang from it, which reads upright along the bottom edge.")
                }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                FormRow {
                    label: i18n("Size (pt)")
                    WSpin {
                        objectName: "sizeSpin"
                        Layout.fillWidth: true
                        from: DiscMath.minPt
                        to: DiscMath.maxPt
                        value: page.sel ? Math.round(page.sel.size_pt) : 18
                        onValueModified: page.change(d => { d.texts[page.selected].size_pt = value; })
                    }
                }
                FormRow {
                    visible: !(page.sel && page.sel.ring)
                    label: i18n("Turn (°)")
                    WSpin {
                        objectName: "rotationSpin"
                        Layout.fillWidth: true
                        from: -180
                        to: 180
                        value: page.sel ? Math.round(page.sel.rotation) : 0
                        onValueModified: page.change(d => { d.texts[page.selected].rotation = value; })
                    }
                }
            }
            RowLayout {
                visible: !(page.sel && page.sel.ring)
                Layout.fillWidth: true
                spacing: 10
                FormRow {
                    label: i18n("Across (mm)")
                    WSpin {
                        objectName: "xSpin"
                        Layout.fillWidth: true
                        from: -60
                        to: 60
                        value: page.sel ? Math.round(page.sel.x_mm) : 0
                        onValueModified: page.change(d => { d.texts[page.selected].x_mm = value; })
                    }
                }
                FormRow {
                    label: i18n("Down (mm)")
                    WSpin {
                        objectName: "ySpin"
                        Layout.fillWidth: true
                        from: -60
                        to: 60
                        value: page.sel ? Math.round(page.sel.y_mm) : 0
                        onValueModified: page.change(d => { d.texts[page.selected].y_mm = value; })
                    }
                }
            }
            FormRow {
                label: i18n("Alignment")
                RowLayout {
                    spacing: 12
                    Repeater {
                        model: [{ v: "left", t: i18n("Left") }, { v: "center", t: i18n("Center") }, { v: "right", t: i18n("Right") }]
                        delegate: WRadio {
                            required property var modelData
                            objectName: "align-" + modelData.v
                            text: modelData.t
                            checked: page.sel && page.sel.align === modelData.v
                            onClicked: page.change(d => { d.texts[page.selected].align = modelData.v; })
                        }
                    }
                }
            }
            FormRow {
                label: i18n("Color")
                ColorPicker {
                    Layout.fillWidth: true
                    value: page.sel ? page.sel.color : "#000000"
                    onPicked: c => page.change(d => { d.texts[page.selected].color = c; })
                }
            }
        }

        // ---- the selected picture
        ColumnLayout {
            visible: page.selected === -1 && !!page.design.background
            Layout.fillWidth: true
            spacing: 10
            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: Theme.line
            }
            WText {
                font.weight: Font.DemiBold
                text: i18n("Picture")
            }
            FormRow {
                label: i18n("Fit")
                RowLayout {
                    spacing: 12
                    WRadio {
                        objectName: "fitCover"
                        text: i18n("Fill the disc")
                        checked: page.design.background && page.design.background.fit === "cover"
                        onClicked: page.change(d => { d.background.fit = "cover"; })
                    }
                    WRadio {
                        objectName: "fitContain"
                        text: i18n("Fit inside")
                        checked: page.design.background && page.design.background.fit === "contain"
                        onClicked: page.change(d => { d.background.fit = "contain"; })
                    }
                }
            }
            FormRow {
                label: i18n("Size: %1%", Math.round((page.design.background ? page.design.background.scale : 1) * 100))
                WSlider {
                    objectName: "scaleSlider"
                    Layout.fillWidth: true
                    from: 0.2
                    to: 4
                    stepSize: 0.05
                    value: page.design.background ? page.design.background.scale : 1
                    onMoved: page.change(d => { d.background.scale = Math.round(value * 100) / 100; })
                }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                FormRow {
                    label: i18n("Turn (°)")
                    WSpin {
                        objectName: "picRotation"
                        Layout.fillWidth: true
                        from: -180
                        to: 180
                        value: page.design.background ? Math.round(page.design.background.rotation) : 0
                        onValueModified: page.change(d => { d.background.rotation = value; })
                    }
                }
                WButton {
                    Layout.alignment: Qt.AlignBottom
                    text: i18n("Center")
                    onClicked: page.change(d => { d.background.offset_mm = [0, 0]; })
                }
            }
        }

        // ---- designs
        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Theme.line
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            WButton {
                objectName: "saveDesignOpen"
                Layout.fillWidth: true
                glyph: "folder"
                text: i18n("Save Design…")
                onClicked: saveDlg.open()
            }
            WButton {
                objectName: "openDesignOpen"
                Layout.fillWidth: true
                glyph: "folderopen"
                text: i18n("Open…")
                onClicked: {
                    page.refreshSaved();
                    openDlg.open();
                }
            }
        }
        InfoBar {
            Layout.fillWidth: true
            visible: page.blockedReason !== "" && page.hasContent
            kind: Store.readOnly || !Store.connected || Store.power === "offline" ? "warning" : "info"
            text: page.blockedReason
        }
        WButton {
            Layout.alignment: Qt.AlignHCenter
            kind: "quiet"
            text: i18n("How to load the disc tray")
            onClicked: loadTray.openFor("disc", false, false)
        }
    }
}
