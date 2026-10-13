//! Owns the units, talks to them, and publishes render snapshots.

use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{MissedTickBehavior, interval};

use crate::config::Config;
use crate::gree::{self, GreeClient};
use crate::model::{AcState, FanSpeed, Mode};
use crate::state::{Command, Snapshot, UnitView};
use crate::wifi;

struct Unit {
    info: gree::Device,
    client: Option<GreeClient>,
    state: Option<AcState>,
    error: Option<String>,
}

#[derive(Clone, Copy)]
enum Change {
    Power(bool),
    Mode(Mode),
    Temp(u8),
    Fan(FanSpeed),
}

pub struct Controller {
    config: Config,
    units: Vec<Unit>,
    home: bool,
    message: Option<String>,
    snapshot_tx: UnboundedSender<Snapshot>,
}

impl Controller {
    pub fn new(config: Config, snapshot_tx: UnboundedSender<Snapshot>) -> Self {
        Controller {
            config,
            units: Vec::new(),
            home: true,
            message: None,
            snapshot_tx,
        }
    }

    /// Run until Quit or the command channel closes.
    pub async fn run(mut self, mut commands: UnboundedReceiver<Command>) {
        self.update_home().await;
        self.ensure_discovered(false).await;
        self.ensure_connected().await;
        self.read_states().await;
        self.publish();

        let mut ticker = interval(Duration::from_secs(25));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(Command::Quit) | None => break,
                        Some(command) => self.handle(command).await,
                    }
                }
                _ = ticker.tick() => {
                    self.update_home().await;
                    self.ensure_discovered(false).await;
                    self.ensure_connected().await;
                    self.read_states().await;
                    self.publish();
                }
            }
        }
    }

    async fn update_home(&mut self) {
        self.home = match &self.config.home_ssid {
            Some(home_ssid) => wifi::current_ssid().await.as_deref() == Some(home_ssid.as_str()),
            None => true,
        };
    }

    fn following_wifi(&self) -> bool {
        self.config.home_ssid.is_some()
    }

    /// Off the home network the daemon pauses: no polling, no commands.
    fn away(&self) -> bool {
        self.config.hide_away && self.following_wifi() && !self.home
    }

    fn active(&self) -> bool {
        !self.away()
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::Quit => {}
            Command::Refresh => {
                self.message = None;
                // Re-read settings from disk (names may have changed).
                self.config = Config::load();
                self.update_home().await;
                self.ensure_discovered(true).await;
                self.ensure_connected().await;
                self.read_states().await;
                self.publish();
            }
            Command::SetLinked(linked) => {
                self.config.linked = linked;
                self.config.save();
                self.message = Some(if linked {
                    "Linked: both ACs move together".to_owned()
                } else {
                    "Unlinked: control each AC separately".to_owned()
                });
                self.publish();
            }
            Command::SetPower { unit, on } => self.apply(Change::Power(on), unit).await,
            Command::SetMode { unit, mode } => self.apply(Change::Mode(mode), unit).await,
            Command::SetTemp { unit, temp } => self.apply(Change::Temp(temp), unit).await,
            Command::SetFan { unit, fan } => self.apply(Change::Fan(fan), unit).await,
        }
    }

    async fn apply(&mut self, change: Change, unit: Option<usize>) {
        if !self.active() {
            self.message = Some("Not on the home network".to_owned());
            self.publish();
            return;
        }

        let targets: Vec<usize> = match unit {
            Some(index) if index < self.units.len() => vec![index],
            Some(index) => {
                self.message = Some(format!("Unknown unit {index}"));
                self.publish();
                return;
            }
            None => (0..self.units.len()).collect(),
        };
        if targets.is_empty() {
            self.message = Some("No units available".to_owned());
            self.publish();
            return;
        }

        // Make sure every target has a live connection first.
        for &index in &targets {
            self.ensure_unit_connected(index).await;
        }

        let beep = self.config.buzzer;
        let futures = targets.iter().map(|&index| {
            let client = self.units[index].client.as_ref();
            let props = command_properties(change, self.units[index].state);
            async move {
                match client {
                    Some(client) => client.send_command(&props, beep).await,
                    None => Err("unit is offline".into()),
                }
            }
        });
        let outcomes = futures::future::join_all(futures).await;

        let mut failures = Vec::new();
        for (&index, outcome) in targets.iter().zip(&outcomes) {
            match outcome {
                Ok(()) => self.units[index].error = None,
                Err(error) => {
                    self.units[index].error = Some(error.to_string());
                    failures.push(self.units[index].display_name(&self.config));
                }
            }
        }

        self.message = Some(if failures.is_empty() {
            if targets.len() > 1 {
                "Both ACs updated".to_owned()
            } else {
                "Updated".to_owned()
            }
        } else {
            let body = format!("Failed: {}", failures.join(", "));
            // The popup may be closed, so also surface this as a notification.
            crate::notify::command_failed(&body).await;
            body
        });

        // Give the units a moment to apply, then reflect the new state.
        tokio::time::sleep(Duration::from_millis(700)).await;
        for index in targets {
            self.read_unit_state(index).await;
        }
        self.publish();
    }

    async fn ensure_discovered(&mut self, force: bool) {
        if !self.units.is_empty() && !force {
            return;
        }
        match gree::discover(Duration::from_secs(3)).await {
            Ok(devices) => self.merge_devices(devices),
            Err(error) => {
                log::warn!("Discovery failed: {error}");
                self.message = Some(format!("Discovery failed: {error}"));
            }
        }
    }

    fn merge_devices(&mut self, devices: Vec<gree::Device>) {
        for device in devices {
            if let Some(unit) = self
                .units
                .iter_mut()
                .find(|unit| unit.info.mac == device.mac)
            {
                if unit.info.ip != device.ip {
                    // The unit moved; its session is no longer valid.
                    unit.client = None;
                }
                unit.info = device;
            } else {
                self.units.push(Unit {
                    info: device,
                    client: None,
                    state: None,
                    error: None,
                });
            }
        }
    }

    async fn ensure_connected(&mut self) {
        if !self.active() {
            return;
        }
        for index in 0..self.units.len() {
            self.ensure_unit_connected(index).await;
        }
    }

    async fn ensure_unit_connected(&mut self, index: usize) {
        if self.units[index].client.is_some() {
            return;
        }
        match GreeClient::connect(self.units[index].info.clone()).await {
            Ok(client) => {
                self.units[index].client = Some(client);
                self.units[index].error = None;
            }
            Err(error) => {
                log::warn!("Bind failed for {}: {error}", self.units[index].info.ip);
                self.units[index].error = Some(error.to_string());
            }
        }
    }

    async fn read_states(&mut self) {
        if !self.active() {
            return;
        }
        for index in 0..self.units.len() {
            self.read_unit_state(index).await;
        }
    }

    async fn read_unit_state(&mut self, index: usize) {
        let Some(client) = self.units[index].client.as_ref() else {
            return;
        };
        match client.read_state().await {
            Ok(state) => {
                self.units[index].state = Some(state);
                self.units[index].error = None;
            }
            Err(error) => {
                log::warn!("Read failed for {}: {error}", self.units[index].info.ip);
                self.units[index].error = Some(error.to_string());
                self.units[index].client = None;
            }
        }
    }

    fn publish(&self) {
        let snapshot = Snapshot {
            linked: self.config.linked,
            home: self.home,
            following_wifi: self.following_wifi(),
            message: self.message.clone(),
            units: self
                .units
                .iter()
                .map(|unit| UnitView {
                    name: unit.display_name(&self.config),
                    address: unit.info.ip.to_string(),
                    model: unit.hardware(),
                    online: unit.client.is_some(),
                    state: unit.state,
                    error: unit.error.clone(),
                })
                .collect(),
        };
        let _ = self.snapshot_tx.send(snapshot);
    }
}

