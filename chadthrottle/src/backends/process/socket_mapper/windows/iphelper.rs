// Windows IP Helper API-based socket mapper
//
// This backend uses Windows IP Helper API (iphlpapi.dll) to enumerate
// TCP and UDP connections with their owning process IDs.
//
// Uses GetExtendedTcpTable and GetExtendedUdpTable which are available
// on Windows XP SP2 and later.

use crate::backends::process::socket_mapper::SocketMapperBackend;
use crate::backends::process::{ConnectionEntry, ConnectionMap};
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::Result;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
    MIB_UDP6TABLE_OWNER_PID, MIB_UDPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

/// Socket mapper using Windows IP Helper API
///
/// This backend uses GetExtendedTcpTable and GetExtendedUdpTable to enumerate
/// network connections with their owning PIDs. This is the standard Windows
/// approach for socket-to-PID mapping (used by netstat, Task Manager, etc.)
pub struct IpHelperSocketMapper;

impl IpHelperSocketMapper {
    pub fn new() -> Result<Self> {
        if !Self::is_available() {
            anyhow::bail!("IP Helper API not available");
        }
        Ok(Self)
    }
}

impl SocketMapperBackend for IpHelperSocketMapper {
    fn name(&self) -> &'static str {
        "iphelper"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Best // Native Windows API
    }

    fn is_available() -> bool {
        // IP Helper API is available on all modern Windows versions (XP SP2+)
        cfg!(target_os = "windows")
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            ipv4_support: true,
            ipv6_support: true,
            per_process: true,
            per_connection: true,
        }
    }

    fn get_connection_map(&self) -> Result<ConnectionMap> {
        let mut socket_to_pid = HashMap::new();
        let mut tcp_connections = Vec::new();
        let mut tcp6_connections = Vec::new();
        let mut udp_connections = Vec::new();
        let mut udp6_connections = Vec::new();

        // PERFORMANCE CRITICAL: Create System once and reuse for all connections
        // Creating System::new_all() for each connection is extremely slow (1-2 sec per call)
        // With 100 connections, this reduces time from 100-200 seconds to 1-2 seconds total
        use sysinfo::System;
        let sys = System::new_all();

        // Get TCP IPv4 connections
        if let Ok(tcp_table) = get_tcp_table() {
            for entry in tcp_table {
                // Create synthetic inode from connection 4-tuple
                // Windows doesn't have inodes, so we create a hash
                let inode = create_synthetic_inode(
                    &entry.local_addr,
                    entry.local_port,
                    &entry.remote_addr,
                    entry.remote_port,
                );

                socket_to_pid.insert(inode, (entry.pid, get_process_name(&sys, entry.pid)));
                tcp_connections.push(ConnectionEntry {
                    local_addr: entry.local_addr,
                    local_port: entry.local_port,
                    remote_addr: entry.remote_addr,
                    remote_port: entry.remote_port,
                    inode,
                    state: entry.state.clone(),
                });
            }
        }

        // Get TCP IPv6 connections
        if let Ok(tcp6_table) = get_tcp6_table() {
            for entry in tcp6_table {
                let inode = create_synthetic_inode(
                    &entry.local_addr,
                    entry.local_port,
                    &entry.remote_addr,
                    entry.remote_port,
                );

                socket_to_pid.insert(inode, (entry.pid, get_process_name(&sys, entry.pid)));
                tcp6_connections.push(ConnectionEntry {
                    local_addr: entry.local_addr,
                    local_port: entry.local_port,
                    remote_addr: entry.remote_addr,
                    remote_port: entry.remote_port,
                    inode,
                    state: entry.state.clone(),
                });
            }
        }

        // Get UDP IPv4 connections
        if let Ok(udp_table) = get_udp_table() {
            for entry in udp_table {
                let inode = create_synthetic_inode(
                    &entry.local_addr,
                    entry.local_port,
                    &IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    0,
                );

                socket_to_pid.insert(inode, (entry.pid, get_process_name(&sys, entry.pid)));
                udp_connections.push(ConnectionEntry {
                    local_addr: entry.local_addr,
                    local_port: entry.local_port,
                    remote_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    remote_port: 0,
                    inode,
                    state: "UDP".to_string(),
                });
            }
        }

        // Get UDP IPv6 connections
        if let Ok(udp6_table) = get_udp6_table() {
            for entry in udp6_table {
                let inode = create_synthetic_inode(
                    &entry.local_addr,
                    entry.local_port,
                    &IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    0,
                );

                socket_to_pid.insert(inode, (entry.pid, get_process_name(&sys, entry.pid)));
                udp6_connections.push(ConnectionEntry {
                    local_addr: entry.local_addr,
                    local_port: entry.local_port,
                    remote_addr: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    remote_port: 0,
                    inode,
                    state: "UDP".to_string(),
                });
            }
        }

        Ok(ConnectionMap {
            socket_to_pid,
            tcp_connections,
            tcp6_connections,
            udp_connections,
            udp6_connections,
        })
    }
}

