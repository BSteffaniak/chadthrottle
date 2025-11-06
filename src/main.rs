mod backends;
mod config;
mod history;
mod keybindings;
mod monitor;
mod process;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;
use tokio::time::interval;

use crate::backends::throttle::ThrottleManager;
use crate::backends::throttle::{
    detect_download_backends, detect_upload_backends, select_download_backend,
    select_upload_backend,
};
use crate::monitor::NetworkMonitor;
use crate::process::ThrottleLimit;
use crate::ui::AppState;

/// ChadThrottle - A TUI network monitor and throttler for Linux
#[derive(Parser, Debug)]
#[command(name = "chadthrottle")]
#[command(version = "0.6.0")]
#[command(about = "Network monitor and throttler - like NetLimiter but chad", long_about = None)]
struct Args {
    /// Upload throttling backend to use
    #[arg(long, value_name = "BACKEND")]
    upload_backend: Option<String>,

    /// Download throttling backend to use
    #[arg(long, value_name = "BACKEND")]
    download_backend: Option<String>,

    /// List all available backends and exit
    #[arg(long)]
    list_backends: bool,

    /// Auto-restore saved throttles on startup
    #[arg(long)]
    restore: bool,

    /// Don't save throttles on exit
    #[arg(long)]
    no_save: bool,
}

fn print_available_backends() {
    println!("ChadThrottle v0.6.0 - Available Backends\n");

    // Upload backends
    println!("Upload Backends:");
    let upload_backends = detect_upload_backends();
    if upload_backends.is_empty() {
        println!("  (none compiled in)");
    } else {
        for backend in upload_backends {
            let status = if backend.available {
                "✅ available"
            } else {
                "❌ unavailable"
            };
            println!(
                "  {:20} [priority: {:?}] {}",
                backend.name, backend.priority, status
            );
        }
    }

    println!();

    // Download backends
    println!("Download Backends:");
    let download_backends = detect_download_backends();
    if download_backends.is_empty() {
        println!("  (none compiled in)");
    } else {
        for backend in download_backends {
            let status = if backend.available {
                "✅ available"
            } else {
                "❌ unavailable"
            };
            println!(
                "  {:20} [priority: {:?}] {}",
                backend.name, backend.priority, status
            );
        }
    }

    println!();
    println!("Usage:");
    println!("  chadthrottle --upload-backend <name> --download-backend <name>");
    println!("  chadthrottle  (auto-selects best available backends)");
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    if std::env::var("RUST_LOG").is_ok() {
        pretty_env_logger::formatted_builder()
            .parse_default_env()
            .init();
    }

    // Handle --list-backends
    if args.list_backends {
        print_available_backends();
        return Ok(());
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::new();

    // Select and create backends
    let upload_backend = select_upload_backend(args.upload_backend.as_deref());
    let download_backend = select_download_backend(args.download_backend.as_deref());

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

    // Load and optionally restore saved config
    let mut config = config::Config::load().unwrap_or_default();
    if args.restore {
        log::info!("Restoring saved throttles...");
        for (pid, saved_throttle) in config.get_throttles() {
            let limit = ThrottleLimit {
                upload_limit: saved_throttle.upload_limit,
                download_limit: saved_throttle.download_limit,
            };
            if let Err(e) =
                throttle_manager.throttle_process(*pid, saved_throttle.process_name.clone(), &limit)
            {
                log::warn!("Failed to restore throttle for PID {}: {}", pid, e);
            } else {
                log::info!(
                    "Restored throttle for {} (PID {})",
                    saved_throttle.process_name,
                    pid
                );
            }
        }
    }

    // Run the app
    let res = run_app(&mut terminal, &mut app, &mut monitor, &mut throttle_manager).await;

    // Save config before exit (unless --no-save specified)
    if !args.no_save {
        config.clear_throttles();
        for (pid, throttle) in throttle_manager.get_all_throttles() {
            config.set_throttle(
                pid,
                config::SavedThrottle {
                    process_name: throttle.process_name,
                    upload_limit: throttle.upload_limit,
                    download_limit: throttle.download_limit,
                },
            );
        }
        if let Err(e) = config.save() {
            log::warn!("Failed to save config: {}", e);
        } else {
            log::info!(
                "Saved {} throttle(s) to config",
                config.get_throttles().len()
            );
        }
    }

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

                // Check for Ctrl+C first
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        return Ok(());
                    }
                    KeyCode::Char('h') | KeyCode::Char('?') => {
                        app.show_help = true;
                    }
                    KeyCode::Char('g') => {
                        app.show_graph = !app.show_graph;
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
                    // Update throttle status and history for each process
                    for (pid, process_info) in process_map.iter_mut() {
                        if let Some(throttle) = throttle_manager.get_throttle(*pid) {
                            process_info.throttle_limit = Some(crate::process::ThrottleLimit {
                                download_limit: throttle.download_limit,
                                upload_limit: throttle.upload_limit,
                            });
                        }

                        // Track bandwidth history
                        app.history.update(
                            *pid,
                            process_info.name.clone(),
                            process_info.download_rate,
                            process_info.upload_rate,
                        );
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
