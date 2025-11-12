// pnet-based network monitoring backend

use crate::backends::monitor::MonitorBackend;
use crate::backends::{BackendCapabilities, BackendPriority};
use crate::monitor::NetworkMonitor as LegacyNetworkMonitor;
use crate::process::{InterfaceMap, ProcessMap};
use anyhow::Result;

/// pnet packet capture monitoring backend
pub struct PnetMonitor {
    inner: LegacyNetworkMonitor,
}

impl PnetMonitor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: LegacyNetworkMonitor::new()?,
        })
    }
}

impl MonitorBackend for PnetMonitor {
    fn name(&self) -> &'static str {
        "pnet"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Good
    }

    fn is_available() -> bool {
        // pnet works on Linux and BSD with raw sockets, and Windows with Npcap
        cfg!(target_os = "linux")
            || cfg!(target_os = "freebsd")
            || cfg!(target_os = "openbsd")
            || cfg!(target_os = "netbsd")
            || cfg!(target_os = "windows")
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            ipv4_support: true,
            ipv6_support: true,
            per_process: true,
            per_connection: false,
        }
    }

    fn init(&mut self) -> Result<()> {
        // Initialization already done in new()
        Ok(())
    }

    fn update(&mut self) -> Result<(ProcessMap, InterfaceMap)> {
        self.inner.update()
    }

    fn cleanup(&mut self) -> Result<()> {
        // NetworkMonitor doesn't need explicit cleanup
        Ok(())
    }
}
