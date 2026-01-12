use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::str::FromStr;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::config::Config;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Determine target area based on frame config
    let target_area = if !app.config.show_frame {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(area)[0]
    } else {
        area
    };

    let is_alert = app.interrupt_text.is_some();
    let style = get_style(&app.config, is_alert, app.dimmed);

    let (block, inner_area) = if app.config.show_frame {
        let title = get_title(is_alert, app.paused);
        let b = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(style);
        let inner = b.inner(target_area);
        (Some(b), inner)
    } else {
        (None, target_area)
    };

    let paragraph = render_ticker(app, inner_area.width as usize, style, is_alert);
    
    // Apply block if exists
    let widget = if let Some(b) = block {
        paragraph.block(b)
    } else {
        paragraph
    };

    f.render_widget(widget, target_area);
}

fn get_style(config: &Config, is_alert: bool, dimmed: bool) -> Style {
    let parse_color = |s: &str, default: Color| -> Color {
        if s.eq_ignore_ascii_case("None") {
            Color::Reset
        } else {
            Color::from_str(s).unwrap_or(default)
        }
    };

    let fg_default = parse_color(&config.colors.fg_default, Color::White);
    let bg_default = parse_color(&config.colors.bg_default, Color::Reset);
    let fg_alert = parse_color(&config.colors.fg_alert, Color::Red);
    let bg_alert = parse_color(&config.colors.bg_alert, Color::Reset);

    let fg = if is_alert {
        fg_alert
    } else if dimmed {
        Color::DarkGray
    } else {
        fg_default
    };

    let bg = if is_alert { bg_alert } else { bg_default };

    Style::default().fg(fg).bg(bg)
}

fn get_title(is_alert: bool, paused: bool) -> &'static str {
    if is_alert {
        if paused {
            "Infotube - ALERT (Paused)"
        } else {
            "Infotube - ALERT"
        }
    } else if paused {
        "Infotube (Paused)"
    } else {
        "Infotube"
    }
}

fn render_ticker(app: &App, width: usize, style: Style, is_alert: bool) -> Paragraph<'static> {
    let (prefix, content_text) = if let Some(ref text) = app.interrupt_text {
        let seconds = (app.interrupt_remaining_ms as f64 / 1000.0).ceil() as usize;
        (format!("({}s)  ", seconds), text.as_str())
    } else {
        (String::new(), app.text.as_str())
    };

    let prefix_width = prefix.width();
    let content_available_width = width.saturating_sub(prefix_width);
    let content_text_width = content_text.width();

    let mut displayed_string = String::from(&prefix);

    let alignment = if is_alert {
        Alignment::Left
    } else if content_text_width <= width && app.config.show_frame {
        Alignment::Center
    } else {
        Alignment::Left
    };

    if content_text_width <= content_available_width {
        displayed_string.push_str(content_text);
    } else {
        let spacer = "   ***   ";
        let content = format!("{}{}", content_text, spacer);
        let content_width = content.width();
        
        let offset = app.scroll_offset % content_width;
        let mut current_width = 0;
        let mut iter = content.chars().cycle();
        
        // Skip chars
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

        // Fill remaining
        for c in iter {
            if current_width >= content_available_width {
                break;
            }
            let w = c.width().unwrap_or(0);
            displayed_string.push(c);
            current_width += w;
        }
    }

    Paragraph::new(displayed_string)
        .alignment(alignment)
        .style(style)
}
