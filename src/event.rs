use crossterm::event::{Event as CrosstermEvent, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

/// Terminal events.
#[derive(Clone, Debug)]
pub enum Event {
    /// Terminal tick.
    Tick,
    /// Key press.
    Key(KeyEvent),
    /// External message (e.g. from TCP).
    Message(String),
    /// Terminal resize.
    Resize(u16),
}

/// Event handler.
#[derive(Debug)]
pub struct EventHandler {
    /// Event receiver channel.
    rx: mpsc::UnboundedReceiver<Event>,
    /// Event sender channel (to clone for other tasks).
    tx: mpsc::UnboundedSender<Event>,
    /// Sender to update the tick rate.
    tick_speed_tx: mpsc::UnboundedSender<u64>,
}

impl EventHandler {
    /// Constructs a new instance of `EventHandler`.
    pub fn new(tick_rate: u64) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let (tick_speed_tx, mut tick_speed_rx) = mpsc::unbounded_channel();
        let _tx = tx.clone();
        
        // Spawn a task to handle tick and key events
        tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut interval = tokio::time::interval(Duration::from_millis(tick_rate));
            
            loop {
                let tick_delay = interval.tick();
                let crossterm_event = reader.next();
                
                tokio::select! {
                    // Update tick rate if requested
                    Some(new_rate) = tick_speed_rx.recv() => {
                        interval = tokio::time::interval(Duration::from_millis(new_rate));
                    }
                    _ = tick_delay => {
                        if _tx.send(Event::Tick).is_err() {
                            break;
                        }
                    }
                    Some(Ok(evt)) = crossterm_event => {
                        match evt {
                            CrosstermEvent::Key(key) => {
                                if key.kind == crossterm::event::KeyEventKind::Press {
                                    if _tx.send(Event::Key(key)).is_err() {
                                        break;
                                    }
                                }
                            }
                            CrosstermEvent::Resize(w, _h) => {
                                if _tx.send(Event::Resize(w)).is_err() {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Self { rx, tx, tick_speed_tx }
    }

    /// Set a new tick rate.
    pub fn set_tick_rate(&self, tick_rate: u64) {
        let _ = self.tick_speed_tx.send(tick_rate);
    }

    /// Get a sender to the event channel.
    pub fn sender(&self) -> mpsc::UnboundedSender<Event> {
        self.tx.clone()
    }

    /// Receive the next event.
    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}
use futures::StreamExt;
