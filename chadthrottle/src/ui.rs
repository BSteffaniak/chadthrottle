use crate::backends::BackendPriority;
use crate::backends::throttle::BackendInfo;
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
use std::collections::HashMap;

pub struct AppState {
    pub process_list: Vec<ProcessInfo>,
    pub selected_index: Option<usize>,
    pub list_state: ListState,
    pub show_help: bool,
    pub show_throttle_dialog: bool,
    pub show_backend_info: bool,
    pub throttle_dialog: ThrottleDialog,
    pub status_message: String,
    pub history: HistoryTracker,
    pub show_graph: bool,
    pub sort_frozen: bool,
    frozen_order: HashMap<i32, usize>, // PID -> position index
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
            show_backend_info: false,
            throttle_dialog: ThrottleDialog::new(),
            status_message: String::from("ChadThrottle started. Press 'h' for help."),
            sort_frozen: false,
            frozen_order: HashMap::new(),
        }
    }

    pub fn update_processes(&mut self, process_map: ProcessMap) {
        let mut processes: Vec<ProcessInfo> = process_map.into_values().collect();

        if self.sort_frozen {
            // FREEZE MODE: Sort by frozen position, new processes go to bottom
            let next_position = self.frozen_order.values().max().copied().unwrap_or(0) + 1;

            processes.sort_by(|a, b| {
                let a_pos = self
                    .frozen_order
                    .get(&a.pid)
                    .copied()
                    .unwrap_or(next_position);
                let b_pos = self
                    .frozen_order
                    .get(&b.pid)
                    .copied()
                    .unwrap_or(next_position);
                a_pos.cmp(&b_pos)
            });

            // Add any new processes to frozen_order map at the end
            let current_max = self.frozen_order.values().max().copied().unwrap_or(0);
            let mut next_pos = current_max + 1;
            for process in &processes {
                if !self.frozen_order.contains_key(&process.pid) {
                    self.frozen_order.insert(process.pid, next_pos);
                    next_pos += 1;
                }
            }
        } else {
            // NORMAL MODE: Deterministic multi-level sort to prevent UI jumping
            // Priority: terminated status -> DL rate -> total DL -> UL rate -> total UL -> throttle status -> name -> PID
            processes.sort_by(|a, b| {
                use std::cmp::Ordering;

                // 1. Terminated processes always go to bottom
                match (a.is_terminated, b.is_terminated) {
                    (true, false) => return Ordering::Greater, // a terminated, b active -> a goes after b
                    (false, true) => return Ordering::Less, // a active, b terminated -> a goes before b
                    _ => {} // Both same state, continue to next criteria
                }

                // 2. Download rate (descending - higher rates first)
                match b.download_rate.cmp(&a.download_rate) {
                    Ordering::Equal => {} // Continue to next criteria
                    other => return other,
                }

                // 3. Total download (descending - higher totals first)
                match b.total_download.cmp(&a.total_download) {
                    Ordering::Equal => {}
                    other => return other,
                }

                // 4. Upload rate (descending - higher rates first)
                match b.upload_rate.cmp(&a.upload_rate) {
                    Ordering::Equal => {}
                    other => return other,
                }

                // 5. Total upload (descending - higher totals first)
                match b.total_upload.cmp(&a.total_upload) {
                    Ordering::Equal => {}
                    other => return other,
                }

                // 6. Throttle status (throttled processes first for visibility)
                match (a.is_throttled(), b.is_throttled()) {
                    (true, false) => return Ordering::Less, // a throttled, b not -> a goes first
                    (false, true) => return Ordering::Greater, // a not throttled, b is -> b goes first
                    _ => {}                                    // Both same throttle state, continue
                }

                // 7. Process name (alphabetical)
                match a.name.cmp(&b.name) {
                    Ordering::Equal => {}
                    other => return other,
                }

                // 8. PID (ascending - smaller PIDs first for determinism)
                a.pid.cmp(&b.pid)
            });
        }

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

    /// Toggle sort freeze mode
    pub fn toggle_sort_freeze(&mut self) {
        self.sort_frozen = !self.sort_frozen;

        if self.sort_frozen {
            // Entering freeze mode - capture current order
            self.frozen_order.clear();
            for (index, process) in self.process_list.iter().enumerate() {
                self.frozen_order.insert(process.pid, index);
            }
        } else {
            // Exiting freeze mode - clear frozen order
            self.frozen_order.clear();
        }
    }
}

