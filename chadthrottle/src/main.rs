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

/// Format bytes as human-readable string (e.g., "1.5 MB", "500 KB")
fn human_readable(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

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

    /// Don't restore saved throttles on startup (default: restore is enabled)
    #[arg(long)]
    no_restore: bool,

    /// Don't save throttles on exit
    #[arg(long)]
    no_save: bool,

    // CLI mode arguments
    /// PID to throttle (CLI mode - skips TUI)
    #[arg(long, value_name = "PID")]
    pid: Option<i32>,

    /// Download limit (e.g., "1M", "500K", "1.5M") - requires --pid
    #[arg(long, value_name = "LIMIT")]
    download_limit: Option<String>,

    /// Upload limit (e.g., "1M", "500K", "1.5M") - requires --pid
    #[arg(long, value_name = "LIMIT")]
    upload_limit: Option<String>,

    /// Duration to run throttle in seconds (default: run until Ctrl+C)
    #[arg(long, value_name = "SECONDS")]
    duration: Option<u64>,

    /// BPF attach method: auto (try link, fallback to legacy), link (bpf_link_create), legacy (bpf_prog_attach)
    #[arg(long, value_name = "METHOD")]
    bpf_attach_method: Option<String>,
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
    println!("  TUI Mode:");
    println!("    chadthrottle [--upload-backend <name>] [--download-backend <name>]");
    println!();
    println!("  CLI Mode:");
    println!(
        "    chadthrottle --pid <PID> [--download-limit <LIMIT>] [--upload-limit <LIMIT>] [--duration <SECONDS>]"
    );
    println!("    Examples:");
    println!("      chadthrottle --pid 1234 --download-limit 1M --upload-limit 500K");
    println!("      chadthrottle --pid 1234 --download-limit 1.5M --duration 60");
    println!();
    println!("  BPF Options:");
    println!(
        "    --bpf-attach-method <METHOD>   BPF attach method: auto, link, legacy (default: auto)"
    );
    println!("      auto   - Try modern method, fallback to legacy on error");
    println!("      link   - Use bpf_link_create only");
    println!("      legacy - Use bpf_prog_attach only");
}

/// Parse bandwidth limit string (e.g., "1M", "500K", "1.5M") to bytes per second
fn parse_bandwidth_limit(limit_str: &str) -> Result<u64> {
    let limit_str = limit_str.trim().to_uppercase();

    // Try to split into number and unit
    let (num_str, unit) = if limit_str.ends_with("M") || limit_str.ends_with("MB") {
        if limit_str.ends_with("MB") {
            (&limit_str[..limit_str.len() - 2], "M")
        } else {
            (&limit_str[..limit_str.len() - 1], "M")
        }
    } else if limit_str.ends_with("K") || limit_str.ends_with("KB") {
        if limit_str.ends_with("KB") {
            (&limit_str[..limit_str.len() - 2], "K")
        } else {
            (&limit_str[..limit_str.len() - 1], "K")
        }
    } else if limit_str.ends_with("G") || limit_str.ends_with("GB") {
        if limit_str.ends_with("GB") {
            (&limit_str[..limit_str.len() - 2], "G")
        } else {
            (&limit_str[..limit_str.len() - 1], "G")
        }
    } else {
        // Assume bytes if no unit
        return limit_str
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("Invalid bandwidth limit: {}", limit_str));
    };

    let number: f64 = num_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid bandwidth limit number: {}", num_str))?;

    let bytes_per_sec = match unit {
        "K" => (number * 1024.0) as u64,
        "M" => (number * 1024.0 * 1024.0) as u64,
        "G" => (number * 1024.0 * 1024.0 * 1024.0) as u64,
        _ => return Err(anyhow::anyhow!("Unknown unit: {}", unit)),
    };

    Ok(bytes_per_sec)
}

