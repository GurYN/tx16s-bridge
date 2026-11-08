//! vJoy backend, loading vJoyInterface.dll at runtime so the receiver still
//! starts (and reports why) when vJoy is missing or misconfigured.

use std::path::PathBuf;

use libloading::Library;
use tracing::{info, warn};
use tx16s_protocol::{Control, OutputState};

use crate::output::{Changes, Output};

const HID_USAGE_X: u32 = 0x30; // then Y, Z, RX, RY, RZ, SL0, SL1

// VjdStat values from vJoy's public header.
const VJD_STAT_OWN: i32 = 0;
const VJD_STAT_FREE: i32 = 1;
const VJD_STAT_BUSY: i32 = 2;
const VJD_STAT_MISS: i32 = 3;

struct Api {
    enabled: unsafe extern "C" fn() -> i32,
    status: unsafe extern "C" fn(u32) -> i32,
    acquire: unsafe extern "C" fn(u32) -> i32,
    relinquish: unsafe extern "C" fn(u32),
    reset: unsafe extern "C" fn(u32) -> i32,
    set_axis: unsafe extern "C" fn(i32, u32, u32) -> i32,
    set_button: unsafe extern "C" fn(i32, u32, u8) -> i32,
    // Keeps the function pointers above valid.
    _library: Library,
}

impl Api {
    fn load(path: &PathBuf) -> Result<Self, libloading::Error> {
        // SAFETY: vJoyInterface.dll has no load-time side effects we depend on, and
        // the symbol types match the C declarations in vjoyinterface.h.
        unsafe {
            let library = Library::new(path)?;
            Ok(Self {
                enabled: *library.get(b"vJoyEnabled\0")?,
                status: *library.get(b"GetVJDStatus\0")?,
                acquire: *library.get(b"AcquireVJD\0")?,
                relinquish: *library.get(b"RelinquishVJD\0")?,
                reset: *library.get(b"ResetVJD\0")?,
                set_axis: *library.get(b"SetAxis\0")?,
                set_button: *library.get(b"SetBtn\0")?,
                _library: library,
            })
        }
    }
}

pub struct VJoy {
    device: u32,
    dll_paths: Vec<PathBuf>,
    api: Option<Api>,
    /// `None` until the first attempt, so that first outcome is always logged.
    state: Option<OutputState>,
    changes: Changes,
}

impl VJoy {
    pub fn new(device: u32, configured_dll: Option<PathBuf>) -> Self {
        let dll_paths = match configured_dll {
            Some(path) => vec![path],
            None => {
                let program_files = std::env::var_os("ProgramFiles")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
                vec![
                    program_files.join(r"vJoy\x64\vJoyInterface.dll"),
                    PathBuf::from("vJoyInterface.dll"),
                ]
            }
        };
        let mut vjoy = Self {
            device,
            dll_paths,
            api: None,
            state: None,
            changes: Changes::default(),
        };
        vjoy.connect();
        vjoy
    }

    fn set_state(&mut self, state: OutputState, detail: &str) {
        if self.state == Some(state) {
            return;
        }
        self.state = Some(state);
        if state == OutputState::Ready {
            info!(device = self.device, "vJoy ready");
        } else {
            warn!(device = self.device, ?state, "vJoy unavailable: {detail}");
        }
    }

    fn connect(&mut self) {
        if self.api.is_none() {
            let mut errors = Vec::new();
            for path in &self.dll_paths {
                match Api::load(path) {
                    Ok(api) => {
                        info!(path = %path.display(), "vJoyInterface.dll loaded");
                        self.api = Some(api);
                        break;
                    }
                    Err(error) => errors.push(format!("{} ({error})", path.display())),
                }
            }
            if self.api.is_none() {
                let detail = format!("is vJoy installed? {}", errors.join(", "));
                self.set_state(OutputState::DriverMissing, &detail);
                return;
            }
        }
        let api = self.api.as_ref().unwrap();
        // SAFETY: plain calls into the loaded vJoy API with a device number.
        let (enabled, status) = unsafe { ((api.enabled)() != 0, (api.status)(self.device)) };
        if !enabled {
            self.set_state(
                OutputState::DriverDisabled,
                "enable vJoy in 'Configure vJoy'",
            );
            return;
        }
        let acquired = match status {
            VJD_STAT_OWN => true,
            // SAFETY: as above.
            VJD_STAT_FREE => unsafe { (api.acquire)(self.device) != 0 },
            _ => false,
        };
        if acquired {
            // SAFETY: as above; the device is ours now.
            unsafe { (api.reset)(self.device) };
            self.changes.reset();
            self.set_state(OutputState::Ready, "");
        } else {
            let detail = match status {
                VJD_STAT_BUSY => "the device is used by another program",
                VJD_STAT_MISS => {
                    "the device does not exist: configure it in 'Configure vJoy'"
                }
                _ => "cannot acquire the device",
            };
            self.set_state(OutputState::DeviceUnavailable, detail);
        }
    }

    fn still_owned(&self) -> bool {
        let api = self.api.as_ref().unwrap();
        // SAFETY: plain call into the loaded vJoy API.
        unsafe { (api.status)(self.device) == VJD_STAT_OWN }
    }
}

impl Output for VJoy {
    fn state(&self) -> OutputState {
        self.state.unwrap_or(OutputState::DriverMissing)
    }

    fn apply(&mut self, control: &Control) {
        if self.state() != OutputState::Ready {
            return;
        }
        let diff = self.changes.diff(control);
        let api = self.api.as_ref().unwrap();
        let mut failed = false;
        for (index, value) in diff.axes {
            // SAFETY: plain call into the loaded vJoy API.
            failed |=
                unsafe { (api.set_axis)(value, self.device, HID_USAGE_X + index as u32) } == 0;
        }
        for (index, pressed) in diff.buttons {
            // Buttons beyond the configured count fail harmlessly, so results are ignored.
            // SAFETY: plain call into the loaded vJoy API.
            unsafe { (api.set_button)(i32::from(pressed), self.device, index as u8 + 1) };
        }
        // An axis can fail just because it is disabled in vJoy; only a lost device matters.
        if failed && !self.still_owned() {
            self.changes.reset();
            self.set_state(
                OutputState::DeviceUnavailable,
                "the device was lost",
            );
        }
    }

    fn maintain(&mut self) {
        if self.state() != OutputState::Ready || !self.still_owned() {
            self.connect();
        }
    }
}

impl Drop for VJoy {
    fn drop(&mut self) {
        if let (OutputState::Ready, Some(api)) = (self.state(), &self.api) {
            // SAFETY: releases the device acquired in `connect`.
            unsafe { (api.relinquish)(self.device) };
        }
    }
}
