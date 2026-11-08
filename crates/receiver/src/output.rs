//! Where received controls go: the vJoy device, or a log in dry-run mode.

use std::time::{Duration, Instant};

use tx16s_protocol::{AXES, AXIS_MAX, BUTTONS, Control, OutputState};

pub trait Output {
    fn state(&self) -> OutputState;
    fn apply(&mut self, control: &Control);
    /// Called every few seconds: an unavailable output tries to come back.
    fn maintain(&mut self) {}
}

pub const VJOY_AXIS_MIN: i32 = 1;
pub const VJOY_AXIS_MAX: i32 = 0x8000;

/// Maps `-AXIS_MAX..=AXIS_MAX` onto vJoy's `1..=0x8000`.
pub fn vjoy_axis(value: i16) -> i32 {
    let value = i32::from(value.clamp(-AXIS_MAX, AXIS_MAX)) + i32::from(AXIS_MAX);
    value * (VJOY_AXIS_MAX - VJOY_AXIS_MIN) / (2 * i32::from(AXIS_MAX)) + VJOY_AXIS_MIN
}

/// What must be written to the device to match a control.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    /// `(axis index, vJoy value)`.
    pub axes: Vec<(usize, i32)>,
    /// `(button index, pressed)`.
    pub buttons: Vec<(usize, bool)>,
}

/// Remembers what the device holds so only changed values are written.
#[derive(Default)]
pub struct Changes {
    axes: [Option<i32>; AXES],
    buttons: Option<u32>,
}

impl Changes {
    /// Forget the device state, so the next control is written in full.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn diff(&mut self, control: &Control) -> Diff {
        let mut axes = Vec::new();
        for (index, (known, value)) in self.axes.iter_mut().zip(control.axes).enumerate() {
            let value = vjoy_axis(value);
            if *known != Some(value) {
                *known = Some(value);
                axes.push((index, value));
            }
        }
        let changed = match self.buttons {
            Some(known) => known ^ control.buttons,
            None => u32::MAX,
        };
        self.buttons = Some(control.buttons);
        let buttons = (0..BUTTONS)
            .filter(|index| changed >> index & 1 == 1)
            .map(|index| (index, control.buttons >> index & 1 == 1))
            .collect();
        Diff { axes, buttons }
    }
}

/// Logs what it receives, at most once per second, instead of driving vJoy.
pub struct DryRun {
    last: Option<(Control, Instant)>,
}

impl DryRun {
    pub fn new() -> Self {
        Self { last: None }
    }
}

impl Output for DryRun {
    fn state(&self) -> OutputState {
        OutputState::DryRun
    }

    fn apply(&mut self, control: &Control) {
        let now = Instant::now();
        let neutral = control.axes == Control::NEUTRAL.axes && control.buttons == 0;
        let due = match self.last {
            None => true,
            Some((last, _)) if last.axes == control.axes && last.buttons == control.buttons => {
                false
            }
            Some((_, at)) => neutral || now - at >= Duration::from_secs(1),
        };
        if due {
            self.last = Some((*control, now));
            tracing::info!(axes = ?control.axes, buttons = format!("{:032b}", control.buttons), "dry-run");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_mapping_covers_vjoy_range() {
        assert_eq!(vjoy_axis(-AXIS_MAX), VJOY_AXIS_MIN);
        assert_eq!(vjoy_axis(AXIS_MAX), VJOY_AXIS_MAX);
        assert_eq!(vjoy_axis(0), 0x4000);
        assert_eq!(vjoy_axis(i16::MIN), VJOY_AXIS_MIN);
        let mut previous = 0;
        for value in -AXIS_MAX..=AXIS_MAX {
            let mapped = vjoy_axis(value);
            assert!(mapped >= previous, "not monotonic at {value}");
            previous = mapped;
        }
    }

    #[test]
    fn first_diff_writes_everything_then_only_changes() {
        let mut changes = Changes::default();
        let mut control = Control::NEUTRAL;
        let first = changes.diff(&control);
        assert_eq!(first.axes.len(), AXES);
        assert_eq!(first.buttons.len(), BUTTONS);
        assert!(first.buttons.iter().all(|&(_, pressed)| !pressed));

        assert_eq!(changes.diff(&control), Diff::default());

        control.axes[2] = AXIS_MAX;
        control.buttons = 0b101;
        assert_eq!(
            changes.diff(&control),
            Diff {
                axes: vec![(2, VJOY_AXIS_MAX)],
                buttons: vec![(0, true), (2, true)]
            }
        );

        control.buttons = 0b100;
        assert_eq!(
            changes.diff(&control),
            Diff {
                axes: vec![],
                buttons: vec![(0, false)]
            }
        );

        changes.reset();
        assert_eq!(changes.diff(&control).axes.len(), AXES);
    }
}