// Internal structs for parsing connection tables

struct TcpEntry {
    local_addr: IpAddr,
    local_port: u16,
    remote_addr: IpAddr,
    remote_port: u16,
    pid: i32,
    state: String,
}

struct UdpEntry {
    local_addr: IpAddr,
    local_port: u16,
    pid: i32,
}

// Helper functions

fn get_tcp_table() -> Result<Vec<TcpEntry>> {
    unsafe {
        // First call to get buffer size
        let mut size: u32 = 0;
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );

        if size == 0 {
            return Ok(Vec::new());
        }

        // Allocate buffer and get actual data
        let mut buffer = vec![0u8; size as usize];
        let result = GetExtendedTcpTable(
            Some(buffer.as_mut_ptr() as *mut _),
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );

        if result != NO_ERROR.0 {
            anyhow::bail!("GetExtendedTcpTable failed with error code: {}", result);
        }

        // Parse the table
        let table = &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let mut entries = Vec::new();

        // Use pointer arithmetic for flexible array member
        // table.table is defined as [MIB_TCPROW_OWNER_PID; 1] but actually contains dwNumEntries elements
        let table_ptr = table.table.as_ptr();

        for i in 0..table.dwNumEntries {
            let row = &*table_ptr.add(i as usize);

            // Convert network byte order addresses
            let local_addr = IpAddr::V4(Ipv4Addr::from(u32::from_be(row.dwLocalAddr)));
            let remote_addr = IpAddr::V4(Ipv4Addr::from(u32::from_be(row.dwRemoteAddr)));

            // Convert network byte order ports (stored in first 16 bits)
            let local_port = ((row.dwLocalPort >> 8) & 0xFF) as u16
                | (((row.dwLocalPort & 0xFF) << 8) & 0xFF00) as u16;
            let remote_port = ((row.dwRemotePort >> 8) & 0xFF) as u16
                | (((row.dwRemotePort & 0xFF) << 8) & 0xFF00) as u16;

            entries.push(TcpEntry {
                local_addr,
                local_port,
                remote_addr,
                remote_port,
                pid: row.dwOwningPid as i32,
                state: tcp_state_to_string(row.dwState),
            });
        }

        Ok(entries)
    }
}

fn get_tcp6_table() -> Result<Vec<TcpEntry>> {
    unsafe {
        let mut size: u32 = 0;
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET6.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );

        if size == 0 {
            return Ok(Vec::new());
        }

        let mut buffer = vec![0u8; size as usize];
        let result = GetExtendedTcpTable(
            Some(buffer.as_mut_ptr() as *mut _),
            &mut size,
            false,
            AF_INET6.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );

        if result != NO_ERROR.0 {
            anyhow::bail!("GetExtendedTcp6Table failed with error code: {}", result);
        }

        let table = &*(buffer.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
        let mut entries = Vec::new();

        // Use pointer arithmetic for flexible array member
        let table_ptr = table.table.as_ptr();

        for i in 0..table.dwNumEntries {
            let row = &*table_ptr.add(i as usize);

            let local_addr = IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr));
            let remote_addr = IpAddr::V6(Ipv6Addr::from(row.ucRemoteAddr));

            // Convert network byte order ports
            let local_port = ((row.dwLocalPort >> 8) & 0xFF) as u16
                | (((row.dwLocalPort & 0xFF) << 8) & 0xFF00) as u16;
            let remote_port = ((row.dwRemotePort >> 8) & 0xFF) as u16
                | (((row.dwRemotePort & 0xFF) << 8) & 0xFF00) as u16;

            entries.push(TcpEntry {
                local_addr,
                local_port,
                remote_addr,
                remote_port,
                pid: row.dwOwningPid as i32,
                state: tcp_state_to_string(row.dwState),
            });
        }

        Ok(entries)
    }
}

