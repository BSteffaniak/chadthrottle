use anyhow::{Result, anyhow, Context};
use pnet::datalink;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::time::Instant;
use crate::process::ThrottleLimit;

const CGROUP_BASE: &str = "/sys/fs/cgroup/net_cls/chadthrottle";

#[derive(Debug, Clone)]
pub struct ActiveThrottle {
    pub pid: i32,
    pub process_name: String,
    pub download_limit: Option<u64>, // bytes/sec
    pub upload_limit: Option<u64>,   // bytes/sec
    pub classid: u32,
    pub cgroup_path: String,
    pub created_at: Instant,
}

pub struct ThrottleManager {
    active_throttles: HashMap<i32, ActiveThrottle>,
    next_classid: u32,
    interface: String,
    tc_initialized: bool,
}

impl ThrottleManager {
    pub fn new() -> Result<Self> {
        let interface = Self::detect_interface()?;
        
        Ok(Self {
            active_throttles: HashMap::new(),
            next_classid: 100, // Start at 100 to avoid conflicts
            interface,
            tc_initialized: false,
        })
    }

    /// Detect the network interface to use for throttling
    fn detect_interface() -> Result<String> {
        use pnet::datalink;
        
        let interface = datalink::interfaces()
            .into_iter()
            .find(|iface| {
                iface.is_up() 
                && !iface.is_loopback() 
                && !iface.ips.is_empty()
            })
            .ok_or_else(|| anyhow!("No suitable network interface found"))?;
        
        Ok(interface.name)
    }

    /// Setup TC root qdisc and cgroup filter (one-time setup)
    fn setup_tc_root(&mut self) -> Result<()> {
        if self.tc_initialized {
            return Ok(());
        }

        // Check if HTB qdisc already exists
        let check_qdisc = Command::new("tc")
            .args(&["qdisc", "show", "dev", &self.interface])
            .output()
            .context("Failed to check existing qdiscs")?;
        
        let output = String::from_utf8_lossy(&check_qdisc.stdout);
        
        // If HTB not present, add it
        if !output.contains("htb") {
            // Remove any existing root qdisc first
            let _ = Command::new("tc")
                .args(&["qdisc", "del", "dev", &self.interface, "root"])
                .output();
            
            // Create HTB (Hierarchical Token Bucket) qdisc
            let status = Command::new("tc")
                .args(&[
                    "qdisc", "add", "dev", &self.interface,
                    "root", "handle", "1:", "htb", "default", "999"
                ])
                .status()
                .context("Failed to create HTB qdisc")?;
            
            if !status.success() {
                return Err(anyhow!("Failed to setup TC root qdisc"));
            }
        }

        // Add cgroup filter to match packets based on cgroup classid
        // This allows us to match all packets from processes in our cgroups
        let status = Command::new("tc")
            .args(&[
                "filter", "add", "dev", &self.interface,
                "parent", "1:", "protocol", "ip",
                "prio", "1", "handle", "1:", "cgroup"
            ])
            .status()
            .context("Failed to add cgroup filter")?;
        
        if !status.success() {
            return Err(anyhow!("Failed to setup cgroup filter"));
        }

        self.tc_initialized = true;
        Ok(())
    }

    /// Create a cgroup for a process
    fn create_cgroup(&self, pid: i32) -> Result<String> {
        let cgroup_name = format!("pid_{}", pid);
        let cgroup_path = format!("{}/{}", CGROUP_BASE, cgroup_name);
        
        // Create base directory if it doesn't exist
        fs::create_dir_all(CGROUP_BASE)
            .context("Failed to create cgroup base directory")?;
        
        // Create cgroup directory for this process
        fs::create_dir_all(&cgroup_path)
            .context(format!("Failed to create cgroup at {}", cgroup_path))?;
        
        Ok(cgroup_path)
    }

    /// Set the network classid for a cgroup
    fn set_cgroup_classid(&self, cgroup_path: &str, classid: u32) -> Result<()> {
        let classid_file = format!("{}/net_cls.classid", cgroup_path);
        
        // classid format: 0xAAAABBBB where AAAA is major, BBBB is minor
        // We use 1:classid, so 0x0001XXXX
        let classid_value = format!("{}", (1 << 16) | classid);
        
        fs::write(&classid_file, classid_value)
            .context(format!("Failed to set classid in {}", classid_file))?;
        
        Ok(())
    }

    /// Move a process to a cgroup
    fn move_process_to_cgroup(&self, pid: i32, cgroup_path: &str) -> Result<()> {
        let procs_file = format!("{}/cgroup.procs", cgroup_path);
        
        fs::write(&procs_file, format!("{}", pid))
            .context(format!("Failed to move process {} to cgroup", pid))?;
        
        Ok(())
    }

