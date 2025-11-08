// Linux libproc-based socket mapper
//
// This backend uses the libproc crate for process enumeration and names,
// but falls back to procfs crate for socket/FD enumeration since libproc
// doesn't expose those APIs on Linux.
//
// This provides a consistent cross-platform API while maintaining the same
// underlying mechanism as ProcfsSocketMapper (direct /proc access).

use super::super::SocketMapperBackend;
use crate::backends::process::{ConnectionEntry, ConnectionMap};
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::{Context, Result};
use procfs::process::FDTarget;
use std::collections::HashMap;

/// Socket mapper using libproc crate (Linux)
///
/// On Linux, this backend combines:
/// - libproc crate for process listing and names (cross-platform API)
/// - procfs crate for socket enumeration (libproc doesn't expose this on Linux)
///
/// This is essentially similar to ProcfsSocketMapper but uses libproc's
/// higher-level API where available.
pub struct LibprocSocketMapper;

impl LibprocSocketMapper {
    pub fn new() -> Result<Self> {
        if !Self::is_available() {
            anyhow::bail!("libproc not available (is /proc mounted?)");
        }
        Ok(Self)
    }
}

impl SocketMapperBackend for LibprocSocketMapper {
    fn name(&self) -> &'static str {
        "libproc"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Good // Wrapper over procfs, slightly more overhead
    }

    fn is_available() -> bool {
        // Check if /proc/net/tcp exists (same as ProcfsSocketMapper)
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

        // Use libproc to list all processes
        let pids = libproc::processes::pids_by_type(libproc::processes::ProcFilter::All)
            .context("Failed to list processes via libproc")?;

        // Build socket inode -> PID map by scanning all processes
        for pid in pids {
            let pid = pid as i32;

            // Get process name using libproc
            let name =
                libproc::libproc::proc_pid::name(pid).unwrap_or_else(|_| format!("PID {}", pid));

            // Get file descriptors using procfs crate
            // (libproc doesn't expose FD enumeration on Linux)
            if let Ok(process) = procfs::process::Process::new(pid) {
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
