use crate::history::HistoryTracker;
use crate::process::{ProcessInfo, ProcessMap};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Clear, Dataset, GraphType, List, ListItem, ListState,
        Paragraph,
    },
};

pub struct AppState {
    pub process_list: Vec<ProcessInfo>,
    pub selected_index: Option<usize>,
    pub list_state: ListState,
    pub show_help: bool,
    pub show_throttle_dialog: bool,
    pub throttle_dialog: ThrottleDialog,
    pub status_message: String,
    pub history: HistoryTracker,
    pub show_graph: bool,
}

pub struct ThrottleDialog {
    pub download_input: String,
    pub upload_input: String,
    pub selected_field: ThrottleField,
    pub target_pid: Option<i32>,
    pub target_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThrottleField {
    Download,
    Upload,
}

impl ThrottleDialog {
    pub fn new() -> Self {
        Self {
            download_input: String::new(),
            upload_input: String::new(),
            selected_field: ThrottleField::Download,
            target_pid: None,
            target_name: None,
        }
    }

    pub fn reset(&mut self) {
        self.download_input.clear();
        self.upload_input.clear();
        self.selected_field = ThrottleField::Download;
        self.target_pid = None;
        self.target_name = None;
    }

    pub fn handle_char(&mut self, c: char) {
        match self.selected_field {
            ThrottleField::Download => self.download_input.push(c),
            ThrottleField::Upload => self.upload_input.push(c),
        }
    }

    pub fn handle_backspace(&mut self) {
        match self.selected_field {
            ThrottleField::Download => {
                self.download_input.pop();
            }
            ThrottleField::Upload => {
                self.upload_input.pop();
            }
        }
    }

    pub fn toggle_field(&mut self) {
        self.selected_field = match self.selected_field {
            ThrottleField::Download => ThrottleField::Upload,
            ThrottleField::Upload => ThrottleField::Download,
        };
    }

    pub fn parse_limits(&self) -> Option<(Option<u64>, Option<u64>)> {
        // Parse KB/s to bytes/sec
        let download = if self.download_input.is_empty() {
            None
        } else {
            self.download_input.parse::<u64>().ok().map(|kb| kb * 1024)
        };

        let upload = if self.upload_input.is_empty() {
            None
        } else {
            self.upload_input.parse::<u64>().ok().map(|kb| kb * 1024)
        };

        Some((download, upload))
    }
}

impl AppState {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(None); // Nothing selected initially

        Self {
            process_list: Vec::new(),
            selected_index: None, // Nothing selected initially
            list_state,
            history: HistoryTracker::new(),
            show_graph: false,
            show_help: false,
            show_throttle_dialog: false,
            throttle_dialog: ThrottleDialog::new(),
            status_message: String::from("ChadThrottle started. Press 'h' for help."),
        }
    }

    pub fn update_processes(&mut self, process_map: ProcessMap) {
        let mut processes: Vec<ProcessInfo> = process_map.into_values().collect();

        // Sort by total bandwidth (download + upload)
        processes.sort_by(|a, b| {
            let a_total = a.download_rate + a.upload_rate;
            let b_total = b.download_rate + b.upload_rate;
            b_total.cmp(&a_total)
        });

        self.process_list = processes;

        // Adjust selection if out of bounds
        if let Some(index) = self.selected_index {
            if index >= self.process_list.len() && !self.process_list.is_empty() {
                self.selected_index = Some(self.process_list.len() - 1);
                self.list_state.select(Some(self.process_list.len() - 1));
            }
        }
    }

    pub fn select_next(&mut self) {
        if self.process_list.is_empty() {
            return;
        }

        let new_index = match self.selected_index {
            None => 0, // If nothing selected, select first item
            Some(idx) => (idx + 1) % self.process_list.len(),
        };

        self.selected_index = Some(new_index);
        self.list_state.select(Some(new_index));
    }

    pub fn select_previous(&mut self) {
        if self.process_list.is_empty() {
            return;
        }

        let new_index = match self.selected_index {
            None => 0, // If nothing selected, select first item
            Some(0) => self.process_list.len() - 1,
            Some(idx) => idx - 1,
        };

        self.selected_index = Some(new_index);
        self.list_state.select(Some(new_index));
    }

    pub fn get_selected_process(&self) -> Option<&ProcessInfo> {
        self.selected_index
            .and_then(|idx| self.process_list.get(idx))
    }
}

