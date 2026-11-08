//! Settings remembered between launches.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tx16s_protocol::DEFAULT_PORT;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// PC address, optionally with `:port`.
    pub target: String,
    /// Resume streaming on launch if it was on when the app was closed.
    pub sending: bool,
}

impl Config {
    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("tx16s-bridge").join("sender.toml"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let (Some(dir), Ok(text)) = (path.parent(), toml::to_string(self)) {
            let _ = fs::create_dir_all(dir).and_then(|()| fs::write(&path, text));
        }
    }

    /// Splits `host[:port]`; the port defaults to the protocol's.
    pub fn host_and_port(&self) -> (String, u16) {
        let target = self.target.trim();
        match target.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => match port.parse() {
                Ok(port) => (host.to_owned(), port),
                Err(_) => (target.to_owned(), DEFAULT_PORT),
            },
            _ => (target.to_owned(), DEFAULT_PORT),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(target: &str) -> (String, u16) {
        Config { target: target.into(), sending: false }.host_and_port()
    }

    #[test]
    fn parses_targets() {
        assert_eq!(split(" 10.0.0.200 "), ("10.0.0.200".into(), DEFAULT_PORT));
        assert_eq!(split("10.0.0.200:5000"), ("10.0.0.200".into(), 5000));
        assert_eq!(split("gaming-pc.local"), ("gaming-pc.local".into(), DEFAULT_PORT));
    }
}
