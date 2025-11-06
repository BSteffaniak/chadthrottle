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
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
        }));

        // Start packet capture thread
        let tracker_clone = Arc::clone(&bandwidth_tracker);
        let capture_handle = thread::spawn(move || {
            if let Err(e) = Self::capture_packets(tracker_clone) {
                log::error!("Packet capture error: {}", e);
            }
        });

        Ok(Self {
            bandwidth_tracker,
            _capture_handle: Some(capture_handle),
            last_update: Instant::now(),
        })
    }

    pub fn update(&mut self) -> Result<ProcessMap> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();

        // Update socket inode mapping
        self.update_socket_map()?;

        // Calculate rates and build process map
        let mut process_map = ProcessMap::new();
        let mut tracker = self.bandwidth_tracker.lock().unwrap();

        for (&pid, bandwidth) in &mut tracker.process_bandwidth {
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

            // Update last values for next calculation
            bandwidth.last_rx_bytes = bandwidth.rx_bytes;
            bandwidth.last_tx_bytes = bandwidth.tx_bytes;

            // Check if process still exists before including it
            if procfs::process::Process::new(pid).is_ok() {
                let mut proc_info = ProcessInfo::new(pid, bandwidth.name.clone());
                proc_info.download_rate = download_rate;
                proc_info.upload_rate = upload_rate;
                proc_info.total_download = bandwidth.rx_bytes;
                proc_info.total_upload = bandwidth.tx_bytes;

                process_map.insert(pid, proc_info);
            }
        }

        self.last_update = now;
        Ok(process_map)
    }

    /// Update socket inode -> PID mapping
    fn update_socket_map(&self) -> Result<()> {
        let mut tracker = self.bandwidth_tracker.lock().unwrap();
        tracker.socket_map.clear();
        tracker.connection_map.clear();

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
                                tracker.socket_map.insert(inode, (pid, name.clone()));
                            }
                        }
                    }
                }
            }
        }

        // Map connections to PIDs via socket inodes
        if let Ok(tcp_entries) = procfs::net::tcp() {
            for entry in tcp_entries {
                if let Some(&(pid, _)) = tracker.socket_map.get(&entry.inode) {
                    let key = ConnectionKey {
                        local_addr: entry.local_address.ip(),
                        local_port: entry.local_address.port(),
                        remote_addr: entry.remote_address.ip(),
                        remote_port: entry.remote_address.port(),
                        protocol: Protocol::Tcp,
                    };
                    tracker.connection_map.insert(key, pid);
                }
            }
        }

        if let Ok(tcp6_entries) = procfs::net::tcp6() {
            for entry in tcp6_entries {
                if let Some(&(pid, _)) = tracker.socket_map.get(&entry.inode) {
                    let key = ConnectionKey {
                        local_addr: entry.local_address.ip(),
                        local_port: entry.local_address.port(),
                        remote_addr: entry.remote_address.ip(),
                        remote_port: entry.remote_address.port(),
                        protocol: Protocol::Tcp,
                    };
                    tracker.connection_map.insert(key, pid);
                }
            }
        }

        if let Ok(udp_entries) = procfs::net::udp() {
            for entry in udp_entries {
                if let Some(&(pid, _)) = tracker.socket_map.get(&entry.inode) {
                    let key = ConnectionKey {
                        local_addr: entry.local_address.ip(),
                        local_port: entry.local_address.port(),
                        remote_addr: entry.remote_address.ip(),
                        remote_port: entry.remote_address.port(),
                        protocol: Protocol::Udp,
                    };
                    tracker.connection_map.insert(key, pid);
                }
            }
        }

        if let Ok(udp6_entries) = procfs::net::udp6() {
            for entry in udp6_entries {
                if let Some(&(pid, _)) = tracker.socket_map.get(&entry.inode) {
                    let key = ConnectionKey {
                        local_addr: entry.local_address.ip(),
                        local_port: entry.local_address.port(),
                        remote_addr: entry.remote_address.ip(),
                        remote_port: entry.remote_address.port(),
                        protocol: Protocol::Udp,
                    };
                    tracker.connection_map.insert(key, pid);
                }
            }
        }

        Ok(())
    }

    /// Packet capture thread - runs continuously
    fn capture_packets(tracker: Arc<Mutex<BandwidthTracker>>) -> Result<()> {
        // Find a suitable network interface
        let interface = Self::find_interface().context("Failed to find network interface")?;

        // Create channel for packet capture
        let (_, mut rx) = match datalink::channel(&interface, Default::default()) {
            Ok(Channel::Ethernet(tx, rx)) => (tx, rx),
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
        datalink::interfaces()
            .into_iter()
            .find(|iface| iface.is_up() && !iface.is_loopback() && !iface.ips.is_empty())
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

        // Check if this is an outbound packet (src = local)
        if let Some(&pid) = tracker.connection_map.get(&key_outbound) {
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
            bandwidth.tx_bytes += packet_len as u64;
        }
        // Check if this is an inbound packet (dst = local)
        else if let Some(&pid) = tracker.connection_map.get(&key_inbound) {
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
            bandwidth.rx_bytes += packet_len as u64;
        }
    }
}
