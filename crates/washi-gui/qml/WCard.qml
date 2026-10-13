import QtQuick

// White card with a light border and rounded corners.
Rectangle {
    property bool padded: true
    color: Theme.card
    radius: Theme.radius
    border.width: 1
    border.color: Theme.cardBorder
}
