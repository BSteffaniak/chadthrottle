// Linux-specific process utilities using procfs

use super::{ConnectionEntry, ConnectionMap, ProcessEntry, ProcessUtils};
use anyhow::Result;
use procfs::process::{FDTarget, all_processes};
use std::collections::HashMap;
use std::net::IpAddr;

/// Linux process utilities using procfs
pub struct LinuxProcessUtils;

impl LinuxProcessUtils {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessUtils for LinuxProcessUtils {
    fn get_process_name(&self, pid: i32) -> Result<String> {
        std::fs::read_to_string(format!("/proc/{}/comm", pid))
            .map(|s| s.trim().to_string())
            .or_else(|_| Ok(format!("PID {}", pid)))
    }

    fn process_exists(&self, pid: i32) -> bool {
        procfs::process::Process::new(pid).is_ok()
    }

    fn get_all_processes(&self) -> Result<Vec<ProcessEntry>> {
        let all_procs = all_processes()?;
        let mut entries = Vec::new();

        for proc_result in all_procs {
            if let Ok(process) = proc_result {
                let pid = process.pid();
                let name = if let Ok(stat) = process.stat() {
                    stat.comm
                } else {
                    format!("PID {}", pid)
                };
                entries.push(ProcessEntry { pid, name });
            }
        }

        Ok(entries)
    }

    fn get_connection_map(&self) -> Result<ConnectionMap> {
        let mut socket_to_pid = HashMap::new();
        let mut tcp_connections = Vec::new();
        let mut tcp6_connections = Vec::new();
        let mut udp_connections = Vec::new();
        let mut udp6_connections = Vec::new();

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
