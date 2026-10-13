import QtQuick

// Text with the app's type scale. `kind`: body, strong, dim, small, muted, heading, title.
Text {
    property string kind: "body"

    color: kind === "dim" || kind === "small" ? Theme.textDim : (kind === "muted" ? Theme.textMuted : Theme.text)
    font.pixelSize: kind === "title" ? Theme.title : (kind === "heading" ? Theme.heading : (kind === "small" || kind === "muted" ? Theme.small : Theme.fontSize))
    font.weight: kind === "strong" || kind === "heading" || kind === "title" ? Font.DemiBold : Font.Normal
    textFormat: Text.PlainText
    elide: Text.ElideNone
}
