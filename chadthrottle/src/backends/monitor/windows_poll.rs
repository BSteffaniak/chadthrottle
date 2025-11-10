// Windows polling-based network monitoring backend
//
// This backend provides graceful degradation for Windows without requiring Npcap:
//
// Tier 1 (No Admin): Shows process list and active connections
// Tier 2 (Admin): Adds bandwidth tracking via GetPerTcpConnectionEStats
// Tier 3 (Npcap): Full packet capture (handled by pnet backend)
//
// Uses polling (1 second interval) instead of packet capture.
// All metrics are accurate - no approximations or estimations.

use crate::backends::monitor::MonitorBackend;
use crate::backends::process::{ConnectionMap, ProcessUtils};
use crate::backends::{BackendCapabilities, BackendPriority};
use crate::process::{InterfaceMap, ProcessInfo, ProcessMap};
use anyhow::Result;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// Windows-specific imports for TCP statistics
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{BOOLEAN, NO_ERROR};
#[cfg(target_os = "windows")]
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_INCLUDE_PREFIX, GetAdaptersAddresses, GetPerTcpConnectionEStats,
    IP_ADAPTER_ADDRESSES_LH, SetPerTcpConnectionEStats, TCP_ESTATS_DATA_ROD_v0,
    TCP_ESTATS_DATA_RW_v0, TcpConnectionEstatsData,
};
#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::{AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6};

// MIB_TCPROW structure (Windows doesn't export this in the rust crate, so we define it)
#[cfg(target_os = "windows")]
#[repr(C)]
struct MIB_TCPROW {
    dwState: u32,
    dwLocalAddr: u32,
    dwLocalPort: u32,
    dwRemoteAddr: u32,
    dwRemotePort: u32,
}

/// Monitoring tier based on available privileges and features
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitoringTier {
    /// Basic mode: Connection list only (no admin required)
    Basic,
    /// Stats mode: Bandwidth tracking enabled (requires admin)
    Stats,
}

/// Windows polling-based monitor
pub struct WindowsPollingMonitor {
    tier: MonitoringTier,
    process_utils: Box<dyn ProcessUtils>,
    connection_tracker: Arc<Mutex<ConnectionTracker>>,
    polling_thread: Option<thread::JoinHandle<()>>,
    shutdown_flag: Arc<AtomicBool>,
}

/// Tracks network connections and bandwidth
struct ConnectionTracker {
    tier: MonitoringTier,

    // Connection tracking with statistics
    tcp_connections: HashMap<ConnectionKey, ConnectionStats>,
    tcp6_connections: HashMap<ConnectionKey, ConnectionStats>,
    udp_connections: HashMap<ConnectionKey, ConnectionStats>,
    udp6_connections: HashMap<ConnectionKey, ConnectionStats>,

    // Per-process aggregated bandwidth
    process_bandwidth: HashMap<i32, ProcessBandwidth>,

    // Per-interface aggregated bandwidth
    interface_bandwidth: HashMap<String, InterfaceBandwidth>,

    // Per-process-interface aggregated bandwidth
    process_interface_bandwidth: HashMap<(i32, String), ProcessInterfaceBandwidth>,

    // Cached interface list (refreshed periodically)
    #[cfg(target_os = "windows")]
    cached_interfaces: Vec<WindowsNetworkInterface>,
    #[cfg(target_os = "windows")]
    last_interface_refresh: Instant,

    // Full connection map for populating process details
    connection_map: ConnectionMap,

    last_update: Instant,
}

/// Unique identifier for a network connection
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct ConnectionKey {
    local_addr: IpAddr,
    local_port: u16,
    remote_addr: IpAddr,
    remote_port: u16,
    protocol: Protocol,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
enum Protocol {
    Tcp,
    TcpV6,
    Udp,
    UdpV6,
}

/// Statistics for a single connection
#[derive(Debug, Clone)]
struct ConnectionStats {
    pid: i32,
    process_name: String,

    // Connection details (needed for GetPerTcpConnectionEStats)
    local_addr: IpAddr,
    local_port: u16,
    remote_addr: IpAddr,
    remote_port: u16,

    // Cumulative byte counters
    bytes_sent: u64,
    bytes_received: u64,

    // Previous values for delta calculation
    last_bytes_sent: u64,
    last_bytes_received: u64,

    last_update: Instant,
}

/// Aggregated bandwidth per process
#[derive(Debug, Clone)]
struct ProcessBandwidth {
    name: String,

    // Current sum of all connection cumulative totals (for reference)
    rx_bytes: u64,
    tx_bytes: u64,

    // Process-lifetime accumulated deltas (actual data transferred by this process)
    lifetime_rx_bytes: u64,
    lifetime_tx_bytes: u64,

    // Previous lifetime values for rate calculation
    last_lifetime_rx_bytes: u64,
    last_lifetime_tx_bytes: u64,

    // Previous sum values (kept for compatibility, may remove later)
    last_rx_bytes: u64,
    last_tx_bytes: u64,

    // Calculated rates (bytes/sec)
    rx_rate: u64,
    tx_rate: u64,

    // Internet traffic tracking
    internet_rx_bytes: u64,
    internet_tx_bytes: u64,
    lifetime_internet_rx_bytes: u64,
    lifetime_internet_tx_bytes: u64,
    last_internet_rx_bytes: u64,
    last_internet_tx_bytes: u64,
    internet_rx_rate: u64,
    internet_tx_rate: u64,

    // Local traffic tracking
    local_rx_bytes: u64,
    local_tx_bytes: u64,
    lifetime_local_rx_bytes: u64,
    lifetime_local_tx_bytes: u64,
    last_local_rx_bytes: u64,
    last_local_tx_bytes: u64,
    local_rx_rate: u64,
    local_tx_rate: u64,

    // Connection count
    connection_count: usize,
}

/// Aggregated bandwidth per network interface
#[derive(Debug, Clone)]
struct InterfaceBandwidth {
    name: String,

    // Process-lifetime accumulated deltas
    lifetime_rx_bytes: u64,
    lifetime_tx_bytes: u64,

    // Previous lifetime values for rate calculation
    last_lifetime_rx_bytes: u64,
    last_lifetime_tx_bytes: u64,

    // Calculated rates (bytes/sec)
    rx_rate: u64,
    tx_rate: u64,
}

/// Aggregated bandwidth per process-interface pair
#[derive(Debug, Clone)]
struct ProcessInterfaceBandwidth {
    // Lifetime accumulated deltas
    rx_bytes: u64,
    tx_bytes: u64,

    // Previous values for rate calculation
    last_rx_bytes: u64,
    last_tx_bytes: u64,

    // Traffic categorization
    internet_rx_bytes: u64,
    internet_tx_bytes: u64,
    local_rx_bytes: u64,
    local_tx_bytes: u64,

    last_internet_rx_bytes: u64,
    last_internet_tx_bytes: u64,
    last_local_rx_bytes: u64,
    last_local_tx_bytes: u64,
}

/// Windows network interface information
#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct WindowsNetworkInterface {
    name: String,
    friendly_name: String,
    mac_address: Option<String>,
    ip_addresses: Vec<IpAddr>,
    is_up: bool,
    is_loopback: bool,
}