    /// Create TC class for rate limiting
    fn create_tc_class(&self, classid: u32, _download_limit_kbps: u32, upload_limit_kbps: u32) -> Result<()> {
        // Create class for download (ingress) - typically limited by ISP, we focus on upload
        // For now, we'll create classes for egress (upload) which is what we can control
        
        // Create HTB class for this throttle
        // Rate is in kbit (kilobits per second)
        let rate = if upload_limit_kbps > 0 {
            format!("{}kbit", upload_limit_kbps)
        } else {
            "1000mbit".to_string() // Very high = unlimited
        };
        
        let status = Command::new("tc")
            .args(&[
                "class", "add", "dev", &self.interface,
                "parent", "1:", "classid", &format!("1:{}", classid),
                "htb", "rate", &rate,
                "ceil", &rate, // Ceiling = no bursting above rate
            ])
            .status()
            .context("Failed to create TC class")?;
        
        if !status.success() {
            return Err(anyhow!("Failed to create TC class for classid {}", classid));
        }

        Ok(())
    }

    /// Remove TC class
    fn remove_tc_class(&self, classid: u32) -> Result<()> {
        let status = Command::new("tc")
            .args(&[
                "class", "del", "dev", &self.interface,
                "parent", "1:", "classid", &format!("1:{}", classid)
            ])
            .status()
            .context("Failed to remove TC class")?;
        
        if !status.success() {
            // Don't fail if class doesn't exist
            eprintln!("Warning: Failed to remove TC class {}", classid);
        }

        Ok(())
    }

    /// Throttle a process using cgroups
    pub fn throttle_process(
        &mut self,
        pid: i32,
        process_name: String,
        limit: &ThrottleLimit,
    ) -> Result<()> {
        // Setup TC root if not already done
        self.setup_tc_root()?;

        // Check if already throttled
        if self.active_throttles.contains_key(&pid) {
            return Err(anyhow!("Process {} is already throttled", pid));
        }

        // Get next classid
        let classid = self.next_classid;
        self.next_classid += 1;

        // Create cgroup
        let cgroup_path = self.create_cgroup(pid)
            .context("Failed to create cgroup")?;

        // Set classid on cgroup
        self.set_cgroup_classid(&cgroup_path, classid)
            .context("Failed to set cgroup classid")?;

        // Move process to cgroup
        self.move_process_to_cgroup(pid, &cgroup_path)
            .context("Failed to move process to cgroup")?;

        // Convert bytes/sec to kbps (kilobits per second)
        let download_kbps = limit.download_limit.map(|b| (b * 8 / 1000) as u32).unwrap_or(0);
        let upload_kbps = limit.upload_limit.map(|b| (b * 8 / 1000) as u32).unwrap_or(0);

        // Create TC class with rate limit
        self.create_tc_class(classid, download_kbps, upload_kbps)
            .context("Failed to create TC class")?;

        // Track active throttle
        let throttle = ActiveThrottle {
            pid,
            process_name,
            download_limit: limit.download_limit,
            upload_limit: limit.upload_limit,
            classid,
            cgroup_path: cgroup_path.clone(),
            created_at: Instant::now(),
        };

        self.active_throttles.insert(pid, throttle);

        Ok(())
    }

    /// Remove throttle from a process
    pub fn remove_throttle(&mut self, pid: i32) -> Result<()> {
        let throttle = self.active_throttles.remove(&pid)
            .ok_or_else(|| anyhow!("Process {} is not throttled", pid))?;

        // Remove TC class
        self.remove_tc_class(throttle.classid)?;

        // Remove cgroup directory (this automatically moves process back to root cgroup)
        if let Err(e) = fs::remove_dir(&throttle.cgroup_path) {
            eprintln!("Warning: Failed to remove cgroup {}: {}", throttle.cgroup_path, e);
        }

        Ok(())
    }

    /// Get throttle info for a process
    pub fn get_throttle(&self, pid: i32) -> Option<&ActiveThrottle> {
        self.active_throttles.get(&pid)
    }

    /// Check if a process is throttled
    pub fn is_throttled(&self, pid: i32) -> bool {
        self.active_throttles.contains_key(&pid)
    }

    /// Get all active throttles
    pub fn get_all_throttles(&self) -> &HashMap<i32, ActiveThrottle> {
        &self.active_throttles
    }

    /// Cleanup all throttles (call on exit)
    pub fn cleanup(&mut self) -> Result<()> {
        // Remove all throttles
        let pids: Vec<i32> = self.active_throttles.keys().copied().collect();
        for pid in pids {
            if let Err(e) = self.remove_throttle(pid) {
                eprintln!("Warning: Failed to remove throttle for PID {}: {}", pid, e);
            }
        }

        // Remove TC qdisc (cleanup)
        let _ = Command::new("tc")
            .args(&["qdisc", "del", "dev", &self.interface, "root"])
            .status();

        // Remove base cgroup directory
        let _ = fs::remove_dir_all(CGROUP_BASE);

        Ok(())
    }
}

impl Drop for ThrottleManager {
    fn drop(&mut self) {
        // Cleanup on drop
        if let Err(e) = self.cleanup() {
            eprintln!("Warning: Failed to cleanup throttles: {}", e);
        }
    }
}
