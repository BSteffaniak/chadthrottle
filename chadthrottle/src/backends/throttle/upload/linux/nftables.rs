// nftables Upload Throttling Backend

use crate::backends::cgroup::{CgroupBackend, CgroupHandle};
use crate::backends::throttle::UploadThrottleBackend;
use crate::backends::throttle::linux_nft_utils::*;
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::{Result, anyhow};
use std::collections::HashMap;

/// nftables-based upload (egress) throttling backend
pub struct NftablesUpload {
    active_throttles: HashMap<i32, ThrottleInfo>,
    initialized: bool,
    cgroup_backend: Option<Box<dyn CgroupBackend>>,
}

struct ThrottleInfo {
    cgroup_handle: CgroupHandle,
    limit_bytes_per_sec: u64,
}

impl NftablesUpload {
    pub fn new() -> Result<Self> {
        Ok(Self {
            active_throttles: HashMap::new(),
            initialized: false,
            cgroup_backend: None,
        })
    }

    fn ensure_initialized(&mut self) -> Result<()> {
        if !self.initialized {
            init_nft_table()?;

            // Select best available cgroup backend
            self.cgroup_backend = crate::backends::cgroup::select_best_backend()?;
            if self.cgroup_backend.is_none() {
                return Err(anyhow!("No cgroup backend available"));
            }

            self.initialized = true;
        }
        Ok(())
    }

    fn get_cgroup_backend(&self) -> Result<&Box<dyn CgroupBackend>> {
        self.cgroup_backend
            .as_ref()
            .ok_or_else(|| anyhow!("Cgroup backend not initialized"))
    }

    fn get_cgroup_backend_mut(&mut self) -> Result<&mut Box<dyn CgroupBackend>> {
        self.cgroup_backend
            .as_mut()
            .ok_or_else(|| anyhow!("Cgroup backend not initialized"))
    }
}

impl UploadThrottleBackend for NftablesUpload {
    fn name(&self) -> &'static str {
        "nftables"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Better // Better than TC, not as good as eBPF
    }

    fn is_available() -> bool {
        // Check nftables
        if !check_nft_available() {
            return false;
        }

        // Check if any cgroup backend is available
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
        self.ensure_initialized()
    }

    fn throttle_upload(
        &mut self,
        pid: i32,
        process_name: String,
        limit_bytes_per_sec: u64,
    ) -> Result<()> {
        self.ensure_initialized()?;

        // Create cgroup for process using backend
        let backend = self.get_cgroup_backend_mut()?;
        let cgroup_handle = backend.create_cgroup(pid, &process_name)?;

        // Add nftables rate limit rule using backend's filter expression
        add_cgroup_rate_limit_with_handle(&cgroup_handle, limit_bytes_per_sec, Direction::Upload)?;

        // Track throttle
        self.active_throttles.insert(
            pid,
            ThrottleInfo {
                cgroup_handle,
                limit_bytes_per_sec,
            },
        );

        Ok(())
    }

    fn remove_upload_throttle(&mut self, pid: i32) -> Result<()> {
        if let Some(info) = self.active_throttles.remove(&pid) {
            // Remove nftables rules for this cgroup
            let _ = remove_cgroup_rules_with_handle(&info.cgroup_handle, Direction::Upload);

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
