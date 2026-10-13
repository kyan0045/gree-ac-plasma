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
    readonly property var fans: ["Auto", "Low", "Medium-low", "Medium", "Medium-high", "High"]
    readonly property bool ready: (card.unit?.online ?? false) && card.st !== null
    // Optimistic target while temp steps debounce. The display and the
    // steppers use this, so rapid clicks do not compute from stale polled
    // state. Cleared once the poll confirms it or a command fails.
    property int pendingTarget: -1
    readonly property int shownTarget: card.pendingTarget >= 0 ? card.pendingTarget : (card.st?.target ?? 24)
    property int pendingModeIndex: -1
    property int pendingFanIndex: -1

    onStChanged: {
        if (card.pendingTarget >= 0 && card.st && card.st.target === card.pendingTarget) {
            card.pendingTarget = -1;
        }
        card.resyncControls();
    }

    Connections {
        target: root
        function onCommandFailed() {
            card.pendingTarget = -1;
            card.resyncControls();
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
        if (card.pendingModeIndex < 0) {
            const mi = Math.max(0, card.modes.indexOf(card.st ? card.st.mode : "Auto"));
            if (modeBox.currentIndex !== mi) {
                modeBox.currentIndex = mi;
            }
        }
        if (card.pendingFanIndex < 0) {
            const fi = Math.max(0, card.fans.indexOf(card.st ? card.st.fan : "Auto"));
            if (fanBox.currentIndex !== fi) {
                fanBox.currentIndex = fi;
            }
        }
    }

    function nudgeTarget(delta) {
        card.pendingTarget = Math.min(30, Math.max(16, card.shownTarget + delta));
        tempCommandTimer.restart();
    }

    Kirigami.Separator {
        Layout.fillWidth: true
    }

    RowLayout {
        Layout.fillWidth: true
        PlasmaComponents.Label {
            Layout.fillWidth: true
            text: card.title
            font.bold: true
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

    PlasmaComponents.Label {
        visible: (card.unit?.error ?? "") !== ""
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        opacity: 0.7
        font.pointSize: Kirigami.Theme.smallFont.pointSize
        text: card.unit?.error ?? ""
    }

    PlasmaComponents.Label {
        visible: card.roomLine !== ""
        Layout.fillWidth: true
        opacity: 0.6
        font.pointSize: Kirigami.Theme.smallFont.pointSize
        text: card.roomLine
    }

    RowLayout {
        Layout.fillWidth: true
        enabled: card.ready
        PlasmaComponents.ToolButton {
            Accessible.name: "Decrease target temperature for " + card.title
            text: "-"
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
            enabled: card.shownTarget < 30
            onClicked: card.nudgeTarget(1)
        }
        Timer {
            id: tempCommandTimer
            interval: 400
            onTriggered: {
                if (card.pendingTarget >= 0) {
                    root.send("--set-temp " + card.pendingTarget + card.unitArg);
                    card.pendingTarget = -1;
                }
            }
        }
    }

    PlasmaComponents.Label {
        text: "Mode"
        opacity: 0.7
        font.pointSize: Kirigami.Theme.smallFont.pointSize
    }
    PlasmaComponents.ComboBox {
        id: modeBox
        Accessible.name: "Mode for " + card.title
        Layout.fillWidth: true
        enabled: card.ready
        model: card.modes
        Component.onCompleted: modeBox.currentIndex = Math.max(0, card.modes.indexOf(card.st ? card.st.mode : "Auto"))
        onActivated: idx => {
            card.pendingModeIndex = idx;
            modeCommandTimer.restart();
        }
        Timer {
            id: modeCommandTimer
            interval: 500
            onTriggered: {
                if (card.pendingModeIndex >= 0) {
                    root.send("--set-mode " + card.modes[card.pendingModeIndex] + card.unitArg);
                    card.pendingModeIndex = -1;
                }
            }
        }
    }

    PlasmaComponents.Label {
        text: "Fan"
        opacity: 0.7
        font.pointSize: Kirigami.Theme.smallFont.pointSize
    }
    PlasmaComponents.ComboBox {
        id: fanBox
        Accessible.name: "Fan speed for " + card.title
        Layout.fillWidth: true
        enabled: card.ready
        model: card.fans
        Component.onCompleted: fanBox.currentIndex = Math.max(0, card.fans.indexOf(card.st ? card.st.fan : "Auto"))
        onActivated: idx => {
            card.pendingFanIndex = idx;
            fanCommandTimer.restart();
        }
        Timer {
            id: fanCommandTimer
            interval: 500
            onTriggered: {
                if (card.pendingFanIndex >= 0) {
                    root.send("--set-fan " + card.fans[card.pendingFanIndex] + card.unitArg);
                    card.pendingFanIndex = -1;
                }
            }
        }
    }
}
