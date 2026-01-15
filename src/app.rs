use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use std::{fs, path::PathBuf};
use unicode_width::UnicodeWidthStr;

use crate::config::Config;
use crate::event::{Event, EventHandler};
use crate::tui::Tui;
use crate::ui;

/// Application state
pub struct App {
    pub running: bool,
    pub config: Config,
    pub text: String,
    pub scroll_offset: usize,
    
    // --- Interrupt State ---
    pub interrupt_text: Option<String>,
    pub interrupt_remaining_ms: usize,
    pub paused_before_interrupt: bool,
    pub saved_scroll_offset: usize,

    // --- User State ---
    pub paused: bool,
    pub dimmed: bool,

    // --- Layout State ---
    pub width: usize,
}

impl App {
    pub fn new(config: Config) -> Self {
        let text = Self::load_content(&config);
        
        let width = if let Ok((w, _)) = crossterm::terminal::size() {
            w as usize
        } else {
            80
        };

        Self {
            running: true,
            config,
            text,
            scroll_offset: 0,
            interrupt_text: None,
            interrupt_remaining_ms: 0,
            paused_before_interrupt: false,
            saved_scroll_offset: 0,
            paused: false,
            dimmed: false,
            width,
        }
    }

    fn load_content(config: &Config) -> String {
        let mut all_files_content = Vec::new();

        for path_str in &config.source_files {
            let path = Self::expand_path(path_str);
            if let Ok(content) = fs::read_to_string(&path) {
                let file_text = content
                    .lines()
                    .map(|line| line.trim())
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<&str>>()
                    .join("    ");
                
                if !file_text.is_empty() {
                    all_files_content.push(file_text);
                }
            } else {
                eprintln!("Failed to read file: {:?}", path);
            }
        }
        
        if all_files_content.is_empty() {
            "No data found in source files.".to_string()
        } else {
            all_files_content.join("    ***    ")
        }
    }

    fn expand_path(path_str: &str) -> PathBuf {
        if path_str.starts_with("~") {
            if let Some(home) = dirs::home_dir() {
                if path_str == "~" {
                     home
                } else if path_str.starts_with("~/") {
                     home.join(&path_str[2..])
                } else {
                     PathBuf::from(path_str)
                }
            } else {
                PathBuf::from(path_str)
            }
        } else {
            PathBuf::from(path_str)
        }
    }

    pub async fn run(&mut self, terminal: &mut Tui, events: &mut EventHandler) -> Result<()> {
        while self.running {
            terminal.draw(|f| ui::draw(f, self))?;

            match events.next().await {
                Some(Event::Tick) => {
                    if !self.paused {
                        self.on_tick();
                    }
                }
                Some(Event::Key(key)) => self.handle_key(key, events),
                Some(Event::Message(msg)) => self.on_message(msg),
                Some(Event::Resize(w)) => {
                    self.width = w as usize;
                }
                None => break,
            }
        }
        Ok(())
    }

    fn on_message(&mut self, msg: String) {
        // Play sound
        let sound_name = &self.config.alert_sound;
        if !sound_name.is_empty() && !sound_name.eq_ignore_ascii_case("None") {
            let mut sound_path = format!("/System/Library/Sounds/{}", sound_name);
            if !sound_name.ends_with(".aiff") {
                sound_path.push_str(".aiff");
            }
            // Only play on macOS for now, or check for generic player?
            // The original code was macOS specific.
            if std::env::consts::OS == "macos" {
                tokio::spawn(async move {
                    let _ = tokio::process::Command::new("afplay")
                        .arg(sound_path)
                        .output()
                        .await;
                });
            }
        }

        self.paused_before_interrupt = self.paused;
        self.saved_scroll_offset = self.scroll_offset;
        self.paused = false;
        self.interrupt_text = Some(msg);
        self.interrupt_remaining_ms = (self.config.interrupt_duration_sec * 1000) as usize;
        self.scroll_offset = 0;
    }

    fn on_tick(&mut self) {
        // Use cached width
        let width = if self.config.show_frame {
            self.width.saturating_sub(2)
        } else {
            self.width
        };
        
        if let Some(ref text) = self.interrupt_text {
            let elapsed = self.config.scroll_speed_ms as usize;
            if self.interrupt_remaining_ms > elapsed {
                self.interrupt_remaining_ms -= elapsed;
            } else {
                self.interrupt_text = None;
                self.paused = self.paused_before_interrupt;
                self.scroll_offset = self.saved_scroll_offset;
                return;
            }
            
            let seconds = (self.interrupt_remaining_ms as f64 / 1000.0).ceil() as usize;
            let display_text = format!("({}s)  {}", seconds, text);

             if display_text.width() > width {
                 self.scroll_offset += 1;
             }
             return;
        }

        if self.text.width() > width {
             self.scroll_offset += 1;
        } else {
             self.scroll_offset = 0;
        }
    }

    fn handle_key(&mut self, key: KeyEvent, events: &EventHandler) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Enter => {
                if self.interrupt_text.is_some() {
                    self.interrupt_text = None;
                    self.paused = self.paused_before_interrupt;
                    self.scroll_offset = self.saved_scroll_offset;
                }
            }
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('f') => self.config.show_frame = !self.config.show_frame,
            KeyCode::Char('b') => self.dimmed = !self.dimmed,
            KeyCode::Char('+') | KeyCode::Char('k') => {
                if self.config.scroll_speed_ms > 10 {
                    self.config.scroll_speed_ms -= 10;
                    events.set_tick_rate(self.config.scroll_speed_ms);
                }
            }
            KeyCode::Char('-') | KeyCode::Char('j') => {
                if self.config.scroll_speed_ms < 2000 {
                    self.config.scroll_speed_ms += 10;
                    events.set_tick_rate(self.config.scroll_speed_ms);
                }
            }
            _ => {}
        }
    }
}