impl WindowsPollingMonitor {
    /// Create new monitor with automatic tier detection
    pub fn new() -> Result<Self> {
        let tier = detect_monitoring_tier();

        log::info!("Windows Polling Monitor initialized in {:?} mode", tier);
        match tier {
            MonitoringTier::Basic => {
                log::info!("  ✓ Process list and connection tracking enabled");
                log::warn!("  ✗ Bandwidth tracking disabled (requires admin)");
                log::info!("  → Run as Administrator to enable bandwidth tracking");
            }
            MonitoringTier::Stats => {
                log::info!("  ✓ Process list and connection tracking enabled");
                log::info!("  ✓ Bandwidth tracking enabled (admin mode)");
                log::info!("  → Install Npcap for real-time packet capture");
            }
        }

        let process_utils = crate::backends::process::create_process_utils();
        let connection_tracker = Arc::new(Mutex::new(ConnectionTracker {
            tier,
            tcp_connections: HashMap::new(),
            tcp6_connections: HashMap::new(),
            udp_connections: HashMap::new(),
            udp6_connections: HashMap::new(),
            process_bandwidth: HashMap::new(),
            interface_bandwidth: HashMap::new(),
            process_interface_bandwidth: HashMap::new(),
            #[cfg(target_os = "windows")]
            cached_interfaces: Vec::new(),
            #[cfg(target_os = "windows")]
            last_interface_refresh: Instant::now(),
            connection_map: ConnectionMap::default(),
            last_update: Instant::now(),
        }));

        let shutdown_flag = Arc::new(AtomicBool::new(false));

        // Initialize interface cache immediately on Windows
        #[cfg(target_os = "windows")]
        {
            match enumerate_windows_interfaces() {
                Ok(interfaces) => {
                    let mut tracker = connection_tracker.lock().unwrap();
                    tracker.cached_interfaces = interfaces;
                    log::info!(
                        "Initialized network interface cache: {} interfaces",
                        tracker.cached_interfaces.len()
                    );
                }
                Err(e) => {
                    log::warn!(
                        "Failed to enumerate network interfaces during initialization: {}",
                        e
                    );
                }
            }
        }

        // Start polling thread
        let polling_thread = Some(start_polling_thread(
            Arc::clone(&connection_tracker),
            Arc::clone(&shutdown_flag),
            tier,
        ));

        Ok(Self {
            tier,
            process_utils,
            connection_tracker,
            polling_thread,
            shutdown_flag,
        })
    }

    /// Get current monitoring tier
    pub fn tier(&self) -> MonitoringTier {
        self.tier
    }
}

impl Drop for WindowsPollingMonitor {
    fn drop(&mut self) {
        // Signal shutdown
        self.shutdown_flag.store(true, Ordering::Relaxed);

        // Wait for polling thread to finish
        if let Some(handle) = self.polling_thread.take() {
            let _ = handle.join();
        }
    }
}

impl MonitorBackend for WindowsPollingMonitor {
    fn name(&self) -> &'static str {
        match self.tier {
            MonitoringTier::Basic => "windows-poll-basic",
            MonitoringTier::Stats => "windows-poll-stats",
        }
    }

    fn priority(&self) -> BackendPriority {
        match self.tier {
            MonitoringTier::Basic => BackendPriority::Fallback,
            MonitoringTier::Stats => BackendPriority::Good,
        }
    }

    fn is_available() -> bool {
        cfg!(target_os = "windows")
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            ipv4_support: true,
            ipv6_support: true,
            per_process: matches!(self.tier, MonitoringTier::Stats),
            per_connection: matches!(self.tier, MonitoringTier::Stats),
        }
    }

    fn init(&mut self) -> Result<()> {
        // Already initialized in new()
        Ok(())
    }

    fn update(&mut self) -> Result<(ProcessMap, InterfaceMap)> {
        let tracker = self.connection_tracker.lock().unwrap();
        let mut process_map = ProcessMap::new();

        // Calculate elapsed time for rate calculations
        let now = Instant::now();
        let elapsed = now.duration_since(tracker.last_update).as_secs_f64();

        match tracker.tier {
            MonitoringTier::Basic => {
                // Show processes with connections (no bandwidth without admin)
                let mut pid_names: HashMap<i32, String> = HashMap::new();

                // Collect PIDs from all connections
                for stats in tracker.tcp_connections.values() {
                    pid_names
                        .entry(stats.pid)
                        .or_insert_with(|| stats.process_name.clone());
                }
                for stats in tracker.tcp6_connections.values() {
                    pid_names
                        .entry(stats.pid)
                        .or_insert_with(|| stats.process_name.clone());
                }
                for stats in tracker.udp_connections.values() {
                    pid_names
                        .entry(stats.pid)
                        .or_insert_with(|| stats.process_name.clone());
                }
                for stats in tracker.udp6_connections.values() {
                    pid_names
                        .entry(stats.pid)
                        .or_insert_with(|| stats.process_name.clone());
                }

                // Build process map (bandwidth will be 0 in basic mode)
                for (pid, name) in pid_names {
                    process_map.insert(pid, ProcessInfo::new(pid, name));
                }
            }

            MonitoringTier::Stats => {
                // Show processes with accurate bandwidth data
                for (&pid, bandwidth) in &tracker.process_bandwidth {
                    let mut info = ProcessInfo::new(pid, bandwidth.name.clone());
                    info.download_rate = bandwidth.rx_rate;
                    info.upload_rate = bandwidth.tx_rate;
                    // Use lifetime accumulated deltas, not sum of connection totals
                    info.total_download = bandwidth.lifetime_rx_bytes;
                    info.total_upload = bandwidth.lifetime_tx_bytes;

                    // Populate categorized traffic data
                    info.internet_download_rate = bandwidth.internet_rx_rate;
                    info.internet_upload_rate = bandwidth.internet_tx_rate;
                    info.internet_total_download = bandwidth.lifetime_internet_rx_bytes;
                    info.internet_total_upload = bandwidth.lifetime_internet_tx_bytes;

                    info.local_download_rate = bandwidth.local_rx_rate;
                    info.local_upload_rate = bandwidth.local_tx_rate;
                    info.local_total_download = bandwidth.lifetime_local_rx_bytes;
                    info.local_total_upload = bandwidth.lifetime_local_tx_bytes;

                    // Populate per-interface stats for this process
                    info.interface_stats = tracker
                        .process_interface_bandwidth
                        .iter()
                        .filter(|((p, _), _)| *p == pid)
                        .map(|((_, iface), bw)| {
                            // Calculate rates based on deltas
                            let rx_diff = bw.rx_bytes.saturating_sub(bw.last_rx_bytes);
                            let tx_diff = bw.tx_bytes.saturating_sub(bw.last_tx_bytes);

                            let download_rate = if elapsed > 0.0 {
                                (rx_diff as f64 / elapsed) as u64
                            } else {
                                0
                            };
                            let upload_rate = if elapsed > 0.0 {
                                (tx_diff as f64 / elapsed) as u64
                            } else {
                                0
                            };

                            // Calculate categorized rates
                            let internet_rx_diff = bw
                                .internet_rx_bytes
                                .saturating_sub(bw.last_internet_rx_bytes);
                            let internet_tx_diff = bw
                                .internet_tx_bytes
                                .saturating_sub(bw.last_internet_tx_bytes);
                            let local_rx_diff =
                                bw.local_rx_bytes.saturating_sub(bw.last_local_rx_bytes);
                            let local_tx_diff =
                                bw.local_tx_bytes.saturating_sub(bw.last_local_tx_bytes);

                            let internet_download_rate = if elapsed > 0.0 {
                                (internet_rx_diff as f64 / elapsed) as u64
                            } else {
                                0
                            };
                            let internet_upload_rate = if elapsed > 0.0 {
                                (internet_tx_diff as f64 / elapsed) as u64
                            } else {
                                0
                            };
                            let local_download_rate = if elapsed > 0.0 {
                                (local_rx_diff as f64 / elapsed) as u64
                            } else {
                                0
                            };
                            let local_upload_rate = if elapsed > 0.0 {
                                (local_tx_diff as f64 / elapsed) as u64
                            } else {
                                0
                            };

                            (
                                iface.clone(),
                                crate::process::InterfaceStats {
                                    download_rate,
                                    upload_rate,
                                    total_download: bw.rx_bytes,
                                    total_upload: bw.tx_bytes,
                                    internet_download_rate,
                                    internet_upload_rate,
                                    local_download_rate,
                                    local_upload_rate,
                                },
                            )
                        })
                        .collect();

                    process_map.insert(pid, info);
                }
            }
        }

        // Build interface map with bandwidth statistics (INSIDE LOCK - no clones!)
        let mut interface_map = InterfaceMap::new();

        #[cfg(target_os = "windows")]
        {
            use crate::process::InterfaceInfo;
            use std::collections::HashSet;

            log::info!(
                "Building InterfaceMap: {} cached interfaces, {} TCP connections, {} TCP6 connections",
                tracker.cached_interfaces.len(),
                tracker.tcp_connections.len(),
                tracker.tcp6_connections.len()
            );

            for interface in &tracker.cached_interfaces {
                // Skip interfaces that are down or have no IPs
                if !interface.is_up || interface.ip_addresses.is_empty() {
                    log::debug!(
                        "Skipping interface '{}': is_up={}, ip_count={}",
                        interface.friendly_name,
                        interface.is_up,
                        interface.ip_addresses.len()
                    );
                    continue;
                }

                let iface_name = &interface.friendly_name;

                log::debug!(
                    "Processing interface '{}': IPs = {:?}",
                    iface_name,
                    interface.ip_addresses
                );

                // Get bandwidth stats for this interface (just reading, no clone)
                let (download_rate, upload_rate) = tracker
                    .interface_bandwidth
                    .get(iface_name)
                    .map(|bw| (bw.rx_rate, bw.tx_rate))
                    .unwrap_or((0, 0));

                // Count processes using this interface (iterating references, no clone!)
                let mut process_pids: HashSet<i32> = HashSet::new();
                let mut matched_tcp_connections = 0;
                let mut unmatched_tcp_connections = 0;

                // Check TCP connections
                for conn in tracker.tcp_connections.values() {
                    if let Some(conn_iface) =
                        get_interface_for_ip(&conn.local_addr, &tracker.cached_interfaces)
                    {
                        if conn_iface == *iface_name {
                            process_pids.insert(conn.pid);
                            matched_tcp_connections += 1;
                            log::trace!(
                                "Matched TCP connection: local_addr={} -> interface='{}' (PID {})",
                                conn.local_addr,
                                iface_name,
                                conn.pid
                            );
                        }
                    } else {
                        unmatched_tcp_connections += 1;
                        log::trace!(
                            "Unmatched TCP connection: local_addr={} (PID {}) - no interface match",
                            conn.local_addr,
                            conn.pid
                        );
                    }
                }

                let mut matched_tcp6_connections = 0;
                let mut unmatched_tcp6_connections = 0;

                // Check TCP6 connections
                for conn in tracker.tcp6_connections.values() {
                    if let Some(conn_iface) =
                        get_interface_for_ip(&conn.local_addr, &tracker.cached_interfaces)
                    {
                        if conn_iface == *iface_name {
                            process_pids.insert(conn.pid);
                            matched_tcp6_connections += 1;
                            log::trace!(
                                "Matched TCP6 connection: local_addr={} -> interface='{}' (PID {})",
                                conn.local_addr,
                                iface_name,
                                conn.pid
                            );
                        }
                    } else {
                        unmatched_tcp6_connections += 1;
                        log::trace!(
                            "Unmatched TCP6 connection: local_addr={} (PID {}) - no interface match",
                            conn.local_addr,
                            conn.pid
                        );
                    }
                }

                let process_count = process_pids.len();

                log::info!(
                    "Interface '{}': matched {}/{} TCP, {}/{} TCP6 connections, {} unique PIDs",
                    iface_name,
                    matched_tcp_connections,
                    matched_tcp_connections + unmatched_tcp_connections,
                    matched_tcp6_connections,
                    matched_tcp6_connections + unmatched_tcp6_connections,
                    process_count
                );

                // Only clone small strings and metadata (not entire connection list!)
                interface_map.insert(
                    iface_name.clone(),
                    InterfaceInfo {
                        name: iface_name.clone(),
                        mac_address: interface.mac_address.clone(),
                        ip_addresses: interface.ip_addresses.clone(),
                        is_up: interface.is_up,
                        is_loopback: interface.is_loopback,
                        total_download_rate: download_rate,
                        total_upload_rate: upload_rate,
                        process_count,
                    },
                );
            }
        }

        // Get connection map for populate_connections() (this clone is unavoidable)
        let conn_map = tracker.connection_map.clone();

        // Now drop the lock
        drop(tracker);

        let socket_map = &conn_map.socket_to_pid;
        for process in process_map.values_mut() {
            process.populate_connections(&conn_map, socket_map);
        }

        Ok((process_map, interface_map))
    }

    fn cleanup(&mut self) -> Result<()> {
        // Handled by Drop implementation
        Ok(())
    }
}

