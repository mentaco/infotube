use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Alignment},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::{fs, time::Duration, str::FromStr};
use tokio::{sync::mpsc, time};
use unicode_width::{UnicodeWidthStr, UnicodeWidthChar};

use crate::config::Config;

pub struct App {
    pub running: bool,
    pub config: Config,
    pub text: String,
    pub scroll_offset: usize,
    
    // Source management
    pub source_lines: Vec<String>,
    pub current_line_index: usize,
    pub static_display_ticks: usize,
    pub last_known_width: usize,

    // Interrupts (rx moved to run method)
    pub interrupt_text: Option<String>,
    pub interrupt_remaining_ticks: usize,

    // State controls
    pub paused: bool,
    pub dimmed: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        let mut source_lines = Vec::new();
        for path in &config.source_files {
            if let Ok(content) = fs::read_to_string(path) {
                for line in content.lines() {
                    if !line.trim().is_empty() {
                        source_lines.push(line.to_string());
                    }
                }
            } else {
                eprintln!("Failed to read file: {}", path);
            }
        }
        
        if source_lines.is_empty() {
            source_lines.push("No data found in source files.".to_string());
        }

        let first_text = source_lines[0].clone();

        Self {
            running: true,
            config,
            text: first_text,
            scroll_offset: 0,
            source_lines,
            current_line_index: 0,
            static_display_ticks: 0,
            last_known_width: 0,
            interrupt_text: None,
            interrupt_remaining_ticks: 0,
            paused: false,
            dimmed: false,
        }
    }

    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>, mut rx: mpsc::Receiver<String>) -> Result<()> {
        let sleep = time::sleep(Duration::from_millis(self.config.scroll_speed_ms));
        tokio::pin!(sleep);

        while self.running {
            terminal.draw(|f| self.ui(f))?;

            tokio::select! {
                () = &mut sleep => {
                    if !self.paused {
                        self.on_tick();
                    }
                    // Reset sleep timer based on current speed
                    sleep.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(self.config.scroll_speed_ms));
                }
                Some(msg) = rx.recv() => {
                    self.interrupt_text = Some(msg);
                    self.interrupt_remaining_ticks = 100; // ~10 seconds
                    self.scroll_offset = 0;
                }
                _ = self.handle_events() => {}
            }
        }
        Ok(())
    }

    fn ui(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0)])
            .split(f.area());

        let is_alert = self.interrupt_text.is_some();
        
        // Parse colors from config
        let fg_default = Color::from_str(&self.config.colors.fg_default).unwrap_or(Color::White);
        let bg_default = Color::from_str(&self.config.colors.bg_default).unwrap_or(Color::Black);
        let fg_alert = Color::from_str(&self.config.colors.fg_alert).unwrap_or(Color::Red);
        let bg_alert = Color::from_str(&self.config.colors.bg_alert).unwrap_or(Color::Black);

        let fg_color = if is_alert {
            fg_alert
        } else if self.dimmed {
            Color::DarkGray
        } else {
            fg_default
        };
        let bg_color = if is_alert { bg_alert } else { bg_default };

        let title = if is_alert {
            "Infotube - ALERT"
        } else if self.paused {
            "Infotube (Paused)"
        } else {
            "Infotube"
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(Style::default().fg(fg_color).bg(bg_color));
        
        let area = chunks[0];
        let inner_area = block.inner(area);
        let area_width = inner_area.width as usize;
        
        let display_text = if let Some(ref text) = self.interrupt_text {
            text
        } else {
            &self.text
        };
        
        let text_width = display_text.width();
        let style = Style::default().fg(fg_color).bg(bg_color);

        let paragraph = if text_width <= area_width {
            Paragraph::new(display_text.clone())
                .block(block)
                .alignment(Alignment::Center)
                .style(style)
        } else {
            let spacer = "   ***   ";
            let content = format!("{}{}", display_text, spacer);
            let content_width = content.width();
            let offset = self.scroll_offset % content_width;

            let mut displayed_string = String::new();
            let mut current_width = 0;
            let mut iter = content.chars().cycle();
            
            let mut skipped_width = 0;
            for c in iter.by_ref() {
                let w = c.width().unwrap_or(0);
                if skipped_width + w > offset {
                    displayed_string.push(c);
                    current_width += w;
                    break;
                }
                skipped_width += w;
            }

            for c in iter {
                if current_width >= area_width {
                    break;
                }
                let w = c.width().unwrap_or(0);
                displayed_string.push(c);
                current_width += w;
            }

            Paragraph::new(displayed_string)
                .block(block)
                .alignment(Alignment::Left) 
                .style(style)
        };

        f.render_widget(paragraph, chunks[0]);
    }

    fn on_tick(&mut self) {
        let width = if let Ok((w, _h)) = crossterm::terminal::size() {
             w.saturating_sub(2) as usize
        } else {
             80
        };
        self.last_known_width = width;
        
        if let Some(_) = self.interrupt_text {
            if self.interrupt_remaining_ticks > 0 {
                self.interrupt_remaining_ticks -= 1;
            } else {
                self.interrupt_text = None;
                self.scroll_offset = 0;
            }
            
            let current_text = self.interrupt_text.as_ref().unwrap();
             if current_text.width() > width {
                 self.scroll_offset += 1;
             }
             return;
        }

        let content_width = self.text.width() + "   ***   ".width();

        if self.text.width() > width {
             self.scroll_offset += 1;
             if self.scroll_offset >= content_width {
                 self.next_message();
             }
        } else {
             self.static_display_ticks += 1;
             let ticks_needed = 5000 / self.config.scroll_speed_ms.max(1) as usize;
             if self.static_display_ticks >= ticks_needed {
                 self.next_message();
             }
        }
    }

    fn next_message(&mut self) {
        if self.source_lines.is_empty() { return; }
        self.current_line_index = (self.current_line_index + 1) % self.source_lines.len();
        self.text = self.source_lines[self.current_line_index].clone();
        self.scroll_offset = 0;
        self.static_display_ticks = 0;
    }

    async fn handle_events(&mut self) {
        if event::poll(Duration::from_millis(0)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => self.running = false,
                        KeyCode::Char(' ') => self.paused = !self.paused,
                        KeyCode::Char('b') => self.dimmed = !self.dimmed,
                        KeyCode::Char('+') | KeyCode::Char('k') => {
                            if self.config.scroll_speed_ms > 10 {
                                self.config.scroll_speed_ms -= 10;
                            }
                        }
                        KeyCode::Char('-') | KeyCode::Char('j') => {
                            if self.config.scroll_speed_ms < 2000 {
                                self.config.scroll_speed_ms += 10;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}