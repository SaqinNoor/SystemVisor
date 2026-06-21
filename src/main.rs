mod telemetry;
mod audio_visualizer;
mod media_controls;
mod art_processor;
mod gpu_telemetry;

use std::io::stdout;
use std::time::Duration;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    cursor::{Hide, Show},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Row, Table, TableState, Sparkline, Cell},
    Terminal,
};
use telemetry::{SystemSnapshot, ProcessInfo};
use media_controls::{MediaMetadata, MediaEvent};

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        );
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum SortColumn {
    Pid,
    Name,
    Cpu,
    Memory,
}

impl SortColumn {
    fn cycle(self) -> Self {
        match self {
            Self::Cpu => Self::Memory,
            Self::Memory => Self::Pid,
            Self::Pid => Self::Name,
            Self::Name => Self::Cpu,
        }
    }
}

struct ColorTheme {
    name: &'static str,
    border: Color,
    highlight: Color,
    neutral: Color,
    accent1: Color,
    accent2: Color,
}

const THEMES: &[ColorTheme] = &[
    ColorTheme { name: "tokyonight",        border: clr(0x56,0x5f,0x89), highlight: clr(0x7a,0xa2,0xf7), neutral: clr(0xc0,0xca,0xf5), accent1: clr(0xbb,0x9a,0xf7), accent2: clr(0x7a,0xa2,0xf7) },
    ColorTheme { name: "everforest",        border: clr(0x7a,0x84,0x78), highlight: clr(0xa7,0xc0,0x80), neutral: clr(0xd3,0xc6,0xaa), accent1: clr(0x7f,0xbb,0xb3), accent2: clr(0x7f,0xbb,0xb3) },
    ColorTheme { name: "ayu",               border: clr(0x62,0x6a,0x73), highlight: clr(0xff,0xb4,0x54), neutral: clr(0xb3,0xb1,0xad), accent1: clr(0x59,0xc2,0xff), accent2: clr(0x39,0xba,0xe6) },
    ColorTheme { name: "catppuccin",        border: clr(0x6c,0x70,0x86), highlight: clr(0xcb,0xa6,0xf7), neutral: clr(0xcd,0xd6,0xf4), accent1: clr(0x89,0xb4,0xfa), accent2: clr(0x89,0xb4,0xfa) },
    ColorTheme { name: "gruvbox",           border: clr(0x92,0x83,0x74), highlight: clr(0xfa,0xbd,0x2f), neutral: clr(0xeb,0xdb,0xb2), accent1: clr(0x83,0xa5,0x98), accent2: clr(0x83,0xa5,0x98) },
    ColorTheme { name: "kanagawa",          border: clr(0x72,0x71,0x69), highlight: clr(0x95,0x7f,0xb8), neutral: clr(0xdc,0xd7,0xba), accent1: clr(0x7e,0x9c,0xd8), accent2: clr(0x7e,0x9c,0xd8) },
    ColorTheme { name: "nord",              border: clr(0x4c,0x56,0x6a), highlight: clr(0x88,0xc0,0xd0), neutral: clr(0xd8,0xde,0xe9), accent1: clr(0x81,0xa1,0xc1), accent2: clr(0x88,0xc0,0xd0) },
    ColorTheme { name: "one-dark",          border: clr(0x5c,0x63,0x70), highlight: clr(0x61,0xaf,0xef), neutral: clr(0xab,0xb2,0xbf), accent1: clr(0xc6,0x78,0xdd), accent2: clr(0x61,0xaf,0xef) },
    ColorTheme { name: "matrix",            border: clr(0x00,0x5f,0x1a), highlight: clr(0x00,0xff,0x41), neutral: clr(0x00,0xff,0x41), accent1: clr(0x00,0x8f,0x11), accent2: clr(0x00,0xff,0x41) },
];

const fn clr(r: u8, g: u8, b: u8) -> Color { Color::Rgb(r, g, b) }

struct ScrollingText {
    content: String,
    tick_offset: usize,
    last_tick: std::time::Instant,
}

impl ScrollingText {
    fn new(content: String) -> Self {
        Self { content, tick_offset: 0, last_tick: std::time::Instant::now() }
    }

    fn get_visible_slice(&mut self, max_width: usize) -> String {
        let chars: Vec<char> = self.content.chars().collect();
        if chars.len() <= max_width {
            return self.content.clone();
        }
        let mut pool: Vec<char> = chars.clone();
        pool.extend(vec![' ', ' ', '─', ' ', ' ']);
        pool.extend(chars);
        let start = self.tick_offset % (self.content.chars().count() + 5);
        let end = (start + max_width).min(pool.len());
        pool[start..end].iter().collect()
    }

    fn tick(&mut self) {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_tick) >= std::time::Duration::from_millis(280) {
            self.last_tick = now;
            self.tick_offset = self.tick_offset.wrapping_add(1);
        }
    }
}

enum RightView {
    Media,
    Processes,
}

struct App {
    snapshot: Option<Box<SystemSnapshot>>,
    cpu_history: Vec<u64>,
    sort_column: SortColumn,
    sort_ascending: bool,
    process_table_state: TableState,
    sorted_processes: Vec<ProcessInfo>,

    visualizer_bars: Vec<f32>,
    max_visualizer_bars: usize,

