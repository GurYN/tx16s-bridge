//! Receives the TX16S radio from the Mac over UDP and feeds a vJoy joystick.

// The vJoy and service helpers are only reachable on Windows.
#![cfg_attr(not(windows), allow(dead_code))]

mod config;
mod link;
mod logging;
mod output;
mod server;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod vjoy;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::output::{DryRun, Output};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Configuration file (default: %ProgramData%\tx16s-bridge\config.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the receiver in this terminal (Ctrl+C to stop).
    Run {
        /// Do not use vJoy: log what is received instead.
        #[arg(long)]
        dry_run: bool,
        /// UDP port, overriding the configuration.
        #[arg(long)]
        port: Option<u16>,
    },
    /// Install and start the Windows service (administrator terminal).
    Install,
    /// Stop and remove the Windows service.
    Uninstall,
    /// Start the service.
    Start,
    /// Stop the service.
    Stop,
    /// Show the service state.
    Status,
    /// Entry point used by the Windows service manager.
    #[command(hide = true)]
    Service,
}

pub fn make_output(config: &Config, dry_run: bool) -> Result<Box<dyn Output>> {
    if dry_run {
        return Ok(Box::new(DryRun::new()));
    }
    #[cfg(windows)]
    return Ok(Box::new(vjoy::VJoy::new(
        config.device,
        config.vjoy_dll.clone(),
    )));
    #[cfg(not(windows))]
    {
        let _ = config;
        anyhow::bail!("vJoy only exists on Windows: use --dry-run to test")
    }
}

fn run(config_path: Option<PathBuf>, dry_run: bool, port: Option<u16>) -> Result<()> {
    logging::console();
    let mut config = Config::load(config_path.or_else(config::default_path).as_deref())?;
    if let Some(port) = port {
        config.port = port;
    }
    let mut output = make_output(&config, dry_run)?;
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = stop.clone();
    ctrlc::set_handler(move || handler_stop.store(true, Ordering::Relaxed))?;
    server::run(&config, output.as_mut(), &stop)
}

#[cfg(windows)]
fn manage(command: Command, config_path: Option<PathBuf>) -> Result<()> {
    match command {
        Command::Install => service::install(config_path),
        Command::Uninstall => service::uninstall(),
        Command::Start => service::start(),
        Command::Stop => service::stop(),
        Command::Status => service::status(),
        Command::Service => service::dispatch(config_path),
        Command::Run { .. } => unreachable!(),
    }
}

#[cfg(not(windows))]
fn manage(_command: Command, _config_path: Option<PathBuf>) -> Result<()> {
    anyhow::bail!("the service only exists on Windows; here, use 'run --dry-run'")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run { dry_run, port } => run(cli.config, dry_run, port),
        command => manage(command, cli.config),
    }
}
