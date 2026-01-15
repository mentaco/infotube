use anyhow::Result;
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

use crate::config::WsConfig;
use crate::event::Event;
use crate::json;

pub fn start(ws_configs: Vec<WsConfig>, tx: mpsc::UnboundedSender<Event>) {
    for config in ws_configs {
        if !config.enabled {
            continue;
        }

        let tx = tx.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = connect_and_listen(&config, &tx).await {
                    eprintln!("WebSocket Error [{}]: {:?}", config.name, e);
                }
                // Retry delay
                time::sleep(Duration::from_secs(5)).await;
            }
        });
    }
}

async fn connect_and_listen(config: &WsConfig, tx: &mpsc::UnboundedSender<Event>) -> Result<()> {
    let url = Url::parse(&config.url)?;
    
    // Auto-convert https to wss if needed, though usually user should provide wss
    let url_str = if url.scheme() == "https" {
        config.url.replace("https://", "wss://")
    } else if url.scheme() == "http" {
        config.url.replace("http://", "ws://")
    } else {
        config.url.clone()
    };

    let (ws_stream, _) = connect_async(&url_str).await?;
    let (_, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        match msg? {
            Message::Text(text) => {
                if let Some(display_text) = parse_message(&text, &config.json_keys) {
                    let _ = tx.send(Event::Message(format!("[{}] {}", config.name, display_text)));
                }
            }
            Message::Binary(_) => {} 
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => return Ok(()),
            Message::Frame(_) => {}
        }
    }

    Ok(())
}

fn parse_message(text: &str, keys: &Option<Vec<String>>) -> Option<String> {
    let keys = match keys {
        Some(k) => k,
        None => return Some(text.to_string()),
    };

    let Ok(json) = serde_json::from_str::<Value>(text) else {
        return None;
    };

    json::extract_message(&json, keys)
}
