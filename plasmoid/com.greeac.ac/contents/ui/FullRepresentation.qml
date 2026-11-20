pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents
import org.kde.plasma.extras as PlasmaExtras
import org.kde.kirigami as Kirigami

// The native popup anchored to the panel icon. Built on the same
// PlasmaExtras.Representation container as the stock widgets. The title
// bar comes from a BasicPlasmoidHeading page header (stock convention for
// panel-hosted applets). The popup follows the content's implicit height,
// and the scroll view handles content taller than the available popup area.
PlasmaExtras.Representation {
    id: fullRoot

    readonly property real heightLimit: Kirigami.Units.gridUnit * 60

    Layout.minimumWidth: root.switchWidth
    Layout.minimumHeight: 0
    Layout.preferredWidth: Kirigami.Units.gridUnit * 20
    Layout.preferredHeight: Math.min(fullRoot.heightLimit,
        (heading.visible ? heading.implicitHeight : 0)
        + contentBody.implicitHeight + topPadding + bottomPadding)
    Layout.maximumWidth: Kirigami.Units.gridUnit * 40
    Layout.maximumHeight: fullRoot.heightLimit

    collapseMarginsHint: true

    // Title bar ("Gree AC" from the package metadata) with the actions and
    // configure buttons. Hidden automatically where the containment draws
    // its own heading (system tray), which is what provides the back/pin
    // chrome around tray-hosted stock popups.
    header: PlasmaExtras.BasicPlasmoidHeading {
        id: heading

        PlasmaComponents.ToolButton {
            icon.name: "view-refresh"
            display: PlasmaComponents.AbstractButton.IconOnly
            Accessible.name: "Refresh"
            PlasmaComponents.ToolTip.text: "Refresh"
            PlasmaComponents.ToolTip.visible: hovered
            PlasmaComponents.ToolTip.delay: Kirigami.Units.toolTipDelay
            onClicked: root.daemonOnline ? root.requestRefresh() : root.refresh()
        }
    }

    // The scroll view reports its content's implicit size to the Page, so
    // changing linked mode or unit state resizes the popup. If it reaches the
    // popup height cap, the controls remain reachable by scrolling.
    contentItem: PlasmaComponents.ScrollView {
        id: content

        Item {
            id: contentBody
            width: content.availableWidth
            implicitWidth: body.implicitWidth + Kirigami.Units.largeSpacing * 2
            implicitHeight: body.implicitHeight + Kirigami.Units.largeSpacing * 2

            ColumnLayout {
                id: body
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.leftMargin: Kirigami.Units.largeSpacing
                anchors.rightMargin: Kirigami.Units.largeSpacing
                y: Kirigami.Units.largeSpacing
                spacing: Kirigami.Units.smallSpacing

                Kirigami.PlaceholderMessage {
                    visible: !root.daemonOnline
                    Layout.fillWidth: true
                    Layout.topMargin: Kirigami.Units.largeSpacing
                    Layout.bottomMargin: Kirigami.Units.largeSpacing
                    icon.name: "network-disconnect"
                    text: "Daemon not running"
                    explanation: "Start the gree-ac-plasma user service, or run gree-ac-plasma manually."
                    helpfulAction: Kirigami.Action {
                        icon.name: "media-playback-start"
                        text: root.daemonStarting ? "Starting…" : "Start daemon"
                        enabled: !root.daemonStarting
                        onTriggered: root.startDaemon()
                    }
                }

                Kirigami.InlineMessage {
                    visible: !root.daemonOnline && root.daemonStartError !== ""
                    Layout.fillWidth: true
                    type: Kirigami.MessageType.Error
                    text: root.daemonStartError
                }

                ColumnLayout {
                    visible: root.daemonOnline
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    RowLayout {
                        Layout.fillWidth: true
                        PlasmaComponents.Switch {
                            id: linkedSwitch
                            property bool syncing: false
                            Accessible.name: "Link both ACs"
                            Component.onCompleted: {
                                linkedSwitch.syncing = true;
                                linkedSwitch.checked = root.ac.linked ?? false;
                                linkedSwitch.syncing = false;
                            }
                            onToggled: {
                                if (!linkedSwitch.syncing) {
                                    root.send("--set-linked " + (checked ? "on" : "off"));
                                }
                            }
                            Connections {
                                target: root
                                function onAcChanged() {
                                    linkedSwitch.syncing = true;
                                    linkedSwitch.checked = root.ac.linked ?? false;
                                    linkedSwitch.syncing = false;
                                }
                                function onCommandFailed() {
                                    linkedSwitch.syncing = true;
                                    linkedSwitch.checked = root.ac.linked ?? false;
                                    linkedSwitch.syncing = false;
                                }
                            }
                        }
                        PlasmaComponents.Label {
                            Layout.fillWidth: true
                            text: "Link both ACs"
                        }
                        PlasmaComponents.BusyIndicator {
                            visible: root.commandBusy
                            running: root.commandBusy
                            Layout.preferredWidth: Kirigami.Units.iconSizes.small
                            Layout.preferredHeight: Kirigami.Units.iconSizes.small
                        }
                        Rectangle {
                            visible: root.ac.following_wifi ?? false
                            implicitWidth: badgeRow.implicitWidth + Kirigami.Units.smallSpacing * 2
                            implicitHeight: badgeRow.implicitHeight + Kirigami.Units.smallSpacing
                            radius: height / 2
                            color: Qt.alpha(root.ac.home ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor, 0.15)
                            border.width: 1
                            border.color: Qt.alpha(root.ac.home ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor, 0.35)

                            RowLayout {
                                id: badgeRow
                                anchors.centerIn: parent
                                spacing: Kirigami.Units.smallSpacing / 2

                                Kirigami.Icon {
                                    Layout.preferredWidth: Kirigami.Units.iconSizes.small
                                    Layout.preferredHeight: Kirigami.Units.iconSizes.small
                                    source: "network-wireless-symbolic"
                                    color: root.ac.home ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                                }
                                PlasmaComponents.Label {
                                    text: root.ac.home ? "Home" : "Away"
                                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                                    font.bold: true
                                    color: root.ac.home ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                                }
                            }
                        }
                    }

                    Kirigami.InlineMessage {
                        visible: root.bannerError !== ""
                        Layout.fillWidth: true
                        type: Kirigami.MessageType.Error
                        text: root.bannerError
                        actions: [
                            Kirigami.Action {
                                icon.name: "view-refresh"
                                text: "Retry"
                                onTriggered: root.requestRefresh()
                            }
                        ]
                    }

                    PlasmaComponents.Label {
                        visible: (root.ac.units ?? []).length === 0
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: "No units found. Check that this computer and the ACs share a network."
                    }

                    AcCard {
                        visible: root.ac.linked && root.bothUnit !== null
                        unitIndex: -1
                        title: "Both ACs"
                        unit: root.bothUnit
                        roomLine: root.bothRoomText
                    }

                    Repeater {
                        model: root.ac.linked ? unitModel : null
                        delegate: RowLayout {
                            required property string name
                            required property string summary
                            required property bool online
                            required property bool hasState
                            Layout.fillWidth: true
                            opacity: 0.75
                            PlasmaComponents.Label {
                                Layout.fillWidth: true
                                font.pointSize: Kirigami.Theme.smallFont.pointSize
                                font.bold: true
                                elide: Text.ElideRight
                                text: name
                            }
                            PlasmaComponents.Label {
                                font.pointSize: Kirigami.Theme.smallFont.pointSize
                                text: (online && hasState) ? summary : "offline"
                            }
                        }
                    }

                    Repeater {
                        model: root.ac.linked ? null : unitModel
                        delegate: AcCard {
                            required property int index
                            required property string name
                            required property bool online
                            required property bool hasState
                            required property bool power
                            required property string mode
                            required property int target
                            required property string fan
                            required property bool fahrenheit
                            required property string roomText
                            required property string error
                            unitIndex: index
                            title: name
                            roomLine: roomText
                            unit: ({
                                online: online,
                                error: error,
                                state: hasState ? {
                                    power: power,
                                    mode: mode,
                                    target: target,
                                    fan: fan,
                                    fahrenheit: fahrenheit
                                } : null
                            })
                        }
                    }

                    PlasmaComponents.Label {
                        visible: root.lastChecked !== ""
                        Layout.fillWidth: true
                        Layout.topMargin: Kirigami.Units.smallSpacing
                        horizontalAlignment: Text.AlignRight
                        opacity: 0.55
                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                        text: "Checked " + root.lastChecked
                    }
                }
            }
        }
    }
}
