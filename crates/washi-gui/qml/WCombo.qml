import QtQuick
import QtQuick.Controls as QQC2

// Drop-down list. `model` is an array of strings or of {text, value} objects.
QQC2.ComboBox {
    id: control

    implicitHeight: 34
    implicitWidth: 200
    hoverEnabled: true

    // Choose the entry whose `value` (valueRole) equals this; stays right when the model changes
    // (a model reset puts the current index back to 0, which a plain binding would not notice).
    property var selectedValue: undefined

    function syncSelected() {
        if (selectedValue === undefined || count === 0) {
            return;
        }
        for (let i = 0; i < count; i++) {
            if (valueAt(i) === selectedValue) {
                if (currentIndex !== i) {
                    currentIndex = i;
                }
                return;
            }
        }
    }
    onSelectedValueChanged: syncSelected()
    onCountChanged: syncSelected()
    onModelChanged: Qt.callLater(syncSelected)
    Component.onCompleted: syncSelected()

    delegate: QQC2.ItemDelegate {
        id: item
        required property var modelData
        required property int index
        width: control.width
        height: 34
        highlighted: control.highlightedIndex === index
        contentItem: Text {
            text: typeof item.modelData === "string" ? item.modelData : item.modelData.text
            font.pixelSize: Theme.fontSize
            color: Theme.text
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
        background: Rectangle {
            color: item.highlighted ? Theme.accentTint : (control.currentIndex === item.index ? Theme.accentSoft : "transparent")
        }
    }

    indicator: Glyph {
        x: control.width - width - 10
        y: (control.height - height) / 2
        name: "chevdown"
        size: 16
        color: control.enabled ? Theme.textDim : Theme.textMuted
    }
    contentItem: Text {
        leftPadding: 10
        rightPadding: 32
        text: control.displayText
        font.pixelSize: Theme.fontSize
        color: control.enabled ? Theme.text : Theme.textMuted
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    background: Rectangle {
        radius: 4
        color: !control.enabled ? "#F7F7F7" : (control.hovered ? "#F9F9F9" : "#FFFFFF")
        border.width: control.visualFocus || control.popup.visible ? 2 : 1
        border.color: control.visualFocus || control.popup.visible ? Theme.accent : (control.enabled ? Theme.fieldBorder : "#D6D6D6")
    }
    popup: QQC2.Popup {
        y: control.height + 2
        width: control.width
        padding: 2
        implicitHeight: Math.min(contentItem.implicitHeight + 4, 300)
        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: control.popup.visible ? control.delegateModel : null
            currentIndex: control.highlightedIndex
            QQC2.ScrollIndicator.vertical: QQC2.ScrollIndicator {}
        }
        background: Rectangle {
            radius: 4
            color: "#FFFFFF"
            border.width: 1
            border.color: Theme.fieldBorder
        }
    }
}
