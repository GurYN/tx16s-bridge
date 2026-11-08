//! Background thread that reads the radio and streams it to the PC.
//!
//! The UI only exchanges plain data with it: it pushes `Settings` and polls a
//! `Snapshot`, so a slow frame never delays the control stream.

use std::f32::consts::TAU;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use hidapi::HidApi;
use tx16s_protocol::{AXES, AXIS_MAX, Control, Status};

use crate::radio::{Radio, RadioInput, ReadOutcome};

/// Upper bound between two packets, so the PC's failsafe never fires while the radio is idle.
const KEEPALIVE: Duration = Duration::from_millis(20);
const RADIO_RETRY: Duration = Duration::from_secs(1);
/// The receiver reports about once per second.
pub const RECEIVER_STALE_AFTER: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub host: String,
    pub port: u16,
    pub sending: bool,
    pub test_signal: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RadioState {
    #[default]
    Searching,
    Connected(String),
    UnexpectedReport { name: String, len: usize },
    HidUnavailable(String),
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub radio: RadioState,
    pub input: Option<RadioInput>,
    pub sent_per_second: u32,
    pub target: Option<SocketAddr>,
    pub network_error: Option<String>,
    pub receiver: Option<(Status, Instant)>,
}

struct Shared {
    settings: Mutex<Settings>,
    snapshot: Mutex<Snapshot>,
    stop: AtomicBool,
}

pub struct Engine {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn start(settings: Settings) -> Self {
        let shared = Arc::new(Shared {
            settings: Mutex::new(settings),
            snapshot: Mutex::new(Snapshot::default()),
            stop: AtomicBool::new(false),
        });
        let worker = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("tx16s-engine".into())
            .spawn(move || Worker::new(worker).run())
            .expect("failed to start the engine thread");
        Self { shared, thread: Some(thread) }
    }

    pub fn update(&self, settings: Settings) {
        *self.shared.settings.lock().unwrap() = settings;
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot.lock().unwrap().clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    shared: Arc<Shared>,
    hid: Result<HidApi, String>,
    radio: Option<Radio>,
    last_radio_search: Option<Instant>,
    socket: Option<UdpSocket>,
    resolved: Option<(String, u16, Result<SocketAddr, String>)>,
    sequence: u32,
    last_send: Instant,
    sent_in_window: u32,
    window_start: Instant,
    started: Instant,
    snapshot: Snapshot,
}

impl Worker {
    fn new(shared: Arc<Shared>) -> Self {
        let hid = HidApi::new().map_err(|e| e.to_string());
        #[cfg(target_os = "macos")]
        if let Ok(api) = &hid {
            // Leave the radio usable by other apps (a simulator running on the Mac).
            api.set_open_exclusive(false);
        }
        let socket = UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| s.set_nonblocking(true).map(|()| s))
            .ok();
        let now = Instant::now();
        Self {
            shared,
            hid,
            radio: None,
            last_radio_search: None,
            socket,
            resolved: None,
            sequence: 0,
            last_send: now,
            sent_in_window: 0,
            window_start: now,
            started: now,
            snapshot: Snapshot::default(),
        }
    }

    fn run(mut self) {
        while !self.shared.stop.load(Ordering::Relaxed) {
            let settings = self.shared.settings.lock().unwrap().clone();
            let fresh_input = self.poll_input(&settings);
            self.stream(&settings, fresh_input);
            self.receive_status();
            *self.shared.snapshot.lock().unwrap() = self.snapshot.clone();
        }
    }

    /// Waits for the next radio report (or the keepalive delay) and records it.
    fn poll_input(&mut self, settings: &Settings) -> bool {
        if settings.test_signal {
            thread::sleep(KEEPALIVE);
            self.snapshot.input = Some(test_pattern(self.started.elapsed()));
            return true;
        }
        self.connect_radio();
        let Some(radio) = &self.radio else {
            self.snapshot.input = None;
            thread::sleep(KEEPALIVE);
            return false;
        };
        match radio.read(KEEPALIVE.as_millis() as i32) {
            ReadOutcome::Input(input) => {
                self.snapshot.radio = RadioState::Connected(radio.name.clone());
                self.snapshot.input = Some(input);
                true
            }
            ReadOutcome::Idle => false,
            ReadOutcome::UnexpectedReport(len) => {
                self.snapshot.radio = RadioState::UnexpectedReport { name: radio.name.clone(), len };
                self.snapshot.input = None;
                false
            }
            ReadOutcome::Disconnected => {
                self.radio = None;
                self.snapshot.radio = RadioState::Searching;
                self.snapshot.input = None;
                false
            }
        }
    }