pub fn draw_ui(f: &mut Frame, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Process list
            Constraint::Length(3), // Status bar
        ])
        .split(f.area());

    // Header
    draw_header(f, chunks[0]);

    // Process list
    draw_process_list(f, chunks[1], app);

    // Status bar
    draw_status_bar(f, chunks[2], app);

    // Help overlay
    if app.show_help {
        draw_help_overlay(f, f.area());
    }

    // Throttle dialog
    if app.show_throttle_dialog {
        draw_throttle_dialog(f, f.area(), app);
    }

    // Bandwidth graph overlay
    if app.show_graph {
        draw_bandwidth_graph(f, f.area(), app);
    }
}

fn draw_header(f: &mut Frame, area: Rect) {
    let header = Paragraph::new("🔥 ChadThrottle v0.1.0 - Network Monitor & Throttler 🔥")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(header, area);
}

fn draw_process_list(f: &mut Frame, area: Rect, app: &mut AppState) {
    let items: Vec<ListItem> = app
        .process_list
        .iter()
        .enumerate()
        .map(|(index, proc)| {
            let throttle_indicator = if proc.is_throttled() { "⚡" } else { " " };

            // Manual selection indicator - always present for consistent alignment
            let selection_indicator = if Some(index) == app.list_state.selected() {
                "▶ "
            } else {
                "  "
            };

            let content = Line::from(vec![
                Span::styled(selection_indicator, Style::default().fg(Color::Yellow)),
                Span::raw(format!("{:7} ", proc.pid)),
                Span::styled(
                    format!(
                        "{:20} ",
                        if proc.name.len() > 20 {
                            format!("{}...", &proc.name[..17])
                        } else {
                            proc.name.clone()
                        }
                    ),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("↓{:>10} ", ProcessInfo::format_rate(proc.download_rate)),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("↑{:>10} ", ProcessInfo::format_rate(proc.upload_rate)),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:>10} ", ProcessInfo::format_bytes(proc.total_download)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:>10} ", ProcessInfo::format_bytes(proc.total_upload)),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(
                    throttle_indicator,
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]);

            ListItem::new(content)
        })
        .collect();

    let header = Line::from(vec![
        Span::styled("PID     ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            "Process              ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled("DL Rate    ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("UL Rate    ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("Total DL   ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("Total UL   ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("Throttled", Style::default().add_modifier(Modifier::BOLD)),
    ]);

    // Split the area: header takes first row inside border, list gets the rest
    let header_area = Rect {
        x: area.x + 4, // +2 for border, +2 for manual selection indicator
        y: area.y + 1,
        width: area.width - 4,
        height: 1,
    };

    let list_area = Rect {
        x: area.x,
        y: area.y + 2, // Start below the header
        width: area.width,
        height: area.height.saturating_sub(2),
    };

    // Render the border and title separately
    let border = Block::default()
        .borders(Borders::ALL)
        .title("Network Activity");
    f.render_widget(border, area);

    // Render header
    f.render_widget(Paragraph::new(header), header_area);

    // Render list without its own border (since we drew it above)
    let list = List::new(items).highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    // Adjust list_state rendering to account for the inner area (inside borders)
    let inner_list_area = Rect {
        x: list_area.x + 1,
        y: list_area.y,
        width: list_area.width.saturating_sub(2),
        height: list_area.height.saturating_sub(1),
    };

    f.render_stateful_widget(list, inner_list_area, &mut app.list_state);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &AppState) {
    // Auto-generate status bar from centralized keybindings
    let mut spans = vec![];

    for (i, (key, description)) in crate::keybindings::get_status_bar_keybindings()
        .iter()
        .enumerate()
    {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!("[{}]", key),
            Style::default().fg(Color::Yellow),
        ));
        spans.push(Span::raw(format!(" {}  ", description)));
    }

    spans.push(Span::raw("|  "));
    spans.push(Span::styled(
        &app.status_message,
        Style::default().fg(Color::Gray),
    ));

    let status =
        Paragraph::new(vec![Line::from(spans)]).block(Block::default().borders(Borders::ALL));

    f.render_widget(status, area);
}

fn draw_help_overlay(f: &mut Frame, area: Rect) {
    // Auto-generate help text from centralized keybindings
    let mut help_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "ChadThrottle - Keyboard Shortcuts",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    // Get all keybindings and generate help lines
    for binding in crate::keybindings::get_all_keybindings() {
        help_text.push(Line::from(format!(
            "  {:12} - {}",
            binding.key, binding.description
        )));
    }

    help_text.push(Line::from(""));
    help_text.push(Line::from("Press any key to close..."));

    let help = Paragraph::new(help_text)
        .style(Style::default().bg(Color::Black).fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .style(Style::default().fg(Color::Cyan)),
        );

    let help_area = centered_rect(60, 50, area);
    f.render_widget(ratatui::widgets::Clear, help_area);
    f.render_widget(help, help_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn draw_throttle_dialog(f: &mut Frame, area: Rect, app: &AppState) {
    let dialog = &app.throttle_dialog;

    let title = if let (Some(pid), Some(name)) = (dialog.target_pid, &dialog.target_name) {
        format!("Throttle: {} (PID {})", name, pid)
    } else {
        "Throttle Process".to_string()
    };

    let download_style = if dialog.selected_field == ThrottleField::Download {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let upload_style = if dialog.selected_field == ThrottleField::Upload {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let dialog_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Download Limit (KB/s): ", download_style),
            Span::styled(
                if dialog.download_input.is_empty() {
                    "unlimited"
                } else {
                    &dialog.download_input
                },
                download_style,
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Upload Limit (KB/s):   ", upload_style),
            Span::styled(
                if dialog.upload_input.is_empty() {
                    "unlimited"
                } else {
                    &dialog.upload_input
                },
                upload_style,
            ),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "[Tab] Switch field  [Enter] Apply  [Esc] Cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let dialog_widget = Paragraph::new(dialog_text)
        .style(Style::default().bg(Color::Black).fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().fg(Color::Cyan)),
        );

    let dialog_area = centered_rect(60, 30, area);
    f.render_widget(Clear, dialog_area);
    f.render_widget(dialog_widget, dialog_area);
}

fn draw_bandwidth_graph(f: &mut Frame, area: Rect, app: &AppState) {
    // Get selected process
    let selected_proc = app.get_selected_process();
    if selected_proc.is_none() {
        return;
    }

    let proc = selected_proc.unwrap();
    let history = app.history.get_history(proc.pid);

    if history.is_none() || history.unwrap().samples.is_empty() {
        // No history data available
        let no_data = Paragraph::new("No historical data available yet...")
            .style(Style::default().bg(Color::Black).fg(Color::Yellow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Bandwidth Graph: {} (PID {})", proc.name, proc.pid))
                    .style(Style::default().fg(Color::Cyan)),
            );

        let graph_area = centered_rect(80, 60, area);
        f.render_widget(Clear, graph_area);
        f.render_widget(no_data, graph_area);
        return;
    }

    let history = history.unwrap();
    let (download_data, upload_data) = history.get_graph_data();

    // Find max values for scaling
    let max_download = history.max_download_rate() as f64;
    let max_upload = history.max_upload_rate() as f64;
    let max_value = max_download.max(max_upload).max(1.0); // Avoid division by zero

    // Create datasets
    let datasets = vec![
        Dataset::default()
            .name("Download")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Green))
            .data(&download_data),
        Dataset::default()
            .name("Upload")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Yellow))
            .data(&upload_data),
    ];

    // Create chart
    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    "Bandwidth Graph: {} (PID {}) | Max: ↓{} ↑{} | Avg: ↓{} ↑{}",
                    proc.name,
                    proc.pid,
                    ProcessInfo::format_rate(history.max_download_rate()),
                    ProcessInfo::format_rate(history.max_upload_rate()),
                    ProcessInfo::format_rate(history.avg_download_rate()),
                    ProcessInfo::format_rate(history.avg_upload_rate()),
                ))
                .style(Style::default().fg(Color::Cyan)),
        )
        .x_axis(
            Axis::default()
                .title("Time (samples)")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, 60.0]),
        )
        .y_axis(
            Axis::default()
                .title("Bandwidth (bytes/s)")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_value * 1.1]), // Add 10% headroom
        );

    let graph_area = centered_rect(90, 70, area);
    f.render_widget(Clear, graph_area);
    f.render_widget(chart, graph_area);

    // Draw instructions at bottom
    let instructions = Paragraph::new("Press 'g' to close graph")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Center);

    let inst_area = Rect {
        x: graph_area.x,
        y: graph_area.y + graph_area.height - 2,
        width: graph_area.width,
        height: 1,
    };
    f.render_widget(instructions, inst_area);
}
