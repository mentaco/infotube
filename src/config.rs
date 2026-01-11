use serde::Deserialize;
use std::fs;
use std::path::Path;
use anyhow::Result;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub source_files: Vec<String>,
    pub scroll_speed_ms: u64,
    pub listen_port: u16,
    pub colors: Colors,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Colors {
    pub fg_default: String,
    pub bg_default: String,
    pub fg_alert: String,
    pub bg_alert: String,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source_files: vec![],
            scroll_speed_ms: 100,
            listen_port: 8080,
            colors: Colors {
                fg_default: "White".to_string(),
                bg_default: "Black".to_string(),
                fg_alert: "Red".to_string(),
                bg_alert: "Black".to_string(),
            },
        }
    }
}
