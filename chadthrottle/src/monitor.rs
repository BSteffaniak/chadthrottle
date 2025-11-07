use crate::process::{ProcessInfo, ProcessMap};
use anyhow::{Context, Result};
use pnet::datalink::{self, Channel, NetworkInterface};
use pnet::packet::Packet;
use pnet::packet::ethernet::{EtherTypes, EthernetPacket};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::ipv6::Ipv6Packet;
use pnet::packet::tcp::TcpPacket;
use pnet::packet::udp::UdpPacket;
use procfs::process::{FDTarget, all_processes};
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
    // Handle to packet capture thread
    _capture_handle: Option<thread::JoinHandle<()>>,
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
}

impl NetworkMonitor {
    pub fn new() -> Result<Self> {
        let bandwidth_tracker = Arc::new(Mutex::new(BandwidthTracker {
            connection_map: HashMap::new(),
            socket_map: HashMap::new(),
            process_bandwidth: HashMap::new(),
            terminated_processes: HashMap::new(),
            packets_captured: 0,
            packets_matched: 0,
            packets_unmatched: 0,
        }));

        // Create monitor instance first (without starting capture thread yet)
        let mut monitor = Self {
            bandwidth_tracker,
            _capture_handle: None,
            last_update: Instant::now(),
        };

        // CRITICAL: Populate socket and connection maps BEFORE starting packet capture
        // This ensures that packets arriving immediately after startup are properly tracked
        // Without this, all packets in the first second are lost because connection_map is empty
        monitor.update_socket_map()?;

        // Now start packet capture thread with pre-populated maps
        let tracker_clone = Arc::clone(&monitor.bandwidth_tracker);
        let capture_handle = thread::spawn(move || {
            if let Err(e) = Self::capture_packets(tracker_clone) {
                log::error!("Packet capture error: {}", e);
            }
        });

        monitor._capture_handle = Some(capture_handle);

        Ok(monitor)
    }

    pub fn update(&mut self) -> Result<ProcessMap> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();

        // Update socket inode mapping
        self.update_socket_map()?;

        // Calculate rates and build process map
        let mut process_map = ProcessMap::new();
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

            let process_exists = procfs::process::Process::new(pid).is_ok();
            let term_time = tracker.terminated_processes.get(&pid).copied();

