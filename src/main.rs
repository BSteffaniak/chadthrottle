mod process;
mod monitor;
mod throttle;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use std::time::Duration;
use tokio::time::interval;

use crate::monitor::NetworkMonitor;
use crate::throttle::ThrottleManager;
use crate::ui::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    // Check if trickle is available
    if !ThrottleManager::check_trickle_available() {
        eprintln!("Warning: 'trickle' not found. Throttling features will be limited.");
        eprintln!("Install trickle with: sudo apt install trickle  (or your distro's package manager)");
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::new();
    let mut monitor = NetworkMonitor::new()?;
    let _throttle_manager = ThrottleManager::new();

    // Run the app
    let res = run_app(&mut terminal, &mut app, &mut monitor).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut AppState,
    monitor: &mut NetworkMonitor,
) -> Result<()> {
    let mut update_interval = interval(Duration::from_secs(1));

    loop {
        // Draw UI
        terminal.draw(|f| ui::draw_ui(f, app))?;

        // Handle input with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // If help is shown, any key closes it
                if app.show_help {
                    app.show_help = false;
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        return Ok(());
                    }
                    KeyCode::Char('h') | KeyCode::Char('?') => {
                        app.show_help = true;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        app.select_next();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.select_previous();
                    }
                    KeyCode::Char('t') => {
                        if let Some(process) = app.get_selected_process() {
                            app.status_message = format!(
                                "Throttling not yet implemented for PID {} ({})",
                                process.pid, process.name
                            );
                        } else {
                            app.status_message = "No process selected".to_string();
                        }
                    }
                    KeyCode::Char('l') => {
                        app.status_message = 
                            "Launch with throttle not yet implemented. Use CLI for now.".to_string();
                    }
                    KeyCode::Char('r') => {
                        if let Some(process) = app.get_selected_process() {
                            app.status_message = format!(
                                "Remove throttle not yet implemented for PID {}",
                                process.pid
                            );
                        }
                    }
                    _ => {}
                }
            }
        }

        // Update network stats periodically
        if tokio::time::timeout(Duration::from_millis(1), update_interval.tick()).await.is_ok() {
            match monitor.update() {
                Ok(process_map) => {
                    app.update_processes(process_map);
                    // Update status with process count
                    if !app.status_message.starts_with("Throttling") 
                        && !app.status_message.starts_with("Launch")
                        && !app.status_message.starts_with("Remove") {
                        app.status_message = format!(
                            "Monitoring {} process(es) with network activity",
                            app.process_list.len()
                        );
                    }
                }
                Err(e) => {
                    app.status_message = format!("Error updating: {}", e);
                }
            }
        }
    }
}