pub fn draw_ui(f: &mut Frame, app: &mut AppState) {
    draw_ui_with_backend_info(f, app, None);
}

pub fn draw_ui_with_backend_info(
    f: &mut Frame,
    app: &mut AppState,
    backend_info: Option<&BackendInfo>,
) {
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

    // Backend info modal (highest priority, renders on top)
    if app.show_backend_info {
        if let Some(info) = backend_info {
            draw_backend_info(f, f.area(), info);
        }
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
            // Determine status indicator: throttled (⚡), terminated (💀), or nothing
            let status_indicator = if proc.is_throttled() {
                "⚡"
            } else if proc.is_terminated {
                "💀"
            } else {
                " "
            };

            // Manual selection indicator - always present for consistent alignment
            let selection_indicator = if Some(index) == app.list_state.selected() {
                "▶ "
            } else {
                "  "
            };

            // Use gray colors for terminated processes
            let terminated_color = Color::Gray;

            let name_color = if proc.is_terminated {
                terminated_color
            } else {
                Color::White
            };
            let dl_rate_color = if proc.is_terminated {
                terminated_color
            } else {
                Color::Green
            };
            let ul_rate_color = if proc.is_terminated {
                terminated_color
            } else {
                Color::Yellow
            };
            let dl_total_color = if proc.is_terminated {
                terminated_color
            } else {
                Color::Cyan
            };
            let ul_total_color = if proc.is_terminated {
                terminated_color
            } else {
                Color::Magenta
            };
            let status_color = if proc.is_terminated {
                terminated_color
            } else {
                Color::Red
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
                    Style::default().fg(name_color),
                ),
                Span::styled(
                    format!("↓{:>10} ", ProcessInfo::format_rate(proc.download_rate)),
                    Style::default().fg(dl_rate_color),
                ),
                Span::styled(
                    format!("↑{:>10} ", ProcessInfo::format_rate(proc.upload_rate)),
                    Style::default().fg(ul_rate_color),
                ),
                Span::styled(
                    format!("{:>10} ", ProcessInfo::format_bytes(proc.total_download)),
                    Style::default().fg(dl_total_color),
                ),
                Span::styled(
                    format!("{:>10} ", ProcessInfo::format_bytes(proc.total_upload)),
                    Style::default().fg(ul_total_color),
                ),
                Span::styled(
                    status_indicator,
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
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
        Span::styled("Status", Style::default().add_modifier(Modifier::BOLD)),
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
    let title = if app.sort_frozen {
        "Network Activity [FROZEN ❄️]"
    } else {
        "Network Activity"
    };

    let border = Block::default().borders(Borders::ALL).title(title);
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

fn draw_backend_info(f: &mut Frame, area: Rect, backend_info: &BackendInfo) {
    let mut text = vec![Line::from("")];

    // Upload Backends Section
    text.push(Line::from(Span::styled(
        "Upload Backends:",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    if backend_info.available_upload.is_empty() {
        text.push(Line::from(Span::styled(
            "  ⚪ (none compiled)",
            Style::default().fg(Color::Gray),
        )));
    } else {
        for (name, priority, available) in &backend_info.available_upload {
            let is_active = backend_info.active_upload.as_ref() == Some(name);
            let (symbol, color) = if is_active {
                ("✅", Color::Green)
            } else if *available {
                ("✅", Color::Green)
            } else {
                ("❌", Color::Red)
            };

            let status = if is_active {
                "[ACTIVE]"
            } else if *available {
                "Available"
            } else {
                "Unavailable"
            };

            let priority_str = format!("{:?}", priority);

            text.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(symbol, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(
                    format!("{:15}", name),
                    if is_active {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:12}", status),
                    Style::default().fg(if is_active {
                        Color::Yellow
                    } else {
                        Color::Gray
                    }),
                ),
                Span::raw("  Priority: "),
                Span::styled(priority_str, Style::default().fg(Color::Cyan)),
            ]));
        }
    }

    text.push(Line::from(""));

    // Download Backends Section
    text.push(Line::from(Span::styled(
        "Download Backends:",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    if backend_info.available_download.is_empty() {
        text.push(Line::from(Span::styled(
            "  ⚪ (none compiled)",
            Style::default().fg(Color::Gray),
        )));
    } else {
        for (name, priority, available) in &backend_info.available_download {
            let is_active = backend_info.active_download.as_ref() == Some(name);
            let (symbol, color) = if is_active {
                ("✅", Color::Green)
            } else if *available {
                ("✅", Color::Green)
            } else {
                ("❌", Color::Red)
            };

            let status = if is_active {
                "[ACTIVE]"
            } else if *available {
                "Available"
            } else {
                "Unavailable"
            };

            let priority_str = format!("{:?}", priority);

            text.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(symbol, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(
                    format!("{:15}", name),
                    if is_active {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:12}", status),
                    Style::default().fg(if is_active {
                        Color::Yellow
                    } else {
                        Color::Gray
                    }),
                ),
                Span::raw("  Priority: "),
                Span::styled(priority_str, Style::default().fg(Color::Cyan)),
            ]));
        }
    }

    text.push(Line::from(""));

    // Configuration Section
    text.push(Line::from(Span::styled(
        "Configuration:",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    let preferred_upload_display = backend_info
        .preferred_upload
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("Auto");
    let active_upload_display = backend_info
        .active_upload
        .as_ref()
        .map(|s| format!(" ({} selected)", s))
        .unwrap_or_else(|| " (none available)".to_string());

    text.push(Line::from(vec![
        Span::raw("  Preferred Upload:     "),
        Span::styled(
            format!("{}{}", preferred_upload_display, active_upload_display),
            Style::default().fg(Color::White),
        ),
    ]));

    let preferred_download_display = backend_info
        .preferred_download
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("Auto");
    let active_download_display = backend_info
        .active_download
        .as_ref()
        .map(|s| format!(" ({} selected)", s))
        .unwrap_or_else(|| " (none available)".to_string());

    text.push(Line::from(vec![
        Span::raw("  Preferred Download:   "),
        Span::styled(
            format!("{}{}", preferred_download_display, active_download_display),
            Style::default().fg(Color::White),
        ),
    ]));

    text.push(Line::from(vec![
        Span::raw("  Config File:          "),
        Span::styled(
            "~/.config/chadthrottle/throttles.json",
            Style::default().fg(Color::Gray),
        ),
    ]));

    text.push(Line::from(""));

    // Capabilities Section (only if we have active backends)
    if backend_info.upload_capabilities.is_some() || backend_info.download_capabilities.is_some() {
        text.push(Line::from(Span::styled(
            "Capabilities (Active Backends):",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        // Get capabilities - prefer upload, fall back to download
        let caps = backend_info
            .upload_capabilities
            .as_ref()
            .or(backend_info.download_capabilities.as_ref());

        if let Some(capabilities) = caps {
            text.push(Line::from(vec![
                Span::raw("  IPv4:              "),
                Span::styled(
                    if capabilities.ipv4_support {
                        "✅"
                    } else {
                        "❌"
                    },
                    Style::default().fg(if capabilities.ipv4_support {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
                Span::raw("   IPv6:            "),
                Span::styled(
                    if capabilities.ipv6_support {
                        "✅"
                    } else {
                        "❌"
                    },
                    Style::default().fg(if capabilities.ipv6_support {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
            ]));

            text.push(Line::from(vec![
                Span::raw("  Per-Process:       "),
                Span::styled(
                    if capabilities.per_process {
                        "✅"
                    } else {
                        "❌"
                    },
                    Style::default().fg(if capabilities.per_process {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
                Span::raw("   Per-Connection:  "),
                Span::styled(
                    if capabilities.per_connection {
                        "✅"
                    } else {
                        "❌"
                    },
                    Style::default().fg(if capabilities.per_connection {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
            ]));
        }

        text.push(Line::from(""));
    }

    // Footer
    text.push(Line::from(""));
    text.push(Line::from(Span::styled(
        "Press 'b' or 'Esc' to close",
        Style::default().fg(Color::DarkGray),
    )));

    let backend_widget = Paragraph::new(text)
        .style(Style::default().bg(Color::Black).fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("ChadThrottle - Backends")
                .style(Style::default().fg(Color::Cyan)),
        );

    let popup_area = centered_rect(70, 80, area);
    f.render_widget(Clear, popup_area);
    f.render_widget(backend_widget, popup_area);
}
