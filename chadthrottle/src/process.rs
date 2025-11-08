use std::collections::HashMap;
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: i32,
    pub name: String,
    pub download_rate: u64,  // bytes per second
    pub upload_rate: u64,    // bytes per second
    pub total_download: u64, // total bytes
    pub total_upload: u64,   // total bytes
    pub throttle_limit: Option<ThrottleLimit>,
    pub is_terminated: bool, // whether the process has terminated
    pub interface_stats: HashMap<String, InterfaceStats>, // per-interface statistics
}

#[derive(Debug, Clone)]
pub struct InterfaceStats {
    pub download_rate: u64,
    pub upload_rate: u64,
    pub total_download: u64,
    pub total_upload: u64,
}

#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub name: String,
    pub mac_address: Option<String>,
    pub ip_addresses: Vec<IpAddr>,
    pub is_up: bool,
    pub is_loopback: bool,
    pub total_download_rate: u64,
    pub total_upload_rate: u64,
    pub process_count: usize,
}

#[derive(Debug, Clone)]
pub struct ThrottleLimit {
    pub download_limit: Option<u64>, // bytes per second
    pub upload_limit: Option<u64>,   // bytes per second
}

impl ProcessInfo {
    pub fn new(pid: i32, name: String) -> Self {
        Self {
            pid,
            name,
            download_rate: 0,
            upload_rate: 0,
            total_download: 0,
            total_upload: 0,
            throttle_limit: None,
            is_terminated: false,
            interface_stats: HashMap::new(),
        }
    }

    pub fn format_rate(bytes_per_sec: u64) -> String {
        if bytes_per_sec < 1024 {
            format!("{} B/s", bytes_per_sec)
        } else if bytes_per_sec < 1024 * 1024 {
            format!("{:.1} KB/s", bytes_per_sec as f64 / 1024.0)
        } else if bytes_per_sec < 1024 * 1024 * 1024 {
            format!("{:.1} MB/s", bytes_per_sec as f64 / (1024.0 * 1024.0))
        } else {
            format!(
                "{:.1} GB/s",
                bytes_per_sec as f64 / (1024.0 * 1024.0 * 1024.0)
            )
        }
    }

    pub fn format_bytes(bytes: u64) -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else if bytes < 1024 * 1024 * 1024 {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    pub fn is_throttled(&self) -> bool {
        self.throttle_limit.is_some()
    }
}

pub type ProcessMap = HashMap<i32, ProcessInfo>;
pub type InterfaceMap = HashMap<String, InterfaceInfo>;
