mod telemetry;

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
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Row, Table, TableState, Sparkline, Cell},
    Terminal,
};
use telemetry::{SystemSnapshot, ProcessInfo};

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
        }
    }

    /// Update the snapshot and sort the inner processes.
    fn update_snapshot(&mut self, snap: Box<SystemSnapshot>) {
        // Record CPU global usage in history for sparkline (limit to 120 ticks)
        self.cpu_history.push(snap.global_cpu_usage as u64);
        if self.cpu_history.len() > 120 {
            self.cpu_history.remove(0);
        }

        let mut procs = snap.processes.clone();
        
        // Sort the processes based on configuration
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

        // Clamping the table state selected index to the new processes length
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
    Resize(#[allow(dead_code)] u16, #[allow(dead_code)] u16),
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

    // 2. Setup channels & spawn worker threads
    let (telemetry_tx, telemetry_rx) = crossbeam_channel::unbounded();
    let (input_tx, input_rx) = crossbeam_channel::unbounded();

    // Input polling thread
    std::thread::spawn(move || {
        loop {
            // Check for crossterm events (keyboard/resize)
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                match event::read() {
                    Ok(event::Event::Key(key)) => {
                        // Filter out release events to prevent double processing on Windows
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

    // 3. Initialize terminal and application state
    let backend = CrosstermBackend::new(stdout_stream);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new();

    // Clear terminal screen initially
    terminal.clear()?;

    // 4. Main Event Selector Loop
    loop {
        // Wait and select over incoming telemetry updates and user input events
        crossbeam_channel::select! {
            recv(telemetry_rx) -> telemetry_res => {
                match telemetry_res {
                    Ok(snapshot) => {
                        app.update_snapshot(snapshot);
                    }
                    Err(_) => {
                        // Telemetry worker thread hung up
                        break;
                    }
                }
            }
            recv(input_rx) -> input_res => {
                match input_res {
                    Ok(AppEvent::Key(key)) => {
                        // Global quit keys
                        if key.code == KeyCode::Char('q') || 
                           (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
                            break;
                        }

                        // Keyboard controls
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
                    Ok(AppEvent::Resize(_, _)) => {
                        // Ratatui handles resizing automatically on drawing
                    }
                    Err(_) => {
                        // Input thread closed
                        break;
                    }
                }
            }
        }

        // Render screen
        terminal.draw(|f| draw_ui(f, &mut app))?;
    }

    Ok(())
}

/// Dynamic draw method compiling components into sections.
fn draw_ui(frame: &mut ratatui::Frame, app: &mut App) {
    let size = frame.size();

    // 1. Grid layout division
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // Header block
            Constraint::Length(10), // CPU and Memory Gauges block
            Constraint::Length(7),  // Storage and Network Speed block
            Constraint::Min(8),     // Processes Table list
            Constraint::Length(1),  // Footer controls help
        ])
        .split(size);

    // Color definitions for curated palette
    let border_color = Color::Rgb(71, 85, 105); // Slate 600
    let text_highlight = Color::Rgb(34, 211, 238); // Cyan 400
    let text_neutral = Color::Rgb(226, 232, 240); // Slate 200
    let theme_violet = Color::Rgb(167, 139, 250); // Violet 400
    let theme_indigo = Color::Rgb(129, 140, 248); // Indigo 400

    // Render components
    let snapshot_ref = match &app.snapshot {
        Some(snap) => snap,
        None => {
            // Render loading screen if no snapshot has been received yet
            let loading = Paragraph::new("Gathering Windows telemetry. Please wait...")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)));
            frame.render_widget(loading, size);
            return;
        }
    };

    // --- 1. HEADER BANNER ---
    let uptime_str = format_uptime(snapshot_ref.uptime);
    let title_line = Line::from(vec![
        Span::styled(" WIN11 TELEMETRY SYSTEM MONITOR ", Style::default().fg(text_highlight).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" 🖥️  Host: {} ", snapshot_ref.host_name), Style::default().fg(text_neutral)),
        Span::styled(format!(" | ⚙️  OS: {} {} ({}, {} Cores) ", snapshot_ref.os_name, snapshot_ref.os_version, snapshot_ref.cpu_arch, snapshot_ref.cpu_count), Style::default().fg(text_neutral)),
        Span::styled(format!(" | 🛡️  Kernel: {} ", snapshot_ref.kernel_version), Style::default().fg(text_neutral)),
        Span::styled(format!(" | 🕒  Uptime: {} ", uptime_str), Style::default().fg(theme_violet)),
    ]);
    
    let header_widget = Paragraph::new(title_line)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)));
    frame.render_widget(header_widget, chunks[0]);

    // --- 2. CPU & MEMORY BLOCK (HORIZONTAL SPLIT) ---
    let middle_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // CPU
            Constraint::Percentage(50), // Memory
        ])
        .split(chunks[1]);

    // CPU Section
    let cpu_sub_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(45), // Global Usage gauge & Sparkline
            Constraint::Percentage(55), // Per-Core grid
        ])
        .split(middle_chunks[0]);

    let cpu_global_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Gauge
            Constraint::Min(4),    // Sparkline
        ])
        .split(cpu_sub_layout[0]);

    // Global CPU Gauge
    let cpu_pct = snapshot_ref.global_cpu_usage;
    let cpu_color = if cpu_pct < 50.0 {
        Color::Rgb(52, 211, 153) // Emerald 400
    } else if cpu_pct < 85.0 {
        Color::Rgb(251, 191, 36) // Amber 400
    } else {
        Color::Rgb(251, 113, 133) // Rose 400
    };

    let cpu_gauge = Gauge::default()
        .block(Block::default().title(" CPU Global ").title_style(Style::default().fg(theme_indigo)))
        .gauge_style(Style::default().fg(cpu_color).bg(Color::Rgb(30, 41, 59)))
        .ratio(cpu_pct as f64 / 100.0)
        .label(format!("{:.1}%", cpu_pct));
    frame.render_widget(cpu_gauge, cpu_global_layout[0]);

    // CPU History Sparkline
    let sparkline = Sparkline::default()
        .block(Block::default().title(" CPU History ").title_style(Style::default().fg(theme_indigo)))
        .style(Style::default().fg(text_highlight))
        .data(&app.cpu_history);
    frame.render_widget(sparkline, cpu_global_layout[1]);

    // CPU Per-Core grid
    let cores_widget = render_cpu_cores_table(&snapshot_ref.per_core_cpu_usage)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Logical Cores "));
    frame.render_widget(cores_widget, cpu_sub_layout[1]);

    // Memory Section
    let mem_sub_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // RAM Gauge
            Constraint::Percentage(50), // Swap Gauge
        ])
        .split(middle_chunks[1]);

    // RAM Gauge
    let ram_used = snapshot_ref.used_memory;
    let ram_total = snapshot_ref.total_memory;
    let ram_pct = if ram_total > 0 { (ram_used as f64 / ram_total as f64) * 100.0 } else { 0.0 };
    let ram_color = if ram_pct < 60.0 { Color::Rgb(52, 211, 153) } else if ram_pct < 85.0 { Color::Rgb(251, 191, 36) } else { Color::Rgb(251, 113, 133) };
    
    let ram_widget = Gauge::default()
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" RAM Util ")
            .title_style(Style::default().fg(theme_indigo)))
        .gauge_style(Style::default().fg(ram_color).bg(Color::Rgb(30, 41, 59)))
        .ratio(ram_used as f64 / ram_total.max(1) as f64)
        .label(format!("{:.1}% ({}/{})", ram_pct, format_bytes(ram_used), format_bytes(ram_total)));
    frame.render_widget(ram_widget, mem_sub_layout[0]);

    // Swap Gauge
    let swap_used = snapshot_ref.used_swap;
    let swap_total = snapshot_ref.total_swap;
    let swap_pct = if swap_total > 0 { (swap_used as f64 / swap_total as f64) * 100.0 } else { 0.0 };
    let swap_color = if swap_pct < 60.0 { Color::Rgb(52, 211, 153) } else if swap_pct < 85.0 { Color::Rgb(251, 191, 36) } else { Color::Rgb(251, 113, 133) };

    let swap_widget = Gauge::default()
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" SWAP Util ")
            .title_style(Style::default().fg(theme_indigo)))
        .gauge_style(Style::default().fg(swap_color).bg(Color::Rgb(30, 41, 59)))
        .ratio(swap_used as f64 / swap_total.max(1) as f64)
        .label(format!("{:.1}% ({}/{})", swap_pct, format_bytes(swap_used), format_bytes(swap_total)));
    frame.render_widget(swap_widget, mem_sub_layout[1]);

    // --- 3. STORAGE & NETWORK BLOCK (HORIZONTAL SPLIT) ---
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(65), // Storage
            Constraint::Percentage(35), // Network
        ])
        .split(chunks[2]);

    // Disk Storage component
    let mut disk_rows = Vec::new();
    for disk in &snapshot_ref.disks {
        let used = disk.total_space.saturating_sub(disk.available_space);
        let pct = if disk.total_space > 0 { (used as f64 / disk.total_space as f64) * 100.0 } else { 0.0 };
        let filled_bars = (pct / 10.0).round() as usize;
        let bar_graph: String = std::iter::repeat('█').take(filled_bars.min(10))
            .chain(std::iter::repeat('░').take(10 - filled_bars.min(10)))
            .collect();
        
        let row_style = Style::default().fg(text_neutral);
        disk_rows.push(Row::new(vec![
            Cell::from(disk.name.clone()),
            Cell::from(disk.mount_point.clone()),
            Cell::from(disk.file_system.clone()),
            Cell::from(format_bytes(disk.available_space)),
            Cell::from(format_bytes(disk.total_space)),
            Cell::from(format!("{} {:.1}%", bar_graph, pct)).style(Style::default().fg(if pct > 85.0 { Color::Rgb(251, 113, 133) } else { theme_indigo })),
        ]).style(row_style));
    }

    let disk_table = Table::new(
        disk_rows,
        vec![
            Constraint::Percentage(15), // Name
            Constraint::Percentage(15), // Mount
            Constraint::Percentage(15), // FS
            Constraint::Percentage(20), // Free
            Constraint::Percentage(20), // Total
            Constraint::Percentage(15), // Usage %
        ]
    )
    .header(Row::new(vec!["Drive", "Mount", "FS", "Free", "Total", "Usage (Visual)"])
        .style(Style::default().bold().fg(text_highlight)))
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Storage Devices "));
    frame.render_widget(disk_table, bottom_chunks[0]);

    // Network speeds component
    let rx_formatted = format_speed(snapshot_ref.net_rx_bytes_sec);
    let tx_formatted = format_speed(snapshot_ref.net_tx_bytes_sec);
    let rx_tot = format_bytes(snapshot_ref.net_total_rx);
    let tx_tot = format_bytes(snapshot_ref.net_total_tx);

    let net_rows = vec![
        Row::new(vec![
            Cell::from(" 📥 DL Speed:"), 
            Cell::from(rx_formatted).style(Style::default().fg(Color::Rgb(52, 211, 153)).bold())
        ]),
        Row::new(vec![
            Cell::from(" 📤 UL Speed:"), 
            Cell::from(tx_formatted).style(Style::default().fg(theme_violet).bold())
        ]),
        Row::new(vec![
            Cell::from(" 📦 Recv Cumulative:"), 
            Cell::from(rx_tot)
        ]),
        Row::new(vec![
            Cell::from(" 📤 Sent Cumulative:"), 
            Cell::from(tx_tot)
        ]),
    ];

    let net_table = Table::new(
        net_rows,
        vec![
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ]
    )
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Network (I/O) "))
    .style(Style::default().fg(text_neutral));
    frame.render_widget(net_table, bottom_chunks[1]);

    // --- 4. PROCESSES LIST TABLE ---
    let get_header_style = |col: SortColumn| {
        if app.sort_column == col {
            Style::default().bold().fg(text_highlight)
        } else {
            Style::default().bold().fg(Color::White)
        }
    };

    let dir_symbol = if app.sort_ascending { "▲" } else { "▼" };
    
    let pid_title = format!("PID {}", if app.sort_column == SortColumn::Pid { dir_symbol } else { "" });
    let name_title = format!("Process Name {}", if app.sort_column == SortColumn::Name { dir_symbol } else { "" });
    let cpu_title = format!("CPU % {}", if app.sort_column == SortColumn::Cpu { dir_symbol } else { "" });
    let mem_title = format!("Memory {}", if app.sort_column == SortColumn::Memory { dir_symbol } else { "" });

    let process_header = Row::new(vec![
        Cell::from(pid_title).style(get_header_style(SortColumn::Pid)),
        Cell::from(name_title).style(get_header_style(SortColumn::Name)),
        Cell::from(cpu_title).style(get_header_style(SortColumn::Cpu)),
        Cell::from(mem_title).style(get_header_style(SortColumn::Memory)),
    ]).style(Style::default().bg(Color::Rgb(30, 41, 59))); // Slate 800 background for header row

    // Map list of processes to rows
    let process_rows: Vec<Row> = app.sorted_processes.iter().enumerate().map(|(idx, proc)| {
        let is_selected = app.process_table_state.selected() == Some(idx);
        let row_style = if is_selected {
            Style::default().fg(Color::Black).bg(text_highlight)
        } else if idx % 2 == 0 {
            Style::default().fg(text_neutral).bg(Color::Rgb(15, 23, 42)) // Slate 900
        } else {
            Style::default().fg(text_neutral).bg(Color::Rgb(30, 41, 59)) // Slate 800
        };

        Row::new(vec![
            Cell::from(proc.pid.to_string()),
            Cell::from(proc.name.clone()),
            Cell::from(format!("{:.1}%", proc.cpu_usage)),
            Cell::from(format_bytes(proc.memory)),
        ]).style(row_style)
    }).collect();

    let procs_table = Table::new(
        process_rows,
        vec![
            Constraint::Percentage(15),
            Constraint::Percentage(45),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ]
    )
    .header(process_header)
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Active Processes Explorer "))
    .highlight_symbol("▶ ");

    // Stateful widget drawing handles internal scrolling viewports nicely
    frame.render_stateful_widget(procs_table, chunks[3], &mut app.process_table_state);

    // --- 5. FOOTER HELP CONTROLS ---
    let sort_col_name = format!("{:?}", app.sort_column);
    let footer_text = format!(
        " [q] Quit | [Tab/s] Cycle Sort | [r] Reverse Sort | [1] Sort PID | [2] Sort Name | [3] Sort CPU | [4] Sort Memory | [▲/▼/j/k] Scroll Process list (Current Sort: {} {})",
        sort_col_name,
        if app.sort_ascending { "Asc" } else { "Desc" }
    );
    let footer = Paragraph::new(Span::styled(footer_text, Style::default().fg(Color::Rgb(148, 163, 184)))); // Slate 400
    frame.render_widget(footer, chunks[4]);
}

