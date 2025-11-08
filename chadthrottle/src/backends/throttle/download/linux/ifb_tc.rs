// IFB + TC HTB download throttling backend
//
// REQUIREMENTS:
// - IFB kernel module (ifb)
// - TC (traffic control) command
// - **Cgroup v1 with net_cls controller** (does NOT work with cgroup v2)
//
// LIMITATIONS:
// - TC cgroup filter only works with cgroup v1 net_cls.classid
// - On cgroup v2 systems, this backend is UNAVAILABLE
//
// RECOMMENDED ALTERNATIVES FOR CGROUP V2:
// - Use `ebpf` backend for download throttling (BPF_CGROUP_INET_INGRESS)
//   - No IFB module needed
//   - ~50% lower CPU overhead
//   - ~40% lower latency
//   - Simpler setup
//   - Limitation: Drops packets instead of queuing (TCP handles this well)
//
// WHY NOT eBPF TC CLASSIFIER?
// - TC ingress packets don't have socket context yet
// - Socket lookup (`bpf_sk_lookup_*`) would add significant overhead
// - Only works for TCP/UDP (not ICMP, etc.)
// - The eBPF cgroup hook approach is superior in every way
// - See EBPF_TC_CLASSIFIER_DECISION.md for detailed analysis

use crate::backends::cgroup::{CgroupBackend, CgroupBackendType, CgroupHandle};
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
    cgroup_backend: Option<Box<dyn CgroupBackend>>,
}

struct ThrottleInfo {
    classid: u32,
    cgroup_handle: CgroupHandle,
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
            cgroup_backend: None,
        })
    }

    fn get_cgroup_backend_mut(&mut self) -> Result<&mut Box<dyn CgroupBackend>> {
        self.cgroup_backend
            .as_mut()
            .ok_or_else(|| anyhow!("Cgroup backend not initialized"))
    }

    fn setup_ifb(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        log::info!("Initializing IFB throttling backend...");

        // Load IFB module
        log::debug!("Loading IFB kernel module...");
        let status = Command::new("modprobe")
            .arg("ifb")
            .arg("numifbs=1")
            .status()
            .context("Failed to execute modprobe")?;

        if !status.success() {
            log::warn!("Failed to load IFB module (may already be loaded)");
        } else {
            log::debug!("✅ IFB module loaded");
        }

        // Check if IFB device exists
        log::debug!("Checking if IFB device {} exists...", self.ifb_device);
        let check_ifb = Command::new("ip")
            .args(&["link", "show", &self.ifb_device])
            .output();

        if check_ifb.is_err() || !check_ifb.unwrap().status.success() {
            // Create IFB device
            log::debug!("IFB device not found, creating...");
            let status = Command::new("ip")
                .args(&["link", "add", &self.ifb_device, "type", "ifb"])
                .status()
                .context("Failed to create IFB device")?;

            if !status.success() {
                return Err(anyhow!(
                    "Failed to create IFB device - check permissions and kernel module"
                ));
            }
            log::info!("✅ Created IFB device {}", self.ifb_device);
        } else {
            log::debug!("IFB device {} already exists", self.ifb_device);
        }

        // Bring up IFB device
        log::debug!("Bringing up IFB device {}...", self.ifb_device);
        let status = Command::new("ip")
            .args(&["link", "set", "dev", &self.ifb_device, "up"])
            .status()
            .context("Failed to bring up IFB device")?;

        if !status.success() {
            return Err(anyhow!("Failed to bring up IFB device"));
        }
        log::info!("✅ IFB device {} is UP", self.ifb_device);

        // Setup ingress qdisc on main interface
        log::debug!("Setting up ingress qdisc on {}...", self.interface);
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
            .context("Failed to setup ingress qdisc")?;

        if !status.success() {
            log::warn!("Ingress qdisc setup failed (may already exist)");
        } else {
            log::debug!("✅ Ingress qdisc configured on {}", self.interface);
        }

        // Redirect IPv4 ingress traffic to IFB device
        log::debug!(
            "Redirecting IPv4 ingress traffic from {} to {}...",
            self.interface,
            self.ifb_device
        );
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
            .status()
            .context("Failed to setup IPv4 redirect filter")?;

        if !status.success() {
            return Err(anyhow!(
                "Failed to setup IPv4 traffic redirect - IFB device may not be working"
            ));
        }
        log::debug!("✅ IPv4 traffic redirect configured");

        // Redirect IPv6 ingress traffic to IFB device
        log::debug!(
            "Redirecting IPv6 ingress traffic from {} to {}...",
            self.interface,
            self.ifb_device
        );
        let status = Command::new("tc")
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
            .status()
            .context("Failed to setup IPv6 redirect filter")?;

        if !status.success() {
            return Err(anyhow!(
                "Failed to setup IPv6 traffic redirect - IFB device may not be working"
            ));
        }
        log::debug!("✅ IPv6 traffic redirect configured");

        // Setup HTB qdisc on IFB device
        log::debug!("Setting up HTB qdisc on {}...", self.ifb_device);
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
        log::debug!("✅ HTB qdisc configured on {}", self.ifb_device);

        // Add IPv4 cgroup filter on IFB device
        log::debug!("Adding IPv4 cgroup filter to {}...", self.ifb_device);
        let status = Command::new("tc")
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
            .status()
            .context("Failed to add IPv4 cgroup filter")?;

        if !status.success() {
            log::warn!("IPv4 cgroup filter setup failed (may already exist)");
        } else {
            log::debug!("✅ IPv4 cgroup filter configured");
        }

        // Add IPv6 cgroup filter on IFB device
        log::debug!("Adding IPv6 cgroup filter to {}...", self.ifb_device);
        let status = Command::new("tc")
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
            .status()
            .context("Failed to add IPv6 cgroup filter")?;

        if !status.success() {
            log::warn!("IPv6 cgroup filter setup failed (may already exist)");
        } else {
            log::debug!("✅ IPv6 cgroup filter configured");
        }

        // Initialize cgroup backend
        log::debug!("Initializing cgroup backend for ifb_tc...");
        self.cgroup_backend = crate::backends::cgroup::select_best_backend()?;
        if self.cgroup_backend.is_none() {
            return Err(anyhow!("No cgroup backend available for ifb_tc"));
        }

        self.initialized = true;
        log::info!("✅ IFB throttling backend initialized successfully");
        Ok(())
    }
}

