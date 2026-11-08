//! The UDP loop: receive controls, apply them, answer with status packets.

use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{info, warn};
use tx16s_protocol::{Control, Status};

use crate::config::Config;
use crate::link::Link;
use crate::output::Output;

/// Bounds how long a stop request waits for the loop to notice it.
const READ_TIMEOUT: Duration = Duration::from_millis(100);
const MAINTAIN_INTERVAL: Duration = Duration::from_secs(3);

pub fn run(config: &Config, output: &mut dyn Output, stop: &Arc<AtomicBool>) -> Result<()> {
    let socket =
        UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], config.port))).with_context(|| {
            format!(
                "cannot listen on UDP port {} (the service may already be running: \
                 tx16s-receiver status)",
                config.port
            )
        })?;
    socket.set_read_timeout(Some(READ_TIMEOUT))?;
    info!(
        port = config.port,
        allowed_sender = ?config.allowed_sender,
        timeout_ms = config.timeout_ms,
        "listening"
    );

    let mut link = Link::new(config.timeout(), config.allowed_sender, Instant::now());
    let mut last_maintain = Instant::now();
    let mut buffer = [0u8; 64];
    output.apply(&Control::NEUTRAL);

    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buffer) {
            Ok((length, from)) => {
                if let Some(accepted) = link.receive(Instant::now(), from, &buffer[..length]) {
                    if accepted.connected {
                        info!(%from, "radio connected");
                    }
                    output.apply(&accepted.control);
                }
            }
            Err(error) if is_benign(error.kind()) => {}
            Err(error) => {
                warn!(%error, "receive error");
                std::thread::sleep(READ_TIMEOUT);
            }
        }

        let now = Instant::now();
        let tick = link.tick(now);
        if tick.failsafe {
            info!("signal lost: axes centered, buttons released");
            output.apply(&Control::NEUTRAL);
        }
        if let Some(due) = tick.status {
            let status = Status {
                output: output.state(),
                packets_per_second: due.packets_per_second,
                lost: due.lost,
            };
            if let Err(error) = socket.send_to(&status.encode(), due.to) {
                warn!(%error, to = %due.to, "cannot send status");
            }
        }
        if let Some(stats) = tick.stats {
            info!(
                packets_per_second = stats.packets_per_second,
                lost = stats.lost,
                "last minute statistics"
            );
        }
        if now.duration_since(last_maintain) >= MAINTAIN_INTERVAL {
            last_maintain = now;
            output.maintain();
            if link.peer().is_none() {
                // A device acquired while nobody sends must still sit at neutral.
                output.apply(&Control::NEUTRAL);
            }
        }
    }

    output.apply(&Control::NEUTRAL);
    info!("stopped");
    Ok(())
}

fn is_benign(kind: ErrorKind) -> bool {
    // Windows reports ICMP "port unreachable" from a previous send_to as ConnectionReset.
    matches!(
        kind,
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset
    )
}
