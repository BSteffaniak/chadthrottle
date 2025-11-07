// nftables Download Throttling Backend
//
// # LIMITATION - DISABLED
//
// This backend is DISABLED because nftables `socket cgroupv2` expression
// only works on OUTPUT chain, not INPUT chain. This is a kernel/netfilter
// limitation, not a bug in our code.
//
// Download (ingress) traffic arrives on INPUT chain BEFORE socket association,
// so the kernel cannot determine which cgroup the packet belongs to at INPUT
// hook time. The socket matcher requires the socket to already be associated
// with the packet, which doesn't happen until after INPUT hook processing.
//
// ## Alternatives for Download Throttling:
//
// 1. **ifb_tc** (Recommended) - Uses IFB device to redirect ingress to egress,
//    where cgroup matching works. Supports both cgroup v1 and v2.
// 2. **tc_police** - Simple per-interface rate limiting (no per-process support)
// 3. **eBPF TC** (Future) - TC classifier with BPF_PROG_TYPE_SCHED_CLS that can
//    inspect cgroup and drop packets on ingress.
//
// ## Why nftables Upload Throttling Works:
//
// Upload traffic uses OUTPUT chain where socket association is already complete,
// so `socket cgroupv2` matching works correctly. The nftables upload backend
// is fully functional.
//
// ## Technical Details:
//
// nftables documentation states: "The socket expression is only valid in the
// OUTPUT and POSTROUTING chains." Attempting to use it on INPUT chain results
// in rules that never match, causing throttling to silently fail.

use crate::backends::cgroup::{CgroupBackend, CgroupHandle};
use crate::backends::throttle::DownloadThrottleBackend;
use crate::backends::throttle::linux_nft_utils::*;
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::{Result, anyhow};
use std::collections::HashMap;

/// nftables-based download (ingress) throttling backend
///
/// **DISABLED** - See module documentation for why this cannot work with
/// nftables `socket cgroupv2` on INPUT chain.
pub struct NftablesDownload {
    active_throttles: HashMap<i32, ThrottleInfo>,
    initialized: bool,
    cgroup_backend: Option<Box<dyn CgroupBackend>>,
}

struct ThrottleInfo {
    cgroup_handle: CgroupHandle,
    limit_bytes_per_sec: u64,
}

impl NftablesDownload {
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

impl DownloadThrottleBackend for NftablesDownload {
    fn name(&self) -> &'static str {
        "nftables_download"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Better // Better than TC/IFB, not as good as eBPF
    }

    fn is_available() -> bool {
        // DISABLED: nftables socket cgroupv2 only works on OUTPUT chain, not INPUT.
        //
        // This is a fundamental kernel/netfilter limitation. The socket matcher
        // requires socket association to be complete, which doesn't happen until
        // AFTER the INPUT hook where download (ingress) traffic is processed.
        //
        // Socket association timing:
        // - INPUT hook (download): Packet arrives → socket association happens LATER → no cgroup info
        // - OUTPUT hook (upload): Socket sends packet → association already exists → cgroup info available
        //
        // The INPUT chain rule would never match any packets, causing throttling to
        // silently fail. This is not a bug we can fix - it's architectural.
        //
        // Use ifb_tc (recommended) or tc_police backends for download throttling.
        // nftables upload throttling works fine on OUTPUT chain.
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

    fn throttle_download(
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
        add_cgroup_rate_limit_with_handle(
            &cgroup_handle,
            limit_bytes_per_sec,
            Direction::Download,
        )?;

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

    fn remove_download_throttle(&mut self, pid: i32) -> Result<()> {
        if let Some(info) = self.active_throttles.remove(&pid) {
            // Remove nftables rules for this cgroup
            let _ = remove_cgroup_rules_with_handle(&info.cgroup_handle, Direction::Download);

            // Remove cgroup using backend
            if let Ok(backend) = self.get_cgroup_backend_mut() {
                let _ = backend.remove_cgroup(&info.cgroup_handle);
            }
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

        // Cleanup nftables table (shared with upload)
        // Only cleanup if no upload throttles either
        let _ = cleanup_nft_table();

        Ok(())
    }
}

impl Drop for NftablesDownload {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
