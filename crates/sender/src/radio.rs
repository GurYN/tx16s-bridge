//! Finding the radio among HID devices and decoding its joystick reports.

use hidapi::{DeviceInfo, HidApi, HidDevice};
use tx16s_protocol::{AXES, AXIS_MAX};

const GENERIC_DESKTOP: u16 = 1;
/// Joystick, gamepad and multi-axis controller usages.
const JOYSTICK_USAGES: [u16; 3] = [4, 5, 8];
const NAME_HINTS: [&str; 4] = ["tx16", "radiomaster", "edgetx", "opentx"];

/// EdgeTX "classic" joystick report: 24 buttons, 8 axes as u16 in 0..=2048,
/// one battery byte, no report ID.
pub const REPORT_SIZE: usize = 3 + 2 * AXES + 1;
const AXIS_CENTER: i32 = 1024;

/// What the radio is reporting, with axes already scaled to the wire range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RadioInput {
    pub axes: [i16; AXES],
    pub buttons: u32,
}

pub fn decode_report(report: &[u8]) -> Option<RadioInput> {
    if report.len() != REPORT_SIZE {
        return None;
    }
    let buttons = u32::from_le_bytes([report[0], report[1], report[2], 0]);
    let mut axes = [0; AXES];
    for (i, axis) in axes.iter_mut().enumerate() {
        let raw = i32::from(u16::from_le_bytes([report[3 + 2 * i], report[4 + 2 * i]]));
        let scaled = (raw - AXIS_CENTER) * i32::from(AXIS_MAX) / AXIS_CENTER;
        *axis = scaled.clamp(-i32::from(AXIS_MAX), i32::from(AXIS_MAX)) as i16;
    }
    Some(RadioInput { axes, buttons })
}

pub struct Radio {
    pub name: String,
    device: HidDevice,
}

pub enum ReadOutcome {
    Input(RadioInput),
    /// Nothing arrived before the timeout.
    Idle,
    /// A report arrived but does not have the classic joystick layout.
    UnexpectedReport(usize),
    Disconnected,
}

impl Radio {
    /// Opens the first joystick that looks like an EdgeTX radio, or the only joystick present.
    pub fn find(api: &mut HidApi) -> Option<Self> {
        api.refresh_devices().ok()?;
        let joysticks: Vec<&DeviceInfo> = api
            .device_list()
            .filter(|d| d.usage_page() == GENERIC_DESKTOP && JOYSTICK_USAGES.contains(&d.usage()))
            .collect();
        let info = joysticks
            .iter()
            .find(|d| looks_like_radio(d.product_string().unwrap_or_default()))
            .or(if joysticks.len() == 1 { joysticks.first() } else { None })?;
        let device = api.open_path(info.path()).ok()?;
        let name = info.product_string().unwrap_or("Joystick").to_owned();
        Some(Self { name, device })
    }

    pub fn read(&self, timeout_ms: i32) -> ReadOutcome {
        let mut buffer = [0u8; 64];
        match self.device.read_timeout(&mut buffer, timeout_ms) {
            Ok(0) => ReadOutcome::Idle,
            Ok(len) => match decode_report(&buffer[..len]) {
                Some(input) => ReadOutcome::Input(input),
                None => ReadOutcome::UnexpectedReport(len),
            },
            Err(_) => ReadOutcome::Disconnected,
        }
    }
}

fn looks_like_radio(name: &str) -> bool {
    let name = name.to_lowercase();
    NAME_HINTS.iter().any(|hint| name.contains(hint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_report_captured_from_a_tx16s() {
        let report = [
            0x00, 0x00, 0x00, 0x03, 0x04, 0x05, 0x04, 0x00, 0x00, 0xfb, 0x03, 0x00, 0x00, 0x00,
            0x04, 0x00, 0x04, 0x00, 0x04, 0x64,
        ];
        let input = decode_report(&report).unwrap();
        assert_eq!(input.buttons, 0);
        assert_eq!(input.axes[0], 95); // 1027 is just right of center
        assert_eq!(input.axes[2], -AXIS_MAX); // throttle at the bottom
        assert_eq!(input.axes[5..], [0, 0, 0]);
    }

    #[test]
    fn maps_full_range_and_buttons() {
        let mut report = [0u8; REPORT_SIZE];
        report[0] = 0b0000_0001;
        report[2] = 0b1000_0000;
        report[3..5].copy_from_slice(&2048u16.to_le_bytes());
        let input = decode_report(&report).unwrap();
        assert_eq!(input.buttons, 1 | 1 << 23);
        assert_eq!(input.axes[0], AXIS_MAX);
        assert_eq!(input.axes[1], -AXIS_MAX);
    }

    #[test]
    fn rejects_other_layouts() {
        assert_eq!(decode_report(&[0; REPORT_SIZE + 1]), None);
    }
}
