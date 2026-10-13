import QtQuick
import QtQuick.Layouts

// One card of the Utilities grid: icon and bold title, a one-line description, the current
// value, and a centred button ("Start" / "Settings") or a switch.
WCard {
    id: card

    property string glyph: ""
    property string title: ""
    property string description: ""
    property string current: ""
    property string kind: "action"          // action | settings | toggle
    property string buttonText: kind === "action" ? i18n("Start") : i18n("Settings")
    property string secondaryText: ""
    property string tertiaryText: ""
    property bool actionEnabled: true
    property bool secondaryEnabled: true
    property bool checked: false
    property string hint: ""

    signal activated()
    signal secondary()
    signal tertiary()
    signal toggled(bool on)

    implicitHeight: col.implicitHeight + 36

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: 18
        spacing: 8

        RowLayout {
            spacing: 12
            Glyph {
                name: card.glyph
                size: 30
                weight: 1.5
            }
            WText {
                Layout.fillWidth: true
                text: card.title
                font.weight: Font.DemiBold
                font.pixelSize: 16
                wrapMode: Text.WordWrap
            }
        }
        WText {
            Layout.fillWidth: true
            Layout.minimumHeight: 38
            kind: "dim"
            wrapMode: Text.WordWrap
            text: card.description
        }
        WText {
            Layout.fillWidth: true
            visible: card.current !== ""
            kind: "small"
            wrapMode: Text.WordWrap
            text: card.current
            color: Theme.accent
        }
        Item {
            Layout.fillHeight: true
            Layout.minimumHeight: 4
        }
        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 10
            WButton {
                objectName: "primaryButton"
                visible: card.kind !== "toggle"
                kind: card.kind === "action" ? "primary" : "secondary"
                text: card.buttonText
                enabled: card.actionEnabled
                onClicked: card.activated()
            }
            WButton {
                objectName: "secondaryButton"
                visible: card.secondaryText !== ""
                text: card.secondaryText
                enabled: card.secondaryEnabled && card.actionEnabled
                onClicked: card.secondary()
            }
            WButton {
                objectName: "tertiaryButton"
                visible: card.tertiaryText !== ""
                text: card.tertiaryText
                enabled: card.actionEnabled
                onClicked: card.tertiary()
            }
            WSwitch {
                objectName: "toggleSwitch"
                visible: card.kind === "toggle"
                checked: card.checked
                enabled: card.actionEnabled
                text: card.checked ? i18n("On") : i18n("Off")
                // The switch only asks; the real state changes after the printer confirms.
                onToggled: {
                    const want = checked;
                    checked = Qt.binding(() => card.checked);
                    card.toggled(want);
                }
            }
        }
        WText {
            Layout.fillWidth: true
            visible: card.hint !== ""
            horizontalAlignment: Text.AlignHCenter
            kind: "muted"
            wrapMode: Text.WordWrap
            text: card.hint
        }
    }
}
