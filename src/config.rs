//! Persisted user settings, stored as JSON under the XDG config directory.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Name of the home Wi-Fi network. When set, the daemon follows it.
    pub home_ssid: Option<String>,
    /// Pause polling and refuse commands while off the home network.
    pub hide_away: bool,
    /// Beep when a command is sent (matches the physical remote).
    pub buzzer: bool,
    /// Whether both units are controlled together by default.
    pub linked: bool,
    /// Optional friendly names, keyed by the unit's MAC address.
    pub names: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            home_ssid: None,
            hide_away: true,
            buzzer: true,
            linked: true,
            names: BTreeMap::new(),
        }
    }
}

impl Config {
    fn config_base() -> Option<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }

    fn path() -> Option<PathBuf> {
        Some(
            Self::config_base()?
                .join("gree-ac-plasma")
                .join("config.json"),
        )
    }

    fn legacy_path() -> Option<PathBuf> {
        Some(
            Self::config_base()?
                .join("gree-ac-tray")
                .join("config.json"),
        )
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Config::default();
        };
        let (contents, migrate) = match fs::read_to_string(&path) {
            Ok(contents) => (contents, false),
            Err(_) => {
                let Some(legacy_path) = Self::legacy_path() else {
                    return Config::default();
                };
                match fs::read_to_string(legacy_path) {
                    Ok(contents) => (contents, true),
                    Err(_) => return Config::default(),
                }
            }
        };
        match serde_json::from_str::<Config>(&contents) {
            Ok(config) => {
                if migrate {
                    config.save();
                }
                config
            }
            Err(error) => {
                eprintln!("Ignoring invalid config at {}: {error}", path.display());
                Config::default()
            }
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!(
                "Could not create config directory {}: {error}",
                parent.display()
            );
            return;
        }
        match serde_json::to_string_pretty(self) {
            Ok(contents) => {
                if let Err(error) = fs::write(&path, contents) {
                    eprintln!("Could not write config {}: {error}", path.display());
                }
            }
            Err(error) => eprintln!("Could not serialize config: {error}"),
        }
    }
}