fn get_udp_table() -> Result<Vec<UdpEntry>> {
    unsafe {
        let mut size: u32 = 0;
        let _ = GetExtendedUdpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            UDP_TABLE_OWNER_PID,
            0,
        );

        if size == 0 {
            return Ok(Vec::new());
        }

        let mut buffer = vec![0u8; size as usize];
        let result = GetExtendedUdpTable(
            Some(buffer.as_mut_ptr() as *mut _),
            &mut size,
            false,
            AF_INET.0 as u32,
            UDP_TABLE_OWNER_PID,
            0,
        );

        if result != NO_ERROR.0 {
            anyhow::bail!("GetExtendedUdpTable failed with error code: {}", result);
        }

        let table = &*(buffer.as_ptr() as *const MIB_UDPTABLE_OWNER_PID);
        let mut entries = Vec::new();

        // Use pointer arithmetic for flexible array member
        let table_ptr = table.table.as_ptr();

        for i in 0..table.dwNumEntries {
            let row = &*table_ptr.add(i as usize);

            let local_addr = IpAddr::V4(Ipv4Addr::from(u32::from_be(row.dwLocalAddr)));
            let local_port = ((row.dwLocalPort >> 8) & 0xFF) as u16
                | (((row.dwLocalPort & 0xFF) << 8) & 0xFF00) as u16;

            entries.push(UdpEntry {
                local_addr,
                local_port,
                pid: row.dwOwningPid as i32,
            });
        }

        Ok(entries)
    }
}

fn get_udp6_table() -> Result<Vec<UdpEntry>> {
    unsafe {
        let mut size: u32 = 0;
        let _ = GetExtendedUdpTable(
            None,
            &mut size,
            false,
            AF_INET6.0 as u32,
            UDP_TABLE_OWNER_PID,
            0,
        );

        if size == 0 {
            return Ok(Vec::new());
        }

        let mut buffer = vec![0u8; size as usize];
        let result = GetExtendedUdpTable(
            Some(buffer.as_mut_ptr() as *mut _),
            &mut size,
            false,
            AF_INET6.0 as u32,
            UDP_TABLE_OWNER_PID,
            0,
        );

        if result != NO_ERROR.0 {
            anyhow::bail!("GetExtendedUdp6Table failed with error code: {}", result);
        }

        let table = &*(buffer.as_ptr() as *const MIB_UDP6TABLE_OWNER_PID);
        let mut entries = Vec::new();

        // Use pointer arithmetic for flexible array member
        let table_ptr = table.table.as_ptr();

        for i in 0..table.dwNumEntries {
            let row = &*table_ptr.add(i as usize);

            let local_addr = IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr));
            let local_port = ((row.dwLocalPort >> 8) & 0xFF) as u16
                | (((row.dwLocalPort & 0xFF) << 8) & 0xFF00) as u16;

            entries.push(UdpEntry {
                local_addr,
                local_port,
                pid: row.dwOwningPid as i32,
            });
        }

        Ok(entries)
    }
}

/// Create a synthetic inode from connection tuple
/// Windows doesn't have socket inodes, so we create a deterministic hash
fn create_synthetic_inode(
    local_addr: &IpAddr,
    local_port: u16,
    remote_addr: &IpAddr,
    remote_port: u16,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    local_addr.hash(&mut hasher);
    local_port.hash(&mut hasher);
    remote_addr.hash(&mut hasher);
    remote_port.hash(&mut hasher);
    hasher.finish()
}

/// Convert Windows TCP state constant to string
fn tcp_state_to_string(state: u32) -> String {
    // Windows MIB_TCP_STATE constants
    match state {
        1 => "CLOSED".to_string(),
        2 => "LISTEN".to_string(),
        3 => "SYN_SENT".to_string(),
        4 => "SYN_RCVD".to_string(),
        5 => "ESTABLISHED".to_string(),
        6 => "FIN_WAIT1".to_string(),
        7 => "FIN_WAIT2".to_string(),
        8 => "CLOSE_WAIT".to_string(),
        9 => "CLOSING".to_string(),
        10 => "LAST_ACK".to_string(),
        11 => "TIME_WAIT".to_string(),
        12 => "DELETE_TCB".to_string(),
        _ => format!("UNKNOWN({})", state),
    }
}

/// Get process name by PID (using cached System instance)
fn get_process_name(sys: &sysinfo::System, pid: i32) -> String {
    use sysinfo::Pid;

    let pid_obj = Pid::from_u32(pid as u32);

    sys.process(pid_obj)
        .map(|p| p.name().to_str().unwrap_or("unknown").to_string())
        .unwrap_or_else(|| format!("PID {}", pid))
}