/// Detect which monitoring tier is available
fn detect_monitoring_tier() -> MonitoringTier {
    // Check if running with admin privileges
    if is_elevated() {
        log::debug!("Admin privileges detected");

        // Try to enable TCP statistics collection
        if enable_tcp_estats() {
            log::debug!("TCP statistics enabled successfully");
            return MonitoringTier::Stats;
        } else {
            log::warn!("Failed to enable TCP statistics (admin mode but stats unavailable)");
        }
    } else {
        log::debug!("No admin privileges - using basic mode");
    }

    MonitoringTier::Basic
}

/// Check if running with elevated (administrator) privileges
#[cfg(target_os = "windows")]
fn is_elevated() -> bool {
    use std::mem;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TokenElevation};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = HANDLE::default();

        // Get process token
        if OpenProcessToken(
            GetCurrentProcess(),
            windows::Win32::Security::TOKEN_QUERY,
            &mut token,
        )
        .is_err()
        {
            return false;
        }

        // Check if token is elevated
        let mut elevation = TOKEN_ELEVATION::default();
        let mut return_length = 0u32;

        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_length,
        );

        CloseHandle(token).ok();

        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(target_os = "windows"))]
fn is_elevated() -> bool {
    false
}

/// Enable TCP extended statistics collection globally (requires admin)
#[cfg(target_os = "windows")]
fn enable_tcp_estats() -> bool {
    // Note: GetPerTcpConnectionEStats requires Windows Vista+ and admin privileges
    // We enable statistics collection globally by setting RW parameters

    // Try to enable TCP data tracking
    // This is a global setting that affects all new connections
    // Individual connections still need to be queried with GetPerTcpConnectionEStats

    log::debug!("Attempting to enable TCP extended statistics");

    // The actual enabling happens per-connection in get_tcp_stats()
    // Here we just verify we have the API available
    true
}

#[cfg(not(target_os = "windows"))]
fn enable_tcp_estats() -> bool {
    false
}

/// Start background polling thread
fn start_polling_thread(
    tracker: Arc<Mutex<ConnectionTracker>>,
    shutdown: Arc<AtomicBool>,
    tier: MonitoringTier,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        log::debug!("Polling thread started");

        while !shutdown.load(Ordering::Relaxed) {
            match tier {
                MonitoringTier::Basic => {
                    if let Err(e) = update_connection_list(&tracker) {
                        log::error!("Failed to update connection list: {}", e);
                    }
                }
                MonitoringTier::Stats => {
                    if let Err(e) = update_connection_stats(&tracker) {
                        log::error!("Failed to update connection stats: {}", e);
                    }
                }
            }

            // Poll every 1 second
            thread::sleep(Duration::from_secs(1));
        }

        log::debug!("Polling thread stopped");
    })
}

