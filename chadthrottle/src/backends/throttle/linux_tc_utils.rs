// Shared utilities for Linux TC (traffic control) and cgroup operations

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::process::Command;

pub const CGROUP_BASE: &str = "/sys/fs/cgroup/net_cls/chadthrottle";

/// Detect the primary network interface
/// Prefers interfaces with IPv4 addresses to match monitor behavior
pub fn detect_interface() -> Result<String> {
    use pnet::datalink;

    let interfaces = datalink::interfaces();

    // First priority: Interface with IPv4 address (most traffic is IPv4)
    // This matches the monitor's interface selection logic
    if let Some(iface) = interfaces.iter().find(|iface| {
        iface.is_up() && !iface.is_loopback() && iface.ips.iter().any(|ip| ip.is_ipv4())
    }) {
        log::debug!("TC backends using IPv4 interface: {}", iface.name);
        return Ok(iface.name.clone());
    }

    // Fallback: Any interface with IPs (even IPv6-only)
    if let Some(iface) = interfaces
        .into_iter()
        .find(|iface| iface.is_up() && !iface.is_loopback() && !iface.ips.is_empty())
    {
        log::warn!("No IPv4 interface found, using: {}", iface.name);
        return Ok(iface.name);
    }

    Err(anyhow!("No suitable network interface found"))
}

/// Check if TC (traffic control) is available
pub fn check_tc_available() -> bool {
    Command::new("tc").arg("qdisc").arg("show").output().is_ok()
}

/// Check if cgroups net_cls is available  
pub fn check_cgroups_available() -> bool {
    std::path::Path::new("/sys/fs/cgroup/net_cls").exists()
}

/// Create a cgroup for a process
pub fn create_cgroup(pid: i32) -> Result<String> {
    let cgroup_name = format!("pid_{}", pid);
    let cgroup_path = format!("{}/{}", CGROUP_BASE, cgroup_name);

    // Create base directory if it doesn't exist
    fs::create_dir_all(CGROUP_BASE).context("Failed to create cgroup base directory")?;

    // Create cgroup directory for this process
    fs::create_dir_all(&cgroup_path)
        .context(format!("Failed to create cgroup at {}", cgroup_path))?;

    Ok(cgroup_path)
}

/// Set the network classid for a cgroup
pub fn set_cgroup_classid(cgroup_path: &str, classid: u32) -> Result<()> {
    let classid_file = format!("{}/net_cls.classid", cgroup_path);

    // classid format: 0xAAAABBBB where AAAA is major, BBBB is minor
    // We use 1:classid, so 0x0001XXXX
    let classid_value = format!("{}", (1 << 16) | classid);

    fs::write(&classid_file, classid_value)
        .context(format!("Failed to set classid in {}", classid_file))?;

    Ok(())
}

/// Move a process to a cgroup
pub fn move_process_to_cgroup(pid: i32, cgroup_path: &str) -> Result<()> {
    let procs_file = format!("{}/cgroup.procs", cgroup_path);

    fs::write(&procs_file, format!("{}", pid))
        .context(format!("Failed to move process {} to cgroup", pid))?;

    Ok(())
}

/// Setup TC root HTB qdisc on an interface (for egress/upload)
pub fn setup_tc_htb_root(interface: &str) -> Result<()> {
    // Check if HTB qdisc already exists
    let check_qdisc = Command::new("tc")
        .args(&["qdisc", "show", "dev", interface])
        .output()
        .context("Failed to check existing qdiscs")?;

    let output = String::from_utf8_lossy(&check_qdisc.stdout);

    // If HTB not present, add it
    if !output.contains("htb") {
        // Remove any existing root qdisc first
        let _ = Command::new("tc")
            .args(&["qdisc", "del", "dev", interface, "root"])
            .output();

        // Create HTB (Hierarchical Token Bucket) qdisc
        let status = Command::new("tc")
            .args(&[
                "qdisc", "add", "dev", interface, "root", "handle", "1:", "htb", "default", "999",
            ])
            .status()
            .context("Failed to create HTB qdisc")?;

        if !status.success() {
            return Err(anyhow!("Failed to setup TC root qdisc"));
        }
    }

    // Add IPv4 cgroup filter
    let _ = Command::new("tc")
        .args(&[
            "filter", "add", "dev", interface, "parent", "1:", "protocol", "ip", "prio", "1",
            "handle", "1:", "cgroup",
        ])
        .status();

    // Add IPv6 cgroup filter
    let _ = Command::new("tc")
        .args(&[
            "filter", "add", "dev", interface, "parent", "1:", "protocol", "ipv6", "prio", "1",
            "handle", "2:", "cgroup",
        ])
        .status();

    Ok(())
}

/// Create a TC HTB class for rate limiting on an interface
pub fn create_tc_class(
    interface: &str,
    classid: u32,
    rate_kbps: u32,
    parent_handle: &str,
) -> Result<()> {
    if rate_kbps == 0 {
        return Ok(()); // No limit
    }

    let rate = format!("{}kbit", rate_kbps);

    let status = Command::new("tc")
        .args(&[
            "class",
            "add",
            "dev",
            interface,
            "parent",
            parent_handle,
            "classid",
            &format!("{}:{}", parent_handle.trim_end_matches(':'), classid),
            "htb",
            "rate",
            &rate,
            "ceil",
            &rate, // Ceiling = no bursting above rate
        ])
        .status()
        .context("Failed to create TC class")?;

    if !status.success() {
        return Err(anyhow!("Failed to create TC class for classid {}", classid));
    }

    Ok(())
}

/// Remove a TC class
pub fn remove_tc_class(interface: &str, classid: u32, parent_handle: &str) -> Result<()> {
    let _ = Command::new("tc")
        .args(&[
            "class",
            "del",
            "dev",
            interface,
            "parent",
            parent_handle,
            "classid",
            &format!("{}:{}", parent_handle.trim_end_matches(':'), classid),
        ])
        .status();

    Ok(())
}

/// Remove a cgroup
pub fn remove_cgroup(cgroup_path: &str) -> Result<()> {
    if let Err(e) = fs::remove_dir(cgroup_path) {
        log::error!("Warning: Failed to remove cgroup {}: {}", cgroup_path, e);
    }
    Ok(())
}

/// Check if IFB module is available
pub fn check_ifb_availability() -> bool {
    // Try to load the module first
    let _ = Command::new("modprobe")
        .arg("ifb")
        .arg("numifbs=1")
        .output();

    // Check if we can create an IFB device (or if one exists)
    let check = Command::new("ip")
        .args(&["link", "add", "name", "ifb_test", "type", "ifb"])
        .output();

    if let Ok(output) = check {
        if output.status.success() {
            // Clean up test device
            let _ = Command::new("ip")
                .args(&["link", "del", "ifb_test"])
                .output();
            return true;
        }
    }

    // Also check if IFB device already exists
    let check_existing = Command::new("ip")
        .args(&["link", "show", "type", "ifb"])
        .output();

    if let Ok(output) = check_existing {
        if output.status.success() && !output.stdout.is_empty() {
            return true;
        }
    }

    log::debug!("IFB module not found");
    false
}
