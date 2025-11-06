// TC Police download throttling backend (no IFB required)

use crate::backends::throttle::DownloadThrottleBackend;
use crate::backends::throttle::linux_tc_utils::*;
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::process::Command;

/// TC Police download (ingress) throttling backend
///
/// This backend uses TC police action directly on the ingress qdisc
/// without requiring the IFB (Intermediate Functional Block) module.
///
/// Limitations compared to IFB+TC:
/// - Cannot use cgroups for per-process filtering
/// - Must use u32 filters to match traffic based on IP/port
/// - Less flexible but works when IFB module is unavailable
pub struct TcPoliceDownload {
    interface: String,
    active_throttles: HashMap<i32, ThrottleInfo>,
    next_handle: u32,
    initialized: bool,
}

struct ThrottleInfo {
    handle: u32,
    process_name: String,
    limit_bytes_per_sec: u64,
}

impl TcPoliceDownload {
    pub fn new() -> Result<Self> {
        let interface = detect_interface()?;

        Ok(Self {
            interface,
            active_throttles: HashMap::new(),
            next_handle: 100,
            initialized: false,
        })
    }

    fn setup_ingress(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        // Check if ingress qdisc already exists
        let check_qdisc = Command::new("tc")
            .args(&["qdisc", "show", "dev", &self.interface])
            .output()
            .context("Failed to check existing qdiscs")?;

        let output = String::from_utf8_lossy(&check_qdisc.stdout);

        // If ingress not present, add it
        if !output.contains("ingress") {
            let status = Command::new("tc")
                .args(&[
                    "qdisc",
                    "add",
                    "dev",
                    &self.interface,
                    "handle",
                    "ffff:",
                    "ingress",
                ])
                .status()
                .context("Failed to create ingress qdisc")?;

            if !status.success() {
                return Err(anyhow!("Failed to setup ingress qdisc"));
            }
        }

        self.initialized = true;
        Ok(())
    }
}

impl DownloadThrottleBackend for TcPoliceDownload {
    fn name(&self) -> &'static str {
        "tc_police_download"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Fallback
    }

    fn is_available() -> bool {
        check_tc_available()
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            ipv4_support: true,
            ipv6_support: false, // Police action has limited IPv6 support
            per_process: false,  // Cannot filter by process without cgroups
            per_connection: false,
        }
    }

    fn init(&mut self) -> Result<()> {
        self.setup_ingress()
    }

    fn throttle_download(
        &mut self,
        pid: i32,
        process_name: String,
        limit_bytes_per_sec: u64,
    ) -> Result<()> {
        // Initialize ingress if not already done
        self.init()?;

        // Get next handle
        let handle = self.next_handle;
        self.next_handle += 1;

        // Convert bytes/sec to bits/sec
        let rate_bps = limit_bytes_per_sec * 8;

        // NOTE: TC Police cannot filter by process/PID directly
        // This is a global rate limit on all ingress traffic
        // A better implementation would try to match by IP:port using /proc/net/tcp
        // but for now we'll apply a global limit with a warning

        log::warn!(
            "TC Police backend cannot filter by process - applying global download limit of {} bytes/sec for interface {}",
            limit_bytes_per_sec,
            self.interface
        );

        // Add police filter on ingress
        // This matches all traffic and applies rate limiting
        let status = Command::new("tc")
            .args(&[
                "filter",
                "add",
                "dev",
                &self.interface,
                "parent",
                "ffff:",
                "protocol",
                "ip",
                "prio",
                "1",
                "u32",
                "match",
                "u32",
                "0",
                "0", // Match all traffic
                "police",
                "rate",
                &format!("{}bit", rate_bps),
                "burst",
                &format!("{}k", (rate_bps / 8000).max(32)), // Reasonable burst size
                "drop",                                     // Drop packets exceeding rate
                "flowid",
                &format!(":{}", handle),
            ])
            .status()
            .context("Failed to add police filter")?;

        if !status.success() {
            return Err(anyhow!("Failed to create TC police filter for PID {}", pid));
        }

        // Track throttle
        self.active_throttles.insert(
            pid,
            ThrottleInfo {
                handle,
                process_name,
                limit_bytes_per_sec,
            },
        );

        Ok(())
    }

    fn remove_download_throttle(&mut self, pid: i32) -> Result<()> {
        if let Some(_info) = self.active_throttles.remove(&pid) {
            // Remove filter by handle
            // Note: TC police filters don't have a direct "delete by handle" command
            // We need to use prio and protocol to identify the filter
            let _ = Command::new("tc")
                .args(&[
                    "filter",
                    "del",
                    "dev",
                    &self.interface,
                    "parent",
                    "ffff:",
                    "prio",
                    "1",
                ])
                .status();
        }

        Ok(())
    }

    fn get_download_throttle(&self, pid: i32) -> Option<u64> {
        self.active_throttles
            .get(&pid)
            .map(|info| info.limit_bytes_per_sec)
    }

    fn get_all_throttles(&self) -> HashMap<i32, u64> {
        self.active_throttles
            .iter()
            .map(|(&pid, info)| (pid, info.limit_bytes_per_sec))
            .collect()
    }

    fn cleanup(&mut self) -> Result<()> {
        // Remove all throttles
        let pids: Vec<i32> = self.active_throttles.keys().copied().collect();
        for pid in pids {
            let _ = self.remove_download_throttle(pid);
        }

        // Remove ingress qdisc
        let _ = Command::new("tc")
            .args(&["qdisc", "del", "dev", &self.interface, "ingress"])
            .status();

        Ok(())
    }
}

impl Drop for TcPoliceDownload {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
