import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import QtCore

// Photo Print / Document Print: the preview on the left (grey area, the sheet with the chosen
// size, orientation, borderless and layout), the settings panel on the right. Printing goes
// through the service: a confirmation, then the progress dialog.
RowLayout {
    id: page

    property bool documentMode: false

    spacing: 0

    // ---- files -----------------------------------------------------------------------------
    property var files: []              // absolute local paths
    property int current: 0              // photo index, or document sheet index
    readonly property string currentFile: files.length > 0 ? files[Math.min(current, files.length - 1)] : ""
    readonly property string fileKind: files.length > 0 ? PrintModel.kindOf(files[0]) : ""
    readonly property var tools: Img.tools()

    // ---- the settings ----------------------------------------------------------------------------
    property string sizeKey: ""
    property string orientation: "portrait"
    property string mediaType: ""
    property string source: "auto"
    property bool borderless: false
    property bool borderlessTouched: false
    property string quality: "normal"
    property bool color: true
    property int copies: 1
    property int layout: 1
    property string duplex: ""
    property string pageOrder: "normal"     // documents: normal | reverse (the last sheet first)
    property bool allPages: true
    property string pagesText: ""

    readonly property var caps: Store.printCaps
    readonly property var sizeObj: caps && caps.sizes ? (caps.sizes.find(z => z.key === sizeKey) || null) : null
    readonly property var sizeList: PrintModel.sizesFor(caps, source, "")
    readonly property var typeList: PrintModel.typesFor(caps, source, sizeKey, !documentMode)
    // The edge the printer cannot print on for this paper (null: none to show, or borderless).
    readonly property var edge: !caps || borderless ? null : PrintModel.unprintable(caps, source, mediaType, sizeKey, orientation === "landscape")
    readonly property var sheetMm: PrintModel.sheetMm(sizeObj, orientation)
    readonly property bool borderlessOk: !documentMode && PrintModel.borderlessAvailable(caps, PrintModel.sourceKey(source), mediaType, sizeKey) && layout === 1

    // Document facts (from QtPdf once the page is loaded).
    readonly property int docPages: docLoader.item ? docLoader.item.pageCount : 0
    readonly property var pageCheck: allPages ? ({ ok: true, error: "", pages: [] }) : PrintModel.parsePages(pagesText, docPages)
    readonly property var selectedPages: {
        if (allPages || !pageCheck.ok) {
            const n = [];
            for (let i = 1; i <= docPages; i++) {
                n.push(i);
            }
            return n;
        }
        return pageCheck.pages;
    }
    // The sheets as they come out of the printer: the number of the sheet side (a group of
    // `layout` pages) for each place of the preview; Reverse turns the sheets round.
    readonly property var sheetOrder: PrintModel.sheetOrder(Math.max(1, Math.ceil(selectedPages.length / layout)), duplex !== "", pageOrder === "reverse")
    readonly property int sheetCount: {
        if (documentMode) {
            return Math.max(1, Math.ceil(selectedPages.length / layout));
        }
        return Math.max(1, Math.ceil(files.length / layout));
    }

    readonly property bool toolsOk: !documentMode || fileKind !== "pdf" || tools.pdf
    readonly property bool officeOk: fileKind !== "office" || tools.office
    readonly property bool ready: files.length > 0 && !!caps && mediaType !== "" && sizeKey !== "" && pageCheck.ok && toolsOk && officeOk
    readonly property bool canPrint: Store.canJob && ready

    // Why Print is off, in the user's words ("" when it is on).
    readonly property string blockedReason: {
        if (!Store.connected) return i18n("The Washi service is not running.");
        if (!Store.hasPrinter) return i18n("Add a printer first.");
        if (Store.readOnly) return i18n("Read-only mode: the service will not print.");
        if (Store.power === "offline") return i18n("The printer cannot be reached.");
        if (Store.busy) return i18n("The printer is busy: %1", Store.statusTip);
        if (!caps) return Store.printCapsError !== "" ? i18n("Could not read what the printer prints on: %1", Store.printCapsError) : i18n("Reading what the printer prints on…");
        if (files.length === 0) return documentMode ? i18n("Select a document to print.") : i18n("Select photos to print.");
        if (!toolsOk) return i18n("Printing PDFs needs poppler (pdftoppm and pdfinfo).");
        if (!officeOk) return i18n("Printing Office files needs LibreOffice.");
        if (!pageCheck.ok) return PrintModel.pagesError(pageCheck.error);
        if (mediaType === "") return i18n("The printer offers no paper for this size.");
        return "";
    }

    // ---- behaviour -----------------------------------------------------------------------------
    // The lists are computed afresh here: when `caps` changes, the bound properties further up
    // may not have been re-evaluated yet.
    function chooseDefaults() {
        if (!caps || !caps.sizes || caps.sizes.length === 0) {
            return;
        }
        const sizes = PrintModel.sizesFor(caps, source, "");
        if (sizeKey === "" || !sizes.some(z => z.key === sizeKey)) {
            const prefer = documentMode ? ["a4", "letter"] : ["l", "4x6", "2l", "a4"];
            const hit = prefer.find(k => sizes.some(z => z.key === k));
            sizeKey = hit || (sizes.length > 0 ? sizes[0].key : caps.sizes[0].key);
        }
        reconcile();
        // The screenshot checks can start with margins (WASHI_BORDERLESS=0).
        if (Store.env("WASHI_BORDERLESS") === "0") {
            borderlessTouched = true;
            borderless = false;
        }
    }

    // Keep media type, borderless and layout consistent with what the printer offers.
    function reconcile() {
        if (!caps) {
            return;
        }
        const sizes = PrintModel.sizesFor(caps, source, "");
        if (sizes.length > 0 && !sizes.some(z => z.key === sizeKey)) {
            sizeKey = sizes[0].key;
        }
        const types = PrintModel.typesFor(caps, source, sizeKey, !documentMode);
        if (types.indexOf(mediaType) < 0) {
            mediaType = PrintModel.defaultMedia(types, !documentMode);
        }
        const bl = !documentMode && layout === 1 && PrintModel.borderlessAvailable(caps, source, mediaType, sizeKey);
        if (!bl) {
            borderless = false;
        } else if (!borderlessTouched) {
            borderless = true;
        }
    }

    onCapsChanged: chooseDefaults()
    onSourceChanged: reconcile()
    onSizeKeyChanged: {
        reconcile();
        schedulePreview();
    }
    onMediaTypeChanged: reconcile()
    onLayoutChanged: {
        reconcile();
        schedulePreview();
    }
    onBorderlessChanged: schedulePreview()
    onOrientationChanged: schedulePreview()
    onCurrentChanged: if (!documentMode) schedulePreview()

    Component.onCompleted: {
        Store.loadPrintCaps(false);
        chooseDefaults();
        const env = Store.env(documentMode ? "WASHI_DOC" : "WASHI_FILE");
        if (env !== "") {
            setFiles(env.split(":"));
        }
        const lay = parseInt(Store.env("WASHI_LAYOUT") || "0");
        if (lay > 0) {
            layout = lay;
        }
        const orient = Store.env("WASHI_ORIENTATION");
        if (orient !== "") {
            orientation = orient;
        }
        if (documentMode && (Store.env("WASHI_PAGE_ORDER") !== "" || Store.env("WASHI_DUPLEX") !== "")) {
            docOptionsTimer.start();
        }
        if (Store.env("WASHI_OPEN") === "media") {
            showListTimer.start();
        }
    }
    // Screenshot checks of the document options (WASHI_PAGE_ORDER=reverse, WASHI_DUPLEX=long): set
    // once the first page is up, the way a person would choose them.
    Timer {
        id: docOptionsTimer
        interval: 1500
        onTriggered: {
            page.pageOrder = Store.env("WASHI_PAGE_ORDER") || page.pageOrder;
            page.duplex = Store.env("WASHI_DUPLEX") || page.duplex;
        }
    }
    // Screenshot checks: WASHI_OPEN=media drops down the list of media types once the page is up.
    Timer {
        id: showListTimer
        interval: 1800
        onTriggered: mediaBox.popup.open()
    }
    Connections {
        target: Store
        function onActiveHostChanged() {
            Store.loadPrintCaps(false);
        }
    }
    // The first reading can fail (the printer is busy or waking up): try again every few seconds.
    Timer {
        interval: 4000
        repeat: true
        running: Store.connected && Store.hasPrinter && !Store.printCaps && !Store.printCapsLoading
        onTriggered: Store.loadPrintCaps(true)
    }

    // What the page says about files dropped on it that it cannot print: a message source.
    property var dropNoticeSource: ""
    readonly property string dropNotice: Fmt.text(dropNoticeSource)

    // Files dragged from a file manager onto the preview: the same as choosing them in the dialog.
    // `urls` are file:// URLs; anything the page cannot print is ignored, and said so.
    function dropFiles(urls) {
        const list = urls.map(u => Img.localPath(u)).filter(p => String(p).charAt(0) === "/");
        dropNoticeSource = "";
        if (!setFiles(list)) {
            dropNoticeSource = documentMode
                ? () => i18n("Only PDF and Office documents can be printed here.")
                : () => i18n("Only JPEG, PNG and BMP pictures can be printed here.");
        }
    }

    // Choose the files to print; false when none of them is a kind this page prints.
    function setFiles(paths) {
        const ok = paths.filter(p => {
            const k = PrintModel.kindOf(p);
            return documentMode ? (k === "pdf" || k === "office") : k === "photo";
        });
        if (ok.length === 0) {
            return false;
        }
        files = documentMode ? [ok[0]] : ok;
        current = 0;
        if (!documentMode) {
            const sz = Img.engine ? Img.parse(Img.engine.imageSize(ok[0])) : null;
            if (sz) {
                orientation = sz.width > sz.height ? "landscape" : "portrait";
            }
            schedulePreview();
        }
        return true;
    }

    function removeCurrent() {
        if (documentMode || files.length === 0) {
            return;
        }
        const f = files.slice();
        f.splice(current, 1);
        files = f;
        current = Math.max(0, Math.min(current, f.length - 1));
        schedulePreview();
    }

    // ---- photo sheet preview (rendered by the host, so it is exactly what is printed) -----------
    property string sheetPath: ""
    property bool sheetBusy: false
    property int previewToken: 0
    readonly property int groupStart: Math.floor(current / layout) * layout

    function schedulePreview() {
        previewTimer.restart();
    }

    Timer {
        id: previewTimer
        interval: 90
        onTriggered: {
            if (page.documentMode || page.files.length === 0 || page.sizeKey === "" || !Img.engine) {
                return;
            }
            const token = ++page.previewToken;
            page.sheetBusy = true;
            Img.photoPreview({
                files: page.files.slice(page.groupStart, page.groupStart + page.layout),
                size: page.sizeKey,
                borderless: page.borderless,
                layout: page.layout,
                orientation: page.orientation,
                max_edge: 900
            }, (ok, r) => {
                if (token !== page.previewToken) {
                    return;
                }
                page.sheetBusy = false;
                page.sheetPath = ok ? r.path : "";
            });
        }
    }

    // ---- printing --------------------------------------------------------------------------------
    function settingsObject() {
        return {
            size: sizeKey,
            sizeLabel: sizeObj ? PrintModel.sizeLabel(sizeObj) : sizeKey,
            mediaType: mediaType,
            source: source,
            borderless: borderless,
            quality: quality,
            color: color,
            copies: copies,
            pagesText: documentMode && !allPages ? pagesText : "",
            layout: layout,
            duplex: documentMode ? duplex : "",
            pageOrder: documentMode ? pageOrder : "normal",
            orientation: orientation,
            jobName: files.length === 1 ? Img.baseName(files[0]) : ""
        };
    }

    readonly property string jobLabel: files.length === 1 ? Img.baseName(files[0]) : i18np("%1 photo", "%1 photos", files.length)

    function askToPrint() {
        if (!canPrint) {
            return;
        }
        if (!Prefs.hideLoadPaperHint) {
            loadPaper.openFor(source === "cassette" ? "cassette" : "rear", !documentMode, true);
        } else {
            confirmDlg.open();
        }
    }

    function startPrinting() {
        job.start(files, PrintModel.buildOptions(settingsObject()), jobLabel);
    }

    // The headless screenshot checks open the dialogs through Nav (WASHI_DIALOG=nav-print-confirm).
    Connections {
        target: Nav
        function onDialogRequested(name, args) {
            if (name === "print-load-paper") {
                loadPaper.openFor(page.source === "cassette" ? "cassette" : "rear", !page.documentMode, true);
            } else if (name === "print-confirm") {
                confirmDlg.open();
            } else if (name === "print-start") {
                page.startPrinting();
            }
        }
    }

    LoadPaperDialog {
        id: loadPaper
        onProceed: confirmDlg.open()
    }

    ConfirmDialog {
        id: confirmDlg
        objectName: "printConfirm"
        title: i18n("Print")
        acceptText: i18n("Print")
        glyph: "printer"
        text: i18n("Print %1?\n\nPrinter: %2. This uses paper and ink.", PrintModel.describe(page.settingsObject(), page.jobLabel), Store.displayName)
        onAccepted: page.startPrinting()
    }

    PrintJobDialog {
        id: job
        objectName: "printJob"
    }

    FileDialog {
        id: picker
        title: page.documentMode ? i18n("Select Document") : i18n("Select Photos")
        fileMode: page.documentMode ? FileDialog.OpenFile : FileDialog.OpenFiles
        nameFilters: page.documentMode ? PrintModel.documentFilters(page.tools.office) : PrintModel.photoFilters
        currentFolder: (page.documentMode ? Prefs.documentFolder : Prefs.photoFolder) || StandardPaths.writableLocation(page.documentMode ? StandardPaths.DocumentsLocation : StandardPaths.PicturesLocation)
        onAccepted: {
            const list = picker.selectedFiles.map(u => Img.localPath(u));
            if (page.documentMode) {
                Prefs.documentFolder = String(picker.currentFolder);
            } else {
                Prefs.photoFolder = String(picker.currentFolder);
            }
            page.setFiles(list);
        }
    }

    // ---- preview area -------------------------------------------------------------------------------
    Rectangle {
        id: previewArea
        objectName: "previewArea"
        Layout.fillWidth: true
        Layout.fillHeight: true
        color: "#E6E6E6"

        // Nothing selected yet
        ColumnLayout {
            visible: page.files.length === 0
            anchors.centerIn: parent
            width: Math.min(parent.width - 48, 360)
            spacing: 14

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: page.documentMode ? 150 : 200
                Layout.preferredHeight: page.documentMode ? 200 : 140
                color: "#FFFFFF"
                border.width: 1
                border.color: "#CFCFCF"
                Glyph {
                    anchors.centerIn: parent
                    name: page.documentMode ? "document" : "photo"
                    size: 52
                    weight: 1.1
                    color: "#B5BFD2"
                }
            }
            WText {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                kind: "dim"
                wrapMode: Text.WordWrap
                text: page.documentMode
                    ? (page.tools.pdf ? (page.tools.office ? i18n("Select a PDF or an Office document to preview and print it.") : i18n("Select a PDF to preview and print it.")) : i18n("Printing PDFs needs poppler (pdftoppm and pdfinfo), which is not installed."))
                    : i18n("Select photos (JPEG, PNG or BMP) to preview and print them.")
            }
            WButton {
                objectName: "selectFile"
                Layout.alignment: Qt.AlignHCenter
                kind: "primary"
                glyph: "folder"
                text: page.documentMode ? i18n("Select Document") : i18n("Select Photos")
                onClicked: picker.open()
            }
            WText {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                kind: "muted"
                wrapMode: Text.WordWrap
                text: i18n("You can also drag files from your file manager onto this area.")
            }
        }

        // Files dropped from a file manager
        DropArea {
            id: dropArea
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
            visible: dropArea.containsDrag
            anchors.fill: parent
            anchors.margins: 6
            radius: 8
            color: "#261354BF"
            border.width: 2
            border.color: Theme.accent
            WText {
                anchors.centerIn: parent
                font.weight: Font.DemiBold
                font.pixelSize: 18
                color: Theme.accent
                text: i18n("Drop to select")
            }
        }
        InfoBar {
            objectName: "dropNotice"
            visible: page.dropNotice !== ""
            kind: "warning"
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: 12
            text: page.dropNotice
            WButton {
                kind: "quiet"
                glyph: "close"
                implicitWidth: 30
                onClicked: page.dropNoticeSource = ""
            }
        }

        // The preview
        ColumnLayout {
            visible: page.files.length > 0
            anchors.fill: parent
            anchors.margins: 16
            spacing: 10

            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true

                PhotoSheet {
                    id: photoSheet
                    objectName: "photoSheet"
                    anchors.fill: parent
                    visible: !page.documentMode
                    aspect: PrintModel.sheetAspect(page.sizeObj, page.orientation)
                    edge: page.edge
                    sheetMm: page.sheetMm
                    fitToEdge: true
                    imagePath: page.sheetPath
                    busy: page.sheetBusy
                }
                Loader {
                    id: docLoader
                    anchors.fill: parent
                    active: page.documentMode && page.fileKind === "pdf"
                    sourceComponent: docPreviewComponent
                }
                // Office files cannot be previewed before they are converted.
                PhotoSheet {
                    visible: page.documentMode && page.fileKind === "office"
                    anchors.fill: parent
                    aspect: PrintModel.sheetAspect(page.sizeObj, page.orientation)
                    edge: page.edge
                    sheetMm: page.sheetMm
                    ColumnLayout {
                        anchors.centerIn: parent
                        width: parent.width - 32
                        spacing: 10
                        Glyph {
                            Layout.alignment: Qt.AlignHCenter
                            name: "document"
                            size: 44
                            weight: 1.2
                            color: "#B5BFD2"
                        }
                        WText {
                            Layout.fillWidth: true
                            horizontalAlignment: Text.AlignHCenter
                            wrapMode: Text.WordWrap
                            kind: "dim"
                            text: i18n("Office files are converted by LibreOffice when you print, so there is no preview here.")
                        }
                    }
                }
            }

            Component {
                id: docPreviewComponent
                DocPreview {
                    id: docSheet
                    objectName: "docSheet"
                    source: "file://" + page.files[0]
                    aspect: PrintModel.sheetAspect(page.sizeObj, page.orientation)
                    edge: page.edge
                    sheetMm: page.sheetMm
                    pages: page.selectedPages
                    sheetIndex: page.sheetOrder[Math.min(page.current, page.sheetOrder.length - 1)]
                    layout: page.layout
                    onFirstLandscapeChanged: if (pageCount > 0) page.orientation = firstLandscape ? "landscape" : "portrait"
                    onPageCountChanged: if (pageCount > 0) page.orientation = firstLandscape ? "landscape" : "portrait"
                }
            }

            WText {
                objectName: "edgeHint"
                visible: page.edge !== null
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                kind: "muted"
                wrapMode: Text.WordWrap
                text: i18n("The dashed line marks the edge the printer cannot print on.")
            }

            // Thumbnails of the selected photos
            ListView {
                id: strip
                objectName: "photoStrip"
                visible: !page.documentMode && page.files.length > 1
                Layout.fillWidth: true
                Layout.preferredHeight: 64
                orientation: ListView.Horizontal
                spacing: 8
                clip: true
                model: page.files
                delegate: Rectangle {
                    required property string modelData
                    required property int index
                    readonly property bool inSheet: index >= page.groupStart && index < page.groupStart + page.layout
                    width: 64
                    height: 56
                    color: "#FFFFFF"
                    border.width: inSheet ? 2 : 1
                    border.color: inSheet ? Theme.accent : "#C8C8C8"
                    Image {
                        anchors.fill: parent
                        anchors.margins: 3
                        source: "file://" + modelData
                        sourceSize.width: 128
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: page.current = index
                    }
                }
            }

            // Page / sheet navigation, file name and the buttons
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                WButton {
                    objectName: "prevButton"
                    kind: "quiet"
                    glyph: "chevleft"
                    implicitWidth: 36
                    enabled: page.documentMode ? page.current > 0 : page.current >= page.layout
                    onClicked: page.current = page.documentMode ? page.current - 1 : Math.max(0, page.groupStart - page.layout)
                }
                WText {
                    objectName: "pageLabel"
                    text: page.documentMode
                        ? (page.fileKind === "pdf" ? i18n("%1 / %2", Math.min(page.current, page.sheetCount - 1) + 1, page.sheetCount) : "")
                        : i18n("%1 / %2", Math.floor(page.current / page.layout) + 1, page.sheetCount)
                    kind: "dim"
                }
                WButton {
                    objectName: "nextButton"
                    kind: "quiet"
                    glyph: "chevright"
                    implicitWidth: 36
                    enabled: page.documentMode ? page.current < page.sheetCount - 1 : page.groupStart + page.layout < page.files.length
                    onClicked: page.current = page.documentMode ? page.current + 1 : Math.min(page.files.length - 1, page.groupStart + page.layout)
                }
                WText {
                    Layout.fillWidth: true
                    kind: "muted"
                    elide: Text.ElideMiddle
                    text: page.files.length === 1 ? Img.baseName(page.files[0]) : (page.documentMode ? "" : i18n("%1 photos selected", page.files.length))
                }
                WButton {
                    visible: !page.documentMode && page.files.length > 1
                    kind: "quiet"
                    glyph: "trash"
                    implicitWidth: 36
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: i18n("Remove this photo")
                    onClicked: page.removeCurrent()
                }
                WButton {
                    text: page.documentMode ? i18n("Select Document") : i18n("Select Photos")
                    glyph: "folder"
                    onClicked: picker.open()
                }
            }
        }
    }

    // ---- settings panel ----------------------------------------------------------------------------
    SidePanel {
        panelWidth: 280
        footer: [
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 8
                InfoBar {
                    objectName: "blockedBar"
                    Layout.fillWidth: true
                    visible: page.blockedReason !== "" && page.files.length > 0
                    kind: Store.readOnly || !Store.connected || Store.power === "offline" ? "warning" : "info"
                    text: page.blockedReason
                }
                WButton {
                    objectName: "printButton"
                    Layout.fillWidth: true
                    kind: "primary"
                    enabled: page.canPrint
                    text: i18n("Print")
                    onClicked: page.askToPrint()
                }
                WButton {
                    objectName: "loadPaperLink"
                    Layout.alignment: Qt.AlignHCenter
                    kind: "quiet"
                    text: i18n("How to load paper")
                    onClicked: loadPaper.openFor(page.source === "cassette" ? "cassette" : "rear", !page.documentMode, false)
                }
            }
        ]

            FormRow {
                label: i18n("Page Size")
                WCombo {
                    objectName: "sizeCombo"
                    Layout.fillWidth: true
                    enabled: page.sizeList.length > 0
                    textRole: "text"
                    valueRole: "value"
                    model: page.sizeList.map(z => ({ text: PrintModel.sizeLabel(z), value: z.key }))
                    selectedValue: page.sizeKey
                    onActivated: page.sizeKey = currentValue
                }
            }
            FormRow {
                label: i18n("Orientation")
                RowLayout {
                    spacing: 8
                    GlyphRadio {
                        objectName: "portraitRadio"
                        glyph: "portrait"
                        text: i18n("Portrait")
                        checked: page.orientation === "portrait"
                        onClicked: page.orientation = "portrait"
                    }
                    GlyphRadio {
                        objectName: "landscapeRadio"
                        glyph: "landscape"
                        text: i18n("Landscape")
                        checked: page.orientation === "landscape"
                        onClicked: page.orientation = "landscape"
                    }
                }
            }
            FormRow {
                visible: page.documentMode
                label: i18n("Pages")
                WRadio {
                    objectName: "allPagesRadio"
                    text: i18n("All")
                    checked: page.allPages
                    onClicked: page.allPages = true
                }
                RowLayout {
                    spacing: 8
                    WRadio {
                        objectName: "rangeRadio"
                        checked: !page.allPages
                        onClicked: {
                            page.allPages = false;
                            rangeField.forceActiveFocus();
                        }
                        implicitWidth: 28
                    }
                    WField {
                        id: rangeField
                        objectName: "rangeField"
                        Layout.fillWidth: true
                        placeholderText: i18n("e.g. 1-6, 3, 12-14")
                        text: page.pagesText
                        onTextEdited: {
                            page.pagesText = text;
                            page.allPages = text.trim() === "";
                            page.current = 0;
                        }
                    }
                }
                WText {
                    visible: !page.allPages && !page.pageCheck.ok && page.pagesText.trim() !== ""
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.danger
                    kind: "small"
                    text: PrintModel.pagesError(page.pageCheck.error)
                }
            }
            FormRow {
                label: i18n("Media Type")
                WCombo {
                    id: mediaBox
                    objectName: "mediaCombo"
                    Layout.fillWidth: true
                    enabled: page.typeList.length > 0
                    textRole: "text"
                    valueRole: "value"
                    model: page.typeList.map(t => ({ text: PrintModel.mediaLabel(t), value: t }))
                    selectedValue: page.mediaType
                    onActivated: page.mediaType = currentValue
                }
            }
            FormRow {
                label: i18n("Paper Source")
                WCombo {
                    objectName: "sourceCombo"
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: PrintModel.sourceOptions(page.caps)
                    selectedValue: page.source
                    onActivated: page.source = currentValue
                }
            }
            RowLayout {
                visible: !page.documentMode
                Layout.fillWidth: true
                WSwitch {
                    objectName: "borderlessSwitch"
                    Layout.fillWidth: true
                    text: i18n("Borderless")
                    enabled: page.borderlessOk
                    checked: page.borderless
                    onToggled: {
                        page.borderlessTouched = true;
                        page.borderless = checked;
                    }
                }
            }
            WText {
                visible: !page.documentMode && !page.borderlessOk && page.layout === 1 && !!page.caps && page.mediaType !== ""
                Layout.fillWidth: true
                kind: "muted"
                wrapMode: Text.WordWrap
                text: i18n("The printer has no borderless setting for this paper and size.")
            }
            FormRow {
                label: page.documentMode ? i18n("Page Layout") : i18n("Photo Layout")
                WCombo {
                    objectName: "layoutCombo"
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: PrintModel.layouts.map(n => ({ text: PrintModel.layoutLabel(n, !page.documentMode), value: n }))
                    selectedValue: page.layout
                    onActivated: page.layout = currentValue
                }
            }
            FormRow {
                label: i18n("Quality")
                WCombo {
                    objectName: "qualityCombo"
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: PrintModel.qualityOptions()
                    selectedValue: page.quality
                    onActivated: page.quality = currentValue
                }
            }
            FormRow {
                visible: page.documentMode && !!page.caps && page.caps.duplex
                label: i18n("Two-sided Printing")
                WCombo {
                    objectName: "duplexCombo"
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: PrintModel.duplexOptions()
                    selectedValue: page.duplex
                    onActivated: page.duplex = currentValue
                }
            }
            FormRow {
                objectName: "pageOrderRow"
                visible: page.documentMode
                label: i18n("Page Order")
                WCombo {
                    objectName: "pageOrderCombo"
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: PrintModel.pageOrderOptions()
                    selectedValue: page.pageOrder
                    onActivated: page.pageOrder = currentValue
                }
            }
            FormRow {
                label: i18n("Color")
                WCombo {
                    objectName: "colorCombo"
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: [{ text: i18n("Color"), value: true }, { text: i18n("Black and white"), value: false }]
                    selectedValue: page.color
                    onActivated: page.color = currentValue
                }
            }
            FormRow {
                label: i18n("Copies")
                WSpin {
                    objectName: "copiesSpin"
                    from: 1
                    to: page.caps && page.caps.copies_max ? page.caps.copies_max : 99
                    value: page.copies
                    onValueModified: page.copies = value
                }
            }
    }
}