            process_data.push((
                pid,
                bandwidth.name.clone(),
                bandwidth.rx_bytes,
                bandwidth.tx_bytes,
                download_rate,
                upload_rate,
                process_exists,
                term_time,
            ));
        }

        // Update last values now that we're done reading
        for (&pid, bandwidth) in &mut tracker.process_bandwidth {
            bandwidth.last_rx_bytes = bandwidth.rx_bytes;
            bandwidth.last_tx_bytes = bandwidth.tx_bytes;
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

        self.last_update = now;
        Ok(process_map)
    }

    /// Update socket inode -> PID mapping
    /// Uses a two-phase update to avoid race conditions:
    /// 1. Build new maps while old maps are still valid for packet capture
    /// 2. Atomically swap old maps with new maps
    fn update_socket_map(&self) -> Result<()> {
        // Phase 1: Build new maps WITHOUT holding the lock
        // This allows packet capture to continue using old maps
        let mut new_socket_map = HashMap::new();
        let mut new_connection_map = HashMap::new();

        // Build socket inode -> PID map
        let all_procs = all_processes()?;

        for proc_result in all_procs {
            if let Ok(process) = proc_result {
                let pid = process.pid();

                let name = if let Ok(stat) = process.stat() {
                    stat.comm
                } else {
                    format!("PID {}", pid)
                };

                // Scan file descriptors for sockets
                if let Ok(fds) = process.fd() {
                    for fd_result in fds {
                        if let Ok(fd_info) = fd_result {
                            if let FDTarget::Socket(inode) = fd_info.target {
                                new_socket_map.insert(inode, (pid, name.clone()));
                            }
                        }
                    }
                }
            }
        }

        // Map connections to PIDs via socket inodes
        if let Ok(tcp_entries) = procfs::net::tcp() {
            for entry in tcp_entries {
                if let Some(&(pid, _)) = new_socket_map.get(&entry.inode) {
                    let key = ConnectionKey {
                        local_addr: entry.local_address.ip(),
                        local_port: entry.local_address.port(),
                        remote_addr: entry.remote_address.ip(),
                        remote_port: entry.remote_address.port(),
                        protocol: Protocol::Tcp,
                    };
                    new_connection_map.insert(key, pid);
                }
            }
        }

        if let Ok(tcp6_entries) = procfs::net::tcp6() {
            for entry in tcp6_entries {
                if let Some(&(pid, _)) = new_socket_map.get(&entry.inode) {
                    let key = ConnectionKey {
                        local_addr: entry.local_address.ip(),
                        local_port: entry.local_address.port(),
                        remote_addr: entry.remote_address.ip(),
                        remote_port: entry.remote_address.port(),
                        protocol: Protocol::Tcp,
                    };
                    new_connection_map.insert(key, pid);
                }
            }
        }

        if let Ok(udp_entries) = procfs::net::udp() {
            for entry in udp_entries {
                if let Some(&(pid, _)) = new_socket_map.get(&entry.inode) {
                    let key = ConnectionKey {
                        local_addr: entry.local_address.ip(),
                        local_port: entry.local_address.port(),
                        remote_addr: entry.remote_address.ip(),
                        remote_port: entry.remote_address.port(),
                        protocol: Protocol::Udp,
                    };
                    new_connection_map.insert(key, pid);
                }
            }
        }

        if let Ok(udp6_entries) = procfs::net::udp6() {
            for entry in udp6_entries {
                if let Some(&(pid, _)) = new_socket_map.get(&entry.inode) {
                    let key = ConnectionKey {
                        local_addr: entry.local_address.ip(),
                        local_port: entry.local_address.port(),
                        remote_addr: entry.remote_address.ip(),
                        remote_port: entry.remote_address.port(),
                        protocol: Protocol::Udp,
                    };
                    new_connection_map.insert(key, pid);
                }
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
                });
        }

        Ok(())
    }

    /// Packet capture thread - runs continuously
    fn capture_packets(tracker: Arc<Mutex<BandwidthTracker>>) -> Result<()> {
        // Find a suitable network interface
        let interface = Self::find_interface().context("Failed to find network interface")?;
        log::info!(
            "Packet capture thread started on interface: {}",
            interface.name
        );
        log::debug!("Interface details: {:?}", interface);

        // Create channel for packet capture
        let (_, mut rx) = match datalink::channel(&interface, Default::default()) {
            Ok(Channel::Ethernet(tx, rx)) => {
                log::info!("Successfully created packet capture channel");
                (tx, rx)
            }
            Ok(_) => return Err(anyhow::anyhow!("Unsupported channel type")),
            Err(e) => return Err(anyhow::anyhow!("Failed to create channel: {}", e)),
        };

        // Capture packets
        loop {
            match rx.next() {
                Ok(packet) => {
                    if let Err(e) = Self::process_packet(packet, &tracker) {
                        // Don't spam errors, just continue
                        if std::env::var("DEBUG").is_ok() {
                            log::error!("Packet processing error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::error!("Packet receive error: {}", e);
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    fn find_interface() -> Option<NetworkInterface> {
        let interfaces = datalink::interfaces();

        // First priority: Interface with IPv4 address (most traffic is IPv4)
        // This prevents selecting IPv6-only interfaces which can't capture IPv4 packets properly
        if let Some(iface) = interfaces.iter().find(|iface| {
            iface.is_up() && !iface.is_loopback() && iface.ips.iter().any(|ip| ip.is_ipv4())
        }) {
            log::info!(
                "Selected interface with IPv4: {} ({})",
                iface.name,
                iface
                    .ips
                    .iter()
                    .filter(|ip| ip.is_ipv4())
                    .map(|ip| ip.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Some(iface.clone());
        }

        // Fallback: Any interface with IPs (even IPv6-only)
        let fallback = interfaces
            .into_iter()
            .find(|iface| iface.is_up() && !iface.is_loopback() && !iface.ips.is_empty());

        if let Some(ref iface) = fallback {
            log::warn!(
                "No IPv4 interface found, using IPv6-only interface: {}",
                iface.name
            );
        }

        fallback
    }

    fn process_packet(packet: &[u8], tracker: &Arc<Mutex<BandwidthTracker>>) -> Result<()> {
        let ethernet = EthernetPacket::new(packet).context("Failed to parse Ethernet packet")?;

        match ethernet.get_ethertype() {
            EtherTypes::Ipv4 => {
                if let Some(ipv4) = Ipv4Packet::new(ethernet.payload()) {
                    Self::process_ipv4_packet(&ipv4, packet.len(), tracker)?;
                }
            }
            EtherTypes::Ipv6 => {
                if let Some(ipv6) = Ipv6Packet::new(ethernet.payload()) {
                    Self::process_ipv6_packet(&ipv6, packet.len(), tracker)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn process_ipv4_packet(
        ipv4: &Ipv4Packet,
        packet_len: usize,
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

            let bandwidth = tracker
                .process_bandwidth
                .entry(pid)
                .or_insert(ProcessBandwidth {
                    name,
                    rx_bytes: 0,
                    tx_bytes: 0,
                    last_rx_bytes: 0,
                    last_tx_bytes: 0,
                });

            if is_outbound {
                bandwidth.tx_bytes += packet_len as u64;
            } else {
                bandwidth.rx_bytes += packet_len as u64;
            }
        } else {
            tracker.packets_unmatched += 1;

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