    media_metadata: Option<MediaMetadata>,
    album_art_matrix: Option<Vec<Vec<(u8, u8, u8)>>>,
    marquee_title: ScrollingText,
    marquee_artist: ScrollingText,
    marquee_album: ScrollingText,
    header_scroll: ScrollingText,
    footer_scroll: ScrollingText,

    gpu_data: Option<gpu_telemetry::GpuSnapshot>,

    media_position_secs: f64,
    media_total_secs: f64,
    media_position_captured_at: Option<std::time::Instant>,

    art_target_w: u32,
    art_target_h: u32,

    right_view: RightView,

    theme_index: usize,
    show_theme_picker: bool,
    theme_picker_index: usize,
}

impl App {
    fn new() -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            snapshot: None,
            cpu_history: Vec::new(),
            sort_column: SortColumn::Cpu,
            sort_ascending: false,
            process_table_state: table_state,
            sorted_processes: Vec::new(),
            visualizer_bars: vec![0.0f32; 16],
            max_visualizer_bars: 16,
            media_metadata: None,
            album_art_matrix: None,
            marquee_title: ScrollingText::new(String::new()),
            marquee_artist: ScrollingText::new(String::new()),
            marquee_album: ScrollingText::new(String::new()),
            header_scroll: ScrollingText::new(String::new()),
            footer_scroll: ScrollingText::new(String::new()),
            gpu_data: None,
            media_position_secs: 0.0,
            media_total_secs: 0.0,
            media_position_captured_at: None,
            art_target_w: 30,
            art_target_h: 14,
            right_view: RightView::Media,
            theme_index: 0,
            show_theme_picker: false,
            theme_picker_index: 0,
        }
    }

    fn update_snapshot(&mut self, snap: Box<SystemSnapshot>) {
        self.cpu_history.push(snap.global_cpu_usage as u64);
        if self.cpu_history.len() > 120 {
            self.cpu_history.remove(0);
        }

        let mut procs = snap.processes.clone();
        
        match self.sort_column {
            SortColumn::Pid => procs.sort_by(|a, b| {
                let cmp = a.pid.cmp(&b.pid);
                if self.sort_ascending { cmp } else { cmp.reverse() }
            }),
            SortColumn::Name => procs.sort_by(|a, b| {
                let cmp = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                if self.sort_ascending { cmp } else { cmp.reverse() }
            }),
            SortColumn::Cpu => procs.sort_by(|a, b| {
                let cmp = a.cpu_usage.partial_cmp(&b.cpu_usage).unwrap_or(std::cmp::Ordering::Equal);
                if self.sort_ascending { cmp } else { cmp.reverse() }
            }),
            SortColumn::Memory => procs.sort_by(|a, b| {
                let cmp = a.memory.cmp(&b.memory);
                if self.sort_ascending { cmp } else { cmp.reverse() }
            }),
        }

        self.sorted_processes = procs;
        self.snapshot = Some(snap);

        let len = self.sorted_processes.len();
        if len > 0 {
            if let Some(selected) = self.process_table_state.selected() {
                if selected >= len {
                    self.process_table_state.select(Some(len - 1));
                }
            } else {
                self.process_table_state.select(Some(0));
            }
        } else {
            self.process_table_state.select(None);
        }
    }

    fn scroll_next(&mut self) {
        let len = self.sorted_processes.len();
        if len == 0 { return; }
        let current = self.process_table_state.selected().unwrap_or(0);
        let next = if current >= len - 1 { 0 } else { current + 1 };
        self.process_table_state.select(Some(next));
    }

    fn scroll_prev(&mut self) {
        let len = self.sorted_processes.len();
        if len == 0 { return; }
        let current = self.process_table_state.selected().unwrap_or(0);
        let prev = if current == 0 { len - 1 } else { current - 1 };
        self.process_table_state.select(Some(prev));
    }

    fn scroll_page_down(&mut self) {
        let len = self.sorted_processes.len();
        if len == 0 { return; }
        let current = self.process_table_state.selected().unwrap_or(0);
        let next = (current + 10).min(len - 1);
        self.process_table_state.select(Some(next));
    }

    fn scroll_page_up(&mut self) {
        let len = self.sorted_processes.len();
        if len == 0 { return; }
        let current = self.process_table_state.selected().unwrap_or(0);
        let prev = current.saturating_sub(10);
        self.process_table_state.select(Some(prev));
    }

    fn scroll_to_top(&mut self) {
        if !self.sorted_processes.is_empty() {
            self.process_table_state.select(Some(0));
        }
    }

    fn scroll_to_bottom(&mut self) {
        let len = self.sorted_processes.len();
        if len > 0 {
            self.process_table_state.select(Some(len - 1));
        }
    }
}

enum AppEvent {
    Key(KeyEvent),
    Resize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout_stream = stdout();
    execute!(
        stdout_stream,
        EnterAlternateScreen,
        EnableMouseCapture,
        Hide
    )?;
    
