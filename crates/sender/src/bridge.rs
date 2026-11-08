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
            return ("Signal de test".into(), Tone::Accent);
        }
        match &self.snapshot.radio {
            RadioState::Connected(name) => (name.clone(), Tone::Ok),
            RadioState::Searching => ("Branche la radio en mode USB Joystick".into(), Tone::Warning),
            RadioState::UnexpectedReport { name, len } => (
                format!("{name} : rapport de {len} octets, mode joystick non pris en charge"),
                Tone::Error,
            ),
            RadioState::HidUnavailable(error) => {
                (format!("Accès USB impossible : {error}"), Tone::Error)
            }
        }
    }

    pub fn network_status(&self) -> (String, Tone) {
        if !self.config.sending {
            return ("En pause".into(), Tone::Muted);
        }
        if let Some(error) = &self.snapshot.network_error {
            return (error.clone(), Tone::Error);
        }
        if self.snapshot.input.is_none() {
            return ("En attente de la radio".into(), Tone::Warning);
        }
        match self.snapshot.target {
            Some(target) => (
                format!("{} paquets/s vers {target}", self.snapshot.sent_per_second),
                Tone::Ok,
            ),
            None => ("Résolution de l'adresse…".into(), Tone::Muted),
        }
    }

    pub fn receiver_status(&self) -> (String, Tone) {
        let fresh = self
            .snapshot
            .receiver
            .filter(|(_, at)| at.elapsed() < RECEIVER_STALE_AFTER);
        let Some((status, _)) = fresh else {
            return if self.config.sending {
                ("Pas de réponse du PC".into(), Tone::Warning)
            } else {
                ("—".into(), Tone::Muted)
            };
        };
        let traffic = format!("{} reçus/s, {} perdus", status.packets_per_second, status.lost);
        match status.output {
            OutputState::Ready => (format!("vJoy prêt · {traffic}"), Tone::Ok),
            OutputState::DryRun => (format!("Mode test sans vJoy · {traffic}"), Tone::Warning),
            OutputState::DriverMissing => ("vJoy n'est pas installé".into(), Tone::Error),
            OutputState::DriverDisabled => ("vJoy est désactivé".into(), Tone::Error),
            OutputState::DeviceUnavailable => ("Périphérique vJoy indisponible".into(), Tone::Error),
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
