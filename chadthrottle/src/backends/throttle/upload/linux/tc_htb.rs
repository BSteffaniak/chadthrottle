// TC HTB (Hierarchical Token Bucket) upload throttling backend

use crate::backends::cgroup::{CgroupBackend, CgroupBackendType, CgroupHandle};
use crate::backends::throttle::linux_tc_utils::*;
use crate::backends::throttle::UploadThrottleBackend;
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::{anyhow, Result};
use std::collections::HashMap;

/// TC HTB upload (egress) throttling backend
pub struct TcHtbUpload {
    interface: String,
    active_throttles: HashMap<i32, ThrottleInfo>,
    next_classid: u32,
    initialized: bool,
    cgroup_backend: Option<Box<dyn CgroupBackend>>,
}

struct ThrottleInfo {
    classid: u32,
    cgroup_handle: CgroupHandle,
    limit_bytes_per_sec: u64,
}

impl TcHtbUpload {
    pub fn new() -> Result<Self> {
        let interface = detect_interface()?;

        Ok(Self {
            interface,
            active_throttles: HashMap::new(),
            next_classid: 100, // Start at 100 to avoid conflicts
            initialized: false,
            cgroup_backend: None,
        })
    }

    fn get_cgroup_backend_mut(&mut self) -> Result<&mut Box<dyn CgroupBackend>> {
        self.cgroup_backend
            .as_mut()
            .ok_or_else(|| anyhow!("Cgroup backend not initialized"))
    }
}

impl UploadThrottleBackend for TcHtbUpload {
    fn name(&self) -> &'static str {
        "tc_htb"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Good
    }

    fn is_available() -> bool {
        // Check TC (traffic control)
        if !check_tc_available() {
            return false;
        }

        // Check if any cgroup backend is available (works with v1 or v2)
        if let Ok(Some(backend)) = crate::backends::cgroup::select_best_backend() {
            if let Ok(available) = backend.is_available() {
                return available;
            }
        }

        false
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
        if self.initialized {
            return Ok(());
        }

        // Setup TC HTB root on main interface
        setup_tc_htb_root(&self.interface)?;

        // Initialize cgroup backend
        self.cgroup_backend = crate::backends::cgroup::select_best_backend()?;
        if self.cgroup_backend.is_none() {
            return Err(anyhow!("No cgroup backend available for tc_htb"));
        }

        self.initialized = true;
        Ok(())
    }

    fn throttle_upload(
        &mut self,
        pid: i32,
        process_name: String,
        limit_bytes_per_sec: u64,
        traffic_type: crate::process::TrafficType,
    ) -> Result<()> {
        use crate::process::TrafficType;

        // TC HTB operates at cgroup level and cannot filter by IP address
        // Only TrafficType::All is supported
        if traffic_type != TrafficType::All {
            return Err(anyhow::anyhow!(
                "TC HTB backend does not support traffic type filtering (Internet/Local only). \
                 Traffic type '{:?}' requested but only 'All' is supported. \
                 Use nftables backend for traffic type filtering.",
                traffic_type
            ));
        }

        // Initialize if not already done
        self.init()?;

        // Get next classid
        let classid = self.next_classid;
        self.next_classid += 1;

        // Create cgroup using backend (supports both v1 and v2)
        let backend = self.get_cgroup_backend_mut()?;
        let cgroup_handle = backend.create_cgroup(pid, &process_name)?;

        // For cgroup v1, use classid from handle
        // For cgroup v2, TC cgroup filter requires eBPF or falls back to interface-wide
        if matches!(cgroup_handle.backend_type, CgroupBackendType::V1) {
            // V1: Use classid from handle (format like "1:X")
            if let Some(classid_str) = cgroup_handle.identifier.split(':').nth(1) {
                if let Ok(handle_classid) = classid_str.parse::<u32>() {
                    let rate_kbps = (limit_bytes_per_sec * 8 / 1000) as u32;
                    create_tc_class(&self.interface, handle_classid, rate_kbps, "1:")?;

                    self.active_throttles.insert(
                        pid,
                        ThrottleInfo {
                            classid: handle_classid,
                            cgroup_handle,
                            limit_bytes_per_sec,
                        },
                    );
                    return Ok(());
                }
            }
        }

        // For v2 or if v1 parsing failed, use our own classid sequence
        // Convert bytes/sec to kbps (kilobits per second)
        let rate_kbps = (limit_bytes_per_sec * 8 / 1000) as u32;

        // Create TC class with rate limit
        create_tc_class(&self.interface, classid, rate_kbps, "1:")?;

        // Track throttle
        self.active_throttles.insert(
            pid,
            ThrottleInfo {
                classid,
                cgroup_handle,
                limit_bytes_per_sec,
            },
        );

        Ok(())
    }

    fn remove_upload_throttle(&mut self, pid: i32) -> Result<()> {
        if let Some(info) = self.active_throttles.remove(&pid) {
            // Remove TC class
            let _ = remove_tc_class(&self.interface, info.classid, "1:");

            // Remove cgroup using backend
            if let Ok(backend) = self.get_cgroup_backend_mut() {
                let _ = backend.remove_cgroup(&info.cgroup_handle);
            }
        }

        Ok(())
    }

    fn get_upload_throttle(&self, pid: i32) -> Option<u64> {
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
            let _ = self.remove_upload_throttle(pid);
        }

        // Remove TC qdisc (cleanup)
        let _ = std::process::Command::new("tc")
            .args(&["qdisc", "del", "dev", &self.interface, "root"])
            .status();

        Ok(())
    }
}

impl Drop for TcHtbUpload {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