    let _guard = TerminalGuard;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        );
        original_hook(panic_info);
    }));

    let (telemetry_tx, telemetry_rx) = crossbeam_channel::unbounded();
    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    let (audio_tx, audio_rx) = crossbeam_channel::unbounded();
    let (media_tx, media_rx) = crossbeam_channel::unbounded();
    let (art_tx, art_rx) = crossbeam_channel::unbounded();
    let (gpu_tx, gpu_rx) = crossbeam_channel::unbounded();

    std::thread::spawn(move || {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                match event::read() {
                    Ok(event::Event::Key(key)) => {
                        if key.kind == event::KeyEventKind::Press {
                            if input_tx.send(AppEvent::Key(key)).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(event::Event::Resize(_, _)) => {
                        if input_tx.send(AppEvent::Resize).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    telemetry::spawn_telemetry_thread(telemetry_tx, Duration::from_millis(500));
    audio_visualizer::spawn_audio_visualizer_thread(audio_tx);
    media_controls::spawn_media_controls_thread(media_tx);
    gpu_telemetry::spawn_gpu_telemetry_thread(gpu_tx);

    let backend = CrosstermBackend::new(stdout_stream);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new();

    terminal.clear()?;

    loop {
        crossbeam_channel::select! {
            recv(telemetry_rx) -> telemetry_res => {
                match telemetry_res {
                    Ok(snapshot) => {
                        app.update_snapshot(snapshot);
                    }
                    Err(_) => break,
                }
            }
            recv(input_rx) -> input_res => {
                match input_res {
                    Ok(AppEvent::Key(key)) => {
                        if key.code == KeyCode::Char('q') || 
                           (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
                            break;
                        }

                        if app.show_theme_picker {
                            match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    app.theme_picker_index = app.theme_picker_index.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let max = THEMES.len().saturating_sub(1);
                                    if app.theme_picker_index < max {
                                        app.theme_picker_index += 1;
                                    }
                                }
                                KeyCode::Enter => {
                                    app.theme_index = app.theme_picker_index;
                                    app.show_theme_picker = false;
                                }
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    app.show_theme_picker = false;
                                }
                                _ => {}
                            }
                        } else {
                            match key.code {
                            KeyCode::Tab | KeyCode::Char('s') => {
                                app.sort_column = app.sort_column.cycle();
                                if let Some(snap) = app.snapshot.clone() {
                                    app.update_snapshot(snap);
                                }
                            }
                            KeyCode::Char('r') => {
                                app.sort_ascending = !app.sort_ascending;
                                if let Some(snap) = app.snapshot.clone() {
                                    app.update_snapshot(snap);
                                }
                            }
                            KeyCode::Char('1') => {
                                app.sort_column = SortColumn::Pid;
                                if let Some(snap) = app.snapshot.clone() {
                                    app.update_snapshot(snap);
                                }
                            }
                            KeyCode::Char('2') => {
                                app.sort_column = SortColumn::Name;
                                if let Some(snap) = app.snapshot.clone() {
                                    app.update_snapshot(snap);
                                }
                            }
                            KeyCode::Char('3') => {
                                app.sort_column = SortColumn::Cpu;
                                if let Some(snap) = app.snapshot.clone() {
                                    app.update_snapshot(snap);
                                }
                            }
                            KeyCode::Char('4') => {
                                app.sort_column = SortColumn::Memory;
                                if let Some(snap) = app.snapshot.clone() {
                                    app.update_snapshot(snap);
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.scroll_next();
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.scroll_prev();
                            }
                            KeyCode::PageDown | KeyCode::Char('d') => {
                                app.scroll_page_down();
                            }
                            KeyCode::PageUp | KeyCode::Char('u') => {
                                app.scroll_page_up();
                            }
                            KeyCode::Home => {
                                app.scroll_to_top();
                            }
                            KeyCode::End => {
                                app.scroll_to_bottom();
                            }
                            KeyCode::Char('v') => {
                                app.right_view = match app.right_view {
                                    RightView::Media => RightView::Processes,
                                    RightView::Processes => RightView::Media,
                                };
                            }
                            KeyCode::Char('t') => {
                                app.show_theme_picker = true;
                                app.theme_picker_index = app.theme_index;
                            }
                            _ => {}
                        }
                        }
                    }
                    Ok(AppEvent::Resize) => {}
                    Err(_) => break,
                }
            }
            recv(audio_rx) -> audio_res => {
                match audio_res {
                    Ok(spectrum) => {
                        let target = app.max_visualizer_bars;
                        if app.visualizer_bars.len() != target {
                            app.visualizer_bars.resize(target, 0.0);
                        }
                        let band_count = spectrum.len();
                        for j in 0..target {
                            let pos = j as f32 * (band_count - 1) as f32 / target as f32;
                            let lo = pos.floor() as usize;
                            let hi = (lo + 1).min(band_count - 1);
                            let frac = pos - lo as f32;
                            let interpolated =
                                spectrum[lo] * (1.0 - frac) + spectrum[hi] * frac;
                            app.visualizer_bars[j] =
                                f32::max(interpolated, app.visualizer_bars[j] * 0.92);
                        }
                    }
                    Err(_) => {}
                }
            }
            recv(media_rx) -> media_res => {
                match media_res {
                    Ok(MediaEvent::Metadata(meta)) => {
                        let title_changed = app.media_metadata.as_ref().map(|m| &m.title) != Some(&meta.title);
                        let artist_changed = app.media_metadata.as_ref().map(|m| &m.artist) != Some(&meta.artist);
                        let album_changed = app.media_metadata.as_ref().map(|m| &m.album) != Some(&meta.album);
                        app.media_metadata = Some(*meta);
                        if title_changed {
                            if let Some(m) = &app.media_metadata {
                                app.marquee_title = ScrollingText::new(m.title.clone());
                            }
                        }
                        if artist_changed {
                            if let Some(m) = &app.media_metadata {
                                app.marquee_artist = ScrollingText::new(m.artist.clone());
                            }
                        }
                        if album_changed {
                            if let Some(m) = &app.media_metadata {
                                app.marquee_album = ScrollingText::new(m.album.clone());
                            }
                        }
                    }
                    Ok(MediaEvent::Thumbnail(bytes)) => {
                        let art_tx_clone = art_tx.clone();
                        let art_w = app.art_target_w.max(2);
                        let art_h = app.art_target_h.max(2);
                        std::thread::spawn(move || {
                            if let Some(matrix) = art_processor::process_album_art(&bytes, art_w, art_h) {
                                let _ = art_tx_clone.send(matrix);
                            }
                        });
                    }
                    Ok(MediaEvent::Timeline(pos, total, captured_at)) => {
                        let now = std::time::Instant::now();
                        let new_effective = pos + now.duration_since(captured_at).as_secs_f64();
                        let old_effective = app.media_position_secs
                            + app.media_position_captured_at
                                .map(|t| now.duration_since(t).as_secs_f64())
                                .unwrap_or(0.0);
                        if new_effective > old_effective {
                            app.media_position_secs = new_effective;
                            app.media_position_captured_at = Some(now);
                        } else {
                            app.media_position_captured_at = Some(now);
                        }
                        app.media_total_secs = total;
                    }
                    Ok(MediaEvent::NoMedia) => {
                        app.media_metadata = None;
                        app.album_art_matrix = None;
                    }
                    Err(_) => {}
                }
            }
            recv(art_rx) -> art_res => {
                match art_res {
                    Ok(matrix) => {
                        app.album_art_matrix = Some(matrix);
                    }
                    Err(_) => {}
                }
            }
            recv(gpu_rx) -> gpu_res => {
                match gpu_res {
                    Ok(snap) => {
                        app.gpu_data = Some(*snap);
                    }
                    Err(_) => {}
                }
            }
        }

        app.marquee_title.tick();
        app.marquee_artist.tick();
        app.marquee_album.tick();
        app.header_scroll.tick();
        app.footer_scroll.tick();

        terminal.draw(|f| draw_ui(f, &mut app))?;
    }

    Ok(())
}

fn draw_ui(frame: &mut ratatui::Frame, app: &mut App) {
    let size = frame.size();

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .margin(1)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(size);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(11),
            Constraint::Length(6),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(main_chunks[0]);

    let theme = &THEMES[app.theme_index];
    let border_color = theme.border;
    let text_highlight = theme.highlight;
    let text_neutral = theme.neutral;
    let theme_violet = theme.accent1;
    let theme_indigo = theme.accent2;

    let snapshot_ref = match &app.snapshot {
        Some(snap) => snap,
        None => {
            let loading = Paragraph::new("GATHERING WINDOWS TELEMETRY SYSTEM STATUS...")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)));
            frame.render_widget(loading, size);
            return;
        }
    };

    let header_content = format!(
        " SYSTEMVISOR  Host: {}  | OS: {} {} ({})  | Uptime: {} ",
        snapshot_ref.host_name, snapshot_ref.os_name, snapshot_ref.os_version,
        snapshot_ref.cpu_arch, format_uptime(snapshot_ref.uptime)
    );
    if app.header_scroll.content != header_content {
        app.header_scroll.content = header_content;
    }
    let header_max = (left_chunks[0].width as usize).saturating_sub(2);
    let header_visible = app.header_scroll.get_visible_slice(header_max);
    let header_widget = Paragraph::new(Line::from(Span::styled(
        header_visible,
        Style::default().fg(text_highlight).add_modifier(Modifier::BOLD),
    )))
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color)));
    frame.render_widget(header_widget, left_chunks[0]);

    let cpu_sub_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(65),
        ])
        .split(left_chunks[1]);

    let cpu_global_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
        ])
        .split(cpu_sub_layout[0]);

    let cpu_pct = snapshot_ref.global_cpu_usage;
    let cpu_color = if cpu_pct < 50.0 {
        Color::Rgb(52, 211, 153)
    } else if cpu_pct < 85.0 {
        Color::Rgb(251, 191, 36)
    } else {
        Color::Rgb(251, 113, 133)
    };

    let cpu_gauge = Gauge::default()
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" SYSTEM CPU ")
            .title_style(Style::default().fg(theme_indigo)))
        .gauge_style(Style::default().fg(cpu_color).bg(Color::Rgb(30, 41, 59)))
        .ratio(cpu_pct as f64 / 100.0)
        .label(format!("{:.1}%", cpu_pct));
    frame.render_widget(cpu_gauge, cpu_global_layout[0]);

    let sparkline = Sparkline::default()
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" CPU HISTORY ")
            .title_style(Style::default().fg(theme_indigo)))
        .style(Style::default().fg(text_highlight))
        .data(&app.cpu_history);
    frame.render_widget(sparkline, cpu_global_layout[1]);

    let cores_block_width = cpu_sub_layout[1].width;
    let cores_widget = render_cpu_cores_table(&snapshot_ref.per_core_cpu_usage, cores_block_width)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" LOGICAL CORES MATRIX "));
    frame.render_widget(cores_widget, cpu_sub_layout[1]);

    let mem_sub_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(left_chunks[2]);

    let ram_used = snapshot_ref.used_memory;
    let ram_total = snapshot_ref.total_memory;
    let ram_pct = if ram_total > 0 { (ram_used as f64 / ram_total as f64) * 100.0 } else { 0.0 };
    let ram_color = if ram_pct < 60.0 { Color::Rgb(52, 211, 153) } else if ram_pct < 85.0 { Color::Rgb(251, 191, 36) } else { Color::Rgb(251, 113, 133) };
    let ram_label = format!("{:.1}% ({}/{})", ram_pct, format_bytes(ram_used), format_bytes(ram_total));
    
    draw_metric_gauge(
        frame,
        mem_sub_layout[0],
        "RAM UTILIZATION",
        ram_used as f64 / ram_total.max(1) as f64,
        &ram_label,
        ram_color,
        Color::Rgb(30, 41, 59),
        border_color,
        theme_indigo,
    );

    let (gpu_pct, gpu_used, gpu_total) = match &app.gpu_data {
        Some(g) => (g.utilization, g.vram_used, g.vram_total),
        None => (0.0, 0, 0),
    };
    let gpu_color = if gpu_pct < 60.0 { Color::Rgb(52, 211, 153) } else if gpu_pct < 85.0 { Color::Rgb(251, 191, 36) } else { Color::Rgb(251, 113, 133) };
    let gpu_label = format!("{:.1}% ({}/{})", gpu_pct, format_bytes(gpu_used), format_bytes(gpu_total));

    draw_metric_gauge(
        frame,
        mem_sub_layout[1],
        "GPU UTILIZATION",
        gpu_pct as f64 / 100.0,
        &gpu_label,
        gpu_color,
        Color::Rgb(30, 41, 59),
        border_color,
        theme_indigo,
    );

    let bottom_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55), // Disks Table
            Constraint::Percentage(45), // Network and Processes Sort controls
        ])
        .split(left_chunks[3]);

    let mut disk_rows = Vec::new();
    for disk in &snapshot_ref.disks {
        let used = disk.total_space.saturating_sub(disk.available_space);
        let pct = if disk.total_space > 0 { (used as f64 / disk.total_space as f64) * 100.0 } else { 0.0 };
        let filled_bars = (pct / 10.0).round() as usize;
        let bar_graph: String = std::iter::repeat('▓').take(filled_bars.min(10))
            .chain(std::iter::repeat('░').take(10 - filled_bars.min(10)))
            .collect();
        
        disk_rows.push(Row::new(vec![
            Cell::from(disk.name.clone()),
            Cell::from(disk.mount_point.clone()),
            Cell::from(disk.file_system.clone()),
            Cell::from(format_bytes(disk.available_space)),
            Cell::from(format_bytes(disk.total_space)),
            Cell::from(format!("{} {:.1}%", bar_graph, pct)).style(Style::default().fg(if pct > 85.0 { Color::Rgb(251, 113, 133) } else { theme_indigo })),
        ]).style(Style::default().fg(text_neutral)));
    }

    let disk_table = Table::new(
        disk_rows,
        vec![
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
        ]
    )
    .header(Row::new(vec!["Drive", "Mount", "Format", "Free Space", "Total Size", "Usage (Visual)"])
        .style(Style::default().bold().fg(text_highlight)))
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" STORAGE DEVICES "));
    frame.render_widget(disk_table, bottom_chunks[0]);

    let rx_formatted = format_speed(snapshot_ref.net_rx_bytes_sec);
    let tx_formatted = format_speed(snapshot_ref.net_tx_bytes_sec);
    let rx_tot = format_bytes(snapshot_ref.net_total_rx);
    let tx_tot = format_bytes(snapshot_ref.net_total_tx);

    let net_rows = vec![
        Row::new(vec![
            Cell::from(" DL Speed:"), 
            Cell::from(rx_formatted).style(Style::default().fg(Color::Rgb(52, 211, 153)).bold()),
            Cell::from(" Sent Total:"), 
            Cell::from(tx_tot)
        ]),
        Row::new(vec![
            Cell::from(" UL Speed:"), 
            Cell::from(tx_formatted).style(Style::default().fg(theme_violet).bold()),
            Cell::from(" Recv Total:"), 
            Cell::from(rx_tot)
        ]),
    ];

    let net_table = Table::new(
        net_rows,
        vec![
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ]
    )
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" NETWORK THROUGHPUT "))
    .style(Style::default().fg(text_neutral));
    frame.render_widget(net_table, bottom_chunks[1]);

    let sort_col_name = format!("{:?}", app.sort_column);
    let view_label = match app.right_view {
        RightView::Media => "Media",
        RightView::Processes => "Process",
    };
    let theme_name = THEMES[app.theme_index].name;
    let footer_content = format!(
        " [q] Quit | [v] View({}) | [t] {} | [Tab/s] Sort | [r] Rev | [j/k] Scroll (Sort: {} {})",
        view_label,
        theme_name,
        sort_col_name,
        if app.sort_ascending { "Asc" } else { "Desc" }
    );
    if app.footer_scroll.content != footer_content {
        app.footer_scroll.content = footer_content;
    }
    let footer_max = left_chunks[4].width as usize;
    let footer_visible = app.footer_scroll.get_visible_slice(footer_max);
    let footer = Paragraph::new(Span::styled(footer_visible, Style::default().fg(Color::Rgb(148, 163, 184))));
    frame.render_widget(footer, left_chunks[4]);


    match app.right_view {
        RightView::Media => {
            draw_media_view(frame, app, main_chunks[1], theme);
        }
        RightView::Processes => {
            draw_process_table(frame, app, main_chunks[1], theme);
        }
    }

    if app.show_theme_picker {
        draw_theme_picker(frame, size, theme, app.theme_picker_index);
    }
}

