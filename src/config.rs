use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Where files land. `Month` = Phone/2026-10/, `Mirror` = keep the phone's
/// folder layout, `Flat` = everything directly inside the device folder.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Organize {
    #[default]
    Month,
    Mirror,
    Flat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub organize: Organize,
    /// Skip dotfiles and Android bookkeeping folders
    pub skip_junk: bool,
    /// Write a small .json next to every file describing where it came from
    pub sidecars: bool,
    /// Pause all uploads without stopping the server
    pub paused: bool,
    /// Storage folder chosen in the dashboard; `None` = the default
    /// `%USERPROFILE%\PhoneSync`.
    pub storage_root: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            organize: Organize::Month,
            skip_junk: true,
            sidecars: false,
            paused: false,
            storage_root: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub token: String,
    pub port: u16,
    pub device_name: String,
    pub settings: Settings,
    #[serde(skip)]
    pub root: PathBuf,
    #[serde(skip)]
    pub config_file: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            token: util_token(),
            port: 8787,
            device_name: hostname(),
            settings: Settings::default(),
            root: default_root(),
            config_file: default_config_file(),
        }
    }
}

pub fn default_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("PhoneSync")
}

pub fn default_config_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("phonesync")
        .join("config.json")
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "laptop".to_string())
}

fn util_token() -> String {
    // Short enough to type, long enough that guessing on a LAN is hopeless.
    let raw = uuid::Uuid::new_v4().simple().to_string();
    raw[..12].to_string()
}

impl Config {
    pub fn load(root_override: Option<PathBuf>) -> Result<Self> {
        let config_file = default_config_file();
        let mut cfg = match std::fs::read(&config_file) {
            Ok(bytes) => serde_json::from_slice::<Config>(&bytes).unwrap_or_default(),
            Err(_) => Config::default(),
        };
        cfg.config_file = config_file;
        let saved_root = cfg.settings.storage_root.as_ref().map(PathBuf::from);
        cfg.root = root_override.or(saved_root).unwrap_or_else(default_root);
        if cfg.token.is_empty() {
            cfg.token = util_token();
        }
        if cfg.device_name.is_empty() {
            cfg.device_name = hostname();
        }
        cfg.save()?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.config_file.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let tmp = self.config_file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.config_file)
            .with_context(|| format!("saving {}", self.config_file.display()))?;
        Ok(())
    }

    pub fn meta_dir(&self) -> PathBuf {
        self.root.join(".phonesync")
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.meta_dir().join("sessions")
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [self.root.clone(), self.meta_dir(), self.sessions_dir()] {
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        Ok(())
    }
}

/// Directory holding the watcher state of `send --watch`.
pub fn client_state_file() -> PathBuf {
    default_config_file().with_file_name("sent.json")
}
