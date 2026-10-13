//! D-Bus service for the headless daemon, consumed by the Plasma applet and
//! the `gree-ac-plasma --set-*` / `--state-json` client commands.
//!
//! Service `com.greeac.Controller`, object `/com/greeac/Controller`,
//! interface `com.greeac.Controller`:
//!
//! * `GetState() -> s`: the whole UI state as JSON (see [`snapshot_json`]).
//! * `Refresh()`, `SetLinked(b)`, `SetPower(i, b)`, `SetMode(i, s)`,
//!   `SetTemp(i, y)`, `SetFan(i, s)`: send a command to the controller.
//!   The unit index is `-1` for "both ACs together", otherwise `0`, `1`, ...

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::mpsc::UnboundedSender;
use zbus::interface;

use crate::model::{FanSpeed, Mode, TEMP_MAX, TEMP_MIN};
use crate::state::{Command, Snapshot};

pub const SERVICE_NAME: &str = "com.greeac.Controller";
pub const OBJECT_PATH: &str = "/com/greeac/Controller";
pub const INTERFACE_NAME: &str = "com.greeac.Controller";

#[derive(Serialize)]
struct UnitStateDto {
    power: bool,
    mode: String,
    target: u8,
    fan: String,
    fahrenheit: bool,
    /// Raw `TemSen` room-sensor value as reported by the unit.
    room_raw: Option<i64>,
    /// `room_raw` decoded to Celsius (`TemSen` minus 40).
    room_celsius: Option<i64>,
    summary: String,
}

#[derive(Serialize)]
struct UnitDto {
    index: usize,
    name: String,
    address: String,
    model: String,
    online: bool,
    state: Option<UnitStateDto>,
    error: Option<String>,
}

#[derive(Serialize)]
struct SnapshotDto {
    linked: bool,
    home: bool,
    following_wifi: bool,
    message: Option<String>,
    units: Vec<UnitDto>,
}

/// Serialize a controller snapshot for the applet. Serialization cannot fail
/// for these types; on error the applet gets an empty state.
pub fn snapshot_json(snapshot: &Snapshot) -> String {
    let dto = SnapshotDto {
        linked: snapshot.linked,
        home: snapshot.home,
        following_wifi: snapshot.following_wifi,
        message: snapshot.message.clone(),
        units: snapshot
            .units
            .iter()
            .enumerate()
            .map(|(index, unit)| UnitDto {
                index,
                name: unit.name.clone(),
                address: unit.address.clone(),
                model: unit.model.clone(),
                online: unit.online,
                state: unit.state.map(|state| UnitStateDto {
                    power: state.power,
                    mode: state.mode.label().to_owned(),
                    target: state.target,
                    fan: state.fan.label().to_owned(),
                    fahrenheit: state.fahrenheit,
                    room_raw: state.room_raw,
                    room_celsius: state.room_celsius(),
                    summary: state.summary(),
                }),
                error: unit.error.clone(),
            })
            .collect(),
    };
    serde_json::to_string(&dto).unwrap_or_else(|_| "{\"units\":[]}".to_owned())
}

/// Match a mode label case-insensitively (`Cool`, `cool`, ...).
pub fn parse_mode(text: &str) -> Option<Mode> {
    let want = text.trim().to_lowercase();
    Mode::ALL
        .iter()
        .copied()
        .find(|mode| mode.label().to_lowercase() == want)
}

/// Match a fan label leniently (`Medium-high`, `mediumhigh`, ...).
pub fn parse_fan(text: &str) -> Option<FanSpeed> {
    let want = alphanumeric_lower(text);
    FanSpeed::ALL
        .iter()
        .copied()
        .find(|fan| alphanumeric_lower(fan.label()) == want)
}

fn alphanumeric_lower(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// `-1` (or any negative) means "both ACs"; otherwise a unit index.
fn unit_index(unit: i32) -> Option<usize> {
    usize::try_from(unit).ok()
}

pub struct ControllerApi {
    commands: UnboundedSender<Command>,
    cached: Arc<Mutex<String>>,
}

impl ControllerApi {
    pub fn new(commands: UnboundedSender<Command>, cached: Arc<Mutex<String>>) -> Self {
        ControllerApi { commands, cached }
    }
}

#[interface(name = "com.greeac.Controller")]
impl ControllerApi {
    async fn get_state(&self) -> String {
        self.cached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    async fn refresh(&self) {
        let _ = self.commands.send(Command::Refresh);
    }

    async fn set_linked(&self, linked: bool) {
        let _ = self.commands.send(Command::SetLinked(linked));
    }

    async fn set_power(&self, unit: i32, on: bool) {
        let _ = self.commands.send(Command::SetPower {
            unit: unit_index(unit),
            on,
        });
    }

    async fn set_mode(&self, unit: i32, mode: String) {
        if let Some(mode) = parse_mode(&mode) {
            let _ = self.commands.send(Command::SetMode {
                unit: unit_index(unit),
                mode,
            });
        }
    }

    async fn set_temp(&self, unit: i32, temp: u8) {
        let temp = temp.clamp(TEMP_MIN, TEMP_MAX);
        let _ = self.commands.send(Command::SetTemp {
            unit: unit_index(unit),
            temp,
        });
    }

    async fn set_fan(&self, unit: i32, fan: String) {
        if let Some(fan) = parse_fan(&fan) {
            let _ = self.commands.send(Command::SetFan {
                unit: unit_index(unit),
                fan,
            });
        }
    }
}

/// Call a daemon method on the session bus. Maps a missing service to a
/// human-readable hint instead of a raw D-Bus error.
pub async fn call_daemon<R, A>(method: &str, args: &A) -> Result<R, String>
where
    A: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
{
    let connection = zbus::Connection::session()
        .await
        .map_err(|error| format!("could not reach the session bus: {error}"))?;
    let proxy = zbus::Proxy::new(&connection, SERVICE_NAME, OBJECT_PATH, INTERFACE_NAME)
        .await
        .map_err(|error| format!("could not talk to the daemon: {error}"))?;
    proxy.call(method, args).await.map_err(|error| {
        if matches!(
            &error,
            zbus::Error::FDO(e) if matches!(e.as_ref(), zbus::fdo::Error::ServiceUnknown(_))
        ) {
            "the gree-ac-plasma daemon is not running (start it with `gree-ac-plasma`)".to_owned()
        } else {
            format!("daemon call {method} failed: {error}")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mode_labels() {
        assert_eq!(parse_mode("Cool"), Some(Mode::Cool));
        assert_eq!(parse_mode("heat"), Some(Mode::Heat));
        assert_eq!(parse_mode(" Turbo "), None);
    }

    #[test]
    fn parses_fan_labels_leniently() {
        assert_eq!(parse_fan("Medium-high"), Some(FanSpeed::MediumHigh));
        assert_eq!(parse_fan("mediumhigh"), Some(FanSpeed::MediumHigh));
        assert_eq!(parse_fan("Low"), Some(FanSpeed::Low));
        assert_eq!(parse_fan("ludicrous"), None);
    }

    #[test]
    fn negative_unit_means_both() {
        assert_eq!(unit_index(-1), None);
        assert_eq!(unit_index(1), Some(1));
    }
}
