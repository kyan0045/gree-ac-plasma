pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents
import org.kde.kirigami as Kirigami

// Controls for one unit, or for both together when unitIndex is -1.
// `unit` is a plain nested object ({online, error, state, roomText}); the
// instance persists across polls and values rebind in place.
ColumnLayout {
    id: card

    property int unitIndex: -1
    property string title: ""
    property var unit: null
    // Room line override. The shared card's unit object carries no room text,
    // so it passes one explicitly; per-unit delegates pass their role value.
    // (Named differently from the model role on purpose: same-named bindings
    // would resolve to this property itself.)
    property string roomLine: ""

    spacing: Kirigami.Units.smallSpacing
    Layout.fillWidth: true

    readonly property var st: card.unit?.state ?? null
    readonly property string unitArg: card.unitIndex < 0 ? "" : (" --unit " + card.unitIndex)
    readonly property var modes: ["Auto", "Cool", "Dry", "Fan", "Heat"]
    readonly property var modeIcons: ["system-run", "temperature-cold", "weather-showers", "weather-clear-wind", "temperature-warm"]
    readonly property var fans: ["Auto", "Low", "Medium-low", "Medium", "Medium-high", "High"]
    readonly property bool ready: (card.unit?.online ?? false) && card.st !== null
    readonly property bool isOn: card.ready && !!(card.st?.power)
    // Optimistic target while temp steps debounce. The display and the
    // steppers use this, so rapid clicks do not compute from stale polled
    // state. Cleared once the poll confirms it or a command fails.
    property int pendingTarget: -1
    readonly property int shownTarget: card.pendingTarget >= 0 ? card.pendingTarget : (card.st?.target ?? 24)
    property int pendingModeIndex: -1
    readonly property int shownModeIndex: card.pendingModeIndex >= 0
        ? card.pendingModeIndex
        : Math.max(0, card.modes.indexOf(card.st ? card.st.mode : "Auto"))
    property int pendingFanIndex: -1
    readonly property int shownFanIndex: card.pendingFanIndex >= 0
        ? card.pendingFanIndex
        : Math.max(0, card.fans.indexOf(card.st ? card.st.fan : "Auto"))

    onStChanged: {
        if (card.pendingTarget >= 0 && !tempCommandTimer.running && card.st && card.st.target === card.pendingTarget) {
            card.pendingTarget = -1;
        }
        if (card.pendingModeIndex >= 0 && !modeCommandTimer.running && card.st && card.modes.indexOf(card.st.mode) === card.pendingModeIndex) {
            card.pendingModeIndex = -1;
        }
        if (card.pendingFanIndex >= 0 && !fanCommandTimer.running && card.st && card.fans.indexOf(card.st.fan) === card.pendingFanIndex) {
            card.pendingFanIndex = -1;
        }
        card.clearPendingIfIdle();
        card.resyncControls();
    }

    Connections {
        target: root
        function onCommandFailed() {
            pendingTimeoutTimer.stop();
            card.pendingTarget = -1;
            card.pendingModeIndex = -1;
            card.pendingFanIndex = -1;
            card.resyncControls();
        }
    }

    Timer {
        id: pendingTimeoutTimer
        interval: 12000
        onTriggered: {
            card.pendingTarget = -1;
            card.pendingModeIndex = -1;
            card.pendingFanIndex = -1;
            card.resyncControls();
        }
    }

    function clearPendingIfIdle() {
        if (card.pendingTarget < 0 && card.pendingModeIndex < 0 && card.pendingFanIndex < 0) {
            pendingTimeoutTimer.stop();
        }
    }

    // Re-assert every control from the last polled state. Controls are set
    // imperatively (not bound) because user interaction would otherwise
    // break the binding and the control would stop tracking polls. The
    // syncing guard keeps programmatic sets from re-sending commands.
    function resyncControls() {
        const on = card.st ? !!card.st.power : false;
        if (powerSwitch.checked !== on) {
            powerSwitch.syncing = true;
            powerSwitch.checked = on;
            powerSwitch.syncing = false;
        }
        if (card.pendingFanIndex < 0) {
            const fi = Math.max(0, card.fans.indexOf(card.st ? card.st.fan : "Auto"));
            if (Math.round(fanSlider.value) !== fi) {
                fanSlider.value = fi;
            }
        }
        if (card.pendingTarget < 0) {
            const target = card.st?.target ?? 24;
            if (Math.round(tempSlider.value) !== target) {
                tempSlider.value = target;
            }
        }
    }

    function nudgeTarget(delta) {
        card.pendingTarget = Math.min(30, Math.max(16, card.shownTarget + delta));
        tempSlider.value = card.shownTarget;
        tempCommandTimer.restart();
    }

    Kirigami.Separator {
        Layout.fillWidth: true
    }

    RowLayout {
        Layout.fillWidth: true
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            PlasmaComponents.Label {
                Layout.fillWidth: true
                text: card.title
                font.bold: true
                elide: Text.ElideRight
            }
            PlasmaComponents.Label {
                visible: card.roomLine !== ""
                Layout.fillWidth: true
                opacity: 0.7
                elide: Text.ElideRight
                font.pointSize: Kirigami.Theme.smallFont.pointSize
                text: card.roomLine
            }
        }
        PlasmaComponents.Switch {
            id: powerSwitch
            property bool syncing: false
            Accessible.name: card.title + " power"
            enabled: card.ready
            Component.onCompleted: {
                powerSwitch.syncing = true;
                powerSwitch.checked = card.st ? !!card.st.power : false;
                powerSwitch.syncing = false;
            }
            onToggled: {
                if (!powerSwitch.syncing) {
                    root.send("--set-power " + (checked ? "on" : "off") + card.unitArg);
                }
            }
        }
    }

    Kirigami.InlineMessage {
        visible: (card.unit?.error ?? "") !== ""
        Layout.fillWidth: true
        type: Kirigami.MessageType.Error
        text: card.unit?.error ?? ""
        actions: [
            Kirigami.Action {
                icon.name: "view-refresh"
                text: "Retry"
                onTriggered: root.requestRefresh()
            }
        ]
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
        enabled: card.ready
        opacity: card.isOn ? 1.0 : 0.55

        RowLayout {
            Layout.fillWidth: true

            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: event => {
                    if (event.angleDelta.y > 0) {
                        card.nudgeTarget(1);
                    } else if (event.angleDelta.y < 0) {
                        card.nudgeTarget(-1);
                    }
                }
            }

            PlasmaComponents.ToolButton {
                Accessible.name: "Decrease target temperature for " + card.title
                text: "−"
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.3
                flat: false
                implicitWidth: Kirigami.Units.gridUnit * 2
                implicitHeight: Kirigami.Units.gridUnit * 2
                enabled: card.shownTarget > 16
                onClicked: card.nudgeTarget(-1)
            }
            PlasmaComponents.Label {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.6
                font.bold: true
                text: card.st ? (card.shownTarget + (card.st.fahrenheit ? "°F" : "°C")) : "-"
            }
            PlasmaComponents.ToolButton {
                Accessible.name: "Increase target temperature for " + card.title
                text: "+"
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.3
                flat: false
                implicitWidth: Kirigami.Units.gridUnit * 2
                implicitHeight: Kirigami.Units.gridUnit * 2
                enabled: card.shownTarget < 30
                onClicked: card.nudgeTarget(1)
            }
            Timer {
                id: tempCommandTimer
                interval: 400
                onTriggered: {
                    if (card.pendingTarget >= 0) {
                        root.send("--set-temp " + card.pendingTarget + card.unitArg);
                        pendingTimeoutTimer.restart();
                    }
                }
            }
        }

        PlasmaComponents.Slider {
            id: tempSlider
            Accessible.name: "Target temperature for " + card.title
            Layout.fillWidth: true
            from: 16
            to: 30
            stepSize: 1
            snapMode: PlasmaComponents.Slider.SnapAlways
            Component.onCompleted: tempSlider.value = card.shownTarget
            onMoved: {
                card.pendingTarget = Math.round(value);
                tempCommandTimer.restart();
            }
        }

        PlasmaComponents.Label {
            text: "Mode"
            opacity: 0.7
            font.pointSize: Kirigami.Theme.smallFont.pointSize
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing / 2

            Repeater {
                model: card.modes
                delegate: PlasmaComponents.ToolButton {
                    required property int index
                    required property string modelData
                    Layout.fillWidth: true
                    highlighted: card.shownModeIndex === index
                    flat: !highlighted
                    icon.name: card.modeIcons[index]
                    text: modelData
                    display: PlasmaComponents.AbstractButton.TextUnderIcon
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                    Accessible.name: modelData + " mode for " + card.title
                    onClicked: {
                        card.pendingModeIndex = index;
                        modeCommandTimer.restart();
                    }
                }
            }

            Timer {
                id: modeCommandTimer
                interval: 500
                onTriggered: {
                    if (card.pendingModeIndex >= 0) {
                        root.send("--set-mode " + card.modes[card.pendingModeIndex] + card.unitArg);
                        pendingTimeoutTimer.restart();
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            PlasmaComponents.Label {
                Layout.fillWidth: true
                text: "Fan"
                opacity: 0.7
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }
            PlasmaComponents.Label {
                text: card.fans[card.shownFanIndex]
                font.bold: true
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }
        }

        PlasmaComponents.Slider {
            id: fanSlider
            Accessible.name: "Fan speed for " + card.title
            Layout.fillWidth: true
            from: 0
            to: card.fans.length - 1
            stepSize: 1
            snapMode: PlasmaComponents.Slider.SnapAlways
            Component.onCompleted: fanSlider.value = card.shownFanIndex
            onMoved: {
                card.pendingFanIndex = Math.round(value);
                fanCommandTimer.restart();
            }
            Timer {
                id: fanCommandTimer
                interval: 500
                onTriggered: {
                    if (card.pendingFanIndex >= 0) {
                        root.send("--set-fan " + card.fans[card.pendingFanIndex] + card.unitArg);
                        pendingTimeoutTimer.restart();
                    }
                }
            }
        }
    }
}
