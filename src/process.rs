use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: i32,
    pub name: String,
    pub download_rate: u64,  // bytes per second
    pub upload_rate: u64,    // bytes per second
    pub total_download: u64, // total bytes
    pub total_upload: u64,   // total bytes
    pub throttle_limit: Option<ThrottleLimit>,
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
