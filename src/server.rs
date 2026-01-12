use crate::event::Event;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// Starts the TCP listener and sends received messages to the event channel.
pub fn start(port: u16, tx: mpsc::UnboundedSender<Event>) {
    tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", port);
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind to {}: {}", addr, e);
                return;
            }
        };

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
                                let _ = tx.send(Event::Message(msg));
                            }
                        }
                    }
                });
            }
        }
    });
}
