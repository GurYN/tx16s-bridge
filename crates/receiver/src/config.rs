//! Receiver settings, read from a TOML file.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// UDP port the Mac sends to.
    pub port: u16,
    /// vJoy device number (1 to 16).
    pub device: u32,
    /// When set, packets from any other address are ignored.
    pub allowed_sender: Option<IpAddr>,
    /// Silence after which axes are centered and buttons released.
    pub timeout_ms: u64,
    /// Full path to vJoyInterface.dll, when vJoy is not in its default folder.
    pub vjoy_dll: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: tx16s_protocol::DEFAULT_PORT,
            device: 1,
            allowed_sender: None,
            timeout_ms: 500,
            vjoy_dll: None,
        }
    }
}

/// Written by `install` so the settings are discoverable next to the logs.
pub const DEFAULT_FILE: &str = r#"# Configuration de tx16s-receiver.
# Redemarre le service apres une modification : tx16s-receiver stop puis start.

# Port UDP sur lequel le Mac envoie la radio.
port = 47800

# Numero du peripherique vJoy a piloter (1 a 16).
device = 1

# N'accepte que les paquets venant de cette adresse IP (celle du Mac).
# allowed_sender = "10.0.0.52"

# Delai sans paquet, en millisecondes, avant de recentrer les axes.
timeout_ms = 500

# Chemin complet de vJoyInterface.dll, si vJoy n'est pas dans son dossier par defaut.
# vjoy_dll = 'C:\Program Files\vJoy\x64\vJoyInterface.dll'
"#;

impl Config {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    /// Reads `path`; a missing file means default settings.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("configuration invalide : {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => {
                Err(error).with_context(|| format!("lecture impossible : {}", path.display()))
            }
        }
    }
}

/// `%ProgramData%\tx16s-bridge`, where the service keeps its settings and logs.
#[cfg(windows)]
pub fn data_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("tx16s-bridge")
}

/// The config file used when `--config` is not given.
pub fn default_path() -> Option<PathBuf> {
    #[cfg(windows)]
    return Some(data_dir().join("config.toml"));
    #[cfg(not(windows))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_defaults() {
        let parsed: Config = toml::from_str(DEFAULT_FILE).unwrap();
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let parsed: Config =
            toml::from_str("allowed_sender = \"10.0.0.52\"\ntimeout_ms = 250").unwrap();
        assert_eq!(parsed.allowed_sender, Some("10.0.0.52".parse().unwrap()));
        assert_eq!(parsed.timeout(), Duration::from_millis(250));
        assert_eq!(parsed.port, tx16s_protocol::DEFAULT_PORT);
    }

    #[test]
    fn rejects_typos() {
        assert!(toml::from_str::<Config>("prot = 1").is_err());
    }

    #[test]
    fn missing_file_means_defaults() {
        let config = Config::load(Some(Path::new("/nonexistent/tx16s.toml"))).unwrap();
        assert_eq!(config, Config::default());
    }
}