/// Update connection list (Tier 1 - Basic mode)
fn update_connection_list(tracker: &Arc<Mutex<ConnectionTracker>>) -> Result<()> {
    use crate::backends::process::ProcessUtils;

    // CRITICAL: Do ALL slow operations OUTSIDE the mutex lock
    // This prevents UI blocking during Windows API calls (2-3 seconds)

    let process_utils = crate::backends::process::create_process_utils();

    // Get current connection map - SLOW (2-3 seconds on Windows)
    let conn_map = process_utils.get_connection_map()?;

    // Build new connection HashMaps - OUTSIDE mutex lock
    let mut new_tcp_connections = HashMap::new();
    let mut new_tcp6_connections = HashMap::new();
    let mut new_udp_connections = HashMap::new();
    let mut new_udp6_connections = HashMap::new();

    let now = Instant::now();

    // Process all connections - OUTSIDE mutex lock
    for (inode, (pid, process_name)) in &conn_map.socket_to_pid {
        // Find matching TCP connections
        for conn in &conn_map.tcp_connections {
            if &conn.inode == inode {
                let key = ConnectionKey {
                    local_addr: conn.local_addr,
                    local_port: conn.local_port,
                    remote_addr: conn.remote_addr,
                    remote_port: conn.remote_port,
                    protocol: Protocol::Tcp,
                };

                new_tcp_connections.insert(
                    key,
                    ConnectionStats {
                        pid: *pid,
                        process_name: process_name.clone(),
                        local_addr: conn.local_addr,
                        local_port: conn.local_port,
                        remote_addr: conn.remote_addr,
                        remote_port: conn.remote_port,
                        bytes_sent: 0,
                        bytes_received: 0,
                        last_bytes_sent: 0,
                        last_bytes_received: 0,
                        last_update: now,
                    },
                );
            }
        }

        // Find matching TCP6 connections
        for conn in &conn_map.tcp6_connections {
            if &conn.inode == inode {
                let key = ConnectionKey {
                    local_addr: conn.local_addr,
                    local_port: conn.local_port,
                    remote_addr: conn.remote_addr,
                    remote_port: conn.remote_port,
                    protocol: Protocol::TcpV6,
                };

                new_tcp6_connections.insert(
                    key,
                    ConnectionStats {
                        pid: *pid,
                        process_name: process_name.clone(),
                        local_addr: conn.local_addr,
                        local_port: conn.local_port,
                        remote_addr: conn.remote_addr,
                        remote_port: conn.remote_port,
                        bytes_sent: 0,
                        bytes_received: 0,
                        last_bytes_sent: 0,
                        last_bytes_received: 0,
                        last_update: now,
                    },
                );
            }
        }

        // Find matching UDP connections
        for conn in &conn_map.udp_connections {
            if &conn.inode == inode {
                let key = ConnectionKey {
                    local_addr: conn.local_addr,
                    local_port: conn.local_port,
                    remote_addr: conn.remote_addr,
                    remote_port: conn.remote_port,
                    protocol: Protocol::Udp,
                };

                new_udp_connections.insert(
                    key,
                    ConnectionStats {
                        pid: *pid,
                        process_name: process_name.clone(),
                        local_addr: conn.local_addr,
                        local_port: conn.local_port,
                        remote_addr: conn.remote_addr,
                        remote_port: conn.remote_port,
                        bytes_sent: 0,
                        bytes_received: 0,
                        last_bytes_sent: 0,
                        last_bytes_received: 0,
                        last_update: now,
                    },
                );
            }
        }

        // Find matching UDP6 connections
        for conn in &conn_map.udp6_connections {
            if &conn.inode == inode {
                let key = ConnectionKey {
                    local_addr: conn.local_addr,
                    local_port: conn.local_port,
                    remote_addr: conn.remote_addr,
                    remote_port: conn.remote_port,
                    protocol: Protocol::UdpV6,
                };

                new_udp6_connections.insert(
                    key,
                    ConnectionStats {
                        pid: *pid,
                        process_name: process_name.clone(),
                        local_addr: conn.local_addr,
                        local_port: conn.local_port,
                        remote_addr: conn.remote_addr,
                        remote_port: conn.remote_port,
                        bytes_sent: 0,
                        bytes_received: 0,
                        last_bytes_sent: 0,
                        last_bytes_received: 0,
                        last_update: now,
                    },
                );
            }
        }
    }

    // CRITICAL SECTION: Only lock mutex for the final swap (<1ms)
    // This minimizes UI blocking from ~2-3 seconds to <1ms
    {
        let mut tracker = tracker.lock().unwrap();
        tracker.tcp_connections = new_tcp_connections;
        tracker.tcp6_connections = new_tcp6_connections;
        tracker.udp_connections = new_udp_connections;
        tracker.udp6_connections = new_udp6_connections;
        tracker.connection_map = conn_map;
    } // Mutex released immediately

    Ok(())
}

