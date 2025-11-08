use crate::backends::process::ProcessUtils;
use crate::process::{InterfaceInfo, InterfaceMap, ProcessInfo, ProcessMap};
use anyhow::{Context, Result};
use pnet::datalink::{self, Channel, NetworkInterface};
use pnet::packet::Packet;
use pnet::packet::ethernet::{EtherTypes, EthernetPacket};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::ipv6::Ipv6Packet;
use pnet::packet::tcp::TcpPacket;
use pnet::packet::udp::UdpPacket;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long to keep terminated processes visible (in seconds)
const TERMINATED_DISPLAY_DURATION: Duration = Duration::from_secs(5);

/// Connection identifier for tracking packets
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
    Udp,
}

/// Tracks network activity per process using packet capture
pub struct NetworkMonitor {
    // Shared state between packet capture thread and update thread
    bandwidth_tracker: Arc<Mutex<BandwidthTracker>>,
    // Platform-specific process utilities
    process_utils: Box<dyn ProcessUtils>,
    // Socket mapper backend information
    socket_mapper_name: String,
    socket_mapper_capabilities: crate::backends::BackendCapabilities,
    // Handles to packet capture threads (one per interface)
    _capture_handles: Vec<thread::JoinHandle<()>>,
    last_update: Instant,
}

struct BandwidthTracker {
    // Connection -> PID mapping
    connection_map: HashMap<ConnectionKey, i32>,
    // Socket inode -> (PID, process name) mapping
    socket_map: HashMap<u64, (i32, String)>,
    // PID -> bandwidth tracking
    process_bandwidth: HashMap<i32, ProcessBandwidth>,
    // PID -> termination time (for showing terminated processes temporarily)
    terminated_processes: HashMap<i32, Instant>,
    // Interface statistics: interface_name -> InterfaceBandwidth
    interface_bandwidth: HashMap<String, InterfaceBandwidth>,
    // Per-process, per-interface stats: (pid, interface) -> bandwidth
    process_interface_bandwidth: HashMap<(i32, String), ProcessInterfaceBandwidth>,
    // Packet capture statistics for debugging
    packets_captured: u64,
    packets_matched: u64,
    packets_unmatched: u64,
}

struct ProcessBandwidth {
    name: String,
    rx_bytes: u64,
    tx_bytes: u64,
    last_rx_bytes: u64,
    last_tx_bytes: u64,
    // NEW: Traffic categorization
    internet_rx_bytes: u64,
    internet_tx_bytes: u64,
    local_rx_bytes: u64,
    local_tx_bytes: u64,
    last_internet_rx_bytes: u64,
    last_internet_tx_bytes: u64,
    last_local_rx_bytes: u64,
    last_local_tx_bytes: u64,
}

struct InterfaceBandwidth {
    name: String,
    rx_bytes: u64,
    tx_bytes: u64,
    last_rx_bytes: u64,
    last_tx_bytes: u64,
}

struct ProcessInterfaceBandwidth {
    rx_bytes: u64,
    tx_bytes: u64,
    last_rx_bytes: u64,
    last_tx_bytes: u64,
    // NEW: Traffic categorization
    internet_rx_bytes: u64,
    internet_tx_bytes: u64,
    local_rx_bytes: u64,
    local_tx_bytes: u64,
    last_internet_rx_bytes: u64,
    last_internet_tx_bytes: u64,
    last_local_rx_bytes: u64,
    last_local_tx_bytes: u64,
}

impl NetworkMonitor {
    pub fn new() -> Result<Self> {
        Self::with_socket_mapper(None)
    }

