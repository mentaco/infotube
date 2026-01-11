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

/// アプリケーションの状態を管理する構造体
pub struct App {
    /// ループを実行し続けるかどうかのフラグ
    pub running: bool,
    /// 読み込まれた設定
    pub config: Config,
    /// 現在表示中のメインテキスト
    pub text: String,
    /// マーキー表示（スクロール）の開始位置オフセット
    pub scroll_offset: usize,
    
    // --- 情報ソース管理 ---
    /// 設定ファイルから読み込まれた静的なテキスト行のリスト
    pub source_lines: Vec<String>,
    /// 現在表示しているテキストのインデックス
    pub current_line_index: usize,
    /// 静止テキストを表示し続けている時間（ティック数）のカウント
    pub static_display_ticks: usize,
    /// 前回の描画時に判明した表示領域の幅（ bordersを除いた内側）
    pub last_known_width: usize,

    // --- 割り込み通知管理 ---
    /// TCP経由で受信した緊急割り込みメッセージ（存在する場合）
    pub interrupt_text: Option<String>,
    /// 割り込みメッセージを表示し続ける残り時間（ティック数）
    pub interrupt_remaining_ticks: usize,

    // --- ユーザー操作状態 ---
    /// 一時停止中かどうかのフラグ
    pub paused: bool,
    /// 輝度を下げているか（Dimmedモード）どうかのフラグ
    pub dimmed: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        // 設定されたソースファイルから行を読み込む
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
        
        // ファイルが空だった場合のフォールバック
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