fn draw_theme_picker(frame: &mut ratatui::Frame, area: Rect, theme: &ColorTheme, picker_index: usize) {
    let pick_w = 32.min(area.width.saturating_sub(4));
    let pick_h = (THEMES.len() as u16 + 4).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(pick_w)) / 2;
    let y = (area.height.saturating_sub(pick_h)) / 2;
    let popup = Rect { x, y, width: pick_w, height: pick_h };

    frame.render_widget(Clear, popup);

    let mut lines: Vec<Line> = Vec::with_capacity(THEMES.len() + 2);
    lines.push(Line::from(Span::styled(
        " SELECT THEME ",
        Style::default().bold().fg(theme.highlight),
    )));
    lines.push(Line::from(Span::styled(
        " ".to_string() + &"─".repeat((pick_w as usize).saturating_sub(2)),
        Style::default().fg(theme.border),
    )));
    for (i, t) in THEMES.iter().enumerate() {
        let marker = if i == picker_index { ">" } else { " " };
        let style = if i == picker_index {
            Style::default().fg(theme.highlight).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.neutral)
        };
        lines.push(Line::from(Span::styled(
            format!(" {} {}", marker, t.name),
            style,
        )));
    }
    lines.push(Line::from(Span::styled(
        " [arrows] move | [Enter] select | [Esc] cancel ",
        Style::default().fg(theme.border),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.highlight));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_media_view(
    frame: &mut ratatui::Frame,
    app: &mut App,
    area: Rect,
    theme: &ColorTheme,
) {
    let border_color = theme.border;
    let text_highlight = theme.highlight;
    let text_neutral = theme.neutral;
    let theme_violet = theme.accent1;
    let theme_indigo = theme.accent2;

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(14),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(area);

    let media_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" MEDIA CONTROL DASHBOARD ")
        .title_style(Style::default().fg(text_highlight));
    let media_inner = media_block.inner(right_chunks[0]);
    frame.render_widget(media_block, right_chunks[0]);

    let media_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // Album art (scales with terminal width)
            Constraint::Min(10),        // Song metadata information
        ])
        .split(media_inner);

    app.art_target_w = media_layout[0].width.saturating_sub(2) as u32;
    app.art_target_h = media_layout[0].height.saturating_sub(2) as u32;

    let art_cols = app.art_target_w as usize;
    let art_rows = app.art_target_h as usize;
    match &app.album_art_matrix {
        Some(matrix) => {
            let m_cols = matrix.first().map(|r| r.len()).unwrap_or(0);
            let m_rows = matrix.len() / 2;
            if m_cols == 0 || m_rows == 0 {
                let placeholder_row = "▒".repeat(art_cols);
                let mut art_lines = Vec::new();
                for _ in 0..art_rows {
                    art_lines.push(Line::from(vec![Span::styled(&placeholder_row, Style::default().fg(border_color))]));
                }
                let art_widget = Paragraph::new(art_lines)
                    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)));
                frame.render_widget(art_widget, media_layout[0]);
            } else {
                let x_off = (art_cols.saturating_sub(m_cols)) / 2;
                let y_off = (art_rows.saturating_sub(m_rows)) / 2;
                let mut spans_lines: Vec<Line> = Vec::with_capacity(art_rows);
                for _ in 0..y_off {
                    spans_lines.push(Line::from(" ".repeat(art_cols)));
                }
                for y in 0..m_rows {
                    let mut row_spans: Vec<Span> = Vec::with_capacity(art_cols);
                    if x_off > 0 {
                        row_spans.push(Span::styled(" ".repeat(x_off), Style::default()));
                    }
                    for x in 0..m_cols.min(art_cols.saturating_sub(x_off)) {
                        let upper = matrix[y * 2][x];
                        let lower = matrix[y * 2 + 1][x];
                        row_spans.push(Span::styled(
                            "▄",
                            Style::default()
                                .fg(Color::Rgb(upper.0, upper.1, upper.2))
                                .bg(Color::Rgb(lower.0, lower.1, lower.2)),
                        ));
                    }
                    let remaining = art_cols.saturating_sub(x_off + m_cols);
                    if remaining > 0 {
                        row_spans.push(Span::styled(" ".repeat(remaining), Style::default()));
                    }
                    spans_lines.push(Line::from(row_spans));
                }
                let bottom = art_rows.saturating_sub(y_off + m_rows);
                for _ in 0..bottom {
                    spans_lines.push(Line::from(" ".repeat(art_cols)));
                }
                let art_widget = Paragraph::new(spans_lines)
                    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)));
                frame.render_widget(art_widget, media_layout[0]);
            }
        }
        None => {
            let placeholder_row = "▒".repeat(art_cols);
            let mut art_lines = Vec::new();
            for _ in 0..art_rows {
                art_lines.push(Line::from(vec![Span::styled(&placeholder_row, Style::default().fg(border_color))]));
            }
            let art_placeholder = Paragraph::new(art_lines)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)));
            frame.render_widget(art_placeholder, media_layout[0]);
        }
    }

    let meta_panel = match &app.media_metadata {
        Some(meta) => {
            let status_text = if meta.is_playing { "Playing" } else { "Paused" };
            let status_style = if meta.is_playing { Style::default().fg(Color::Rgb(52, 211, 153)).bold() } else { Style::default().fg(Color::Yellow).bold() };

            let year_resolved = meta.year.as_deref().unwrap_or("Fetching...");

            let text_max = (media_layout[1].width as usize).saturating_sub(4);
            
            vec![
                Line::from(vec![Span::styled("Now Playing", Style::default().bold().fg(theme_violet))]),
                Line::from(vec![Span::styled("------------------------", Style::default().fg(border_color))]),
                Line::from(vec![
                    Span::styled("Track:  ", Style::default().fg(theme_indigo)),
                    Span::styled(app.marquee_title.get_visible_slice(text_max.saturating_sub(8)), Style::default().bold().fg(Color::White))
                ]),
                Line::from(vec![
                    Span::styled("Artist: ", Style::default().fg(theme_indigo)),
                    Span::styled(app.marquee_artist.get_visible_slice(text_max.saturating_sub(8)), Style::default().fg(text_neutral))
                ]),
                Line::from(vec![
                    Span::styled("Album:  ", Style::default().fg(theme_indigo)),
                    Span::styled(app.marquee_album.get_visible_slice(text_max.saturating_sub(8)), Style::default().fg(text_neutral))
                ]),
                Line::from(vec![
                    Span::styled("Year:   ", Style::default().fg(theme_indigo)),
                    Span::styled(year_resolved, Style::default().fg(text_highlight))
                ]),
                {
                    let total = app.media_total_secs;
                    let extrapolated = app.media_position_secs
                        + app.media_position_captured_at
                            .map(|t| t.elapsed().as_secs_f64())
                            .unwrap_or(0.0);
                    let pos = extrapolated.min(total).max(0.0);
                    let bar_len = text_max.saturating_sub(24).max(4);
                    let filled = if total > 0.0 {
                        ((pos / total) * bar_len as f64).round() as usize
                    } else {
                        0
                    };
                    let filled = filled.min(bar_len);
                    let bar: String = std::iter::repeat('█').take(filled)
                        .chain(std::iter::repeat('░').take(bar_len - filled))
                        .collect();
                    let pos_str = format_duration(pos);
                    let total_str = format_duration(total);
                    Line::from(vec![
                        Span::styled("Progress: ", Style::default().fg(theme_indigo)),
                        Span::styled(bar, Style::default().fg(theme.highlight)),
                        Span::styled(format!(" {}/{}", pos_str, total_str), Style::default().fg(text_neutral)),
                    ])
                },
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(theme_indigo)), 
                    Span::styled(status_text, status_style)
                ]),
            ]
        }
        None => {
            vec![
                Line::from(vec![Span::styled("Media Controls Offline", Style::default().bold().fg(Color::DarkGray))]),
                Line::from(vec![Span::styled("------------------------", Style::default().fg(border_color))]),
                Line::from(vec![Span::styled("No active GSMTC media session found.", Style::default().fg(Color::DarkGray))]),
                Line::from(vec![Span::styled("Play audio in Spotify or YouTube to activate.", Style::default().fg(Color::DarkGray))]),
            ]
        }
    };
    let meta_widget = Paragraph::new(meta_panel).block(Block::default().padding(ratatui::widgets::Padding::new(2, 0, 1, 0)));
    frame.render_widget(meta_widget, media_layout[1]);

    draw_spectrum_visualizer(frame, app, right_chunks[1], theme);

    let spacer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    frame.render_widget(spacer, right_chunks[2]);
}