    pub fn with_socket_mapper(socket_mapper_preference: Option<&str>) -> Result<Self> {
        let bandwidth_tracker = Arc::new(Mutex::new(BandwidthTracker {
            connection_map: HashMap::new(),
            socket_map: HashMap::new(),
            process_bandwidth: HashMap::new(),
            terminated_processes: HashMap::new(),
            interface_bandwidth: HashMap::new(),
            process_interface_bandwidth: HashMap::new(),
            packets_captured: 0,
            packets_matched: 0,
            packets_unmatched: 0,
        }));

        // Create platform-specific process utilities
        let process_utils = crate::backends::process::create_process_utils_with_socket_mapper(
            socket_mapper_preference,
        );

        // Get socket mapper info before moving process_utils
        let (socket_mapper_name, socket_mapper_capabilities) = {
            #[cfg(target_os = "linux")]
            {
                use crate::backends::process::LinuxProcessUtils;
                // We need to get the info from the actual implementation
                // Create a temporary one to get the info
                let temp_utils = crate::backends::process::LinuxProcessUtils::with_socket_mapper(
                    socket_mapper_preference,
                );
                (
                    temp_utils.socket_mapper_name().to_string(),
                    temp_utils.socket_mapper_capabilities(),
                )
            }
            #[cfg(target_os = "macos")]
            {
                use crate::backends::process::MacOSProcessUtils;
                let temp_utils = crate::backends::process::MacOSProcessUtils::with_socket_mapper(
                    socket_mapper_preference,
                );
                (
                    temp_utils.socket_mapper_name().to_string(),
                    temp_utils.socket_mapper_capabilities(),
                )
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                (
                    "unknown".to_string(),
                    crate::backends::BackendCapabilities::default(),
                )
            }
        };

        // Create monitor instance first (without starting capture thread yet)
        let mut monitor = Self {
            bandwidth_tracker,
            process_utils,
            socket_mapper_name,
            socket_mapper_capabilities,
            _capture_handles: Vec::new(),
            last_update: Instant::now(),
        };

        // CRITICAL: Populate socket and connection maps BEFORE starting packet capture
        // This ensures that packets arriving immediately after startup are properly tracked
        // Without this, all packets in the first second are lost because connection_map is empty
        monitor.update_socket_map()?;

        // Start packet capture threads for all interfaces
        let interfaces = Self::find_all_interfaces();
        log::info!("Starting packet capture on {} interfaces", interfaces.len());

        for interface in interfaces {
            let tracker_clone = Arc::clone(&monitor.bandwidth_tracker);
            let iface_name = interface.name.clone();

            let capture_handle = thread::spawn(move || {
                if let Err(e) = Self::capture_packets_on_interface(interface, tracker_clone) {
                    log::error!("Packet capture error on {}: {}", iface_name, e);
                }
            });

            monitor._capture_handles.push(capture_handle);
        }

        Ok(monitor)
    }

    /// Get socket mapper backend information
    pub fn get_socket_mapper_info(&self) -> (&str, &crate::backends::BackendCapabilities) {
        (&self.socket_mapper_name, &self.socket_mapper_capabilities)
    }

    pub fn update(&mut self) -> Result<(ProcessMap, InterfaceMap)> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();

        // Update socket inode mapping
        self.update_socket_map()?;

        // Calculate rates and build process map and interface map
        let mut process_map = ProcessMap::new();
        let mut interface_map = InterfaceMap::new();
        let mut tracker = self.bandwidth_tracker.lock().unwrap();

        // First pass: collect all data we need (to avoid multiple borrows)
        let mut process_data = Vec::new();

        for (&pid, bandwidth) in &tracker.process_bandwidth {
            let rx_diff = bandwidth.rx_bytes.saturating_sub(bandwidth.last_rx_bytes);
            let tx_diff = bandwidth.tx_bytes.saturating_sub(bandwidth.last_tx_bytes);

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

            // NEW: Calculate categorized rates
            let internet_rx_diff = bandwidth
                .internet_rx_bytes
                .saturating_sub(bandwidth.last_internet_rx_bytes);
            let internet_tx_diff = bandwidth
                .internet_tx_bytes
                .saturating_sub(bandwidth.last_internet_tx_bytes);
            let local_rx_diff = bandwidth
                .local_rx_bytes
                .saturating_sub(bandwidth.last_local_rx_bytes);
            let local_tx_diff = bandwidth
                .local_tx_bytes
                .saturating_sub(bandwidth.last_local_tx_bytes);

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

            let process_exists = self.process_utils.process_exists(pid);
            let term_time = tracker.terminated_processes.get(&pid).copied();

            process_data.push((
                pid,
                bandwidth.name.clone(),
                bandwidth.rx_bytes,
                bandwidth.tx_bytes,
                download_rate,
                upload_rate,
                bandwidth.internet_rx_bytes,
                bandwidth.internet_tx_bytes,
                internet_download_rate,
                internet_upload_rate,
                bandwidth.local_rx_bytes,
                bandwidth.local_tx_bytes,
                local_download_rate,
                local_upload_rate,
                process_exists,
                term_time,
            ));
        }

