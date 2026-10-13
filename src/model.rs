//! Shared value types for the air-conditioner state.

use serde_json::Value;

/// Minimum and maximum settable temperature in Celsius.
pub const TEMP_MIN: u8 = 16;
pub const TEMP_MAX: u8 = 30;

/// Operating mode reported by the unit's `Mod` property.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Auto,
    Cool,
    Dry,
    Fan,
    Heat,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Auto, Mode::Cool, Mode::Dry, Mode::Fan, Mode::Heat];

    pub fn from_raw(value: i64) -> Option<Self> {
        Some(match value {
            0 => Mode::Auto,
            1 => Mode::Cool,
            2 => Mode::Dry,
            3 => Mode::Fan,
            4 => Mode::Heat,
            _ => return None,
        })
    }

    pub fn raw(self) -> i64 {
        match self {
            Mode::Auto => 0,
            Mode::Cool => 1,
            Mode::Dry => 2,
            Mode::Fan => 3,
            Mode::Heat => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Auto => "Auto",
            Mode::Cool => "Cool",
            Mode::Dry => "Dry",
            Mode::Fan => "Fan",
            Mode::Heat => "Heat",
        }
    }
}

/// Fan speed reported by the unit's `WdSpd` property.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FanSpeed {
    #[default]
    Auto,
    Low,
    MediumLow,
    Medium,
    MediumHigh,
    High,
}

impl FanSpeed {
    pub const ALL: [FanSpeed; 6] = [
        FanSpeed::Auto,
        FanSpeed::Low,
        FanSpeed::MediumLow,
        FanSpeed::Medium,
        FanSpeed::MediumHigh,
        FanSpeed::High,
    ];

    pub fn from_raw(value: i64) -> Option<Self> {
        Self::ALL.get(value as usize).copied()
    }

    pub fn raw(self) -> i64 {
        match self {
            FanSpeed::Auto => 0,
            FanSpeed::Low => 1,
            FanSpeed::MediumLow => 2,
            FanSpeed::Medium => 3,
            FanSpeed::MediumHigh => 4,
            FanSpeed::High => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FanSpeed::Auto => "Auto",
            FanSpeed::Low => "Low",
            FanSpeed::MediumLow => "Medium-low",
            FanSpeed::Medium => "Medium",
            FanSpeed::MediumHigh => "Medium-high",
            FanSpeed::High => "High",
        }
    }
}

/// A snapshot of one unit's controllable state, decoded from its `status` reply.
#[derive(Clone, Copy, Debug)]
pub struct AcState {
    pub power: bool,
    pub mode: Mode,
    pub target: u8,
    pub fan: FanSpeed,
    pub fahrenheit: bool,
    /// Raw `TemSen` value as reported by the unit. Decoded to Celsius with
    /// `room_celsius()` (`TemSen` minus a base offset of 40).
    pub room_raw: Option<i64>,
}

impl AcState {
    pub fn from_properties(properties: &Value) -> Self {
        let mode = properties
            .get("Mod")
            .and_then(Value::as_i64)
            .and_then(Mode::from_raw)
            .unwrap_or_default();
        let fan = properties
            .get("WdSpd")
            .and_then(Value::as_i64)
            .and_then(FanSpeed::from_raw)
            .unwrap_or_default();
        let target = properties
            .get("SetTem")
            .and_then(Value::as_i64)
            .map(|value| value.clamp(TEMP_MIN as i64, TEMP_MAX as i64) as u8)
            .unwrap_or(24);

        AcState {
            power: properties.get("Pow").and_then(Value::as_i64) == Some(1),
            mode,
            target,
            fan,
            fahrenheit: properties.get("TemUn").and_then(Value::as_i64) == Some(1),
            room_raw: properties.get("TemSen").and_then(Value::as_i64),
        }
    }

    pub fn temperature_label(&self) -> String {
        let symbol = if self.fahrenheit { "°F" } else { "°C" };
        format!("{}{symbol}", self.target)
    }

    pub fn summary(&self) -> String {
        if !self.power {
            "Off".to_owned()
        } else {
            format!(
                "{} · {} · {}",
                self.mode.label(),
                self.temperature_label(),
                self.fan.label()
            )
        }
    }

    /// Decode the raw `TemSen` room-sensor value to Celsius.
    /// Community-reversed across several implementations
    /// (tomikaa87/gree-remote, homebridge-gree-airconditioner):
    /// `TemSen` is the actual room temperature in °C plus a base offset of 40.
    pub fn room_celsius(&self) -> Option<i64> {
        self.room_raw.map(|raw| raw - 40)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_running_unit() {
        let state = AcState::from_properties(&json!({
            "Pow": 1, "Mod": 1, "SetTem": 22, "WdSpd": 3, "TemSen": 48
        }));
        assert!(state.power);
        assert_eq!(state.mode, Mode::Cool);
        assert_eq!(state.target, 22);
        assert_eq!(state.fan, FanSpeed::Medium);
        assert_eq!(state.summary(), "Cool · 22°C · Medium");
    }

    #[test]
    fn clamps_out_of_range_temperature() {
        let state = AcState::from_properties(&json!({ "SetTem": 45 }));
        assert_eq!(state.target, TEMP_MAX);
    }

    #[test]
    fn handles_missing_properties() {
        let state = AcState::from_properties(&json!({}));
        assert!(!state.power);
        assert_eq!(state.mode, Mode::Auto);
        assert_eq!(state.fan, FanSpeed::Auto);
        assert_eq!(state.room_raw, None);
    }

    #[test]
    fn detects_fahrenheit_units() {
        let state = AcState::from_properties(&json!({ "Pow": 1, "TemUn": 1, "SetTem": 24 }));
        assert!(state.fahrenheit);
        assert!(state.temperature_label().ends_with("°F"));
    }

    #[test]
    fn decodes_room_sensor_with_base_offset() {
        let state = AcState::from_properties(&json!({ "TemSen": 58 }));
        assert_eq!(state.room_raw, Some(58));
        assert_eq!(state.room_celsius(), Some(18));
    }
}
