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
    /// 前回の描画時に判明した表示領域の幅（ bordersを除いた内側）
    pub last_known_width: usize,

    // --- 割り込み通知管理 ---
    /// TCP経由で受信した緊急割り込みメッセージ（存在する場合）
    pub interrupt_text: Option<String>,
    /// 割り込みメッセージを表示し続ける残り時間（ミリ秒）
    pub interrupt_remaining_ms: usize,
    /// 割り込み発生前の一時停止状態を保持
    pub paused_before_interrupt: bool,
    /// 割り込み発生前のスクロール位置を保持
    pub saved_scroll_offset: usize,

    // --- ユーザー操作状態 ---
    /// 一時停止中かどうかのフラグ
    pub paused: bool,
    /// 輝度を下げているか（Dimmedモード）どうかのフラグ
    pub dimmed: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        let mut all_files_content = Vec::new();

        for path in &config.source_files {
            if let Ok(content) = fs::read_to_string(path) {
                // ファイル内の全行を読み込み、トリムして空行を除外後、スペース4つで結合
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
                eprintln!("Failed to read file: {}", path);
            }
        }
        
        let mut text = all_files_content.join("    ***    ");

        // コンテンツが空だった場合のフォールバック
        if text.is_empty() {
            text = "No data found in source files.".to_string();
        }

        Self {
            running: true,
            config,
            text,
            scroll_offset: 0,
            last_known_width: 0,
            interrupt_text: None,
            interrupt_remaining_ms: 0,
            paused_before_interrupt: false,
            saved_scroll_offset: 0,
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
                    self.paused_before_interrupt = self.paused;
                    self.saved_scroll_offset = self.scroll_offset;
                    self.paused = false; // 強制的に再生
                    self.interrupt_text = Some(msg);
                    self.interrupt_remaining_ms = 9000; // 9秒間表示
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
        let area = f.area();
        
        // 枠線がない場合は1行目のみを使用するようにレイアウトを分割
        let target_area = if !self.config.show_frame {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(area)[0]
        } else {
            area
        };

        let is_alert = self.interrupt_text.is_some();
        
        // --- 色の設定 ---
        // 文字列からColorへの変換ヘルパー
        let parse_color = |s: &str, default: Color| -> Color {
            if s.eq_ignore_ascii_case("None") {
                Color::Reset
            } else {
                Color::from_str(s).unwrap_or(default)
            }
        };

        // 設定ファイルから色をパース
        let fg_default = parse_color(&self.config.colors.fg_default, Color::White);
        let bg_default = parse_color(&self.config.colors.bg_default, Color::Reset); // デフォルトはReset(なし)
        let fg_alert = parse_color(&self.config.colors.fg_alert, Color::Red);
        let bg_alert = parse_color(&self.config.colors.bg_alert, Color::Reset);

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
        
        let style = Style::default().fg(fg_color).bg(bg_color);

        // 枠線の有無に応じてブロックと内部領域を決定
        let (block, inner_area) = if self.config.show_frame {
            let title = if is_alert {
                if self.paused {
                    "Infotube - ALERT (Paused)"
                } else {
                    "Infotube - ALERT"
                }
            } else if self.paused {
                "Infotube (Paused)"
            } else {
                "Infotube"
            };
            
            let b = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(style);
            let inner = b.inner(target_area);
            (Some(b), inner)
        } else {
            (None, target_area)
        };
        
        let area_width = inner_area.width as usize;
        
        // 表示するテキストとプレフィックスの決定
        let (prefix, content_text) = if let Some(ref text) = self.interrupt_text {
            let seconds = (self.interrupt_remaining_ms as f64 / 1000.0).ceil() as usize;
            (format!("({}s)  ", seconds), text.as_str())
        } else {
            (String::new(), self.text.as_str())
        };
        
        let prefix_width = prefix.width();
        // 本文が利用できる幅（プレフィックス分を引く）
        let content_available_width = area_width.saturating_sub(prefix_width);
        let content_text_width = content_text.width();

        let mut displayed_string = String::from(&prefix);

        // Alignmentの決定: 割り込み時は左詰め（時間を固定するため）、それ以外は設定依存
        let alignment = if self.interrupt_text.is_some() {
            Alignment::Left
        } else if content_text_width <= area_width && self.config.show_frame {
            Alignment::Center
        } else {
            Alignment::Left
        };

        if content_text_width <= content_available_width {
            // 1. テキストが領域内に収まる場合
            displayed_string.push_str(content_text);
        } else {
            // 2. テキストが領域を超える場合：マーキー（スクロール）表示
            // ここでのスクロールは「本文部分のみ」に行う
            let spacer = "   ***   "; // 行の継ぎ目を示すスペーサー
            let content = format!("{}{}", content_text, spacer);
            let content_width = content.width();
            
            // 現在のオフセットに基づいて表示する文字列を循環生成
            let offset = self.scroll_offset % content_width;
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

            // 表示領域が埋まるまで文字を追加（プレフィックス分を引いた幅まで）
            for c in iter {
                if current_width >= content_available_width {
                    break;
                }
                let w = c.width().unwrap_or(0);
                displayed_string.push(c);
                current_width += w;
            }
        };

        let mut paragraph = Paragraph::new(displayed_string)
            .alignment(alignment)
            .style(style);
        
        if let Some(b) = block {
            paragraph = paragraph.block(b);
        }

        // 描画
        f.render_widget(paragraph, target_area);
    }

    /// 時間経過による状態更新ロジック
    fn on_tick(&mut self) {
        // ターミナルの現在の幅を取得（スクロール判定に使用）
        let width = if let Ok((w, _h)) = crossterm::terminal::size() {
             if self.config.show_frame {
                 w.saturating_sub(2) as usize // 枠線分を引く
             } else {
                 w as usize
             }
        } else {
             80
        };
        self.last_known_width = width;
        
        // 割り込みメッセージ表示中の処理
        if let Some(ref text) = self.interrupt_text {
            let elapsed = self.config.scroll_speed_ms as usize;
            if self.interrupt_remaining_ms > elapsed {
                self.interrupt_remaining_ms -= elapsed;
            } else {
                // 表示期限切れ
                self.interrupt_text = None;
                self.paused = self.paused_before_interrupt;
                self.scroll_offset = self.saved_scroll_offset;
                return;
            }
            
            // 割り込みメッセージ自体のスクロール（プレフィックス込みの長さを判定）
            let seconds = (self.interrupt_remaining_ms as f64 / 1000.0).ceil() as usize;
            let display_text = format!("({}s)  {}", seconds, text);

             if display_text.width() > width {
                 self.scroll_offset += 1;
             }
             return;
        }

        // 通常メッセージのスクロール
        if self.text.width() > width {
             // スクロールが必要な場合
             self.scroll_offset += 1;
             // next_message()による切り替えは不要なので、offsetを増加させ続ける
             // ui側で剰余計算を行っているため、offset自体は無限に増えても(usize上限まで)問題ない
        } else {
             // スクロール不要な場合：常にオフセット0で静止
             self.scroll_offset = 0;
        }
    }

    /// キーイベント処理
    async fn handle_events(&mut self) {
        if event::poll(Duration::from_millis(0)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => self.running = false, // 終了
                        KeyCode::Enter => {
                            // Enterキーで割り込みを即時終了し、元の状態に復帰
                            if self.interrupt_text.is_some() {
                                self.interrupt_text = None;
                                self.paused = self.paused_before_interrupt;
                                self.scroll_offset = self.saved_scroll_offset;
                            }
                        }
                        KeyCode::Char(' ') => self.paused = !self.paused,        // 一時停止
                        KeyCode::Char('f') => self.config.show_frame = !self.config.show_frame, // 枠線表示切替
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