fn draw_process_table(
    frame: &mut ratatui::Frame,
    app: &mut App,
    area: Rect,
    theme: &ColorTheme,
) {
    let border_color = theme.border;
    let text_highlight = theme.highlight;
    let text_neutral = theme.neutral;
    let theme_indigo = theme.accent2;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(area);

    let sort_indicator = |col: SortColumn| -> &'static str {
        if app.sort_column != col {
            return " ";
        }
        if app.sort_ascending { " ^" } else { " v" }
    };

    let header_cells = [
        format!(" PID{}", sort_indicator(SortColumn::Pid)),
        format!(" Name{}", sort_indicator(SortColumn::Name)),
        format!(" CPU%{}", sort_indicator(SortColumn::Cpu)),
        format!(" Memory{}", sort_indicator(SortColumn::Memory)),
    ];
    let header = Row::new(header_cells.iter().map(|c| Cell::from(c.as_str())))
        .style(Style::default().bold().fg(text_highlight));

    let rows: Vec<Row> = app.sorted_processes.iter().map(|p| {
        let mem_str = format_bytes(p.memory);
        Row::new(vec![
            Cell::from(format!(" {}", p.pid)),
            Cell::from(format!(" {}", p.name)),
            Cell::from(format!(" {:.1}", p.cpu_usage)),
            Cell::from(format!(" {}", mem_str)),
        ])
        .style(Style::default().fg(text_neutral))
    }).collect();

    let selected_style = Style::default()
        .fg(Color::Rgb(15, 23, 42))
        .bg(text_highlight);

    let table = Table::new(
        rows,
        vec![
            Constraint::Length(8),
            Constraint::Percentage(50),
            Constraint::Length(8),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(format!(" SYSTEM PROCESSES ({}) ", app.sorted_processes.len()))
            .title_style(Style::default().fg(theme_indigo)),
    )
    .highlight_style(selected_style)
    .highlight_symbol(">");

    frame.render_stateful_widget(table, chunks[0], &mut app.process_table_state);

    let spacer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    frame.render_widget(spacer, chunks[1]);
}

