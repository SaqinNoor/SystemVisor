mod telemetry;
mod audio_visualizer;
mod media_controls;
mod art_processor;

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
    widgets::{Block, Borders, Gauge, Paragraph, Row, Table, TableState, Sparkline, Cell},
    Terminal,
};
use telemetry::{SystemSnapshot, ProcessInfo};
use media_controls::{MediaMetadata, MediaEvent};

/// A Drop guard to guarantee the terminal returns to its normal state even on unexpected panics or exits.
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

/// The state of the UI application.
struct App {
    snapshot: Option<Box<SystemSnapshot>>,
    cpu_history: Vec<u64>,
    sort_column: SortColumn,
    sort_ascending: bool,
    process_table_state: TableState,
    sorted_processes: Vec<ProcessInfo>,
    
    // Audio Visualizer states
    visualizer_bars: Vec<f32>,

    // Media states
    media_metadata: Option<MediaMetadata>,
    album_art_matrix: Option<Vec<Vec<(u8, u8, u8)>>>,
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
            media_metadata: None,
            album_art_matrix: None,
        }
    }

    /// Update the snapshot and sort the inner processes.
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
    Resize(u16, u16),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Terminal setup (raw mode, alternate screen, capture mouse, hide cursor)
    enable_raw_mode()?;
    let mut stdout_stream = stdout();
    execute!(
        stdout_stream,
        EnterAlternateScreen,
        EnableMouseCapture,
        Hide
    )?;
    
    // Explicit Drop guard to ensure cleanup on normal exit
    let _guard = TerminalGuard;

    // Set custom panic hook to reset the terminal on panics
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

    // 2. Setup channels & spawn background worker threads
    let (telemetry_tx, telemetry_rx) = crossbeam_channel::unbounded();
    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    let (audio_tx, audio_rx) = crossbeam_channel::unbounded();
    let (media_tx, media_rx) = crossbeam_channel::unbounded();
    let (art_tx, art_rx) = crossbeam_channel::unbounded();

    // Input polling thread
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
                    Ok(event::Event::Resize(w, h)) => {
                        if input_tx.send(AppEvent::Resize(w, h)).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    // Telemetry gathering thread (refresh metrics every 500ms)
    telemetry::spawn_telemetry_thread(telemetry_tx, Duration::from_millis(500));

    // Audio capturing loopback visualizer thread
    audio_visualizer::spawn_audio_visualizer_thread(audio_tx);

    // Media tracking and online metadata year resolver thread
    media_controls::spawn_media_controls_thread(media_tx);

    // 3. Initialize terminal and application state
    let backend = CrosstermBackend::new(stdout_stream);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new();

    terminal.clear()?;

    // 4. Main Event Selector Loop
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
                            _ => {}
                        }
                    }
                    Ok(AppEvent::Resize(_, _)) => {}
                    Err(_) => break,
                }
            }
            recv(audio_rx) -> audio_res => {
                match audio_res {
                    Ok(spectrum) => {
                        // Apply exponential decay filter (~0.85) to smooth out visualizer drop
                        for i in 0..16 {
                            app.visualizer_bars[i] = f32::max(spectrum[i], app.visualizer_bars[i] * 0.92);
                        }
                    }
                    Err(_) => {}
                }
            }
            recv(media_rx) -> media_res => {
                match media_res {
                    Ok(MediaEvent::Metadata(meta)) => {
                        app.media_metadata = Some(*meta);
                    }
                    Ok(MediaEvent::Thumbnail(bytes)) => {
                        // Trigger cover art downsampling on a separate thread to keep UI locked fluid at 60fps
                        let art_tx_clone = art_tx.clone();
                        std::thread::spawn(move || {
                            // Target grid coordinates: 20 cols by 10 rows (downsamples internally to 20x20 pixels)
                            if let Some(matrix) = art_processor::process_album_art(&bytes, 20, 10) {
                                let _ = art_tx_clone.send(matrix);
                            }
                        });
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
        }

        // Draw TUI frame
        terminal.draw(|f| draw_ui(f, &mut app))?;
    }

    Ok(())
}

