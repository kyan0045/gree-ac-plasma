pragma ComponentBehavior: Bound

import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.plasma5support as Plasma5Support
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root

    switchWidth: Kirigami.Units.gridUnit * 18
    switchHeight: Kirigami.Units.gridUnit * 24

    // Last state polled from the daemon (`gree-ac-plasma --state-json`).
    property var ac: ({ linked: true, home: true, following_wifi: false, message: "", units: [] })
    property bool daemonOnline: false
    // Last accepted poll payload. Polls that change nothing skip the model
    // assignment below, so open popups are not torn down and rebuilt for
    // identical data.
    property string lastStateJson: ""
    // When the current poll started; guards against a poll that never
    // reports back (e.g. the helper is missing from PATH), which would
    // otherwise wedge `connectedSources` forever.
    property double pollStarted: 0
    // Outgoing command queue. The executable engine runs one source line at
    // a time, so queueing avoids replacing `connectedSources` mid-flight
    // and dropping rapid commands such as repeated temp steps.
    property var commandQueue: []
    property bool commandBusy: false
    // When the in-flight command started; guards against a command that
    // never reports back, which would otherwise stall the queue forever.
    property double commandStarted: 0

    signal commandFailed(string details)

    readonly property string helper: {
        const configured = String(Plasmoid.configuration.helperBinary ?? "").trim();
        return configured !== "" ? configured : "gree-ac-plasma";
    }

    // Unit shown on the shared "Both ACs" card: first online unit, else first.
    readonly property var bothUnit: {
        const units = root.ac.units ?? [];
        return units.find(u => u.state) ?? units[0] ?? null;
    }

    // Room line for the shared card, which has no per-unit object of its own.
    readonly property string bothRoomText: {
        const r = root.bothUnit?.state?.room_celsius ?? null;
        if (r === null || r === undefined) {
            return "";
        }
        if (root.bothUnit.state.fahrenheit) {
            return "Room: " + Math.round(r * 9 / 5 + 32) + "°F";
        }
        return "Room: " + r + "°C";
    }

    // First unit that is currently running, if any (drives the panel icon
    // and middle-click toggle).
    readonly property var activeUnit: {
        const units = root.ac.units ?? [];
        return units.find(u => u.state && u.state.power) ?? null;
    }

    // Unit rows as a stable model. Polls sync values IN PLACE (set/append)
    // instead of replacing the array: wholesale replacement destroys and
    // rebuilds every popup delegate, which races layout measurement.
    ListModel {
        id: unitModel
    }

    function syncUnits(arr) {
        const list = arr ?? [];
        while (unitModel.count > list.length) {
            unitModel.remove(unitModel.count - 1);
        }
        for (let i = 0; i < list.length; i++) {
            const u = list[i] ?? {};
            const s = u.state ?? {};
            const room = (s.room_celsius === undefined || s.room_celsius === null) ? null : s.room_celsius;
            const entry = {
                name: u.name ?? "",
                online: !!u.online,
                hasState: !!u.state,
                power: !!s.power,
                mode: s.mode ?? "Auto",
                target: s.target ?? 24,
                fan: s.fan ?? "Auto",
                fahrenheit: !!s.fahrenheit,
                roomText: room === null ? "" : (s.fahrenheit
                    ? "Room: " + Math.round(room * 9 / 5 + 32) + "°F"
                    : "Room: " + room + "°C"),
                summary: s.summary ?? "",
                error: u.error ?? ""
            };
            if (i < unitModel.count) {
                unitModel.set(i, entry);
            } else {
                unitModel.append(entry);
            }
        }
    }

    // Neutral thermometer normally; cold/warm while actively cooling/heating,
    // mirroring how Media Player swaps its icon by playback state.
    Plasmoid.icon: {
        const unit = root.activeUnit;
        if (!unit) {
            return "temperature-normal";
        }
        switch (unit.state.mode) {
        case "Cool":
            return "temperature-cold";
        case "Heat":
            return "temperature-warm";
        default:
            return "temperature-normal";
        }
    }
    Plasmoid.status: PlasmaCore.Types.ActiveStatus
    toolTipMainText: "Gree AC"
    toolTipSubText: root.tooltipText()
    toolTipTextFormat: Text.PlainText

    compactRepresentation: CompactRepresentation {}
    fullRepresentation: FullRepresentation {}

    // Polls `GetState` from the daemon. `connectedSources` is only set while
    // no poll is in flight; each result disconnects and re-arms via Timer.
    // A nonzero exit status (or empty stdout) means the daemon is down.
    Plasma5Support.DataSource {
        id: stateSource
        engine: "executable"
        connectedSources: []
        onNewData: (sourceName, data) => {
            stateSource.disconnectSource(sourceName);
            const exitCode = Number(data["exit code"] ?? 0);
            const out = String(data["stdout"] ?? "").trim();
            if (exitCode !== 0 || out === "") {
                root.daemonOnline = false;
                return;
            }
            if (out === root.lastStateJson) {
                root.daemonOnline = true;
                return;
            }
            try {
                const parsed = JSON.parse(out);
                root.ac = parsed;
                root.syncUnits(parsed.units);
                root.lastStateJson = out;
                root.daemonOnline = true;
            } catch (e) {
                root.daemonOnline = false;
            }
        }
    }

    // Commands run one at a time from the queue. Failures surface as a
    // transient message instead of a blind refresh, so controls can fall
    // back to the last polled state instead of showing a lying value.
    Plasma5Support.DataSource {
        id: commandSource
        engine: "executable"
        connectedSources: []
        onNewData: (sourceName, data) => {
            commandSource.disconnectSource(sourceName);
            root.commandBusy = false;
            const exitCode = Number(data["exit code"] ?? 0);
            const err = String(data["stderr"] ?? "").trim();
            if (exitCode !== 0 || err !== "") {
                root.failCommand(err !== "" ? err : "Command failed (exit " + exitCode + ")");
            } else {
                refreshTimer.restart();
            }
            root.pumpCommands();
        }
    }

    Timer {
        id: refreshTimer
        interval: 900
        onTriggered: root.refresh()
    }

    Timer {
        interval: Math.max(2000, Number(Plasmoid.configuration.pollInterval) || 8000)
        running: true
        repeat: true
        onTriggered: {
            if (root.commandBusy && Date.now() - root.commandStarted > 10000) {
                commandSource.connectedSources = [];
                root.commandBusy = false;
                root.pumpCommands();
            }
            root.refresh();
        }
    }

    // Background polling keeps the state at most one interval old, so opening
    // the popup needs no extra refresh (and no model swap mid-animation).
    Component.onCompleted: root.refresh()

    function refresh() {
        const line = root.commandLine("--state-json");
        if (line === "") {
            return;
        }
        if (stateSource.connectedSources.length === 0) {
            pollStarted = Date.now();
            stateSource.connectedSources = [line];
        } else if (Date.now() - pollStarted > 10000) {
            stateSource.connectedSources = [];
            pollStarted = Date.now();
            stateSource.connectedSources = [line];
        }
    }

    // The helper setting must be a single binary name or path. Anything
    // with shell metacharacters is rejected so it cannot inject commands.
    function helperValid(value) {
        return /^[A-Za-z0-9_\/:@%+=,.-]+$/.test(String(value));
    }

    function shellQuote(value) {
        return "'" + String(value).replace(/'/g, "'\\''") + "'";
    }

    function commandLine(args) {
        const h = root.helper;
        if (!root.helperValid(h)) {
            return "";
        }
        return root.shellQuote(h) + " " + args;
    }

    function failCommand(details) {
        const text = details !== "" ? details : "Command failed";
        const next = Object.assign({}, root.ac);
        next.message = text;
        root.ac = next;
        root.commandFailed(text);
    }

    function unitKey(args) {
        const m = /--unit\s+(\S+)/.exec(String(args));
        return m ? m[1] : "";
    }

    function send(args) {
        const clean = String(args);
        const line = root.commandLine(clean);
        if (line === "") {
            root.failCommand("Invalid helper binary. Set a single command name or path in settings.");
            return;
        }
        // Coalesce rapid temp tweaks for the same unit: only the latest
        // value still needs to run.
        if (clean.indexOf("--set-temp ") === 0) {
            const key = root.unitKey(clean);
            for (let i = 0; i < commandQueue.length; i++) {
                const queued = String(commandQueue[i]);
                if (queued.indexOf("--set-temp ") === 0 && root.unitKey(queued) === key) {
                    commandQueue[i] = clean;
                    root.pumpCommands();
                    return;
                }
            }
        }
        commandQueue.push(clean);
        root.pumpCommands();
    }

    function pumpCommands() {
        if (root.commandBusy || root.commandQueue.length === 0) {
            return;
        }
        const args = String(commandQueue.shift());
        const line = root.commandLine(args);
        if (line === "") {
            root.failCommand("Invalid helper binary. Set a single command name or path in settings.");
            root.pumpCommands();
            return;
        }
        root.commandBusy = true;
        root.commandStarted = Date.now();
        commandSource.connectedSources = [line];
    }

    function tooltipText() {
        if (!root.daemonOnline) {
            return "Daemon not running";
        }
        const units = root.ac.units ?? [];
        if (units.length === 0) {
            return "No units found";
        }
        return units.map(u => u.state ? (u.state.summary ?? "") : "offline").join(" · ");
    }
}