    /// メインの実行ループ
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>, mut rx: mpsc::Receiver<String>) -> Result<()> {
        // 設定された速度（ms）ごとに発火するタイマー
        let sleep = time::sleep(Duration::from_millis(self.config.scroll_speed_ms));
        tokio::pin!(sleep);

        while self.running {
            // 1. 描画
            terminal.draw(|f| self.ui(f))?;

            // 2. 非同期イベント待機
            tokio::select! {
                // タイマー発火（スクロールやテキスト切り替え）
                () = &mut sleep => {
                    if !self.paused {
                        self.on_tick();
                    }
                    // 次のタイマーを再セット
                    sleep.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(self.config.scroll_speed_ms));
                }
                // TCP割り込みメッセージの受信
                Some(msg) = rx.recv() => {
                    self.interrupt_text = Some(msg);
                    self.interrupt_remaining_ticks = 100; // 約10秒間表示（100ms * 100）
                    self.scroll_offset = 0; // スクロール位置をリセット
                }
                // キーボードイベントの処理
                _ = self.handle_events() => {}
            }
        }
        Ok(())
    }

    /// ユーザーインターフェースの描画ロジック
    fn ui(&self, f: &mut Frame) {
        // 全画面を1つのチャンクとして使用
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0)])
            .split(f.area());

        let is_alert = self.interrupt_text.is_some();
        
        // --- 色の設定 ---
        // 設定ファイルから色をパース（失敗時はデフォルトを使用）
        let fg_default = Color::from_str(&self.config.colors.fg_default).unwrap_or(Color::White);
        let bg_default = Color::from_str(&self.config.colors.bg_default).unwrap_or(Color::Black);
        let fg_alert = Color::from_str(&self.config.colors.fg_alert).unwrap_or(Color::Red);
        let bg_alert = Color::from_str(&self.config.colors.bg_alert).unwrap_or(Color::Black);

        // 現在の状態（アラート中か、輝度調整中か）に応じて前景色を選択
        let fg_color = if is_alert {
            fg_alert
        } else if self.dimmed {
            // ここが「輝度調整モード」の色設定
            Color::DarkGray
        } else {
            fg_default
        };
        // 背景色の選択
        let bg_color = if is_alert { bg_alert } else { bg_default };

        // --- ウィジェットの作成 ---
        let title = if is_alert {
            "Infotube - ALERT"
        } else if self.paused {
            "Infotube (Paused)"
        } else {
            "Infotube"
        };

        // 枠線の設定
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            // Block全体にスタイルを適用（ここが枠線の色にも影響する）
            .style(Style::default().fg(fg_color).bg(bg_color));
        
        let area = chunks[0];
        let inner_area = block.inner(area); // 枠線の内側の領域
        let area_width = inner_area.width as usize;
        
        // 表示するテキストの決定（割り込みがあればそれを優先）
        let display_text = if let Some(ref text) = self.interrupt_text {
            text
        } else {
            &self.text
        };
        
        let text_width = display_text.width();
        let style = Style::default().fg(fg_color).bg(bg_color);

        let paragraph = if text_width <= area_width {
            // 1. テキストが領域内に収まる場合：中央表示
            Paragraph::new(display_text.clone())
                .block(block)
                .alignment(Alignment::Center)
                .style(style)
        } else {
            // 2. テキストが領域を超える場合：マーキー（スクロール）表示
            let spacer = "   ***   "; // 行の継ぎ目を示すスペーサー
            let content = format!("{}{}", display_text, spacer);
            let content_width = content.width();
            
            // 現在のオフセットに基づいて表示する文字列を循環生成
            let offset = self.scroll_offset % content_width;
            let mut displayed_string = String::new();
            let mut current_width = 0;
            let mut iter = content.chars().cycle();
            
            // 開始位置（オフセット）まで文字を飛ばす
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

            // 表示領域が埋まるまで文字を追加
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

        // 描画
        f.render_widget(paragraph, chunks[0]);
    }

    /// 時間経過による状態更新ロジック
    fn on_tick(&mut self) {
        // ターミナルの現在の幅を取得（スクロール判定に使用）
        let width = if let Ok((w, _h)) = crossterm::terminal::size() {
             w.saturating_sub(2) as usize // 枠線分を引く
        } else {
             80
        };
        self.last_known_width = width;
        
        // 割り込みメッセージ表示中の処理
        if let Some(_) = self.interrupt_text {
            if self.interrupt_remaining_ticks > 0 {
                self.interrupt_remaining_ticks -= 1;
            } else {
                // 表示期限切れ
                self.interrupt_text = None;
                self.scroll_offset = 0;
            }
            
            // 割り込みメッセージ自体のスクロール
            let current_text = self.interrupt_text.as_ref().unwrap();
             if current_text.width() > width {
                 self.scroll_offset += 1;
             }
             return;
        }

        // 通常メッセージのスクロールと切り替え処理
        let content_width = self.text.width() + "   ***   ".width();

        if self.text.width() > width {
             // スクロールが必要な場合
             self.scroll_offset += 1;
             if self.scroll_offset >= content_width {
                 // 一周したら次のメッセージへ
                 self.next_message();
             }
        } else {
             // スクロール不要な場合：一定時間静止
             self.static_display_ticks += 1;
             let ticks_needed = 5000 / self.config.scroll_speed_ms.max(1) as usize;
             if self.static_display_ticks >= ticks_needed {
                 self.next_message();
             }
        }
    }

    /// 次の表示メッセージに切り替える
    fn next_message(&mut self) {
        if self.source_lines.is_empty() { return; }
        self.current_line_index = (self.current_line_index + 1) % self.source_lines.len();
        self.text = self.source_lines[self.current_line_index].clone();
        self.scroll_offset = 0;
        self.static_display_ticks = 0;
    }

    /// キーイベント処理
    async fn handle_events(&mut self) {
        if event::poll(Duration::from_millis(0)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => self.running = false, // 終了
                        KeyCode::Char(' ') => self.paused = !self.paused,        // 一時停止
                        KeyCode::Char('b') => self.dimmed = !self.dimmed,        // 輝度調整
                        KeyCode::Char('+') | KeyCode::Char('k') => {             // 加速
                            if self.config.scroll_speed_ms > 10 {
                                self.config.scroll_speed_ms -= 10;
                            }
                        }
                        KeyCode::Char('-') | KeyCode::Char('j') => {             // 減速
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