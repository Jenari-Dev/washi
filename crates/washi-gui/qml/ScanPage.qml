import QtQuick
import QtQuick.Layouts

// Scan: four screens in one flow (setup, preview and edit, rotate/crop, image correction).
// The steps and the host-side work live in ScanFlow; this file only chooses what to draw.
Item {
    id: page

    readonly property alias flow: scanFlow
    readonly property bool inEdit: scanFlow.step === "crop" || scanFlow.step === "correct"

    ScanFlow {
        id: scanFlow
    }

    Component.onCompleted: {
        Store.loadScanCaps(false);
        // The screenshot checks can open Scan in a mode (WASHI_SCAN_MODE=stitch) with a join
        // direction (WASHI_STITCH=top-bottom).
        const mode = Store.env("WASHI_SCAN_MODE");
        if (mode !== "") {
            scanFlow.mode = mode;
        }
        const join = Store.env("WASHI_STITCH");
        if (join !== "") {
            scanFlow.stitchDirection = join;
        }
        // The screenshot checks start from a given step with the pages of a fake printer.
        const env = Store.env("WASHI_SCAN_STEP");
        if (env !== "") {
            envTimer.start();
        }
    }

    // WASHI_SCAN_STEP=preview|crop|correct runs a scan (the printer must be a fake) and walks to
    // that step; used by tests/screens.sh.
    Timer {
        id: envTimer
        interval: 400
        repeat: true
        property int phase: 0
        onTriggered: {
            const want = Store.env("WASHI_SCAN_STEP");
            if (phase === 0 && scanFlow.canScan) {
                scanFlow.kind = Store.env("WASHI_SCAN_KIND") || "document";
                scanFlow.resolution = 75;
                scanFlow.startScan();
                phase = 1;
            } else if (phase === 1 && scanFlow.step === "preview" && !scanFlow.scanning && scanFlow.mainPath !== "") {
                if (want === "crop") {
                    scanFlow.openCrop();
                    scanFlow.setSlant(1.5);
                    scanFlow.setAspect("4:3");
                } else if (want === "correct") {
                    scanFlow.openCorrect();
                    scanFlow.setCorrection({ color_correction: "Document", reduce_show_through: true, unsharp_amount: 0.4 });
                }
                phase = 2;
                stop();
            }
        }
    }

    StackLayout {
        anchors.fill: parent
        currentIndex: ({ setup: 0, preview: 1, crop: 2, correct: 3 })[scanFlow.step]

        ScanSetupView {
            flow: scanFlow
        }
        ScanPreviewView {
            flow: scanFlow
        }
        Loader {
            active: scanFlow.step === "crop"
            sourceComponent: ScanCropView {
                flow: scanFlow
            }
        }
        Loader {
            active: scanFlow.step === "correct"
            sourceComponent: ScanCorrectView {
                flow: scanFlow
            }
        }
    }
}
