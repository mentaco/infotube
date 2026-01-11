mod app;
mod config;

use anyhow::Result;
use app::App;
use config::Config;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use tokio::{io::AsyncReadExt, net::TcpListener, sync::mpsc};

#[tokio::main]
async fn main() -> Result<()> {
    // Load config
    let config = Config::load("config.toml").unwrap_or_else(|e| {
        eprintln!("Failed to load config: {}, using defaults", e);
        Config::default()
    });

    // Setup channel
    let (tx, rx) = mpsc::channel(32);
    let port = config.listen_port;

    // Start TCP listener
    tokio::spawn(async move {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await;
        if let Ok(listener) = listener {
            loop {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let mut buf = vec![0; 1024];
                        if let Ok(n) = socket.read(&mut buf).await {
                            if n > 0 {
                                let msg = String::from_utf8_lossy(&buf[..n]).to_string();
                                let msg = msg.trim().to_string();
                                if !msg.is_empty() {
                                    let _ = tx.send(msg).await;
                                }
                            }
                        }
                    });
                }
            }
        } else {
             eprintln!("Failed to bind to port {}", port);
        }
    });

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create and run app
    let mut app = App::new(config);
    let res = app.run(&mut terminal, rx).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}