    fn connect_radio(&mut self) {
        if self.radio.is_some()
            || self.last_radio_search.is_some_and(|t| t.elapsed() < RADIO_RETRY)
        {
            return;
        }
        self.last_radio_search = Some(Instant::now());
        match &mut self.hid {
            Ok(api) => {
                self.radio = Radio::find(api);
                self.snapshot.radio = match &self.radio {
                    Some(radio) => RadioState::Connected(radio.name.clone()),
                    None => RadioState::Searching,
                };
            }
            Err(error) => self.snapshot.radio = RadioState::HidUnavailable(error.clone()),
        }
    }

    fn stream(&mut self, settings: &Settings, fresh_input: bool) {
        self.update_rate();
        let target = self.resolve(settings);
        self.snapshot.target = target;
        let (Some(target), Some(input), Some(socket)) = (target, self.snapshot.input, &self.socket)
        else {
            return;
        };
        if !settings.sending || (!fresh_input && self.last_send.elapsed() < KEEPALIVE) {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1);
        let packet = Control { sequence: self.sequence, buttons: input.buttons, axes: input.axes };
        match socket.send_to(&packet.encode(), target) {
            Ok(_) => {
                self.sent_in_window += 1;
                self.snapshot.network_error = None;
            }
            Err(error) => self.snapshot.network_error = Some(error.to_string()),
        }
        self.last_send = Instant::now();
    }

    fn update_rate(&mut self) {
        let elapsed = self.window_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.snapshot.sent_per_second = (self.sent_in_window as f32 / elapsed.as_secs_f32()) as u32;
            self.sent_in_window = 0;
            self.window_start = Instant::now();
        }
    }

    /// Resolves the host only when the user changes it, since lookups can block.
    fn resolve(&mut self, settings: &Settings) -> Option<SocketAddr> {
        let host = settings.host.trim();
        let stale = !matches!(&self.resolved, Some((h, p, _)) if h == host && *p == settings.port);
        if stale {
            let result = if host.is_empty() {
                Err("Enter the PC address".to_owned())
            } else {
                (host, settings.port)
                    .to_socket_addrs()
                    .map_err(|e| e.to_string())
                    .and_then(|mut addrs| {
                        addrs.find(SocketAddr::is_ipv4).ok_or_else(|| "No IPv4 address found".to_owned())
                    })
            };
            self.snapshot.receiver = None;
            self.resolved = Some((host.to_owned(), settings.port, result));
        }
        match &self.resolved {
            Some((_, _, Ok(addr))) => Some(*addr),
            Some((_, _, Err(error))) => {
                self.snapshot.network_error = Some(error.clone());
                None
            }
            None => None,
        }
    }

    fn receive_status(&mut self) {
        let Some(socket) = &self.socket else { return };
        let mut buffer = [0u8; 64];
        while let Ok((len, from)) = socket.recv_from(&mut buffer) {
            if Some(from.ip()) != self.snapshot.target.map(|t| t.ip()) {
                continue;
            }
            if let Some(status) = Status::decode(&buffer[..len]) {
                self.snapshot.receiver = Some((status, Instant::now()));
            }
        }
    }
}

/// Slow sine waves on every axis and one button lit at a time, to test without the radio.
fn test_pattern(elapsed: Duration) -> RadioInput {
    let t = elapsed.as_secs_f32();
    let mut axes = [0; AXES];
    for (i, axis) in axes.iter_mut().enumerate() {
        *axis = ((t * TAU / 6.0 + i as f32 * 0.8).sin() * f32::from(AXIS_MAX)) as i16;
    }
    RadioInput { axes, buttons: 1 << (elapsed.as_secs() % 24) }
}
