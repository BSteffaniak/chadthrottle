// Process utilities trait for platform-specific operations
//
// This module provides a platform-agnostic interface for process-related operations
// that vary across operating systems (Linux, macOS, Windows).

use anyhow::Result;
use std::collections::HashMap;
use std::net::IpAddr;

/// Platform-agnostic process utilities interface
pub trait ProcessUtils: Send + Sync {
    /// Get process name by PID
    fn get_process_name(&self, pid: i32) -> Result<String>;

    /// Check if process exists
    fn process_exists(&self, pid: i32) -> bool;

    /// Get all running processes with their names
    fn get_all_processes(&self) -> Result<Vec<ProcessEntry>>;

    /// Get socket-to-PID mapping for network connections
    fn get_connection_map(&self) -> Result<ConnectionMap>;
}

/// Process entry with PID and name
#[derive(Debug, Clone)]
pub struct ProcessEntry {
    pub pid: i32,
    pub name: String,
}

/// Complete connection map including socket inodes and connections
#[derive(Debug, Clone)]
pub struct ConnectionMap {
    /// Socket inode -> (PID, process name) mapping
    pub socket_to_pid: HashMap<u64, (i32, String)>,
    /// TCP IPv4 connections
    pub tcp_connections: Vec<ConnectionEntry>,
    /// TCP IPv6 connections
    pub tcp6_connections: Vec<ConnectionEntry>,
    /// UDP IPv4 connections
    pub udp_connections: Vec<ConnectionEntry>,
    /// UDP IPv6 connections
    pub udp6_connections: Vec<ConnectionEntry>,
}

impl Default for ConnectionMap {
    fn default() -> Self {
        Self {
            socket_to_pid: HashMap::new(),
            tcp_connections: Vec::new(),
            tcp6_connections: Vec::new(),
            udp_connections: Vec::new(),
            udp6_connections: Vec::new(),
        }
    }
}

/// Network connection entry
#[derive(Debug, Clone)]
pub struct ConnectionEntry {
    pub local_addr: IpAddr,
    pub local_port: u16,
    pub remote_addr: IpAddr,
    pub remote_port: u16,
    pub inode: u64,
    pub state: String, // "Established", "Listen", etc.
}

// Socket mapper backend system (cross-platform)
pub mod socket_mapper;

// Platform-specific implementations
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::LinuxProcessUtils;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacOSProcessUtils;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsProcessUtils;

/// Factory function to create platform-specific ProcessUtils
pub fn create_process_utils() -> Box<dyn ProcessUtils> {
    create_process_utils_with_socket_mapper(None)
}

/// Factory function to create platform-specific ProcessUtils with custom socket mapper
pub fn create_process_utils_with_socket_mapper(
    socket_mapper_preference: Option<&str>,
) -> Box<dyn ProcessUtils> {
    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxProcessUtils::with_socket_mapper(
            socket_mapper_preference,
        ))
    }

    #[cfg(target_os = "macos")]
    {
        Box::new(MacOSProcessUtils::with_socket_mapper(
            socket_mapper_preference,
        ))
    }

    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsProcessUtils::with_socket_mapper(
            socket_mapper_preference,
        ))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        compile_error!(
            "Unsupported platform - only Linux, macOS, and Windows are currently supported"
        );
    }
}