/// Run CLI mode - apply throttle and wait
async fn run_cli_mode(args: &Args) -> Result<()> {
    use tokio::signal;

    let pid = args.pid.unwrap();

    // Parse bandwidth limits
    let download_limit = if let Some(ref limit_str) = args.download_limit {
        Some(parse_bandwidth_limit(limit_str)?)
    } else {
        None
    };

    let upload_limit = if let Some(ref limit_str) = args.upload_limit {
        Some(parse_bandwidth_limit(limit_str)?)
    } else {
        None
    };

    if download_limit.is_none() && upload_limit.is_none() {
        return Err(anyhow::anyhow!(
            "At least one of --download-limit or --upload-limit is required with --pid"
        ));
    }

    // Get process name
    let process_name = std::fs::read_to_string(format!("/proc/{}/comm", pid))
        .unwrap_or_else(|_| format!("PID {}", pid))
        .trim()
        .to_string();

    println!("ChadThrottle v0.6.0 - CLI Mode");
    println!();
    println!("Throttling process: {} (PID {})", process_name, pid);
    if let Some(dl) = download_limit {
        println!("  Download limit: {}/s", human_readable(dl));
    }
    if let Some(ul) = upload_limit {
        println!("  Upload limit:   {}/s", human_readable(ul));
    }
    if let Some(dur) = args.duration {
        println!("  Duration:       {} seconds", dur);
    } else {
        println!("  Duration:       Until Ctrl+C");
    }
    println!();

    // Select backends
    let upload_backend = select_upload_backend(args.upload_backend.as_deref());
    let download_backend = select_download_backend(args.download_backend.as_deref());

    if let Some(ref backend) = upload_backend {
        println!("Using upload backend:   {}", backend.name());
    } else {
        println!("Upload backend:         Not available");
    }

    if let Some(ref backend) = download_backend {
        println!("Using download backend: {}", backend.name());
    } else {
        println!("Download backend:       Not available");
    }
    println!();

    // Create throttle manager
    let mut throttle_manager = ThrottleManager::new(upload_backend, download_backend);

    // Apply throttle
    let limit = ThrottleLimit {
        upload_limit,
        download_limit,
    };

    throttle_manager.throttle_process(pid, process_name.clone(), &limit)?;
    println!("✅ Throttle applied successfully!");
    println!();

    // Wait for duration or Ctrl+C
    if let Some(duration) = args.duration {
        println!(
            "Running for {} seconds... (Press Ctrl+C to stop early)",
            duration
        );
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(duration)) => {
                println!("\n⏱️  Duration elapsed, removing throttle...");
            }
            _ = signal::ctrl_c() => {
                println!("\n🛑 Received Ctrl+C, removing throttle...");
            }
        }
    } else {
        println!("Press Ctrl+C to stop and remove throttle...");
        signal::ctrl_c().await?;
        println!("\n🛑 Received Ctrl+C, removing throttle...");
    }

    // Remove throttle
    throttle_manager.remove_throttle(pid)?;
    println!("✅ Throttle removed successfully!");

    Ok(())
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

    // Initialize BPF configuration
    #[cfg(feature = "throttle-ebpf")]
    {
        use crate::backends::throttle::{BpfAttachMethod, BpfConfig, init_bpf_config};

        // Parse attach method from CLI arg or environment
        let attach_method = BpfAttachMethod::from_env_and_arg(args.bpf_attach_method.as_deref());
        init_bpf_config(BpfConfig::new(attach_method));

        log::info!("BPF attach method: {:?}", attach_method);
    }

    // Handle --list-backends
    if args.list_backends {
        print_available_backends();
        return Ok(());
    }

    // Handle CLI mode (--pid specified)
    if args.pid.is_some() {
        return run_cli_mode(&args).await;
    }

    // Setup terminal for TUI mode
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
    if !args.no_restore {
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
    } else {
        log::info!("Skipping throttle restoration (--no-restore flag)");
    }

    // Run the app
    let res = run_app(
        &mut terminal,
        &mut app,
        &mut monitor,
        &mut throttle_manager,
        &config,
    )
    .await;

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
    config: &config::Config,
) -> Result<()> {
    let mut update_interval = interval(Duration::from_secs(1));
    let mut bandwidth_log_counter = 0u32; // Log bandwidth every N updates

    loop {
        // Get backend info for UI display
        let backend_info = throttle_manager.get_backend_info(
            config.preferred_upload_backend.clone(),
            config.preferred_download_backend.clone(),
        );

        // Draw UI
        terminal.draw(|f| ui::draw_ui_with_backend_info(f, app, Some(&backend_info)))?;

        // Handle input with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // ALWAYS check Ctrl+C first - force quit regardless of modal state
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }

                // If help is shown, any key closes it
                if app.show_help {
                    app.show_help = false;
                    continue;
                }

                // If backend info is shown, b/Esc/q closes it
                if app.show_backend_info {
                    match key.code {
                        KeyCode::Char('b') | KeyCode::Char('q') | KeyCode::Esc => {
                            app.show_backend_info = false;
                        }
                        _ => {}
                    }
                    continue;
                }

                // If graph is shown, g/Esc/q closes it
                if app.show_graph {
                    match key.code {
                        KeyCode::Char('g') | KeyCode::Char('q') | KeyCode::Esc => {
                            app.show_graph = false;
                        }
                        _ => {}
                    }
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
                                            log::warn!("Failed to apply throttle: {e}");
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
                    KeyCode::Char('b') => {
                        app.show_backend_info = !app.show_backend_info;
                    }
                    KeyCode::Char('f') => {
                        app.toggle_sort_freeze();
                        app.status_message = if app.sort_frozen {
                            "Sort order frozen ❄️ - Stats continue updating, order preserved"
                                .to_string()
                        } else {
                            "Sort order unfrozen - Dynamic sorting re-enabled".to_string()
                        };
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
                    // Increment bandwidth log counter
                    bandwidth_log_counter += 1;
                    let should_log_bandwidth = bandwidth_log_counter % 5 == 0; // Log every 5 seconds

                    // Update throttle status and history for each process
                    for (pid, process_info) in process_map.iter_mut() {
                        if let Some(throttle) = throttle_manager.get_throttle(*pid) {
                            process_info.throttle_limit = Some(crate::process::ThrottleLimit {
                                download_limit: throttle.download_limit,
                                upload_limit: throttle.upload_limit,
                            });

                            // Log bandwidth vs throttle limit periodically
                            if should_log_bandwidth {
                                // Check download throttle
                                if let Some(download_limit) = throttle.download_limit {
                                    let actual_bps = process_info.download_rate;
                                    let ratio = actual_bps as f64 / download_limit as f64;

                                    let status = if ratio > 1.5 {
                                        "⚠️  THROTTLE NOT WORKING"
                                    } else if ratio > 1.1 {
                                        "⚠️  OVER LIMIT"
                                    } else {
                                        "✅ THROTTLED"
                                    };

                                    log::info!(
                                        "PID {} ({}) download: actual={}/s, limit={}/s, ratio={:.2}x {}",
                                        pid,
                                        process_info.name,
                                        human_readable(actual_bps),
                                        human_readable(download_limit),
                                        ratio,
                                        status
                                    );
                                }

                                // Log eBPF stats if using eBPF backend
                                #[cfg(feature = "throttle-ebpf")]
                                let _ = throttle_manager.log_ebpf_stats(*pid);
                            }
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