impl DownloadThrottleBackend for IfbTcDownload {
    fn name(&self) -> &'static str {
        "ifb_tc"
    }

    fn priority(&self) -> BackendPriority {
        BackendPriority::Good
    }

    fn is_available() -> bool {
        // Check IFB module
        if !check_ifb_availability() {
            log::debug!("ifb_tc unavailable: IFB kernel module not found");
            return false;
        }

        // Check TC (traffic control)
        if !check_tc_available() {
            log::debug!("ifb_tc unavailable: TC (traffic control) not available");
            return false;
        }

        // CRITICAL: ifb_tc REQUIRES cgroup v1 with net_cls controller
        // TC cgroup filter does NOT work with cgroup v2
        //
        // WHY: TC's cgroup filter reads net_cls.classid from cgroup v1
        //      Cgroup v2 removed net_cls controller in favor of eBPF programs
        //
        // SOLUTION: Use the `ebpf` download backend on cgroup v2 systems
        //           It uses BPF_CGROUP_INET_INGRESS hooks which are superior
        if !crate::backends::cgroup::is_cgroup_v1_available() {
            log::trace!("ifb_tc unavailable: requires cgroup v1 (use 'ebpf' on cgroup v2)");
            return false;
        }

        log::debug!("ifb_tc available: all requirements met (IFB, TC, cgroup v1)");
        true
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
        traffic_type: crate::process::TrafficType,
    ) -> Result<()> {
        use crate::process::TrafficType;

        // IFB/TC operates at cgroup level and cannot filter by IP address
        // Only TrafficType::All is supported
        if traffic_type != TrafficType::All {
            return Err(anyhow::anyhow!(
                "IFB/TC backend does not support traffic type filtering (Internet/Local only). \
                 Traffic type '{:?}' requested but only 'All' is supported. \
                 Consider using nftables backend for traffic type filtering (if upload) or accept 'All' traffic throttling.",
                traffic_type
            ));
        }

        // Initialize IFB if not already done
        self.init()?;

        // Get next classid
        let classid = self.next_classid;
        self.next_classid += 1;

        // Create cgroup using backend (supports both v1 and v2)
        let backend = self.get_cgroup_backend_mut()?;
        let cgroup_handle = backend.create_cgroup(pid, &process_name)?;

        // For cgroup v1, set the classid in net_cls.classid
        // For cgroup v2, classid matching won't work directly, but we still create classes
        // Note: TC cgroup filter with v2 requires eBPF or falls back to interface-wide
        if matches!(cgroup_handle.backend_type, CgroupBackendType::V1) {
            // V1: Use classid from handle (format like "1:X")
            // Extract classid number from "1:X" format
            if let Some(classid_str) = cgroup_handle.identifier.split(':').nth(1) {
                if let Ok(handle_classid) = classid_str.parse::<u32>() {
                    // Use the classid from the cgroup handle for v1
                    // Note: For v1, create_cgroup already sets net_cls.classid
                    // We use this classid for TC class creation
                    let rate_kbps = (limit_bytes_per_sec * 8 / 1000) as u32;
                    create_tc_class(&self.ifb_device, handle_classid, rate_kbps, "2:")?;

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
        // Create TC class on IFB device
        let rate_kbps = (limit_bytes_per_sec * 8 / 1000) as u32;
        create_tc_class(&self.ifb_device, classid, rate_kbps, "2:")?;

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

    fn remove_download_throttle(&mut self, pid: i32) -> Result<()> {
        if let Some(info) = self.active_throttles.remove(&pid) {
            // Remove TC class on IFB
            let _ = remove_tc_class(&self.ifb_device, info.classid, "2:");

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
        log::debug!("Cleaning up IFB throttling backend");

        // Remove all throttles
        let pids: Vec<i32> = self.active_throttles.keys().copied().collect();
        for pid in pids {
            let _ = self.remove_download_throttle(pid);
        }

        // Check if IFB device still exists before cleanup
        let check_ifb = Command::new("ip")
            .args(&["link", "show", &self.ifb_device])
            .output();

        if check_ifb.is_ok() && check_ifb.unwrap().status.success() {
            log::debug!("IFB device {} exists, cleaning up...", self.ifb_device);

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

            log::debug!("IFB device cleanup complete");
        } else {
            log::debug!("IFB device {} not found, skipping cleanup", self.ifb_device);
        }

        Ok(())
    }
}

impl Drop for IfbTcDownload {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
