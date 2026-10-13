//! Shared UI state: the command channel and the render snapshot fed to the
//! Plasma applet over D-Bus.

use crate::model::{AcState, FanSpeed, Mode};

/// Actions requested from the applet or CLI, handled by the controller task.
#[derive(Debug, Clone)]
pub enum Command {
    Refresh,
    SetPower { unit: Option<usize>, on: bool },
    SetMode { unit: Option<usize>, mode: Mode },
    SetTemp { unit: Option<usize>, temp: u8 },
    SetFan { unit: Option<usize>, fan: FanSpeed },
    SetLinked(bool),
    Quit,
}

/// One unit as shown in the UI.
#[derive(Clone, Debug)]
pub struct UnitView {
    pub name: String,
    pub address: String,
    pub model: String,
    pub online: bool,
    pub state: Option<AcState>,
    pub error: Option<String>,
}

/// A complete render snapshot pushed from the controller.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub linked: bool,
    pub home: bool,
    pub following_wifi: bool,
    pub message: Option<String>,
    pub units: Vec<UnitView>,
}