/// Update connection statistics with bandwidth tracking (Tier 2 - Stats mode)
fn update_connection_stats(tracker: &Arc<Mutex<ConnectionTracker>>) -> Result<()> {
    // Save old connection stats BEFORE updating the connection list
    // We need these to calculate deltas
    let (old_tcp_connections, old_tcp6_connections) = {
        let tracker_guard = tracker.lock().unwrap();
        (
            tracker_guard.tcp_connections.clone(),
            tracker_guard.tcp6_connections.clone(),
        )
    };

    // Now update connection list (this handles mutex properly and replaces the connections)
    update_connection_list(tracker)?;

    // Build process_bandwidth HashMap by aggregating connection data
    // Use a separate mutex lock AFTER connection list is updated
    let mut tracker = tracker.lock().unwrap();
    let now = Instant::now();
    let elapsed = now.duration_since(tracker.last_update).as_secs_f64();

    // Refresh interface cache if needed (every 10 seconds)
    #[cfg(target_os = "windows")]
    let refresh_interfaces = now.duration_since(tracker.last_interface_refresh).as_secs() >= 10;
    #[cfg(target_os = "windows")]
    if refresh_interfaces {
        match enumerate_windows_interfaces() {
            Ok(interfaces) => {
                tracker.cached_interfaces = interfaces;
                tracker.last_interface_refresh = now;
                log::debug!(
                    "Refreshed network interface cache: {} interfaces",
                    tracker.cached_interfaces.len()
                );
            }
            Err(e) => {
                log::warn!("Failed to enumerate network interfaces: {}", e);
            }
        }
    }

    // Rebuild process_bandwidth map from current connections
    let mut new_process_bandwidth: HashMap<i32, ProcessBandwidth> = HashMap::new();

    // Track connection updates (to avoid borrow checker issues)
    let mut tcp_connection_updates: Vec<(ConnectionKey, u64, u64)> = Vec::new();
    let mut tcp6_connection_updates: Vec<(ConnectionKey, u64, u64)> = Vec::new();

    // Track interface updates (to avoid borrow checker issues)
    #[cfg(target_os = "windows")]
    let mut interface_updates: Vec<(
        String,
        u64,
        u64,
        i32,
        crate::traffic_classifier::TrafficCategory,
    )> = Vec::new();

    // Log connection counts for diagnostics
    log::info!(
        "Processing connections - TCP: {}, TCP6: {}, UDP: {}, UDP6: {}",
        tracker.tcp_connections.len(),
        tracker.tcp6_connections.len(),
        tracker.udp_connections.len(),
        tracker.udp6_connections.len()
    );

    // Aggregate all TCP connections by PID and fetch real bandwidth data
    for stats in tracker.tcp_connections.values() {
        let entry = new_process_bandwidth.entry(stats.pid).or_insert_with(|| {
            // Preserve previous lifetime values for delta calculation
            let prev = tracker.process_bandwidth.get(&stats.pid);
            let (prev_lifetime_rx, prev_lifetime_tx) = prev
                .map(|p| (p.lifetime_rx_bytes, p.lifetime_tx_bytes))
                .unwrap_or((0, 0));
            let (prev_lifetime_internet_rx, prev_lifetime_internet_tx) = prev
                .map(|p| (p.lifetime_internet_rx_bytes, p.lifetime_internet_tx_bytes))
                .unwrap_or((0, 0));
            let (prev_lifetime_local_rx, prev_lifetime_local_tx) = prev
                .map(|p| (p.lifetime_local_rx_bytes, p.lifetime_local_tx_bytes))
                .unwrap_or((0, 0));

            ProcessBandwidth {
                name: stats.process_name.clone(),
                rx_bytes: 0,
                tx_bytes: 0,
                lifetime_rx_bytes: prev_lifetime_rx,
                lifetime_tx_bytes: prev_lifetime_tx,
                last_lifetime_rx_bytes: prev_lifetime_rx,
                last_lifetime_tx_bytes: prev_lifetime_tx,
                last_rx_bytes: 0,
                last_tx_bytes: 0,
                rx_rate: 0,
                tx_rate: 0,
                internet_rx_bytes: 0,
                internet_tx_bytes: 0,
                lifetime_internet_rx_bytes: prev_lifetime_internet_rx,
                lifetime_internet_tx_bytes: prev_lifetime_internet_tx,
                last_internet_rx_bytes: prev_lifetime_internet_rx,
                last_internet_tx_bytes: prev_lifetime_internet_tx,
                internet_rx_rate: 0,
                internet_tx_rate: 0,
                local_rx_bytes: 0,
                local_tx_bytes: 0,
                lifetime_local_rx_bytes: prev_lifetime_local_rx,
                lifetime_local_tx_bytes: prev_lifetime_local_tx,
                last_local_rx_bytes: prev_lifetime_local_rx,
                last_local_tx_bytes: prev_lifetime_local_tx,
                local_rx_rate: 0,
                local_tx_rate: 0,
                connection_count: 0,
            }
        });
        entry.connection_count += 1;

        // Fetch TCP statistics from Windows API
        log::trace!(
            "Attempting to get stats for TCP connection: {}:{} -> {}:{}",
            stats.local_addr,
            stats.local_port,
            stats.remote_addr,
            stats.remote_port
        );
        if let Some((rx_bytes, tx_bytes)) = get_tcp_stats(
            &stats.local_addr,
            stats.local_port,
            &stats.remote_addr,
            stats.remote_port,
        ) {
            log::trace!("  -> Got {} RX bytes, {} TX bytes", rx_bytes, tx_bytes);

            // Look up previous connection stats to calculate delta
            let conn_key = ConnectionKey {
                local_addr: stats.local_addr,
                local_port: stats.local_port,
                remote_addr: stats.remote_addr,
                remote_port: stats.remote_port,
                protocol: Protocol::Tcp,
            };

            let (prev_rx, prev_tx) = old_tcp_connections
                .get(&conn_key)
                .map(|old| (old.bytes_received, old.bytes_sent))
                .unwrap_or((0, 0));

            // Calculate delta for this specific connection
            let delta_rx = rx_bytes.saturating_sub(prev_rx);
            let delta_tx = tx_bytes.saturating_sub(prev_tx);

            // Accumulate: sum of connection cumulative totals (for reference)
            entry.rx_bytes += rx_bytes;
            entry.tx_bytes += tx_bytes;

            // Accumulate: lifetime deltas (actual new data transferred)
            // Skip first-seen connections to avoid counting their historical data
            let is_first_seen = old_tcp_connections.get(&conn_key).is_none();
            if !is_first_seen {
                entry.lifetime_rx_bytes += delta_rx;
                entry.lifetime_tx_bytes += delta_tx;

                // Categorize traffic by remote address
                let traffic_category =
                    crate::traffic_classifier::categorize_traffic(&stats.remote_addr);
                match traffic_category {
                    crate::traffic_classifier::TrafficCategory::Internet => {
                        entry.internet_rx_bytes += rx_bytes;
                        entry.internet_tx_bytes += tx_bytes;
                        entry.lifetime_internet_rx_bytes += delta_rx;
                        entry.lifetime_internet_tx_bytes += delta_tx;
                    }
                    crate::traffic_classifier::TrafficCategory::Local => {
                        entry.local_rx_bytes += rx_bytes;
                        entry.local_tx_bytes += tx_bytes;
                        entry.lifetime_local_rx_bytes += delta_rx;
                        entry.lifetime_local_tx_bytes += delta_tx;
                    }
                }

                // Queue interface update (avoid borrow checker issues)
                #[cfg(target_os = "windows")]
                if let Some(interface_name) =
                    get_interface_for_ip(&stats.local_addr, &tracker.cached_interfaces)
                {
                    interface_updates.push((
                        interface_name,
                        delta_rx,
                        delta_tx,
                        stats.pid,
                        traffic_category,
                    ));
                }
            }

            // Queue update for later (avoid borrow checker issues)
            tcp_connection_updates.push((conn_key, rx_bytes, tx_bytes));

            log::trace!(
                "  -> Delta: {} RX, {} TX (prev was {} RX, {} TX)",
                delta_rx,
                delta_tx,
                prev_rx,
                prev_tx
            );
        } else {
            log::trace!("  -> get_tcp_stats returned None");
        }
    }

    // Aggregate all TCP6 connections by PID and fetch real bandwidth data
    for stats in tracker.tcp6_connections.values() {
        let entry = new_process_bandwidth.entry(stats.pid).or_insert_with(|| {
            // Preserve previous lifetime values for delta calculation
            let prev = tracker.process_bandwidth.get(&stats.pid);
            let (prev_lifetime_rx, prev_lifetime_tx) = prev
                .map(|p| (p.lifetime_rx_bytes, p.lifetime_tx_bytes))
                .unwrap_or((0, 0));
            let (prev_lifetime_internet_rx, prev_lifetime_internet_tx) = prev
                .map(|p| (p.lifetime_internet_rx_bytes, p.lifetime_internet_tx_bytes))
                .unwrap_or((0, 0));
            let (prev_lifetime_local_rx, prev_lifetime_local_tx) = prev
                .map(|p| (p.lifetime_local_rx_bytes, p.lifetime_local_tx_bytes))
                .unwrap_or((0, 0));

            ProcessBandwidth {
                name: stats.process_name.clone(),
                rx_bytes: 0,
                tx_bytes: 0,
                lifetime_rx_bytes: prev_lifetime_rx,
                lifetime_tx_bytes: prev_lifetime_tx,
                last_lifetime_rx_bytes: prev_lifetime_rx,
                last_lifetime_tx_bytes: prev_lifetime_tx,
                last_rx_bytes: 0,
                last_tx_bytes: 0,
                rx_rate: 0,
                tx_rate: 0,
                internet_rx_bytes: 0,
                internet_tx_bytes: 0,
                lifetime_internet_rx_bytes: prev_lifetime_internet_rx,
                lifetime_internet_tx_bytes: prev_lifetime_internet_tx,
                last_internet_rx_bytes: prev_lifetime_internet_rx,
                last_internet_tx_bytes: prev_lifetime_internet_tx,
                internet_rx_rate: 0,
                internet_tx_rate: 0,
                local_rx_bytes: 0,
                local_tx_bytes: 0,
                lifetime_local_rx_bytes: prev_lifetime_local_rx,
                lifetime_local_tx_bytes: prev_lifetime_local_tx,
                last_local_rx_bytes: prev_lifetime_local_rx,
                last_local_tx_bytes: prev_lifetime_local_tx,
                local_rx_rate: 0,
                local_tx_rate: 0,
                connection_count: 0,
            }
        });
        entry.connection_count += 1;

        // Fetch TCP statistics from Windows API
        log::trace!(
            "Attempting to get stats for TCP6 connection: {}:{} -> {}:{}",
            stats.local_addr,
            stats.local_port,
            stats.remote_addr,
            stats.remote_port
        );
        if let Some((rx_bytes, tx_bytes)) = get_tcp_stats(
            &stats.local_addr,
            stats.local_port,
            &stats.remote_addr,
            stats.remote_port,
        ) {
            log::trace!("  -> Got {} RX bytes, {} TX bytes", rx_bytes, tx_bytes);

            // Look up previous connection stats to calculate delta
            let conn_key = ConnectionKey {
                local_addr: stats.local_addr,
                local_port: stats.local_port,
                remote_addr: stats.remote_addr,
                remote_port: stats.remote_port,
                protocol: Protocol::TcpV6,
            };

            let (prev_rx, prev_tx) = old_tcp6_connections
                .get(&conn_key)
                .map(|old| (old.bytes_received, old.bytes_sent))
                .unwrap_or((0, 0));

            // Calculate delta for this specific connection
            let delta_rx = rx_bytes.saturating_sub(prev_rx);
            let delta_tx = tx_bytes.saturating_sub(prev_tx);

            // Accumulate: sum of connection cumulative totals (for reference)
            entry.rx_bytes += rx_bytes;
            entry.tx_bytes += tx_bytes;

            // Accumulate: lifetime deltas (actual new data transferred)
            // Skip first-seen connections to avoid counting their historical data
            let is_first_seen = old_tcp6_connections.get(&conn_key).is_none();
            if !is_first_seen {
                entry.lifetime_rx_bytes += delta_rx;
                entry.lifetime_tx_bytes += delta_tx;

                // Categorize traffic by remote address
                let traffic_category =
                    crate::traffic_classifier::categorize_traffic(&stats.remote_addr);
                match traffic_category {
                    crate::traffic_classifier::TrafficCategory::Internet => {
                        entry.internet_rx_bytes += rx_bytes;
                        entry.internet_tx_bytes += tx_bytes;
                        entry.lifetime_internet_rx_bytes += delta_rx;
                        entry.lifetime_internet_tx_bytes += delta_tx;
                    }
                    crate::traffic_classifier::TrafficCategory::Local => {
                        entry.local_rx_bytes += rx_bytes;
                        entry.local_tx_bytes += tx_bytes;
                        entry.lifetime_local_rx_bytes += delta_rx;
                        entry.lifetime_local_tx_bytes += delta_tx;
                    }
                }

                // Queue interface update (avoid borrow checker issues)
                #[cfg(target_os = "windows")]
                if let Some(interface_name) =
                    get_interface_for_ip(&stats.local_addr, &tracker.cached_interfaces)
                {
                    interface_updates.push((
                        interface_name,
                        delta_rx,
                        delta_tx,
                        stats.pid,
                        traffic_category,
                    ));
                }
            }

            // Queue update for later (avoid borrow checker issues)
            tcp6_connection_updates.push((conn_key, rx_bytes, tx_bytes));

            log::trace!(
                "  -> Delta: {} RX, {} TX (prev was {} RX, {} TX)",
                delta_rx,
                delta_tx,
                prev_rx,
                prev_tx
            );
        } else {
            log::trace!("  -> get_tcp_stats returned None (likely IPv6 - not yet supported)");
        }
    }

    // Aggregate all UDP connections by PID
    for stats in tracker.udp_connections.values() {
        let entry = new_process_bandwidth.entry(stats.pid).or_insert_with(|| {
            // Preserve previous lifetime values for delta calculation
            let prev = tracker.process_bandwidth.get(&stats.pid);
            let (prev_lifetime_rx, prev_lifetime_tx) = prev
                .map(|p| (p.lifetime_rx_bytes, p.lifetime_tx_bytes))
                .unwrap_or((0, 0));
            let (prev_lifetime_internet_rx, prev_lifetime_internet_tx) = prev
                .map(|p| (p.lifetime_internet_rx_bytes, p.lifetime_internet_tx_bytes))
                .unwrap_or((0, 0));
            let (prev_lifetime_local_rx, prev_lifetime_local_tx) = prev
                .map(|p| (p.lifetime_local_rx_bytes, p.lifetime_local_tx_bytes))
                .unwrap_or((0, 0));

            ProcessBandwidth {
                name: stats.process_name.clone(),
                rx_bytes: 0,
                tx_bytes: 0,
                lifetime_rx_bytes: prev_lifetime_rx,
                lifetime_tx_bytes: prev_lifetime_tx,
                last_lifetime_rx_bytes: prev_lifetime_rx,
                last_lifetime_tx_bytes: prev_lifetime_tx,
                last_rx_bytes: 0,
                last_tx_bytes: 0,
                rx_rate: 0,
                tx_rate: 0,
                internet_rx_bytes: 0,
                internet_tx_bytes: 0,
                lifetime_internet_rx_bytes: prev_lifetime_internet_rx,
                lifetime_internet_tx_bytes: prev_lifetime_internet_tx,
                last_internet_rx_bytes: prev_lifetime_internet_rx,
                last_internet_tx_bytes: prev_lifetime_internet_tx,
                internet_rx_rate: 0,
                internet_tx_rate: 0,
                local_rx_bytes: 0,
                local_tx_bytes: 0,
                lifetime_local_rx_bytes: prev_lifetime_local_rx,
                lifetime_local_tx_bytes: prev_lifetime_local_tx,
                last_local_rx_bytes: prev_lifetime_local_rx,
                last_local_tx_bytes: prev_lifetime_local_tx,
                local_rx_rate: 0,
                local_tx_rate: 0,
                connection_count: 0,
            }
        });
        entry.connection_count += 1;
    }

    // Aggregate all UDP6 connections by PID
    for stats in tracker.udp6_connections.values() {
        let entry = new_process_bandwidth.entry(stats.pid).or_insert_with(|| {
            // Preserve previous lifetime values for delta calculation
            let prev = tracker.process_bandwidth.get(&stats.pid);
            let (prev_lifetime_rx, prev_lifetime_tx) = prev
                .map(|p| (p.lifetime_rx_bytes, p.lifetime_tx_bytes))
                .unwrap_or((0, 0));
            let (prev_lifetime_internet_rx, prev_lifetime_internet_tx) = prev
                .map(|p| (p.lifetime_internet_rx_bytes, p.lifetime_internet_tx_bytes))
                .unwrap_or((0, 0));
            let (prev_lifetime_local_rx, prev_lifetime_local_tx) = prev
                .map(|p| (p.lifetime_local_rx_bytes, p.lifetime_local_tx_bytes))
                .unwrap_or((0, 0));

            ProcessBandwidth {
                name: stats.process_name.clone(),
                rx_bytes: 0,
                tx_bytes: 0,
                lifetime_rx_bytes: prev_lifetime_rx,
                lifetime_tx_bytes: prev_lifetime_tx,
                last_lifetime_rx_bytes: prev_lifetime_rx,
                last_lifetime_tx_bytes: prev_lifetime_tx,
                last_rx_bytes: 0,
                last_tx_bytes: 0,
                rx_rate: 0,
                tx_rate: 0,
                internet_rx_bytes: 0,
                internet_tx_bytes: 0,
                lifetime_internet_rx_bytes: prev_lifetime_internet_rx,
                lifetime_internet_tx_bytes: prev_lifetime_internet_tx,
                last_internet_rx_bytes: prev_lifetime_internet_rx,
                last_internet_tx_bytes: prev_lifetime_internet_tx,
                internet_rx_rate: 0,
                internet_tx_rate: 0,
                local_rx_bytes: 0,
                local_tx_bytes: 0,
                lifetime_local_rx_bytes: prev_lifetime_local_rx,
                lifetime_local_tx_bytes: prev_lifetime_local_tx,
                last_local_rx_bytes: prev_lifetime_local_rx,
                last_local_tx_bytes: prev_lifetime_local_tx,
                local_rx_rate: 0,
                local_tx_rate: 0,
                connection_count: 0,
            }
        });
        entry.connection_count += 1;
    }

    // Update the tracker's process_bandwidth map
    tracker.process_bandwidth = new_process_bandwidth;

    // Apply queued connection updates (now that we're not iterating)
    for (conn_key, rx_bytes, tx_bytes) in tcp_connection_updates {
        if let Some(conn_stats) = tracker.tcp_connections.get_mut(&conn_key) {
            conn_stats.bytes_received = rx_bytes;
            conn_stats.bytes_sent = tx_bytes;
        }
    }
    for (conn_key, rx_bytes, tx_bytes) in tcp6_connection_updates {
        if let Some(conn_stats) = tracker.tcp6_connections.get_mut(&conn_key) {
            conn_stats.bytes_received = rx_bytes;
            conn_stats.bytes_sent = tx_bytes;
        }
    }

    // Apply queued interface updates
    #[cfg(target_os = "windows")]
    for (interface_name, delta_rx, delta_tx, pid, traffic_category) in interface_updates {
        // Update per-interface bandwidth
        let iface_entry = tracker
            .interface_bandwidth
            .entry(interface_name.clone())
            .or_insert_with(|| InterfaceBandwidth {
                name: interface_name.clone(),
                lifetime_rx_bytes: 0,
                lifetime_tx_bytes: 0,
                last_lifetime_rx_bytes: 0,
                last_lifetime_tx_bytes: 0,
                rx_rate: 0,
                tx_rate: 0,
            });

        iface_entry.lifetime_rx_bytes += delta_rx;
        iface_entry.lifetime_tx_bytes += delta_tx;

        // Update per-process-interface bandwidth
        let proc_iface_entry = tracker
            .process_interface_bandwidth
            .entry((pid, interface_name.clone()))
            .or_insert_with(|| ProcessInterfaceBandwidth {
                rx_bytes: 0,
                tx_bytes: 0,
                last_rx_bytes: 0,
                last_tx_bytes: 0,
                internet_rx_bytes: 0,
                internet_tx_bytes: 0,
                local_rx_bytes: 0,
                local_tx_bytes: 0,
                last_internet_rx_bytes: 0,
                last_internet_tx_bytes: 0,
                last_local_rx_bytes: 0,
                last_local_tx_bytes: 0,
            });

        proc_iface_entry.rx_bytes += delta_rx;
        proc_iface_entry.tx_bytes += delta_tx;

        match traffic_category {
            crate::traffic_classifier::TrafficCategory::Internet => {
                proc_iface_entry.internet_rx_bytes += delta_rx;
                proc_iface_entry.internet_tx_bytes += delta_tx;
            }
            crate::traffic_classifier::TrafficCategory::Local => {
                proc_iface_entry.local_rx_bytes += delta_rx;
                proc_iface_entry.local_tx_bytes += delta_tx;
            }
        }
    }

    // Calculate rates based on lifetime deltas
    if elapsed > 0.0 {
        for (pid, bandwidth) in &mut tracker.process_bandwidth {
            // Calculate delta from lifetime accumulators (not from sum of connections)
            let rx_diff = bandwidth
                .lifetime_rx_bytes
                .saturating_sub(bandwidth.last_lifetime_rx_bytes);
            let tx_diff = bandwidth
                .lifetime_tx_bytes
                .saturating_sub(bandwidth.last_lifetime_tx_bytes);

            // Log rate calculation details for processes with activity
            if rx_diff > 0 || tx_diff > 0 {
                let rx_rate = (rx_diff as f64 / elapsed) as u64;
                let tx_rate = (tx_diff as f64 / elapsed) as u64;
                log::debug!(
                    "PID {} ({}): Lifetime: RX={} TX={} | Last: RX={} TX={} | Delta: RX={} TX={} | Rate: ↓ {}/s ({:.2} MB/s) ↑ {}/s ({:.2} MB/s) | elapsed={:.2}s",
                    pid,
                    bandwidth.name,
                    bandwidth.lifetime_rx_bytes,
                    bandwidth.lifetime_tx_bytes,
                    bandwidth.last_lifetime_rx_bytes,
                    bandwidth.last_lifetime_tx_bytes,
                    rx_diff,
                    tx_diff,
                    rx_rate,
                    rx_rate as f64 / 1_048_576.0,
                    tx_rate,
                    tx_rate as f64 / 1_048_576.0,
                    elapsed
                );
            }

            bandwidth.rx_rate = (rx_diff as f64 / elapsed) as u64;
            bandwidth.tx_rate = (tx_diff as f64 / elapsed) as u64;

            // Calculate categorized rates
            let internet_rx_diff = bandwidth
                .lifetime_internet_rx_bytes
                .saturating_sub(bandwidth.last_internet_rx_bytes);
            let internet_tx_diff = bandwidth
                .lifetime_internet_tx_bytes
                .saturating_sub(bandwidth.last_internet_tx_bytes);
            let local_rx_diff = bandwidth
                .lifetime_local_rx_bytes
                .saturating_sub(bandwidth.last_local_rx_bytes);
            let local_tx_diff = bandwidth
                .lifetime_local_tx_bytes
                .saturating_sub(bandwidth.last_local_tx_bytes);

            bandwidth.internet_rx_rate = (internet_rx_diff as f64 / elapsed) as u64;
            bandwidth.internet_tx_rate = (internet_tx_diff as f64 / elapsed) as u64;
            bandwidth.local_rx_rate = (local_rx_diff as f64 / elapsed) as u64;
            bandwidth.local_tx_rate = (local_tx_diff as f64 / elapsed) as u64;

            // Update last lifetime values for next cycle
            bandwidth.last_lifetime_rx_bytes = bandwidth.lifetime_rx_bytes;
            bandwidth.last_lifetime_tx_bytes = bandwidth.lifetime_tx_bytes;
            bandwidth.last_internet_rx_bytes = bandwidth.lifetime_internet_rx_bytes;
            bandwidth.last_internet_tx_bytes = bandwidth.lifetime_internet_tx_bytes;
            bandwidth.last_local_rx_bytes = bandwidth.lifetime_local_rx_bytes;
            bandwidth.last_local_tx_bytes = bandwidth.lifetime_local_tx_bytes;
        }

        // Calculate interface rates
        for (_, iface_bandwidth) in &mut tracker.interface_bandwidth {
            let rx_diff = iface_bandwidth
                .lifetime_rx_bytes
                .saturating_sub(iface_bandwidth.last_lifetime_rx_bytes);
            let tx_diff = iface_bandwidth
                .lifetime_tx_bytes
                .saturating_sub(iface_bandwidth.last_lifetime_tx_bytes);

            iface_bandwidth.rx_rate = (rx_diff as f64 / elapsed) as u64;
            iface_bandwidth.tx_rate = (tx_diff as f64 / elapsed) as u64;

            iface_bandwidth.last_lifetime_rx_bytes = iface_bandwidth.lifetime_rx_bytes;
            iface_bandwidth.last_lifetime_tx_bytes = iface_bandwidth.lifetime_tx_bytes;
        }

        // Calculate process-interface rates (no need to store rates, they're calculated on-demand in update())
        for (_, proc_iface_bandwidth) in &mut tracker.process_interface_bandwidth {
            // Update last values for next cycle's rate calculation
            proc_iface_bandwidth.last_rx_bytes = proc_iface_bandwidth.rx_bytes;
            proc_iface_bandwidth.last_tx_bytes = proc_iface_bandwidth.tx_bytes;
            proc_iface_bandwidth.last_internet_rx_bytes = proc_iface_bandwidth.internet_rx_bytes;
            proc_iface_bandwidth.last_internet_tx_bytes = proc_iface_bandwidth.internet_tx_bytes;
            proc_iface_bandwidth.last_local_rx_bytes = proc_iface_bandwidth.local_rx_bytes;
            proc_iface_bandwidth.last_local_tx_bytes = proc_iface_bandwidth.local_tx_bytes;
        }

        tracker.last_update = now;
    }

    Ok(())
}