/// Centralized UI compiler. Implements a zero-emoji visual policy.
fn draw_ui(frame: &mut ratatui::Frame, app: &mut App) {
    let size = frame.size();

    // Minimalist geometry division: Split screen horizontally
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .margin(1)
        .constraints([
            Constraint::Percentage(50), // System Telemetry Monitor (Left column)
            Constraint::Percentage(50), // Media Dashboard & Audio Spectrum (Right column)
        ])
        .split(size);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Host Header Banner
            Constraint::Length(11), // Logical CPU Cores Matrix Grid (Dynamic solver)
            Constraint::Length(6),  // Memory Gauges block
            Constraint::Min(8),     // Storage and Network Speed
            Constraint::Length(1),  // Quick Help controls
        ])
        .split(main_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(14), // Media Controls (Album art & track metadata)
            Constraint::Min(10),    // Real-time Frequency Spectrum Visualizer
        ])
        .split(main_chunks[1]);

    // Slate Indigo palette color tokens
    let border_color = Color::Rgb(71, 85, 105);
    let text_highlight = Color::Rgb(34, 211, 238);
    let text_neutral = Color::Rgb(226, 232, 240);
    let theme_violet = Color::Rgb(167, 139, 250);
    let theme_indigo = Color::Rgb(129, 140, 248);

    // Retrieve snap reference
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

    // --- LEFT COLUMN: SYSTEM TELEMETRY MONITOR ---

    // 1. Host Header Banner (Zero-Emoji)
    let uptime_str = format_uptime(snapshot_ref.uptime);
    let title_line = Line::from(vec![
        Span::styled(" WIN11 TELEMETRY SYSTEM MONITOR ", Style::default().fg(text_highlight).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" Host: {} ", snapshot_ref.host_name), Style::default().fg(text_neutral)),
        Span::styled(format!(" | OS: {} {} ({}) ", snapshot_ref.os_name, snapshot_ref.os_version, snapshot_ref.cpu_arch), Style::default().fg(text_neutral)),
        Span::styled(format!(" | Uptime: {} ", uptime_str), Style::default().fg(theme_violet)),
    ]);
    
    let header_widget = Paragraph::new(title_line)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)));
    frame.render_widget(header_widget, left_chunks[0]);

    // 2. CPU Logical Cores Dynamic Solver Grid
    let cpu_sub_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35), // Global CPU usage gauge & Sparkline
            Constraint::Percentage(65), // Dynamic Grid per-core solver
        ])
        .split(left_chunks[1]);

    let cpu_global_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Global Gauge
            Constraint::Min(4),    // Global History Sparkline
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
        .block(Block::default().title(" SYSTEM CPU ").title_style(Style::default().fg(theme_indigo)))
        .gauge_style(Style::default().fg(cpu_color).bg(Color::Rgb(30, 41, 59)))
        .ratio(cpu_pct as f64 / 100.0)
        .label(format!("{:.1}%", cpu_pct));
    frame.render_widget(cpu_gauge, cpu_global_layout[0]);

    let sparkline = Sparkline::default()
        .block(Block::default().title(" CPU HISTORY ").title_style(Style::default().fg(theme_indigo)))
        .style(Style::default().fg(text_highlight))
        .data(&app.cpu_history);
    frame.render_widget(sparkline, cpu_global_layout[1]);

    // Calculate core display width and solve columns solver programmatically
    let cores_block_width = cpu_sub_layout[1].width;
    let cores_widget = render_cpu_cores_table(&snapshot_ref.per_core_cpu_usage, cores_block_width)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" LOGICAL CORES MATRIX "));
    frame.render_widget(cores_widget, cpu_sub_layout[1]);

    // 3. Memory & GPU Gauges Block
    let mem_sub_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // RAM utilization
            Constraint::Length(3), // GPU utilization
        ])
        .split(left_chunks[2]);

    // RAM
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

    // GPU
    let gpu_used = snapshot_ref.gpu_vram_used;
    let gpu_total = snapshot_ref.gpu_vram_total;
    let gpu_pct = snapshot_ref.gpu_usage;
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

    // 4. Storage & Network Speed (Combined Layout Block)
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

    // Network throughput
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

    // 5. Help Footer
    let sort_col_name = format!("{:?}", app.sort_column);
    let footer_text = format!(
        " [q] Quit | [Tab/s] Cycle Sort | [r] Reverse | [1-4] Sort Col | [j/k] Scroll (Sort: {} {})",
        sort_col_name,
        if app.sort_ascending { "Asc" } else { "Desc" }
    );
    let footer = Paragraph::new(Span::styled(footer_text, Style::default().fg(Color::Rgb(148, 163, 184))));
    frame.render_widget(footer, left_chunks[4]);


    // --- RIGHT COLUMN: MEDIA DASHBOARD & DSP SPECTRUM ---

    // 1. Media Control Dashboard Block (Album art + song info metadata)
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
            Constraint::Length(22), // Downscaled blitted art (Width matches 20 pixels + borders)
            Constraint::Min(10),    // Song metadata information
        ])
        .split(media_inner);

    // Blit raw album art or fallback placeholder
    match &app.album_art_matrix {
        Some(matrix) => {
            let mut spans_lines = Vec::new();
            // Blit using vertical half-block coordinate system (2 vertical pixels mapped per terminal cell)
            for y in 0..10usize {
                let mut row_spans = Vec::new();
                for x in 0..20usize {
                    // Safety check array indices
                    if y * 2 < matrix.len() && y * 2 + 1 < matrix.len() && x < matrix[0].len() {
                        let upper = matrix[y * 2][x];
                        let lower = matrix[y * 2 + 1][x];
                        row_spans.push(Span::styled(
                            "▄",
                            Style::default()
                                .fg(Color::Rgb(upper.0, upper.1, upper.2))
                                .bg(Color::Rgb(lower.0, lower.1, lower.2)),
                        ));
                    }
                }
                spans_lines.push(Line::from(row_spans));
            }
            let art_widget = Paragraph::new(spans_lines)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)));
            frame.render_widget(art_widget, media_layout[0]);
        }
        None => {
            // Render beautiful geometric geometric placeholder if no media is running
            let mut art_lines = Vec::new();
            for _ in 0..10 {
                art_lines.push(Line::from(vec![Span::styled("▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒", Style::default().fg(border_color))]));
            }
            let art_placeholder = Paragraph::new(art_lines)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)));
            frame.render_widget(art_placeholder, media_layout[0]);
        }
    }

    // Render song info metadata panel (Zero-Emoji)
    let meta_panel = match &app.media_metadata {
        Some(meta) => {
            let status_text = if meta.is_playing { "Playing" } else { "Paused" };
            let status_style = if meta.is_playing { Style::default().fg(Color::Rgb(52, 211, 153)).bold() } else { Style::default().fg(Color::Yellow).bold() };

            let year_resolved = meta.year.as_deref().unwrap_or("Fetching...");
            
            vec![
                Line::from(vec![Span::styled("Now Playing", Style::default().bold().fg(theme_violet))]),
                Line::from(vec![Span::styled("------------------------", Style::default().fg(border_color))]),
                Line::from(vec![Span::styled("Track:  ", Style::default().fg(theme_indigo)), Span::styled(meta.title.clone(), Style::default().bold().fg(Color::White))]),
                Line::from(vec![Span::styled("Artist: ", Style::default().fg(theme_indigo)), Span::styled(meta.artist.clone(), Style::default().fg(text_neutral))]),
                Line::from(vec![Span::styled("Album:  ", Style::default().fg(theme_indigo)), Span::styled(meta.album.clone(), Style::default().fg(text_neutral))]),
                Line::from(vec![
                    Span::styled("Year:   ", Style::default().fg(theme_indigo)),
                    Span::styled(year_resolved, Style::default().fg(text_highlight))
                ]),
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

    // 2. Real-time Frequency Spectrum Visualizer (High-Fidelity Block drawing)
    draw_spectrum_visualizer(frame, app, right_chunks[1]);
}

/// Dynamic grid-layout solver mapping physical CPU logical cores into custom wraps
fn render_cpu_cores_table(per_core_usage: &[f32], width: u16) -> Table<'_> {
    // Width allocated to each logical core cell (C01:[▓▓░░]100%)
    let col_width = 16usize;
    
    // Solve layout column bounds dynamically
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
        
        // Pad the remainder cells if row wraps incomplete
        while row_cells.len() < cols {
            row_cells.push(Cell::from(""));
        }
        rows.push(Row::new(row_cells));
    }
    
    let constraints = vec![Constraint::Length(col_width as u16); cols];
    Table::new(rows, constraints)
}