fn render_cpu_cores_table(per_core_usage: &[f32], width: u16) -> Table<'_> {
    let col_width = 16usize;
    
    let cols = ((width as usize) / col_width).max(1).min(8);
    
    let mut rows = Vec::new();
    for chunk in per_core_usage.chunks(cols) {
        let mut row_cells = Vec::new();
        for (i, &usage) in chunk.iter().enumerate() {
            let core_idx = rows.len() * cols + i;
            let filled = (usage / 20.0).round() as usize;
            let filled = filled.min(5);
            let bar: String = std::iter::repeat('▓').take(filled)
                .chain(std::iter::repeat('░').take(5 - filled))
                .collect();
            
            let color = if usage < 50.0 {
                Color::Rgb(52, 211, 153) // Emerald 400
            } else if usage < 85.0 {
                Color::Rgb(251, 191, 36) // Amber 400
            } else {
                Color::Rgb(251, 113, 133) // Rose 400
            };
            
            let cell_text = format!("C{:02}:[{}] {:3.0}%", core_idx, bar, usage);
            row_cells.push(Cell::from(cell_text).style(Style::default().fg(color)));
        }
        
        while row_cells.len() < cols {
            row_cells.push(Cell::from(""));
        }
        rows.push(Row::new(row_cells));
    }
    
    let constraints = vec![Constraint::Length(col_width as u16); cols];
    Table::new(rows, constraints)
}

