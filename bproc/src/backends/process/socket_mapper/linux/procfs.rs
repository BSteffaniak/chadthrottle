// Linux procfs-based socket mapper

use super::super::SocketMapperBackend;
use crate::backends::process::{ConnectionEntry, ConnectionMap};
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::Result;
use procfs::process::{FDTarget, all_processes};
use std::collections::HashMap;

/// Socket mapper using Linux /proc filesystem
///
/// This backend reads socket information from:
/// - /proc/[pid]/fd/ - for socket inodes
/// - /proc/net/tcp - for TCP IPv4 connections
/// - /proc/net/tcp6 - for TCP IPv6 connections
/// - /proc/net/udp - for UDP IPv4 connections
/// - /proc/net/udp6 - for UDP IPv6 connections
pub struct ProcfsSocketMapper;

impl ProcfsSocketMapper {
    pub fn new() -> Result<Self> {
        if !Self::is_available() {
            anyhow::bail!("procfs not available (is /proc mounted?)");
        }
        Ok(Self)
    }
}

impl SocketMapperBackend for ProcfsSocketMapper {
    fn name(&self) -> &'static str {
        "procfs"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Best // Native Linux API, always available, fast
    }

    fn is_available() -> bool {
        // Check if /proc/net/tcp exists
        std::path::Path::new("/proc/net/tcp").exists()
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

        // Build socket inode -> PID map by scanning all processes
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
                                socket_to_pid.insert(inode, (pid, name.clone()));
                            }
                        }
                    }
                }
            }
        }

        // Map TCP connections to PIDs via socket inodes
        if let Ok(tcp_entries) = procfs::net::tcp() {
            for entry in tcp_entries {
                tcp_connections.push(ConnectionEntry {
                    local_addr: entry.local_address.ip(),
                    local_port: entry.local_address.port(),
                    remote_addr: entry.remote_address.ip(),
                    remote_port: entry.remote_address.port(),
                    inode: entry.inode,
                    state: format!("{:?}", entry.state),
                });
            }
        }

        // Map TCP6 connections
        if let Ok(tcp6_entries) = procfs::net::tcp6() {
            for entry in tcp6_entries {
                tcp6_connections.push(ConnectionEntry {
                    local_addr: entry.local_address.ip(),
                    local_port: entry.local_address.port(),
                    remote_addr: entry.remote_address.ip(),
                    remote_port: entry.remote_address.port(),
                    inode: entry.inode,
                    state: format!("{:?}", entry.state),
                });
            }
        }

        // Map UDP connections
        if let Ok(udp_entries) = procfs::net::udp() {
            for entry in udp_entries {
                udp_connections.push(ConnectionEntry {
                    local_addr: entry.local_address.ip(),
                    local_port: entry.local_address.port(),
                    remote_addr: entry.remote_address.ip(),
                    remote_port: entry.remote_address.port(),
                    inode: entry.inode,
                    state: format!("{:?}", entry.state),
                });
            }
        }

        // Map UDP6 connections
        if let Ok(udp6_entries) = procfs::net::udp6() {
            for entry in udp6_entries {
                udp6_connections.push(ConnectionEntry {
                    local_addr: entry.local_address.ip(),
                    local_port: entry.local_address.port(),
                    remote_addr: entry.remote_address.ip(),
                    remote_port: entry.remote_address.port(),
                    inode: entry.inode,
                    state: format!("{:?}", entry.state),
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
