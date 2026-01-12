mod app;
mod config;
mod event;
mod server;
mod tui;
mod ui;

use anyhow::Result;
use app::App;
use config::Config;
use event::EventHandler;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load config
    let config = dirs::home_dir()
        .map(|home| home.join(".config/infotube/config.toml"))
        .filter(|path| path.exists())
        .and_then(|path| Config::load(path).ok())
        .unwrap_or_else(Config::default);

    // 2. Init Event Handler
    // Use scroll_speed_ms as the tick rate for animation
    let events = EventHandler::new(config.scroll_speed_ms);

    // 3. Start TCP Listener
    server::start(config.listen_port, events.sender());

    // 4. Init Terminal
    let mut terminal = tui::init()?;

    // 5. Run App
    let mut app = App::new(config);
    let res = app.run(&mut terminal, &mut (events as EventHandler)).await;

    // 6. Restore Terminal
    tui::restore(&mut terminal)?;

    if let Err(err) = res {
        eprintln!("Application error: {:?}", err);
    }

    Ok(())
}