/// Helper method to create a clean logical core table layout
fn render_cpu_cores_table(per_core_usage: &[f32]) -> Table<'_> {
    let cols = 4;
    let mut rows = Vec::new();
    
    // Chunk logical cores into rows of 4 columns
    for chunk in per_core_usage.chunks(cols) {
        let mut row_cells = Vec::new();
        for (i, &usage) in chunk.iter().enumerate() {
            let core_idx = rows.len() * cols + i;
            let filled = (usage / 20.0).round() as usize;
            let filled = filled.min(5);
            let bar: String = std::iter::repeat('█').take(filled)
                .chain(std::iter::repeat('░').take(5 - filled))
                .collect();
            
            let color = if usage < 50.0 {
                Color::Rgb(52, 211, 153) // Emerald 400 (Low usage)
            } else if usage < 85.0 {
                Color::Rgb(251, 191, 36) // Amber 400 (Mid usage)
            } else {
                Color::Rgb(251, 113, 133) // Rose 400 (High usage)
            };
            
            let cell_text = format!("C{:02}: [{}] {:3.0}%", core_idx, bar, usage);
            row_cells.push(Cell::from(cell_text).style(Style::default().fg(color)));
        }
        rows.push(Row::new(row_cells));
    }
    
    Table::new(rows, vec![
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
}

/// Utility byte conversion function
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

/// Utility throughput speed conversion function
fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

/// Utility uptime duration formatter
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
