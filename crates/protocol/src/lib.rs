//! Wire format shared by the Mac sender and the Windows receiver.
//!
//! The control packet is byte-compatible with the original Python scripts, so
//! either side can be swapped independently. The receiver answers the sender
//! with a status packet about once per second, which is how the sender knows
//! the PC side is alive and whether vJoy is usable.

pub const DEFAULT_PORT: u16 = 47800;
pub const AXES: usize = 8;
/// Buttons carried by a control packet. The radio itself reports 24.
pub const BUTTONS: usize = 32;
pub const AXIS_MAX: i16 = 32767;

const CONTROL_MAGIC: &[u8; 4] = b"TX16";
const STATUS_MAGIC: &[u8; 4] = b"TXST";
const STATUS_VERSION: u8 = 1;

/// Radio state sent from the Mac to the PC.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Control {
    pub sequence: u32,
    /// Bit `n` is button `n + 1`.
    pub buttons: u32,
    /// Each axis is in `-AXIS_MAX..=AXIS_MAX`, 0 being the center.
    pub axes: [i16; AXES],
}

impl Control {
    pub const SIZE: usize = 4 + 4 + 4 + 2 * AXES;

    /// Axes centered and buttons released: what the PC applies when the link drops.
    pub const NEUTRAL: Self = Self { sequence: 0, buttons: 0, axes: [0; AXES] };

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut out = [0; Self::SIZE];
        out[0..4].copy_from_slice(CONTROL_MAGIC);
        out[4..8].copy_from_slice(&self.sequence.to_le_bytes());
        out[8..12].copy_from_slice(&self.buttons.to_le_bytes());
        for (i, axis) in self.axes.iter().enumerate() {
            out[12 + 2 * i..14 + 2 * i].copy_from_slice(&axis.to_le_bytes());
        }
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != Self::SIZE || &data[0..4] != CONTROL_MAGIC {
            return None;
        }
        let mut axes = [0; AXES];
        for (i, axis) in axes.iter_mut().enumerate() {
            *axis = i16::from_le_bytes([data[12 + 2 * i], data[13 + 2 * i]]);
        }
        Some(Self {
            sequence: u32_at(data, 4),
            buttons: u32_at(data, 8),
            axes,
        })
    }
}

/// What the PC reports about its virtual joystick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputState {
    /// The virtual joystick is acquired and receiving input.
    Ready,
    /// vJoyInterface.dll could not be loaded.
    DriverMissing,
    /// vJoy is installed but disabled in "Configure vJoy".
    DriverDisabled,
    /// The vJoy device is missing, not configured, or owned by another program.
    DeviceUnavailable,
    /// The receiver runs without a virtual joystick (dry run).
    DryRun,
}

impl OutputState {
    fn to_byte(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::DriverMissing => 1,
            Self::DriverDisabled => 2,
            Self::DeviceUnavailable => 3,
            Self::DryRun => 4,
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::Ready,
            1 => Self::DriverMissing,
            2 => Self::DriverDisabled,
            3 => Self::DeviceUnavailable,
            4 => Self::DryRun,
            _ => return None,
        })
    }
}

/// Receiver health, sent from the PC back to the Mac.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub output: OutputState,
    /// Control packets accepted during the last reporting window, per second.
    pub packets_per_second: u32,
    /// Control packets missing or out of order during the last reporting window.
    pub lost: u32,
}

impl Status {
    pub const SIZE: usize = 4 + 1 + 1 + 2 + 4 + 4;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut out = [0; Self::SIZE];
        out[0..4].copy_from_slice(STATUS_MAGIC);
        out[4] = STATUS_VERSION;
        out[5] = self.output.to_byte();
        out[8..12].copy_from_slice(&self.packets_per_second.to_le_bytes());
        out[12..16].copy_from_slice(&self.lost.to_le_bytes());
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != Self::SIZE || &data[0..4] != STATUS_MAGIC || data[4] != STATUS_VERSION {
            return None;
        }
        Some(Self {
            output: OutputState::from_byte(data[5])?,
            packets_per_second: u32_at(data, 8),
            lost: u32_at(data, 12),
        })
    }
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_round_trips() {
        let control = Control {
            sequence: 0xDEAD_BEEF,
            buttons: 0b1010_0001,
            axes: [-AXIS_MAX, -1, 0, 1, AXIS_MAX, 100, -100, 12345],
        };
        assert_eq!(Control::decode(&control.encode()), Some(control));
    }

    #[test]
    fn control_matches_python_layout() {
        // struct.pack("<4sII8h", b"TX16", 7, 5, 1, -1, 0, 0, 0, 0, 0, 32767)
        let python: [u8; 28] = [
            b'T', b'X', b'1', b'6', 7, 0, 0, 0, 5, 0, 0, 0, 1, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0xFF, 0x7F,
        ];
        let control = Control::decode(&python).unwrap();
        assert_eq!(control.sequence, 7);
        assert_eq!(control.buttons, 5);
        assert_eq!(control.axes, [1, -1, 0, 0, 0, 0, 0, AXIS_MAX]);
        assert_eq!(control.encode(), python);
    }

    #[test]
    fn rejects_foreign_packets() {
        assert_eq!(Control::decode(b"hello"), None);
        assert_eq!(Control::decode(&[0; Control::SIZE]), None);
        let status = Status { output: OutputState::Ready, packets_per_second: 1, lost: 0 };
        assert_eq!(Control::decode(&status.encode()), None);
        assert_eq!(Status::decode(&Control::NEUTRAL.encode()), None);
    }

    #[test]
    fn status_round_trips() {
        for output in [
            OutputState::Ready,
            OutputState::DriverMissing,
            OutputState::DriverDisabled,
            OutputState::DeviceUnavailable,
            OutputState::DryRun,
        ] {
            let status = Status { output, packets_per_second: 500, lost: 3 };
            assert_eq!(Status::decode(&status.encode()), Some(status));
        }
    }
}
