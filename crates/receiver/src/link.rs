//! Connection state, independent of sockets so it can be tested with fake time.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use tx16s_protocol::Control;

const STATUS_INTERVAL: Duration = Duration::from_secs(1);
const STATS_INTERVAL: Duration = Duration::from_secs(60);

struct Peer {
    address: SocketAddr,
    last_sequence: u32,
    last_packet: Instant,
}

/// Counts accepted and lost packets over a time window.
struct Window {
    started: Instant,
    received: u32,
    lost: u32,
}

impl Window {
    fn new(now: Instant) -> Self {
        Self {
            started: now,
            received: 0,
            lost: 0,
        }
    }

    fn rate(&self, now: Instant) -> u32 {
        let seconds = now.duration_since(self.started).as_secs_f64();
        if seconds > 0.0 {
            (f64::from(self.received) / seconds).round() as u32
        } else {
            0
        }
    }
}

pub struct Accepted {
    pub control: Control,
    /// The first packet after a silence: the link just came up.
    pub connected: bool,
}

/// Network figures to send back to the sender.
#[derive(Debug, PartialEq, Eq)]
pub struct StatusDue {
    pub to: SocketAddr,
    pub packets_per_second: u32,
    pub lost: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Stats {
    pub packets_per_second: u32,
    pub lost: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tick {
    /// The link timed out: the output must go back to neutral.
    pub failsafe: bool,
    pub status: Option<StatusDue>,
    /// Once a minute, when packets were received during that minute.
    pub stats: Option<Stats>,
}

pub struct Link {
    timeout: Duration,
    allowed_sender: Option<IpAddr>,
    peer: Option<Peer>,
    status_window: Window,
    stats_window: Window,
}

impl Link {
    pub fn new(timeout: Duration, allowed_sender: Option<IpAddr>, now: Instant) -> Self {
        Self {
            timeout,
            allowed_sender,
            peer: None,
            status_window: Window::new(now),
            stats_window: Window::new(now),
        }
    }

    /// Returns the control to apply, or `None` for a foreign, stale or duplicate packet.
    pub fn receive(&mut self, now: Instant, from: SocketAddr, data: &[u8]) -> Option<Accepted> {
        if self.allowed_sender.is_some_and(|ip| ip != from.ip()) {
            return None;
        }
        let control = Control::decode(data)?;
        let connected = match &mut self.peer {
            Some(peer) => {
                if control.sequence <= peer.last_sequence {
                    return None;
                }
                let lost = control.sequence - peer.last_sequence - 1;
                self.status_window.lost = self.status_window.lost.saturating_add(lost);
                self.stats_window.lost = self.stats_window.lost.saturating_add(lost);
                peer.address = from;
                peer.last_sequence = control.sequence;
                peer.last_packet = now;
                false
            }
            None => {
                // A restarted sender begins at a low sequence: accept it after the timeout.
                self.peer = Some(Peer {
                    address: from,
                    last_sequence: control.sequence,
                    last_packet: now,
                });
                self.status_window = Window::new(now);
                true
            }
        };
        self.status_window.received += 1;
        self.stats_window.received += 1;
        Some(Accepted { control, connected })
    }

    pub fn tick(&mut self, now: Instant) -> Tick {
        let mut tick = Tick::default();
        if let Some(peer) = &self.peer {
            if now.duration_since(peer.last_packet) > self.timeout {
                self.peer = None;
                tick.failsafe = true;
            } else if now.duration_since(self.status_window.started) >= STATUS_INTERVAL {
                tick.status = Some(StatusDue {
                    to: peer.address,
                    packets_per_second: self.status_window.rate(now),
                    lost: self.status_window.lost,
                });
                self.status_window = Window::new(now);
            }
        }
        if now.duration_since(self.stats_window.started) >= STATS_INTERVAL {
            if self.stats_window.received > 0 {
                tick.stats = Some(Stats {
                    packets_per_second: self.stats_window.rate(now),
                    lost: self.stats_window.lost,
                });
            }
            self.stats_window = Window::new(now);
        }
        tick
    }

    pub fn peer(&self) -> Option<SocketAddr> {
        self.peer.as_ref().map(|peer| peer.address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_millis(500);

    fn mac() -> SocketAddr {
        "10.0.0.52:50000".parse().unwrap()
    }

    fn packet(sequence: u32) -> [u8; Control::SIZE] {
        Control {
            sequence,
            buttons: 0,
            axes: [sequence as i16; 8],
        }
        .encode()
    }

    fn ms(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn first_packet_connects() {
        let t0 = Instant::now();
        let mut link = Link::new(TIMEOUT, None, t0);
        let accepted = link.receive(t0, mac(), &packet(10)).unwrap();
        assert!(accepted.connected);
        assert_eq!(accepted.control.sequence, 10);
        assert!(
            !link
                .receive(ms(t0, 1), mac(), &packet(11))
                .unwrap()
                .connected
        );
        assert_eq!(link.peer(), Some(mac()));
    }

    #[test]
    fn drops_duplicates_and_reordered_packets() {
        let t0 = Instant::now();
        let mut link = Link::new(TIMEOUT, None, t0);
        link.receive(t0, mac(), &packet(5)).unwrap();
        assert!(link.receive(t0, mac(), &packet(5)).is_none());
        assert!(link.receive(t0, mac(), &packet(4)).is_none());
    }

    #[test]
    fn ignores_garbage_and_other_senders() {
        let t0 = Instant::now();
        let mut link = Link::new(TIMEOUT, Some("10.0.0.52".parse().unwrap()), t0);
        assert!(
            link.receive(t0, "10.0.0.99:1".parse().unwrap(), &packet(1))
                .is_none()
        );
        assert!(link.receive(t0, mac(), b"not a packet").is_none());
        assert!(link.receive(t0, mac(), &packet(1)).is_some());
    }

    #[test]
    fn reports_rate_and_losses_every_second() {
        let t0 = Instant::now();
        let mut link = Link::new(TIMEOUT, None, t0);
        // 100 packets over one second, sequences 1..=200 step 2: 99 lost.
        for i in 0..100u64 {
            link.receive(ms(t0, i * 10), mac(), &packet(1 + 2 * i as u32));
        }
        assert_eq!(link.tick(ms(t0, 990)).status, None);
        let tick = link.tick(ms(t0, 1000));
        assert_eq!(
            tick.status,
            Some(StatusDue {
                to: mac(),
                packets_per_second: 100,
                lost: 99
            })
        );
        // The window restarts after each report.
        link.receive(ms(t0, 1600), mac(), &packet(1000));
        link.receive(ms(t0, 1990), mac(), &packet(1001));
        let status = link.tick(ms(t0, 2000)).status.unwrap();
        assert_eq!(
            (status.packets_per_second, status.lost),
            (2, 1000 - 199 - 1)
        );
    }

    #[test]
    fn failsafe_after_timeout_then_reconnects_with_restarted_sender() {
        let t0 = Instant::now();
        let mut link = Link::new(TIMEOUT, None, t0);
        link.receive(t0, mac(), &packet(5000));
        assert!(!link.tick(ms(t0, 500)).failsafe);
        let tick = link.tick(ms(t0, 501));
        assert!(tick.failsafe);
        assert_eq!(link.peer(), None);
        // Only once.
        assert!(!link.tick(ms(t0, 600)).failsafe);
        // A restarted sender counts from 1 again.
        assert!(
            link.receive(ms(t0, 700), mac(), &packet(1))
                .unwrap()
                .connected
        );
    }

    #[test]
    fn no_status_while_disconnected() {
        let t0 = Instant::now();
        let mut link = Link::new(TIMEOUT, None, t0);
        assert_eq!(link.tick(ms(t0, 5000)), Tick::default());
    }

    #[test]
    fn stats_once_a_minute_only_with_traffic() {
        let t0 = Instant::now();
        let mut link = Link::new(TIMEOUT, None, t0);
        link.receive(ms(t0, 59_900), mac(), &packet(1));
        let stats = link.tick(ms(t0, 60_000)).stats.unwrap();
        assert_eq!(stats.lost, 0);
        assert_eq!(link.tick(ms(t0, 120_000)).stats, None);
    }
}
