// Network monitoring backend trait and implementations

use super::{BackendCapabilities, BackendPriority};
use crate::process::{InterfaceMap, ProcessMap};
use anyhow::Result;

#[cfg(feature = "monitor-pnet")]
pub mod pnet;

/// Network monitoring backend trait
pub trait MonitorBackend: Send + Sync {
    /// Backend name (e.g., "pnet", "ebpf", "wfp")
    fn name(&self) -> &'static str;

    /// Backend priority for auto-selection
    fn priority(&self) -> BackendPriority;

    /// Check if this backend is available on the current system
    fn is_available() -> bool
    where
        Self: Sized;

    /// Get backend capabilities
    fn capabilities(&self) -> BackendCapabilities;

    /// Initialize the monitor
    fn init(&mut self) -> Result<()>;

    /// Update network statistics (called periodically)
    fn update(&mut self) -> Result<(ProcessMap, InterfaceMap)>;

    /// Cleanup on shutdown
    fn cleanup(&mut self) -> Result<()>;
}

/// Monitor backend metadata for selection
#[derive(Debug, Clone)]
pub struct MonitorBackendInfo {
    pub name: &'static str,
    pub priority: BackendPriority,
    pub available: bool,
}

/// Detect all available monitor backends on current system
pub fn detect_available_backends() -> Vec<MonitorBackendInfo> {
    let mut backends = Vec::new();

    #[cfg(feature = "monitor-pnet")]
    {
        backends.push(MonitorBackendInfo {
            name: "pnet",
            priority: BackendPriority::Good,
            available: pnet::PnetMonitor::is_available(),
        });
    }

    backends
}

/// Auto-select best available monitor backend
pub fn select_monitor_backend(preference: Option<&str>) -> Result<Box<dyn MonitorBackend>> {
    let available = detect_available_backends();

    if let Some(name) = preference {
        // User explicitly requested a backend
        return create_monitor_backend(name);
    }

    // Auto-select best available
    available
        .iter()
        .filter(|b| b.available)
        .max_by_key(|b| b.priority)
        .and_then(|info| create_monitor_backend(info.name).ok())
        .ok_or_else(|| anyhow::anyhow!("No monitoring backend available"))
}

/// Create a monitor backend by name
fn create_monitor_backend(name: &str) -> Result<Box<dyn MonitorBackend>> {
    match name {
        #[cfg(feature = "monitor-pnet")]
        "pnet" => Ok(Box::new(pnet::PnetMonitor::new()?)),

        _ => Err(anyhow::anyhow!("Unknown monitor backend: {}", name)),
    }
}