/// Blits the DSP analysis vector vertically into frames using block markers
fn draw_spectrum_visualizer(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let w = area.width as usize;
    let h = area.height as usize;
    
    if w <= 2 || h <= 2 {
        return;
    }

    // Inner visualizer coordinates accounting for border
    let render_width = w - 2;
    let render_height = h - 2;
    
    const BARS: usize = 16;
    let bar_width = (render_width / BARS).max(1);

    // High density visualizer block scale: 9 entries for sub-pixel height indices 0..=8
    // Index 0 = empty space, 1 = ▁ (1/8), ..., 8 = █ (full)
    let blocks = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let mut lines = Vec::new();

    // Iterate from top row (render_height-1) down to floor row (0).
    // Row r=render_height-1 is the ceiling; r=0 is the floor.
    // Pushing in reverse order (high r first) means lines[0] = ceiling row = widget top.
    // Do NOT reverse after — the earlier lines.reverse() was the source of the upside-down bug.
    for r in (0..render_height).rev() {
        let mut row_spans = Vec::new();
        for i in 0..BARS {
            let val = app.visualizer_bars[i];
            // Total sub-pixel steps across the full render height
            let total_steps = render_height * 8;
            let steps = (val * total_steps as f32).round() as usize;

            // Determine which sub-pixel block character to draw for this row:
            // - steps >= (r+1)*8 → bar fully covers this row → full block (index 8)
            // - steps <= r*8    → bar is entirely below this row → empty (index 0)
            // - otherwise       → partial fill: how many eighths into this row
            let char_idx = if steps >= (r + 1) * 8 {
                8
            } else if steps <= r * 8 {
                0
            } else {
                steps - r * 8
            };

            let block_char = blocks[char_idx];
            let bar_str = std::iter::repeat(block_char).take(bar_width).collect::<String>();

            // Gradient: Cyan (floor/bottom) -> Indigo (middle) -> Violet (ceiling/top)
            let color = if r < render_height / 3 {
                Color::Rgb(34, 211, 238)   // Cyan 400 — bass frequencies at bottom
            } else if r < 2 * render_height / 3 {
                Color::Rgb(129, 140, 248)  // Indigo 400 — mids
            } else {
                Color::Rgb(167, 139, 250)  // Violet 400 — highs at top
            };

            row_spans.push(Span::styled(bar_str, Style::default().fg(color)));
            row_spans.push(Span::styled(" ", Style::default())); // inter-bar spacing
        }
        lines.push(Line::from(row_spans));
    }
    // NOTE: Do NOT call lines.reverse() here. The rev() in the loop above already
    // builds lines in top-to-bottom order (ceiling first, floor last).

    let visualizer_widget = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(71, 85, 105)))
            .title(" REAL-TIME FREQUENCY SPECTRUM ")
            .title_style(Style::default().fg(Color::Rgb(129, 140, 248))));
            
    frame.render_widget(visualizer_widget, area);
}

/// Helper function to format uptime into a zero-emoji string
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

/// Dynamic byte formatting utility
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

/// Network speed formatting utility
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
        // Drop title/borders, render a highly packed metric indicator:
        // [▓▓░░] 45%
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
