# gree-ac-plasma

A Rust controller daemon plus a native Plasma 6 applet for Gree air
conditioners on Linux (KDE Plasma / Wayland). It talks to the units directly
over the local network using Gree's own UDP protocol.

It was built for a pair of `GWH12AWBXB-K6DNA1F/I` units and can drive both at
once, or each separately.

## Features

- **Native panel widget** ("Gree AC"): a thermometer icon in the toolbar with
  a popup anchored to it, styled by your Plasma theme like Weather or Media
  Player. The icon turns cold/warm while actively cooling/heating, and hovering
  shows each unit's state. Each card
  controls power, mode, temperature (`-` / `+`, 16-30°C) and fan. The panel
  shows the set temperature while a unit runs, and a middle-click optionally
  toggles both ACs (off by default; see widget settings).
- **Linked control**: a "Link both ACs" switch sends one action to both units;
  turn it off to control each AC separately.
- **Headless daemon** owns the units and serves state over D-Bus
  (`com.greeac.Controller`); the widget polls it and sends commands back.
  Failed commands also raise a Plasma notification, so they are visible even
  with the popup closed.
- Follows a chosen home Wi-Fi network (shown as a HOME/AWAY badge).
- Reads each unit's power, mode, set temperature and fan, plus the room
  temperature decoded from the unit's sensor (`TemSen` minus 40,
  community-reversed; whole degrees).
- Retries and reconnects automatically when a unit drops off the network.

Set `RUST_LOG=info` (or `debug`) to see more diagnostics on stderr.

## Prerequisites

- Recent Rust stable (the crate uses edition 2024).
- Plasma 6 with `kpackagetool6` for the widget install.
- NetworkManager for SSID detection behind the HOME/AWAY badge.

## Build

```sh
cargo build --release
```

## Run

Install the binary, start the daemon, and install the widget:

```sh
sudo install -Dm755 target/release/gree-ac-plasma /usr/local/bin/gree-ac-plasma
gree-ac-plasma &              # or enable the user service below
./plasmoid/install.sh       # installs the "Gree AC" widget
```

Then right-click the panel and select **Add or manage widgets**, then **Gree AC**.
Clicking its toolbar icon opens the popup; right-clicking it offers Refresh.
Use Configure in the widget menu to set the helper command, poll interval,
panel temperature, and middle-click behaviour. Defaults are helper
`gree-ac-plasma`, poll interval 8000ms (minimum 2000ms), panel temperature
display on, and middle-click toggle off.

`gree-ac-plasma --daemon` is the same as bare `gree-ac-plasma`. A trailing
`&` runs the daemon only for the current shell session; enable the user
service below to keep it running across logins. Service logs are available
with `journalctl --user -u gree-ac-plasma`, combined with `RUST_LOG=info`
(or `debug`) for more detail.

### Scripting / terminal control (talks to the running daemon)

```sh
gree-ac-plasma --state-json                 # print the UI state as JSON
gree-ac-plasma --refresh                    # re-read everything now
gree-ac-plasma --set-linked on|off
gree-ac-plasma --set-power on|off [--unit N|both]
gree-ac-plasma --set-mode Auto|Cool|Dry|Fan|Heat [--unit N|both]
gree-ac-plasma --set-temp 16..30 [--unit N|both]
gree-ac-plasma --set-fan Auto|Low|Medium-low|Medium|Medium-high|High [--unit N|both]
```

`--unit N` is 0-based and defaults to both when omitted. Unit 0 is the
first unit in `--list` order; note `--list` prints 1-based display numbers,
so display number 1 is `--unit 0`.

Name your units (MACs are shown by `--list`); names are stored in the config
file and need an explicit `gree-ac-plasma --refresh` before the daemon
shows them:

```sh
gree-ac-plasma --set-name 001122aabbcc "Kitchen"
gree-ac-plasma --set-name 001122aabbcc ""   # clear it again
```

### Diagnostics (talk to the units directly)

```sh
gree-ac-plasma --list             # discover units and exit
gree-ac-plasma --read-state       # bind and print current state (read-only)
gree-ac-plasma --self-test        # re-send each unit its own settings
gree-ac-plasma --current-ssid     # print the connected Wi-Fi network
gree-ac-plasma --set-home-ssid N  # remember N as the home network
```

## Home network

Set your home Wi-Fi once:

```sh
gree-ac-plasma --set-home-ssid "MyHomeWiFi"
```

Settings are stored in `~/.config/gree-ac-plasma/config.json`:

| key          | meaning                                                        | default |
| ------------ | -------------------------------------------------------------- | ------- |
| `home_ssid`  | Wi-Fi name the daemon follows (HOME/AWAY badge)                 | unset   |
| `hide_away`  | pause polling and refuse commands when not on `home_ssid`      | `true`  |
| `buzzer`     | beep the unit when a command is sent (like the remote)          | `true`  |
| `linked`     | control both units together                                     | `true`  |
| `names`      | friendly names keyed by MAC address, e.g. `"001122aabbcc": "Kitchen"` | empty |

If `home_ssid` is unset, the daemon always considers itself home.

The daemon loads this file at startup and reloads it only on an explicit
`gree-ac-plasma --refresh`. `hide_away` and `buzzer` have no CLI flags;
edit `~/.config/gree-ac-plasma/config.json` directly to change them.

## Autostart

When upgrading an existing install, stop the service using the previous name
before enabling the renamed service:

```sh
systemctl --user disable --now gree-ac-tray.service
```

On startup the daemon reads `~/.config/gree-ac-tray/config.json` only when
`~/.config/gree-ac-plasma/config.json` is missing, then saves it under the
new name. Invalid JSON falls back to defaults.

Install the binary and the user service (the daemon; the widget needs it
running):

```sh
sudo install -Dm755 target/release/gree-ac-plasma /usr/local/bin/gree-ac-plasma
install -Dm644 packaging/gree-ac-plasma.service ~/.config/systemd/user/gree-ac-plasma.service
systemctl --user daemon-reload
systemctl --user enable --now gree-ac-plasma.service
```

Alternatively, add the command in **System Settings > Autostart**.

## How it works

Discovery is a UDP broadcast on port 7000. Controlling a unit requires a short
`bind` handshake that negotiates a per-session key; after that, state is read
with `status` and changed with `cmd` packets. This mirrors the community
[`greeclimate`](https://github.com/cmroche/greeclimate) implementation and the
`IRremoteESP8266` Gree protocol notes.

Room temperature comes from each unit's `TemSen` sensor value minus a base
offset of 40 (community-reversed; whole degrees only). Calibrated against a
thermometer, it reads within half a degree.

## Notes

- Both the computer and the ACs must be on the same LAN; Wi-Fi client isolation
  must be off.
- UDP broadcasts can be dropped, so discovery scans twice per run.

## License

MIT (c) kyan0045, see LICENSE.