fn draw_spectrum_visualizer(frame: &mut ratatui::Frame, app: &mut App, area: Rect, theme: &ColorTheme) {
    let w = area.width as usize;
    let h = area.height as usize;
    
    if w <= 2 || h <= 2 {
        return;
    }

    let render_width = w - 2;
    let render_height = h - 2;

    let max_bars = (render_width / 2).max(1);
    app.max_visualizer_bars = max_bars;
    if app.visualizer_bars.len() < max_bars {
        app.visualizer_bars.resize(max_bars, 0.0);
    }
    let spacing = max_bars.saturating_sub(1);
    let bar_width = render_width.saturating_sub(spacing) / max_bars;
    let bar_width = bar_width.max(1);

    let blocks = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let mut lines = Vec::new();

    // NOTE: Do NOT reverse. rev() builds top-to-bottom order. An earlier lines.reverse() caused an upside-down bug.
    for r in (0..render_height).rev() {
        let mut row_spans = Vec::new();
        for i in 0..max_bars {
            let val = app.visualizer_bars[i];
            let total_steps = render_height * 8;
            let steps = (val * total_steps as f32).round() as usize;

            let char_idx = if steps >= (r + 1) * 8 {
                8
            } else if steps <= r * 8 {
                0
            } else {
                steps - r * 8
            };

            let block_char = blocks[char_idx];
            let bar_str = std::iter::repeat(block_char).take(bar_width).collect::<String>();

            let color = if r < render_height / 3 {
                theme.highlight
            } else if r < 2 * render_height / 3 {
                theme.accent2
            } else {
                theme.accent1
            };

            row_spans.push(Span::styled(bar_str, Style::default().fg(color)));
            if i < max_bars - 1 {
                row_spans.push(Span::styled(" ", Style::default()));
            }
        }
        lines.push(Line::from(row_spans));
    }

    let visualizer_widget = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" REAL-TIME FREQUENCY SPECTRUM ")
            .title_style(Style::default().fg(theme.accent2)));
            
    frame.render_widget(visualizer_widget, area);
}

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if days > 0 {
        format!("{}d {:02}h {:02}m {:02}s", days, hours, minutes, seconds)
    } else if hours > 0 {
        format!("{}h {:02}m {:02}s", hours, minutes, seconds)
    } else {
        format!("{}m {:02}s", minutes, seconds)
    }
}

