import QtQuick
import org.kde.plasma.components as PlasmaComponents
import org.kde.kirigami as Kirigami

Kirigami.FormLayout {
    property alias cfg_helperBinary: helperField.text
    property alias cfg_pollInterval: pollSpin.value
    property alias cfg_showTemperature: showTempBox.checked
    property alias cfg_middleClickToggles: middleClickBox.checked

    PlasmaComponents.TextField {
        id: helperField
        Kirigami.FormData.label: "Helper command:"
        placeholderText: "Single binary, e.g. gree-ac-plasma"
    }
    PlasmaComponents.SpinBox {
        id: pollSpin
        from: 2000
        to: 60000
        stepSize: 1000
        Kirigami.FormData.label: "Poll interval (ms):"
    }
    PlasmaComponents.CheckBox {
        id: showTempBox
        text: "Show set temperature in the panel"
    }
    PlasmaComponents.CheckBox {
        id: middleClickBox
        text: "Middle-click the panel icon toggles both ACs"
    }
}
