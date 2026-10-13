pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents
import org.kde.kirigami as Kirigami

// The panel icon with the set temperature while a unit runs. Left-click
// toggles the native popup; middle-click optionally toggles both ACs.
MouseArea {
    id: compactMouse
    Accessible.name: "Gree AC"
    anchors.fill: parent
    acceptedButtons: Qt.LeftButton | Qt.MiddleButton
    hoverEnabled: true
    onClicked: mouse => {
        if (mouse.button === Qt.MiddleButton) {
            if (Plasmoid.configuration.middleClickToggles) {
                root.send("--set-power " + (root.activeUnit ? "off" : "on"));
            }
        } else if (mouse.button === Qt.LeftButton) {
            root.expanded = !root.expanded;
        }
    }

    // Reserve room only while the label is actually shown. When the units
    // are off there is no text, so the slot shrinks back to the icon and no
    // empty gap remains. Both request values are stable (icon / icon + the
    // fixed worst-case label width), never measured mid-transition.
    Layout.preferredWidth: (showTemp && root.activeUnit !== null)
        ? Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing + tempMetrics.advanceWidth
        : Kirigami.Units.iconSizes.smallMedium

    readonly property bool showTemp: Plasmoid.configuration.showTemperature
        && Plasmoid.formFactor !== PlasmaCore.Types.Vertical

    TextMetrics {
        id: tempMetrics
        font.bold: true
        text: "88°"
    }

    RowLayout {
        anchors.fill: parent
        spacing: Kirigami.Units.smallSpacing

        Kirigami.Icon {
            Layout.fillHeight: true
            Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
            source: Plasmoid.icon
            active: compactMouse.containsMouse
        }
        PlasmaComponents.Label {
            visible: compactMouse.showTemp && root.activeUnit !== null
            font.bold: true
            elide: Text.ElideRight
            text: root.activeUnit ? ((root.activeUnit.state.target ?? 24) + "°") : ""
        }
    }
}
