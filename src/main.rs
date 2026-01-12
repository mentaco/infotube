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

/// エントリポイント
#[tokio::main]
async fn main() -> Result<()> {
    // 1. 設定の読み込み
    // ~/.config/infotube/config.toml があれば読み込み、なければデフォルト設定を使用
    let config = dirs::home_dir()
        .map(|home| home.join(".config/infotube/config.toml"))
        .filter(|path| path.exists())
        .and_then(|path| Config::load(path).ok())
        .unwrap_or_else(Config::default);

    // 2. TCP割り込み通知用のチャンネル作成
    // 非同期タスクからメインのUIループへメッセージを送るためのMPSCチャンネル
    let (tx, rx) = mpsc::channel(32);
    let port = config.listen_port;

    // 3. TCPリスナーの起動 (バックグラウンドタスク)
    tokio::spawn(async move {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await;
        if let Ok(listener) = listener {
            loop {
                // クライアントからの接続を待機
                if let Ok((mut socket, _)) = listener.accept().await {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let mut buf = vec![0; 1024];
                        // データを読み取り、UTF-8文字列としてチャンネルへ送信
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

    // 4. ターミナルの初期化 (TUIモードへの移行)
    enable_raw_mode()?; // キー入力を即座に受け取るRawモード
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?; // 代替画面への切り替え
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 5. アプリケーションの実行
    let mut app = App::new(config);
    let res = app.run(&mut terminal, rx).await;

    // 6. ターミナルの復元 (終了処理)
    // プログラムが異常終了してもターミナルを元の状態に戻せるようにする
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Application error: {:?}", err);
    }

    Ok(())
}