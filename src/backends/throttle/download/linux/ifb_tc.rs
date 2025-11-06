// IFB + TC HTB download throttling backend

use crate::backends::throttle::DownloadThrottleBackend;
use crate::backends::throttle::linux_tc_utils::*;
use crate::backends::{BackendCapabilities, BackendPriority};
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::process::Command;

/// IFB + TC HTB download (ingress) throttling backend
pub struct IfbTcDownload {
    interface: String,
    ifb_device: String,
    active_throttles: HashMap<i32, ThrottleInfo>,
    next_classid: u32,
    initialized: bool,
}

struct ThrottleInfo {
    classid: u32,
    cgroup_path: String,
    limit_bytes_per_sec: u64,
}

impl IfbTcDownload {
    pub fn new() -> Result<Self> {
        let interface = detect_interface()?;

        Ok(Self {
            interface,
            ifb_device: "ifb0".to_string(),
            active_throttles: HashMap::new(),
            next_classid: 100,
            initialized: false,
        })
    }

    fn setup_ifb(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        // Load IFB module
        let _ = Command::new("modprobe")
            .arg("ifb")
            .arg("numifbs=1")
            .output();

        // Check if IFB device exists
        let check_ifb = Command::new("ip")
            .args(&["link", "show", &self.ifb_device])
            .output();

        if check_ifb.is_err() || !check_ifb.unwrap().status.success() {
            // Create IFB device
            let status = Command::new("ip")
                .args(&["link", "add", &self.ifb_device, "type", "ifb"])
                .status()
                .context("Failed to create IFB device")?;

            if !status.success() {
                return Err(anyhow!("Failed to create IFB device"));
            }
        }

        // Bring up IFB device
        let status = Command::new("ip")
            .args(&["link", "set", "dev", &self.ifb_device, "up"])
            .status()
            .context("Failed to bring up IFB device")?;

        if !status.success() {
            return Err(anyhow!("Failed to bring up IFB device"));
        }

        // Setup ingress qdisc on main interface
        let _ = Command::new("tc")
            .args(&[
                "qdisc",
                "add",
                "dev",
                &self.interface,
                "handle",
                "ffff:",
                "ingress",
            ])
            .status();

        // Redirect IPv4 ingress traffic to IFB device
        let _ = Command::new("tc")
            .args(&[
                "filter",
                "add",
                "dev",
                &self.interface,
                "parent",
                "ffff:",
                "protocol",
                "ip",
                "u32",
                "match",
                "u32",
                "0",
                "0",
                "action",
                "mirred",
                "egress",
                "redirect",
                "dev",
                &self.ifb_device,
            ])
            .status();

        // Redirect IPv6 ingress traffic to IFB device
        let _ = Command::new("tc")
            .args(&[
                "filter",
                "add",
                "dev",
                &self.interface,
                "parent",
                "ffff:",
                "protocol",
                "ipv6",
                "u32",
                "match",
                "u32",
                "0",
                "0",
                "action",
                "mirred",
                "egress",
                "redirect",
                "dev",
                &self.ifb_device,
            ])
            .status();

        // Setup HTB qdisc on IFB device
        let status = Command::new("tc")
            .args(&[
                "qdisc",
                "add",
                "dev",
                &self.ifb_device,
                "root",
                "handle",
                "2:",
                "htb",
                "default",
                "999",
            ])
            .status()
            .context("Failed to create HTB qdisc on IFB")?;

        if !status.success() {
            return Err(anyhow!("Failed to setup IFB HTB qdisc"));
        }

        // Add IPv4 cgroup filter on IFB device
        let _ = Command::new("tc")
            .args(&[
                "filter",
                "add",
                "dev",
                &self.ifb_device,
                "parent",
                "2:",
                "protocol",
                "ip",
                "prio",
                "1",
                "handle",
                "1:",
                "cgroup",
            ])
            .status();

        // Add IPv6 cgroup filter on IFB device
        let _ = Command::new("tc")
            .args(&[
                "filter",
                "add",
                "dev",
                &self.ifb_device,
                "parent",
                "2:",
                "protocol",
                "ipv6",
                "prio",
                "1",
                "handle",
                "2:",
                "cgroup",
            ])
            .status();

        self.initialized = true;
        Ok(())
    }
}

impl DownloadThrottleBackend for IfbTcDownload {
    fn name(&self) -> &'static str {
        "ifb_tc_download"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Good
    }

    fn is_available() -> bool {
        check_ifb_availability() && check_tc_available() && check_cgroups_available()
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
        self.setup_ifb()
    }

    fn throttle_download(
        &mut self,
        pid: i32,
        process_name: String,
        limit_bytes_per_sec: u64,
    ) -> Result<()> {
        // Initialize IFB if not already done
        self.init()?;

        // Get next classid
        let classid = self.next_classid;
        self.next_classid += 1;

        // Create cgroup for process (shared with upload if using TcHtbUpload)
        let cgroup_path = create_cgroup(pid)?;

        // Set cgroup classid
        set_cgroup_classid(&cgroup_path, classid)?;

        // Move process to cgroup
        move_process_to_cgroup(pid, &cgroup_path)?;

        // Convert bytes/sec to kbps
        let rate_kbps = (limit_bytes_per_sec * 8 / 1000) as u32;

        // Create TC class on IFB device
        create_tc_class(&self.ifb_device, classid, rate_kbps, "2:")?;

        // Track throttle
        self.active_throttles.insert(
            pid,
            ThrottleInfo {
                classid,
                cgroup_path,
                limit_bytes_per_sec,
            },
        );

        Ok(())
    }

    fn remove_download_throttle(&mut self, pid: i32) -> Result<()> {
        if let Some(info) = self.active_throttles.remove(&pid) {
            // Remove TC class on IFB
            let _ = remove_tc_class(&self.ifb_device, info.classid, "2:");

            // Remove cgroup (only if no upload throttle using it)
            let _ = remove_cgroup(&info.cgroup_path);
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

        // Remove TC qdisc from IFB
        let _ = Command::new("tc")
            .args(&["qdisc", "del", "dev", &self.ifb_device, "root"])
            .status();

        // Remove ingress qdisc from main interface
        let _ = Command::new("tc")
            .args(&["qdisc", "del", "dev", &self.interface, "ingress"])
            .status();

        // Bring down IFB device
        let _ = Command::new("ip")
            .args(&["link", "set", "dev", &self.ifb_device, "down"])
            .status();

        // Delete IFB device
        let _ = Command::new("ip")
            .args(&["link", "del", &self.ifb_device])
            .status();

        Ok(())
    }
}

impl Drop for IfbTcDownload {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