impl Unit {
    fn hardware(&self) -> String {
        let mut parts = Vec::new();
        if !self.info.model.is_empty() {
            parts.push(self.info.model.clone());
        }
        if !self.info.version.is_empty() {
            parts.push(format!("v{}", self.info.version));
        }
        parts.join(" ")
    }

    fn display_name(&self, config: &Config) -> String {
        if let Some(name) = config.names.get(&self.info.mac.to_lowercase())
            && !name.is_empty()
        {
            return name.clone();
        }
        if !self.info.name.is_empty() {
            return self.info.name.clone();
        }
        let suffix: String = self
            .info
            .mac
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        format!("AC {suffix}")
    }
}

fn command_properties(change: Change, state: Option<AcState>) -> Vec<(String, i64)> {
    match change {
        Change::Power(on) => {
            let mut props = vec![("Pow".to_owned(), if on { 1 } else { 0 })];
            // When switching on, include the known mode/temperature/fan so the
            // unit starts with the settings shown in the UI.
            if on && let Some(state) = state {
                props.push(("Mod".to_owned(), state.mode.raw()));
                props.push(("SetTem".to_owned(), state.target as i64));
                props.push(("WdSpd".to_owned(), state.fan.raw()));
            }
            props
        }
        Change::Mode(mode) => vec![("Mod".to_owned(), mode.raw())],
        Change::Temp(temp) => vec![("SetTem".to_owned(), temp as i64)],
        Change::Fan(fan) => vec![("WdSpd".to_owned(), fan.raw())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AcState {
        AcState {
            power: true,
            mode: Mode::Cool,
            target: 22,
            fan: FanSpeed::Auto,
            fahrenheit: false,
            room_raw: Some(50),
        }
    }

    fn value(props: &[(String, i64)], key: &str) -> Option<i64> {
        props
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| *value)
    }

    #[test]
    fn power_on_includes_known_settings() {
        let props = command_properties(Change::Power(true), Some(sample()));
        assert_eq!(value(&props, "Pow"), Some(1));
        assert_eq!(value(&props, "Mod"), Some(Mode::Cool.raw()));
        assert_eq!(value(&props, "SetTem"), Some(22));
        assert_eq!(value(&props, "WdSpd"), Some(FanSpeed::Auto.raw()));
    }

    #[test]
    fn power_off_sends_only_power() {
        let props = command_properties(Change::Power(false), Some(sample()));
        assert_eq!(props, vec![("Pow".to_owned(), 0)]);
    }

    #[test]
    fn power_on_without_state_sends_only_power() {
        let props = command_properties(Change::Power(true), None);
        assert_eq!(props, vec![("Pow".to_owned(), 1)]);
    }

    #[test]
    fn individual_changes_map_to_single_property() {
        assert_eq!(
            command_properties(Change::Mode(Mode::Heat), None),
            vec![("Mod".to_owned(), Mode::Heat.raw())]
        );
        assert_eq!(
            command_properties(Change::Temp(25), None),
            vec![("SetTem".to_owned(), 25)]
        );
        assert_eq!(
            command_properties(Change::Fan(FanSpeed::High), None),
            vec![("WdSpd".to_owned(), FanSpeed::High.raw())]
        );
    }
}