        // Update last values now that we're done reading
        for (&pid, bandwidth) in &mut tracker.process_bandwidth {
            bandwidth.last_rx_bytes = bandwidth.rx_bytes;
            bandwidth.last_tx_bytes = bandwidth.tx_bytes;
            // NEW: Update categorized last values
            bandwidth.last_internet_rx_bytes = bandwidth.internet_rx_bytes;
            bandwidth.last_internet_tx_bytes = bandwidth.internet_tx_bytes;
            bandwidth.last_local_rx_bytes = bandwidth.local_rx_bytes;
            bandwidth.last_local_tx_bytes = bandwidth.local_tx_bytes;
        }

        // Track PIDs to remove and newly terminated
        let mut pids_to_remove = Vec::new();
        let mut newly_terminated = Vec::new();

        // Second pass: build process map based on collected data
        for (
            pid,
            name,
            rx_bytes,
            tx_bytes,
            download_rate,
            upload_rate,
            internet_rx_bytes,
            internet_tx_bytes,
            internet_download_rate,
            internet_upload_rate,
            local_rx_bytes,
            local_tx_bytes,
            local_download_rate,
            local_upload_rate,
            process_exists,
            term_time,
        ) in process_data
        {
            if process_exists {
                // Process is alive - include it normally
                let mut proc_info = ProcessInfo::new(pid, name);
                proc_info.download_rate = download_rate;
                proc_info.upload_rate = upload_rate;
                proc_info.total_download = rx_bytes;
                proc_info.total_upload = tx_bytes;
                proc_info.is_terminated = false;

                // Populate categorized traffic fields
                proc_info.internet_download_rate = internet_download_rate;
                proc_info.internet_upload_rate = internet_upload_rate;
                proc_info.internet_total_download = internet_rx_bytes;
                proc_info.internet_total_upload = internet_tx_bytes;
                proc_info.local_download_rate = local_download_rate;
                proc_info.local_upload_rate = local_upload_rate;
                proc_info.local_total_download = local_rx_bytes;
                proc_info.local_total_upload = local_tx_bytes;

                // Populate per-interface stats for this process
                proc_info.interface_stats = tracker
                    .process_interface_bandwidth
                    .iter()
                    .filter(|((p, _), _)| *p == pid)
                    .map(|((_, iface), bw)| {
                        let rx_diff = bw.rx_bytes.saturating_sub(bw.last_rx_bytes);
                        let tx_diff = bw.tx_bytes.saturating_sub(bw.last_tx_bytes);

                        let iface_download_rate = if elapsed > 0.0 {
                            (rx_diff as f64 / elapsed) as u64
                        } else {
                            0
                        };

                        let iface_upload_rate = if elapsed > 0.0 {
                            (tx_diff as f64 / elapsed) as u64
                        } else {
                            0
                        };

                        // NEW: Calculate categorized rates
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
                                download_rate: iface_download_rate,
                                upload_rate: iface_upload_rate,
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

                process_map.insert(pid, proc_info);
            } else {
                // Process terminated
                if let Some(term_time) = term_time {
                    let since_termination = now.duration_since(term_time);

                    if since_termination < TERMINATED_DISPLAY_DURATION {
                        // Still within display window - show with skull icon
                        let mut proc_info = ProcessInfo::new(pid, name);
                        proc_info.download_rate = 0;
                        proc_info.upload_rate = 0;
                        proc_info.total_download = rx_bytes;
                        proc_info.total_upload = tx_bytes;
                        proc_info.is_terminated = true;

                        // Populate categorized traffic totals (rates are 0 for terminated)
                        proc_info.internet_download_rate = 0;
                        proc_info.internet_upload_rate = 0;
                        proc_info.internet_total_download = internet_rx_bytes;
                        proc_info.internet_total_upload = internet_tx_bytes;
                        proc_info.local_download_rate = 0;
                        proc_info.local_upload_rate = 0;
                        proc_info.local_total_download = local_rx_bytes;
                        proc_info.local_total_upload = local_tx_bytes;

                        process_map.insert(pid, proc_info);
                    } else {
                        // Too old - mark for removal
                        pids_to_remove.push(pid);
                    }
                } else {
                    // Newly terminated - show it and record termination time
                    newly_terminated.push(pid);

                    let mut proc_info = ProcessInfo::new(pid, name);
                    proc_info.download_rate = 0;
                    proc_info.upload_rate = 0;
                    proc_info.total_download = rx_bytes;
                    proc_info.total_upload = tx_bytes;
                    proc_info.is_terminated = true;

                    // Populate categorized traffic totals (rates are 0 for terminated)
                    proc_info.internet_download_rate = 0;
                    proc_info.internet_upload_rate = 0;
                    proc_info.internet_total_download = internet_rx_bytes;
                    proc_info.internet_total_upload = internet_tx_bytes;
                    proc_info.local_download_rate = 0;
                    proc_info.local_upload_rate = 0;
                    proc_info.local_total_download = local_rx_bytes;
                    proc_info.local_total_upload = local_tx_bytes;

                    process_map.insert(pid, proc_info);
                }
            }
        }

        // Record newly terminated processes
        for pid in newly_terminated {
            tracker.terminated_processes.insert(pid, now);
        }

        // Clean up old terminated processes
        for pid in pids_to_remove {
            tracker.process_bandwidth.remove(&pid);
            tracker.terminated_processes.remove(&pid);
        }

        // Build interface map with aggregated statistics
        let interfaces = Self::find_all_interfaces();
        for interface in interfaces {
            let iface_name = interface.name.clone();

            // Get interface bandwidth stats
            let iface_bandwidth = tracker.interface_bandwidth.get(&iface_name);

            let (total_download_rate, total_upload_rate) = if let Some(bw) = iface_bandwidth {
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

                (download_rate, upload_rate)
            } else {
                (0, 0)
            };

            // Count processes using this interface
            let process_count = tracker
                .process_interface_bandwidth
                .keys()
                .filter(|(_, iface)| iface == &iface_name)
                .map(|(pid, _)| pid)
                .collect::<HashSet<_>>()
                .len();

            // Extract MAC address
            let mac_address = interface.mac.map(|mac| {
                format!(
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac.0, mac.1, mac.2, mac.3, mac.4, mac.5
                )
            });

            // Extract IP addresses
            let ip_addresses: Vec<IpAddr> = interface.ips.iter().map(|ip| ip.ip()).collect();

            interface_map.insert(
                iface_name.clone(),
                InterfaceInfo {
                    name: iface_name,
                    mac_address,
                    ip_addresses,
                    is_up: interface.is_up(),
                    is_loopback: interface.is_loopback(),
                    total_download_rate,
                    total_upload_rate,
                    process_count,
                },
            );
        }

        // Update last values for interface bandwidth
        for (_, bw) in &mut tracker.interface_bandwidth {
            bw.last_rx_bytes = bw.rx_bytes;
            bw.last_tx_bytes = bw.tx_bytes;
        }

        // Update last values for process-interface bandwidth
        for (_, bw) in &mut tracker.process_interface_bandwidth {
            bw.last_rx_bytes = bw.rx_bytes;
            bw.last_tx_bytes = bw.tx_bytes;
            bw.last_internet_rx_bytes = bw.internet_rx_bytes;
            bw.last_internet_tx_bytes = bw.internet_tx_bytes;
            bw.last_local_rx_bytes = bw.local_rx_bytes;
            bw.last_local_tx_bytes = bw.local_tx_bytes;
        }

        self.last_update = now;
        Ok((process_map, interface_map))
    }

    /// Update socket inode -> PID mapping
    /// Uses a two-phase update to avoid race conditions:
    /// 1. Build new maps while old maps are still valid for packet capture
    /// 2. Atomically swap old maps with new maps
    fn update_socket_map(&self) -> Result<()> {
        // Phase 1: Build new maps WITHOUT holding the lock
        // This allows packet capture to continue using old maps

        // Get connection map from platform-specific process utils
        let conn_map = self.process_utils.get_connection_map()?;

        let mut new_socket_map = conn_map.socket_to_pid;
        let mut new_connection_map = HashMap::new();

        // Map TCP connections to PIDs via socket inodes
        for entry in conn_map.tcp_connections {
            if let Some(&(pid, _)) = new_socket_map.get(&entry.inode) {
                let key = ConnectionKey {
                    local_addr: entry.local_addr,
                    local_port: entry.local_port,
                    remote_addr: entry.remote_addr,
                    remote_port: entry.remote_port,
                    protocol: Protocol::Tcp,
                };
                new_connection_map.insert(key, pid);
            }
        }

        // Map TCP6 connections
        for entry in conn_map.tcp6_connections {
            if let Some(&(pid, _)) = new_socket_map.get(&entry.inode) {
                let key = ConnectionKey {
                    local_addr: entry.local_addr,
                    local_port: entry.local_port,
                    remote_addr: entry.remote_addr,
                    remote_port: entry.remote_port,
                    protocol: Protocol::Tcp,
                };
                new_connection_map.insert(key, pid);
            }
        }

        // Map UDP connections
        for entry in conn_map.udp_connections {
            if let Some(&(pid, _)) = new_socket_map.get(&entry.inode) {
                let key = ConnectionKey {
                    local_addr: entry.local_addr,
                    local_port: entry.local_port,
                    remote_addr: entry.remote_addr,
                    remote_port: entry.remote_port,
                    protocol: Protocol::Udp,
                };
                new_connection_map.insert(key, pid);
            }
        }

        // Map UDP6 connections
        for entry in conn_map.udp6_connections {
            if let Some(&(pid, _)) = new_socket_map.get(&entry.inode) {
                let key = ConnectionKey {
                    local_addr: entry.local_addr,
                    local_port: entry.local_port,
                    remote_addr: entry.remote_addr,
                    remote_port: entry.remote_port,
                    protocol: Protocol::Udp,
                };
                new_connection_map.insert(key, pid);
            }
        }

        // Phase 2: Build list of PIDs with connections and their names
        // This ensures all processes with active connections will be visible in the UI
        // even if no packets have been captured yet
        let pids_with_names: Vec<(i32, String)> = new_connection_map
            .values()
            .copied()
            .collect::<HashSet<i32>>()
            .into_iter()
            .map(|pid| {
                let name = new_socket_map
                    .values()
                    .find(|(p, _)| *p == pid)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| format!("PID {}", pid));
                (pid, name)
            })
            .collect();

        // Phase 3: Atomically replace old maps with new maps and initialize process_bandwidth
        // This is the ONLY point where we hold the lock and modify the maps
        // The critical section is minimal - just pointer swaps and initialization
        let mut tracker = self.bandwidth_tracker.lock().unwrap();

        log::debug!(
            "Updating connection maps: {} sockets, {} connections (TCP+UDP)",
            new_socket_map.len(),
            new_connection_map.len()
        );

        tracker.socket_map = new_socket_map;
        tracker.connection_map = new_connection_map;

        // Initialize process_bandwidth entries for all processes with active connections
        // This prevents processes from being "invisible" if they have connections
        // but haven't had packets captured yet (solves the cold start problem)
        for (pid, name) in pids_with_names {
            tracker
                .process_bandwidth
                .entry(pid)
                .or_insert(ProcessBandwidth {
                    name,
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
        }

        Ok(())
    }

    /// Packet capture thread - runs continuously on a specific interface
    fn capture_packets_on_interface(
        interface: NetworkInterface,
        tracker: Arc<Mutex<BandwidthTracker>>,
    ) -> Result<()> {
        let iface_name = interface.name.clone();
        log::info!("Packet capture thread started on interface: {}", iface_name);
        log::debug!("Interface details: {:?}", interface);

        // Create channel for packet capture
        let (_, mut rx) = match datalink::channel(&interface, Default::default()) {
            Ok(Channel::Ethernet(tx, rx)) => {
                log::info!(
                    "Successfully created packet capture channel for {}",
                    iface_name
                );
                (tx, rx)
            }
            Ok(_) => {
                return Err(anyhow::anyhow!(
                    "Unsupported channel type for {}",
                    iface_name
                ));
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Failed to create channel for {}: {}",
                    iface_name,
                    e
                ));
            }
        };

        // Capture packets
        loop {
            match rx.next() {
                Ok(packet) => {
                    if let Err(e) = Self::process_packet(packet, &iface_name, &tracker) {
                        // Don't spam errors, just continue
                        if std::env::var("DEBUG").is_ok() {
                            log::error!("Packet processing error on {}: {}", iface_name, e);
                        }
                    }
                }
                Err(e) => {
                    log::error!("Packet receive error on {}: {}", iface_name, e);
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    fn find_all_interfaces() -> Vec<NetworkInterface> {
        let interfaces = datalink::interfaces();

        // Return all interfaces that are up and have IP addresses
        // We include loopback as it can be useful for local services
        interfaces
            .into_iter()
            .filter(|iface| iface.is_up() && !iface.ips.is_empty())
            .collect()
    }

    fn process_packet(
        packet: &[u8],
        interface_name: &str,
        tracker: &Arc<Mutex<BandwidthTracker>>,
    ) -> Result<()> {
        let ethernet = EthernetPacket::new(packet).context("Failed to parse Ethernet packet")?;

        match ethernet.get_ethertype() {
            EtherTypes::Ipv4 => {
                if let Some(ipv4) = Ipv4Packet::new(ethernet.payload()) {
                    Self::process_ipv4_packet(&ipv4, packet.len(), interface_name, tracker)?;
                }
            }
            EtherTypes::Ipv6 => {
                if let Some(ipv6) = Ipv6Packet::new(ethernet.payload()) {
                    Self::process_ipv6_packet(&ipv6, packet.len(), interface_name, tracker)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn process_ipv4_packet(
        ipv4: &Ipv4Packet,
        packet_len: usize,
        interface_name: &str,
        tracker: &Arc<Mutex<BandwidthTracker>>,
    ) -> Result<()> {
        let src_addr = IpAddr::V4(ipv4.get_source());
        let dst_addr = IpAddr::V4(ipv4.get_destination());

        match ipv4.get_next_level_protocol() {
            IpNextHeaderProtocols::Tcp => {
                if let Some(tcp) = TcpPacket::new(ipv4.payload()) {
                    Self::track_connection(
                        src_addr,
                        tcp.get_source(),
                        dst_addr,
                        tcp.get_destination(),
                        Protocol::Tcp,
                        packet_len,
                        interface_name,
                        tracker,
                    );
                }
            }
            IpNextHeaderProtocols::Udp => {
                if let Some(udp) = UdpPacket::new(ipv4.payload()) {
                    Self::track_connection(
                        src_addr,
                        udp.get_source(),
                        dst_addr,
                        udp.get_destination(),
                        Protocol::Udp,
                        packet_len,
                        interface_name,
                        tracker,
                    );
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn process_ipv6_packet(
        ipv6: &Ipv6Packet,
        packet_len: usize,
        interface_name: &str,
        tracker: &Arc<Mutex<BandwidthTracker>>,
    ) -> Result<()> {
        let src_addr = IpAddr::V6(ipv6.get_source());
        let dst_addr = IpAddr::V6(ipv6.get_destination());

        match ipv6.get_next_header() {
            IpNextHeaderProtocols::Tcp => {
                if let Some(tcp) = TcpPacket::new(ipv6.payload()) {
                    Self::track_connection(
                        src_addr,
                        tcp.get_source(),
                        dst_addr,
                        tcp.get_destination(),
                        Protocol::Tcp,
                        packet_len,
                        interface_name,
                        tracker,
                    );
                }
            }
            IpNextHeaderProtocols::Udp => {
                if let Some(udp) = UdpPacket::new(ipv6.payload()) {
                    Self::track_connection(
                        src_addr,
                        udp.get_source(),
                        dst_addr,
                        udp.get_destination(),
                        Protocol::Udp,
                        packet_len,
                        interface_name,
                        tracker,
                    );
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn track_connection(
        src_addr: IpAddr,
        src_port: u16,
        dst_addr: IpAddr,
        dst_port: u16,
        protocol: Protocol,
        packet_len: usize,
        interface_name: &str,
        tracker: &Arc<Mutex<BandwidthTracker>>,
    ) {
        let mut tracker = tracker.lock().unwrap();
        tracker.packets_captured += 1;

        // Try both directions to find which one matches our connection map
        let key_outbound = ConnectionKey {
            local_addr: src_addr,
            local_port: src_port,
            remote_addr: dst_addr,
            remote_port: dst_port,
            protocol,
        };

        let key_inbound = ConnectionKey {
            local_addr: dst_addr,
            local_port: dst_port,
            remote_addr: src_addr,
            remote_port: src_port,
            protocol,
        };

        // Check for exact matches first
        let mut pid_and_direction: Option<(i32, bool)> = None; // (pid, is_outbound)

        if let Some(&pid) = tracker.connection_map.get(&key_outbound) {
            pid_and_direction = Some((pid, true));
        } else if let Some(&pid) = tracker.connection_map.get(&key_inbound) {
            pid_and_direction = Some((pid, false));
        } else if protocol == Protocol::Udp {
            // Try UDP wildcard matching (for unconnected UDP sockets)
            // Look for UDP socket with matching local addr/port but wildcard remote (0.0.0.0:0)
            for (key, &pid) in tracker.connection_map.iter() {
                if key.protocol == Protocol::Udp
                    && key.local_addr == src_addr
                    && key.local_port == src_port
                    && (key.remote_addr.is_unspecified() || key.remote_port == 0)
                {
                    pid_and_direction = Some((pid, true));
                    break;
                }
            }

            // If still no match, try inbound direction for UDP wildcard
            if pid_and_direction.is_none() {
                for (key, &pid) in tracker.connection_map.iter() {
                    if key.protocol == Protocol::Udp
                        && key.local_addr == dst_addr
                        && key.local_port == dst_port
                        && (key.remote_addr.is_unspecified() || key.remote_port == 0)
                    {
                        pid_and_direction = Some((pid, false));
                        break;
                    }
                }
            }
        }

        if let Some((pid, is_outbound)) = pid_and_direction {
            tracker.packets_matched += 1;

            let name = tracker
                .socket_map
                .values()
                .find(|(p, _)| *p == pid)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| format!("PID {}", pid));

            // NEW: Categorize traffic based on remote IP
            let remote_ip = if is_outbound { dst_addr } else { src_addr };
            let traffic_category = crate::traffic_classifier::categorize_traffic(&remote_ip);

            // Track overall process bandwidth
            let bandwidth = tracker
                .process_bandwidth
                .entry(pid)
                .or_insert(ProcessBandwidth {
                    name,
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

            if is_outbound {
                bandwidth.tx_bytes += packet_len as u64;
                // NEW: Categorized upload
                match traffic_category {
                    crate::traffic_classifier::TrafficCategory::Internet => {
                        bandwidth.internet_tx_bytes += packet_len as u64;
                    }
                    crate::traffic_classifier::TrafficCategory::Local => {
                        bandwidth.local_tx_bytes += packet_len as u64;
                    }
                }
            } else {
                bandwidth.rx_bytes += packet_len as u64;
                // NEW: Categorized download
                match traffic_category {
                    crate::traffic_classifier::TrafficCategory::Internet => {
                        bandwidth.internet_rx_bytes += packet_len as u64;
                    }
                    crate::traffic_classifier::TrafficCategory::Local => {
                        bandwidth.local_rx_bytes += packet_len as u64;
                    }
                }
            }

            // Track per-process, per-interface bandwidth
            let proc_iface_bandwidth = tracker
                .process_interface_bandwidth
                .entry((pid, interface_name.to_string()))
                .or_insert(ProcessInterfaceBandwidth {
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

            if is_outbound {
                proc_iface_bandwidth.tx_bytes += packet_len as u64;
                // NEW: Categorized upload
                match traffic_category {
                    crate::traffic_classifier::TrafficCategory::Internet => {
                        proc_iface_bandwidth.internet_tx_bytes += packet_len as u64;
                    }
                    crate::traffic_classifier::TrafficCategory::Local => {
                        proc_iface_bandwidth.local_tx_bytes += packet_len as u64;
                    }
                }
            } else {
                proc_iface_bandwidth.rx_bytes += packet_len as u64;
                // NEW: Categorized download
                match traffic_category {
                    crate::traffic_classifier::TrafficCategory::Internet => {
                        proc_iface_bandwidth.internet_rx_bytes += packet_len as u64;
                    }
                    crate::traffic_classifier::TrafficCategory::Local => {
                        proc_iface_bandwidth.local_rx_bytes += packet_len as u64;
                    }
                }
            }

            // Track interface-level bandwidth
            let iface_bandwidth = tracker
                .interface_bandwidth
                .entry(interface_name.to_string())
                .or_insert(InterfaceBandwidth {
                    name: interface_name.to_string(),
                    rx_bytes: 0,
                    tx_bytes: 0,
                    last_rx_bytes: 0,
                    last_tx_bytes: 0,
                });

            if is_outbound {
                iface_bandwidth.tx_bytes += packet_len as u64;
            } else {
                iface_bandwidth.rx_bytes += packet_len as u64;
            }
        } else {
            tracker.packets_unmatched += 1;

            // Still track interface bandwidth even if we can't match to a process
            // This is important for seeing total interface utilization
            // We'll count it as RX since we don't know the direction
            let iface_bandwidth = tracker
                .interface_bandwidth
                .entry(interface_name.to_string())
                .or_insert(InterfaceBandwidth {
                    name: interface_name.to_string(),
                    rx_bytes: 0,
                    tx_bytes: 0,
                    last_rx_bytes: 0,
                    last_tx_bytes: 0,
                });
            iface_bandwidth.rx_bytes += packet_len as u64;

            // Log first few unmatched packets for debugging
            if tracker.packets_unmatched <= 10 {
                log::debug!(
                    "Unmatched packet: {}:{} -> {}:{} ({:?}, {} bytes)",
                    src_addr,
                    src_port,
                    dst_addr,
                    dst_port,
                    protocol,
                    packet_len
                );
            } else if tracker.packets_unmatched == 11 {
                log::debug!("Further unmatched packets will not be logged");
            }
        }

        // Periodic statistics logging
        if tracker.packets_captured % 1000 == 0 {
            log::debug!(
                "Packet stats: captured={}, matched={}, unmatched={} (match rate: {:.1}%)",
                tracker.packets_captured,
                tracker.packets_matched,
                tracker.packets_unmatched,
                (tracker.packets_matched as f64 / tracker.packets_captured as f64) * 100.0
            );
        }
    }
}
