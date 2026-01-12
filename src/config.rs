use serde::Deserialize;
use std::fs;
use std::path::Path;
use anyhow::Result;

/// config.tomlの構造を定義する構造体
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// 読み込み対象となるテキストファイルのパスリスト
    pub source_files: Vec<String>,
    /// アニメーションの基本速度（1ティックあたりのミリ秒）
    pub scroll_speed_ms: u64,
    /// 割り込みをリッスンするポート番号
    pub listen_port: u16,
    /// 枠線を表示するかどうか
    #[serde(default = "default_show_frame")]
    pub show_frame: bool,
    /// 配色設定
    pub colors: Colors,
}

fn default_show_frame() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
pub struct Colors {
    /// 通常表示時の前景色 (例: "White", "Yellow")
    pub fg_default: String,
    /// 通常表示時の背景色 (例: "Black")
    pub bg_default: String,
    /// 緊急通知（Alert）時の前景色 (例: "Red")
    pub fg_alert: String,
    /// 緊急通知（Alert）時の背景色 (例: "Black")
    pub bg_alert: String,
}

impl Config {
    /// ファイルから設定を読み込む
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}

impl Default for Config {
    /// デフォルトの設定値
    fn default() -> Self {
        Self {
            source_files: vec![],
            scroll_speed_ms: 100,
            listen_port: 8080,
            show_frame: true,
            colors: Colors {
                fg_default: "White".to_string(),
                bg_default: "None".to_string(),
                fg_alert: "Red".to_string(),
                bg_alert: "None".to_string(),
            },
        }
    }
}