fn format_duration(total_secs: f64) -> String {
    let total = total_secs as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let i = (bytes as f64).log(1024.0).floor() as usize;
    let i = i.min(units.len() - 1);
    let val = bytes as f64 / 1024.0_f64.powi(i as i32);
    if i == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.2} {}", val, units[i])
    }
}

fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

fn draw_metric_gauge(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    ratio: f64,
    label: &str,
    color: Color,
    bg_color: Color,
    border_color: Color,
    title_color: Color,
) {
    if area.width < 30 {
        let pct = (ratio * 100.0).round() as usize;
        let total_chars = (area.width as usize).saturating_sub(8).max(4);
        let filled = (ratio * total_chars as f64).round() as usize;
        let filled = filled.min(total_chars);
        let bar: String = std::iter::repeat('▓').take(filled)
            .chain(std::iter::repeat('░').take(total_chars - filled))
            .collect();
        let text = format!("[{}] {}%", bar, pct);
        let p = Paragraph::new(Span::styled(text, Style::default().fg(color).bold()));
        frame.render_widget(p, area);
    } else {
        let gauge = Gauge::default()
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(format!(" {} ", title))
                .title_style(Style::default().fg(title_color)))
            .gauge_style(Style::default().fg(color).bg(bg_color))
            .ratio(ratio.clamp(0.0, 1.0))
            .label(label);
        frame.render_widget(gauge, area);
    }
}