/// Build MIB_TCPROW structure from connection details (IPv4 only)
#[cfg(target_os = "windows")]
fn build_mib_tcprow(
    local_addr: &IpAddr,
    local_port: u16,
    remote_addr: &IpAddr,
    remote_port: u16,
) -> Option<MIB_TCPROW> {
    match (local_addr, remote_addr) {
        (IpAddr::V4(local_v4), IpAddr::V4(remote_v4)) => {
            // Build the MIB_TCPROW exactly as Windows expects it
            // Addresses: stored as u32 in native byte order
            // Ports: byte-swapped in low 16 bits
            let local_port_formatted = (((local_port & 0xFF) << 8) | (local_port >> 8)) as u32;
            let remote_port_formatted = (((remote_port & 0xFF) << 8) | (remote_port >> 8)) as u32;

            let row = MIB_TCPROW {
                dwState: 0, // Don't care for stats query
                dwLocalAddr: u32::from_ne_bytes(local_v4.octets()),
                dwLocalPort: local_port_formatted,
                dwRemoteAddr: u32::from_ne_bytes(remote_v4.octets()),
                dwRemotePort: remote_port_formatted,
            };

            // Log what we're building for debugging
            log::trace!(
                "Built MIB_TCPROW: addr=0x{:08x} port=0x{:08x} -> addr=0x{:08x} port=0x{:08x}",
                row.dwLocalAddr,
                row.dwLocalPort,
                row.dwRemoteAddr,
                row.dwRemotePort
            );

            Some(row)
        }
        _ => {
            // IPv6 requires different API (GetPerTcp6ConnectionEStats)
            log::debug!(
                "Skipping IPv6 connection {:?}:{} -> {:?}:{} (not yet supported)",
                local_addr,
                local_port,
                remote_addr,
                remote_port
            );
            None
        }
    }
}

