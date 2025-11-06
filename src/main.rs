mod backends;
mod monitor;
mod process;
mod throttle;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;
use tokio::time::interval;

use crate::backends::throttle::ThrottleManager;
use crate::backends::throttle::{select_download_backend, select_upload_backend};
use crate::monitor::NetworkMonitor;
use crate::ui::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_ok() {
        pretty_env_logger::formatted_builder()
            .parse_default_env()
            .init();
    }
    // Throttling uses cgroups + tc, no external dependencies needed

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::new();

    // Select and create backends
    let upload_backend = select_upload_backend(None);
    let download_backend = select_download_backend(None);

    // Show backend status
    log::error!("🔥 ChadThrottle v0.6.0 - Backend Status:");
    log::error!("");

    if let Some(ref backend) = upload_backend {
        log::error!("  ✅ Upload throttling:   {} (available)", backend.name());
    } else {
        log::error!("  ⚠️  Upload throttling:   Not available");
        log::error!("      → Install 'tc' (traffic control) and enable cgroups");
    }

    if let Some(ref backend) = download_backend {
        log::error!("  ✅ Download throttling: {} (available)", backend.name());
    } else {
        log::error!("  ⚠️  Download throttling: Not available");
        log::error!("      → Enable 'ifb' kernel module (see IFB_SETUP.md)");
    }

    log::error!("");

    if upload_backend.is_none() && download_backend.is_none() {
        log::error!("⚠️  Warning: No throttling backends available!");
        log::error!(
            "   Network monitoring will work, but you won't be able to throttle processes."
        );
        log::error!("");
    }

    // Create managers with selected backends
    let mut throttle_manager = ThrottleManager::new(upload_backend, download_backend);
    let mut monitor = NetworkMonitor::new()?;

    // Run the app
    let res = run_app(&mut terminal, &mut app, &mut monitor, &mut throttle_manager).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        log::error!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut AppState,
    monitor: &mut NetworkMonitor,
    throttle_manager: &mut ThrottleManager,
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

                // Handle throttle dialog input
                if app.show_throttle_dialog {
                    match key.code {
                        KeyCode::Esc => {
                            app.show_throttle_dialog = false;
                            app.throttle_dialog.reset();
                        }
                        KeyCode::Tab => {
                            app.throttle_dialog.toggle_field();
                        }
                        KeyCode::Char(c) if c.is_numeric() => {
                            app.throttle_dialog.handle_char(c);
                        }
                        KeyCode::Backspace => {
                            app.throttle_dialog.handle_backspace();
                        }
                        KeyCode::Enter => {
                            // Apply throttle
                            if let Some((download, upload)) = app.throttle_dialog.parse_limits() {
                                if let Some(pid) = app.throttle_dialog.target_pid {
                                    let process_name =
                                        app.throttle_dialog.target_name.clone().unwrap_or_default();
                                    let limit = crate::process::ThrottleLimit {
                                        download_limit: download,
                                        upload_limit: upload,
                                    };

                                    match throttle_manager.throttle_process(
                                        pid,
                                        process_name.clone(),
                                        &limit,
                                    ) {
                                        Ok(_) => {
                                            app.status_message = format!(
                                                "Throttle applied to {} (PID {})",
                                                process_name, pid
                                            );
                                        }
                                        Err(e) => {
                                            app.status_message =
                                                format!("Failed to apply throttle: {}", e);
                                        }
                                    }
                                }
                            }
                            app.show_throttle_dialog = false;
                            app.throttle_dialog.reset();
                        }
                        _ => {}
                    }
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
                            // Clone the values we need
                            let pid = process.pid;
                            let name = process.name.clone();

                            // Open throttle dialog
                            app.throttle_dialog.target_pid = Some(pid);
                            app.throttle_dialog.target_name = Some(name);
                            app.show_throttle_dialog = true;
                        } else {
                            app.status_message = "No process selected".to_string();
                        }
                    }
                    KeyCode::Char('r') => {
                        if let Some(process) = app.get_selected_process() {
                            // Remove throttle
                            match throttle_manager.remove_throttle(process.pid) {
                                Ok(_) => {
                                    app.status_message = format!(
                                        "Throttle removed from {} (PID {})",
                                        process.name, process.pid
                                    );
                                }
                                Err(e) => {
                                    app.status_message =
                                        format!("Failed to remove throttle: {}", e);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Update network stats periodically
        if tokio::time::timeout(Duration::from_millis(1), update_interval.tick())
            .await
            .is_ok()
        {
            match monitor.update() {
                Ok(mut process_map) => {
                    // Update throttle status for each process
                    for (pid, process_info) in process_map.iter_mut() {
                        if let Some(throttle) = throttle_manager.get_throttle(*pid) {
                            process_info.throttle_limit = Some(crate::process::ThrottleLimit {
                                download_limit: throttle.download_limit,
                                upload_limit: throttle.upload_limit,
                            });
                        }
                    }

                    app.update_processes(process_map);
                    // Update status with process count
                    if !app.status_message.starts_with("Throttle")
                        && !app.status_message.starts_with("Failed")
                    {
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
