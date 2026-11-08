//! Link state shared by the window and the menu bar item, alive for the whole
//! app: closing the window does not stop the stream.

use std::time::Duration;

use gpui::{Context, Task};
use tx16s_protocol::OutputState;

use crate::config::Config;
use crate::engine::{Engine, RECEIVER_STALE_AFTER, RadioState, Settings, Snapshot};

const REFRESH: Duration = Duration::from_millis(33);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warning,
    Error,
    Muted,
    Accent,
}

/// Overall state, for the menu bar icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Paused,
    Streaming,
    Problem,
}

pub struct Bridge {
    engine: Engine,
    pub config: Config,
    pub test_signal: bool,
    pub snapshot: Snapshot,
    _refresh: Task<()>,
}

impl Bridge {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH).await;
                let updated = this.update(cx, |this, cx| {
                    this.snapshot = this.engine.snapshot();
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        });
        let mut bridge = Self {
            engine: Engine::start(Settings {
                host: String::new(),
                port: 0,
                sending: false,
                test_signal: false,
            }),
            config: Config::load(),
            // `--test` starts with the test signal on, handy to check the PC side without the radio.
            test_signal: std::env::args().any(|arg| arg == "--test"),
            snapshot: Snapshot::default(),
            _refresh: refresh,
        };
        bridge.push_settings();
        bridge
    }

    fn push_settings(&mut self) {
        let (host, port) = self.config.host_and_port();
        self.engine.update(Settings {
            host,
            port,
            sending: self.config.sending,
            test_signal: self.test_signal,
        });
    }

    pub fn set_target(&mut self, target: &str, cx: &mut Context<Self>) {
        let target = target.trim();
        if self.config.target != target {
            self.config.target = target.to_owned();
            self.config.save();
            self.push_settings();
            cx.notify();
        }
    }

    pub fn toggle_sending(&mut self, cx: &mut Context<Self>) {
        self.config.sending = !self.config.sending;
        self.config.save();
        self.push_settings();
        cx.notify();
    }

    pub fn toggle_test_signal(&mut self, cx: &mut Context<Self>) {
        self.test_signal = !self.test_signal;
        self.push_settings();
        cx.notify();
    }

    pub fn radio_status(&self) -> (String, Tone) {
        if self.test_signal {
            return ("Test signal".into(), Tone::Accent);
        }
        match &self.snapshot.radio {
            RadioState::Connected(name) => (name.clone(), Tone::Ok),
            RadioState::Searching => ("Plug in the radio in USB Joystick mode".into(), Tone::Warning),
            RadioState::UnexpectedReport { name, len } => (
                format!("{name}: {len}-byte report, unsupported joystick mode"),
                Tone::Error,
            ),
            RadioState::HidUnavailable(error) => {
                (format!("Cannot access USB: {error}"), Tone::Error)
            }
        }
    }

    pub fn network_status(&self) -> (String, Tone) {
        if !self.config.sending {
            return ("Paused".into(), Tone::Muted);
        }
        if let Some(error) = &self.snapshot.network_error {
            return (error.clone(), Tone::Error);
        }
        if self.snapshot.input.is_none() {
            return ("Waiting for the radio".into(), Tone::Warning);
        }
        match self.snapshot.target {
            Some(target) => (
                format!("{} packets/s to {target}", self.snapshot.sent_per_second),
                Tone::Ok,
            ),
            None => ("Resolving address…".into(), Tone::Muted),
        }
    }

    pub fn receiver_status(&self) -> (String, Tone) {
        let fresh = self
            .snapshot
            .receiver
            .filter(|(_, at)| at.elapsed() < RECEIVER_STALE_AFTER);
        let Some((status, _)) = fresh else {
            return if self.config.sending {
                ("No answer from the PC".into(), Tone::Warning)
            } else {
                ("—".into(), Tone::Muted)
            };
        };
        let traffic = format!("{} received/s, {} lost", status.packets_per_second, status.lost);
        match status.output {
            OutputState::Ready => (format!("vJoy ready · {traffic}"), Tone::Ok),
            OutputState::DryRun => (format!("Test mode without vJoy · {traffic}"), Tone::Warning),
            OutputState::DriverMissing => ("vJoy is not installed".into(), Tone::Error),
            OutputState::DriverDisabled => ("vJoy is disabled".into(), Tone::Error),
            OutputState::DeviceUnavailable => ("vJoy device unavailable".into(), Tone::Error),
        }
    }

    pub fn health(&self) -> Health {
        if !self.config.sending {
            Health::Paused
        } else if self.network_status().1 == Tone::Ok && self.receiver_status().1 == Tone::Ok {
            Health::Streaming
        } else {
            Health::Problem
        }
    }
}