/// Enable statistics collection for a TCP connection (requires admin)
#[cfg(target_os = "windows")]
unsafe fn enable_connection_stats(row: &MIB_TCPROW) -> bool {
    use std::slice;

    let mut rw = TCP_ESTATS_DATA_RW_v0 {
        EnableCollection: BOOLEAN(1), // TRUE
    };

    // Convert to byte slice for the Rust Windows API
    let rw_slice = slice::from_raw_parts_mut(
        &mut rw as *mut _ as *mut u8,
        std::mem::size_of::<TCP_ESTATS_DATA_RW_v0>(),
    );

    let result = SetPerTcpConnectionEStats(
        row as *const _ as *mut _,
        TcpConnectionEstatsData,
        rw_slice,
        0, // RwVersion = 0
        0, // Offset = 0 (unused)
    );

    if result != NO_ERROR.0 {
        log::trace!(
            "Failed to enable TCP stats (error {}): {}:{} -> {}:{}",
            result,
            std::net::Ipv4Addr::from(u32::from_be(row.dwLocalAddr)),
            (row.dwLocalPort & 0xFF) << 8 | (row.dwLocalPort >> 8),
            std::net::Ipv4Addr::from(u32::from_be(row.dwRemoteAddr)),
            (row.dwRemotePort & 0xFF) << 8 | (row.dwRemotePort >> 8),
        );
        false
    } else {
        log::trace!(
            "Successfully enabled TCP stats: {}:{} -> {}:{}",
            std::net::Ipv4Addr::from(u32::from_be(row.dwLocalAddr)),
            (row.dwLocalPort & 0xFF) << 8 | (row.dwLocalPort >> 8),
            std::net::Ipv4Addr::from(u32::from_be(row.dwRemoteAddr)),
            (row.dwRemotePort & 0xFF) << 8 | (row.dwRemotePort >> 8),
        );
        true
    }
}

