// TC HTB (Hierarchical Token Bucket) upload throttling backend

use anyhow::Result;
use std::collections::HashMap;
use crate::backends::{BackendCapabilities, BackendPriority};
use crate::backends::throttle::UploadThrottleBackend;
use crate::backends::throttle::linux_tc_utils::*;

/// TC HTB upload (egress) throttling backend
pub struct TcHtbUpload {
    interface: String,
    active_throttles: HashMap<i32, ThrottleInfo>,
    next_classid: u32,
    initialized: bool,
}

struct ThrottleInfo {
    classid: u32,
    cgroup_path: String,
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
        })
    }
}

impl UploadThrottleBackend for TcHtbUpload {
    fn name(&self) -> &'static str {
        "tc_htb_upload"
    }
    
    fn priority(&self) -> BackendPriority {
        BackendPriority::Good
    }
    
    fn is_available() -> bool {
        check_tc_available() && check_cgroups_available()
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
        
        self.initialized = true;
        Ok(())
    }
    
    fn throttle_upload(
        &mut self,
        pid: i32,
        process_name: String,
        limit_bytes_per_sec: u64,
    ) -> Result<()> {
        // Initialize if not already done
        self.init()?;
        
        // Get next classid
        let classid = self.next_classid;
        self.next_classid += 1;
        
        // Create cgroup for process
        let cgroup_path = create_cgroup(pid)?;
        
        // Set cgroup classid
        set_cgroup_classid(&cgroup_path, classid)?;
        
        // Move process to cgroup
        move_process_to_cgroup(pid, &cgroup_path)?;
        
        // Convert bytes/sec to kbps (kilobits per second)
        let rate_kbps = (limit_bytes_per_sec * 8 / 1000) as u32;
        
        // Create TC class with rate limit
        create_tc_class(&self.interface, classid, rate_kbps, "1:")?;
        
        // Track throttle
        self.active_throttles.insert(pid, ThrottleInfo {
            classid,
            cgroup_path,
            limit_bytes_per_sec,
        });
        
        Ok(())
    }
    
    fn remove_upload_throttle(&mut self, pid: i32) -> Result<()> {
        if let Some(info) = self.active_throttles.remove(&pid) {
            // Remove TC class
            let _ = remove_tc_class(&self.interface, info.classid, "1:");
            
            // Remove cgroup
            let _ = remove_cgroup(&info.cgroup_path);
        }
        
        Ok(())
    }
    
    fn get_upload_throttle(&self, pid: i32) -> Option<u64> {
        self.active_throttles.get(&pid).map(|info| info.limit_bytes_per_sec)
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
