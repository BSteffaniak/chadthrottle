use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use crate::process::{ProcessInfo, ProcessMap};

pub struct AppState {
    pub process_list: Vec<ProcessInfo>,
    pub selected_index: usize,
    pub list_state: ListState,
    pub show_help: bool,
    pub status_message: String,
}

impl AppState {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        Self {
            process_list: Vec::new(),
            selected_index: 0,
            list_state,
            show_help: false,
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
        if self.selected_index >= self.process_list.len() && !self.process_list.is_empty() {
            self.selected_index = self.process_list.len() - 1;
            self.list_state.select(Some(self.selected_index));
        }
    }

    pub fn select_next(&mut self) {
        if self.process_list.is_empty() {
            return;
        }
        
        self.selected_index = (self.selected_index + 1) % self.process_list.len();
        self.list_state.select(Some(self.selected_index));
    }

    pub fn select_previous(&mut self) {
        if self.process_list.is_empty() {
            return;
        }
        
        if self.selected_index == 0 {
            self.selected_index = self.process_list.len() - 1;
        } else {
            self.selected_index -= 1;
        }
        self.list_state.select(Some(self.selected_index));
    }

    pub fn get_selected_process(&self) -> Option<&ProcessInfo> {
        self.process_list.get(self.selected_index)
    }
}

pub fn draw_ui(f: &mut Frame, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),    // Process list
            Constraint::Length(3),  // Status bar
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
}

fn draw_header(f: &mut Frame, area: Rect) {
    let header = Paragraph::new("🔥 ChadThrottle v0.1.0 - Network Monitor & Throttler 🔥")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));
    
    f.render_widget(header, area);
}

fn draw_process_list(f: &mut Frame, area: Rect, app: &mut AppState) {
    let items: Vec<ListItem> = app.process_list.iter().map(|proc| {
        let throttle_indicator = if proc.is_throttled() {
            "⚡"
        } else {
            " "
        };
        
        let content = Line::from(vec![
            Span::raw(format!("{:6} ", proc.pid)),
            Span::styled(
                format!("{:20} ", 
                    if proc.name.len() > 20 {
                        format!("{}...", &proc.name[..17])
                    } else {
                        proc.name.clone()
                    }
                ),
                Style::default().fg(Color::White)
            ),
            Span::styled(
                format!("↓{:>10} ", ProcessInfo::format_rate(proc.download_rate)),
                Style::default().fg(Color::Green)
            ),
            Span::styled(
                format!("↑{:>10} ", ProcessInfo::format_rate(proc.upload_rate)),
                Style::default().fg(Color::Yellow)
            ),
            Span::styled(
                throttle_indicator,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            ),
        ]);
        
        ListItem::new(content)
    }).collect();

    let header = Line::from(vec![
        Span::styled("PID    ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("Process              ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("Download   ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("Upload     ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("T", Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Network Activity")
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        )
        .highlight_symbol("▶ ");

    // Render header separately
    let header_area = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width - 4,
        height: 1,
    };
    f.render_widget(Paragraph::new(header), header_area);

    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &AppState) {
    let status = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("[↑↓]", Style::default().fg(Color::Yellow)),
            Span::raw(" Navigate  "),
            Span::styled("[t]", Style::default().fg(Color::Yellow)),
            Span::raw(" Throttle  "),
            Span::styled("[h]", Style::default().fg(Color::Yellow)),
            Span::raw(" Help  "),
            Span::styled("[q]", Style::default().fg(Color::Yellow)),
            Span::raw(" Quit  |  "),
            Span::styled(&app.status_message, Style::default().fg(Color::Gray)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL));

    f.render_widget(status, area);
}

fn draw_help_overlay(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(""),
        Line::from(Span::styled("ChadThrottle - Keyboard Shortcuts", Style::default().add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from("  ↑/k         - Move selection up"),
        Line::from("  ↓/j         - Move selection down"),
        Line::from("  t           - Throttle selected process"),
        Line::from("  r           - Remove throttle"),
        Line::from("  l           - Launch process with throttle"),
        Line::from("  h/?         - Toggle this help"),
        Line::from("  q/Esc       - Quit"),
        Line::from(""),
        Line::from("Press any key to close..."),
    ];

    let help = Paragraph::new(help_text)
        .style(Style::default().bg(Color::Black).fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .style(Style::default().fg(Color::Cyan))
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
