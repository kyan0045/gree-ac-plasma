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
        + content.implicitHeight + topPadding + bottomPadding)
    Layout.maximumWidth: Kirigami.Units.gridUnit * 40
    Layout.maximumHeight: fullRoot.heightLimit

    collapseMarginsHint: true

    // Title bar ("Gree AC" from the package metadata) with the actions and
    // configure buttons. Hidden automatically where the containment draws
    // its own heading (system tray), which is what provides the back/pin
    // chrome around tray-hosted stock popups.
    header: PlasmaExtras.BasicPlasmoidHeading {
        id: heading

        PlasmaComponents.Button {
            icon.name: "view-refresh"
            text: "Refresh"
            onClicked: root.refresh()
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

                RowLayout {
                    Layout.fillWidth: true
                    Item {
                        Layout.fillWidth: true
                    }
                    PlasmaComponents.Label {
                        visible: (root.ac.following_wifi ?? false)
                        text: root.ac.home ? "HOME" : "AWAY"
                        font.bold: true
                        color: root.ac.home ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                    }
                }

                PlasmaComponents.Label {
                    visible: !root.daemonOnline
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: "Daemon not running.\nStart it with: gree-ac-plasma"
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
                    }

                    PlasmaComponents.Label {
                        visible: (root.ac.message ?? "") !== ""
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        opacity: 0.7
                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                        text: root.ac.message ?? ""
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
                            PlasmaComponents.Label {
                                Layout.fillWidth: true
                                font.bold: true
                                elide: Text.ElideRight
                                text: name
                            }
                            PlasmaComponents.Label {
                                opacity: 0.75
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
                }
            }
        }
    }
}