/// Get bandwidth statistics for a TCP connection
#[cfg(target_os = "windows")]
unsafe fn get_connection_bandwidth(row: &MIB_TCPROW) -> Option<(u64, u64)> {
    use std::{mem, slice};

    let mut rod: TCP_ESTATS_DATA_ROD_v0 = mem::zeroed();

    // Convert to byte slice for the Rust Windows API
    let rod_slice = slice::from_raw_parts_mut(
        &mut rod as *mut _ as *mut u8,
        mem::size_of::<TCP_ESTATS_DATA_ROD_v0>(),
    );

    let result = GetPerTcpConnectionEStats(
        row as *const _ as *mut _,
        TcpConnectionEstatsData,
        None, // No Rw output needed
        0,    // RwVersion
        None, // No Ros output needed
        0,    // RosVersion
        Some(rod_slice),
        0, // RodVersion = 0
    );

    if result == NO_ERROR.0 {
        // DataBytesIn = bytes received (download)
        // DataBytesOut = bytes sent (upload)
        log::trace!(
            "Got TCP stats: {}:{} -> {}:{} - RX: {} bytes, TX: {} bytes",
            std::net::Ipv4Addr::from(u32::from_be(row.dwLocalAddr)),
            (row.dwLocalPort & 0xFF) << 8 | (row.dwLocalPort >> 8),
            std::net::Ipv4Addr::from(u32::from_be(row.dwRemoteAddr)),
            (row.dwRemotePort & 0xFF) << 8 | (row.dwRemotePort >> 8),
            rod.DataBytesIn,
            rod.DataBytesOut
        );
        Some((rod.DataBytesIn, rod.DataBytesOut))
    } else {
        // Connection may have closed, stats not enabled, or other error
        log::trace!(
            "Failed to get TCP stats (error {}): {}:{} -> {}:{}",
            result,
            std::net::Ipv4Addr::from(u32::from_be(row.dwLocalAddr)),
            (row.dwLocalPort & 0xFF) << 8 | (row.dwLocalPort >> 8),
            std::net::Ipv4Addr::from(u32::from_be(row.dwRemoteAddr)),
            (row.dwRemotePort & 0xFF) << 8 | (row.dwRemotePort >> 8),
        );
        None
    }
}

/// Enumerate all network interfaces on Windows using GetAdaptersAddresses API
#[cfg(target_os = "windows")]
fn enumerate_windows_interfaces() -> Result<Vec<WindowsNetworkInterface>> {
    use std::ffi::CStr;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use windows::core::PWSTR;

    unsafe {
        // Call GetAdaptersAddresses with buffer allocation
        let mut buffer_size = 15000u32; // Initial buffer size
        let mut buffer = vec![0u8; buffer_size as usize];
        let mut attempts = 0;

        loop {
            let result = GetAdaptersAddresses(
                AF_UNSPEC.0 as u32, // Get both IPv4 and IPv6
                GAA_FLAG_INCLUDE_PREFIX,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut buffer_size,
            );

            if result == NO_ERROR.0 {
                break; // Success
            } else if result == 111 && attempts < 3 {
                // ERROR_BUFFER_OVERFLOW - need larger buffer
                buffer.resize(buffer_size as usize, 0);
                attempts += 1;
            } else {
                anyhow::bail!("GetAdaptersAddresses failed with error code: {}", result);
            }
        }

        // Parse adapter list
        let mut interfaces = Vec::new();
        let mut adapter_ptr = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;

        while !adapter_ptr.is_null() {
            let adapter = &*adapter_ptr;

            // Get adapter name (ASCII)
            let name = if !adapter.AdapterName.is_null() {
                CStr::from_ptr(adapter.AdapterName.0 as *const i8)
                    .to_string_lossy()
                    .to_string()
            } else {
                String::from("Unknown")
            };

            // Get friendly name (Unicode)
            let friendly_name = if !adapter.FriendlyName.is_null() {
                let pwstr = PWSTR(adapter.FriendlyName.0);
                pwstr.to_string().unwrap_or_else(|_| name.clone())
            } else {
                name.clone()
            };

            // Get MAC address
            let mac_address = if adapter.PhysicalAddressLength >= 6 {
                Some(format!(
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    adapter.PhysicalAddress[0],
                    adapter.PhysicalAddress[1],
                    adapter.PhysicalAddress[2],
                    adapter.PhysicalAddress[3],
                    adapter.PhysicalAddress[4],
                    adapter.PhysicalAddress[5],
                ))
            } else {
                None
            };

            // Get IP addresses
            let mut ip_addresses = Vec::new();
            let mut unicast_ptr = adapter.FirstUnicastAddress;

            while !unicast_ptr.is_null() {
                let unicast = &*unicast_ptr;
                let sockaddr = &*unicast.Address.lpSockaddr;

                // Parse SOCKADDR to IpAddr
                match sockaddr.sa_family.0 {
                    2 => {
                        // AF_INET (IPv4)
                        let sockaddr_in = &*(sockaddr as *const _ as *const SOCKADDR_IN);
                        let bytes = sockaddr_in.sin_addr.S_un.S_addr.to_ne_bytes();
                        ip_addresses.push(IpAddr::V4(Ipv4Addr::from(bytes)));
                    }
                    23 => {
                        // AF_INET6 (IPv6)
                        let sockaddr_in6 = &*(sockaddr as *const _ as *const SOCKADDR_IN6);
                        let bytes = sockaddr_in6.sin6_addr.u.Byte;
                        ip_addresses.push(IpAddr::V6(Ipv6Addr::from(bytes)));
                    }
                    _ => {}
                }

                unicast_ptr = unicast.Next;
            }

            // Check if interface is up (OperStatus.0 == 1 means IfOperStatusUp)
            let is_up = adapter.OperStatus.0 == 1;

            // Check if loopback (IfType == 24 means IF_TYPE_SOFTWARE_LOOPBACK)
            let is_loopback = adapter.IfType == 24;

            interfaces.push(WindowsNetworkInterface {
                name,
                friendly_name,
                mac_address,
                ip_addresses,
                is_up,
                is_loopback,
            });

            adapter_ptr = adapter.Next;
        }

        Ok(interfaces)
    }
}

/// Get interface name for a given local IP address
#[cfg(target_os = "windows")]
fn get_interface_for_ip(
    local_addr: &IpAddr,
    interfaces: &[WindowsNetworkInterface],
) -> Option<String> {
    for interface in interfaces {
        if interface.ip_addresses.contains(local_addr) {
            // Return friendly name for user display
            return Some(interface.friendly_name.clone());
        }
    }
    None
}

/// Get TCP statistics for a specific connection (Windows only, requires admin)
#[cfg(target_os = "windows")]
fn get_tcp_stats(
    local_addr: &IpAddr,
    local_port: u16,
    remote_addr: &IpAddr,
    remote_port: u16,
) -> Option<(u64, u64)> {
    // Build MIB_TCPROW structure from connection details
    let row = build_mib_tcprow(local_addr, local_port, remote_addr, remote_port)?;

    unsafe {
        // Try to enable statistics for this connection
        // This may fail if not running as admin, but we try anyway
        // If already enabled, this is a no-op
        let _ = enable_connection_stats(&row);

        // Get the bandwidth statistics
        get_connection_bandwidth(&row)
    }
}

#[cfg(not(target_os = "windows"))]
fn get_tcp_stats(
    _local_addr: &IpAddr,
    _local_port: u16,
    _remote_addr: &IpAddr,
    _remote_port: u16,
) -> Option<(u64, u64)> {
    None
}
