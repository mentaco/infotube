use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;

use crate::config::ApiConfig;
use crate::event::Event;
use crate::json;

pub fn start(api_configs: Vec<ApiConfig>, tx: mpsc::UnboundedSender<Event>) {
    let client = Client::new();

    for config in api_configs {
        if !config.enabled {
            continue;
        }
        
        let tx = tx.clone();
        let client = client.clone();
        
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(config.interval_sec));
            // First tick finishes immediately
            interval.tick().await;

            loop {
                interval.tick().await;
                
                match fetch_and_process(&client, &config).await {
                    Ok(Some(msg)) => {
                        let _ = tx.send(Event::Message(format!("[{}] {}", config.name, msg)));
                    }
                    Ok(None) => {
                        // Data not found or empty, ignore
                    }
                    Err(e) => {
                        eprintln!("API Error [{}]: {:?}", config.name, e);
                    }
                }
            }
        });
    }
}

async fn fetch_and_process(client: &Client, config: &ApiConfig) -> Result<Option<String>> {
    let resp = client.get(&config.url).send().await?;
    
    if !resp.status().is_success() {
        return Ok(None);
    }

    if let Some(keys) = &config.json_keys {
        let json: Value = resp.json().await?;
        Ok(json::extract_message(&json, keys))
    } else {
        let text = resp.text().await?;
        Ok(Some(text))
    }
}
