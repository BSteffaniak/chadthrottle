// nftables Upload Throttling Backend

use crate::backends::throttle::UploadThrottleBackend;
use crate::backends::throttle::linux_nft_utils::*;
use crate::backends::throttle::linux_tc_utils::*;
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::{Result, anyhow};
use std::collections::HashMap;

/// nftables-based upload (egress) throttling backend
pub struct NftablesUpload {
    active_throttles: HashMap<i32, ThrottleInfo>,
    initialized: bool,
}

struct ThrottleInfo {
    cgroup_path: String,
    limit_bytes_per_sec: u64,
}

impl NftablesUpload {
    pub fn new() -> Result<Self> {
        Ok(Self {
            active_throttles: HashMap::new(),
            initialized: false,
        })
    }

    fn ensure_initialized(&mut self) -> Result<()> {
        if !self.initialized {
            init_nft_table()?;
            self.initialized = true;
        }
        Ok(())
    }
}

impl UploadThrottleBackend for NftablesUpload {
    fn name(&self) -> &'static str {
        "nftables_upload"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Better // Better than TC, not as good as eBPF
    }

    fn is_available() -> bool {
        check_nft_available() && check_cgroups_available()
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
        self.ensure_initialized()
    }

    fn throttle_upload(
        &mut self,
        pid: i32,
        _process_name: String,
        limit_bytes_per_sec: u64,
    ) -> Result<()> {
        self.ensure_initialized()?;

        // Create cgroup for process
        let cgroup_path = create_cgroup(pid)?;

        // Move process to cgroup
        move_process_to_cgroup(pid, &cgroup_path)?;

        // Add nftables rate limit rule
        add_cgroup_rate_limit(&cgroup_path, limit_bytes_per_sec, Direction::Upload)?;

        // Track throttle
        self.active_throttles.insert(
            pid,
            ThrottleInfo {
                cgroup_path,
                limit_bytes_per_sec,
            },
        );

        Ok(())
    }

    fn remove_upload_throttle(&mut self, pid: i32) -> Result<()> {
        if let Some(info) = self.active_throttles.remove(&pid) {
            // Remove nftables rules for this cgroup
            let _ = remove_cgroup_rules(&info.cgroup_path, Direction::Upload);

            // Remove cgroup
            let _ = remove_cgroup(&info.cgroup_path);
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

        // Cleanup nftables table
        let _ = cleanup_nft_table();

        Ok(())
    }
}

impl Drop for NftablesUpload {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
