// Backend trait definitions and core types

pub mod monitor;
pub mod throttle;
pub mod capability;

use anyhow::Result;

/// Platform identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOS,
    Windows,
    BSD,
}

impl Platform {
    pub fn current() -> Self {
        #[cfg(target_os = "linux")]
        return Platform::Linux;
        
        #[cfg(target_os = "macos")]
        return Platform::MacOS;
        
        #[cfg(target_os = "windows")]
        return Platform::Windows;
        
        #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
        return Platform::BSD;
    }
}

/// Backend priority ranking (higher = better)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BackendPriority {
    Fallback = 1,      // Works but limited (iptables, /proc parsing)
    Good = 2,          // Solid implementation (IFB+TC, WFP)
    Better = 3,        // Modern, efficient (eBPF XDP, nftables)
    Best = 4,          // Optimal (eBPF cgroup, native APIs)
}

/// Capabilities that a backend supports
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub ipv4_support: bool,
    pub ipv6_support: bool,
    pub per_process: bool,
    pub per_connection: bool,
}

/// Active throttle information
#[derive(Debug, Clone)]
pub struct ActiveThrottle {
    pub pid: i32,
    pub process_name: String,
    pub upload_limit: Option<u64>,    // bytes/sec
    pub download_limit: Option<u64>,  // bytes/sec
}
