// Shared utilities for Linux nftables operations

use anyhow::{Context, Result, anyhow};
use std::process::Command;

use crate::backends::cgroup::{CgroupBackendType, CgroupHandle};

const NFT_TABLE: &str = "chadthrottle";
const NFT_CHAIN_OUTPUT: &str = "output_limit";
const NFT_CHAIN_INPUT: &str = "input_limit";

/// Check if nftables is available
pub fn check_nft_available() -> bool {
    Command::new("nft").arg("--version").output().is_ok()
}

/// Initialize nftables table and chains
pub fn init_nft_table() -> Result<()> {
    // Check if table already exists
    let check = Command::new("nft")
        .args(&["list", "table", "inet", NFT_TABLE])
        .output();

    if check.is_ok() && check.unwrap().status.success() {
        // Table exists, we're good
        return Ok(());
    }

    // Create table
    let status = Command::new("nft")
        .args(&["add", "table", "inet", NFT_TABLE])
        .status()
        .context("Failed to create nftables table")?;

    if !status.success() {
        return Err(anyhow!("Failed to create nftables table"));
    }

    // Create output chain (for upload throttling)
    let status = Command::new("nft")
        .args(&[
            "add",
            "chain",
            "inet",
            NFT_TABLE,
            NFT_CHAIN_OUTPUT,
            "{",
            "type",
            "filter",
            "hook",
            "output",
            "priority",
            "0",
            ";",
            "}",
        ])
        .status()
        .context("Failed to create output chain")?;

    if !status.success() {
        return Err(anyhow!("Failed to create output chain"));
    }

    // Create input chain (for download throttling)
    let status = Command::new("nft")
        .args(&[
            "add",
            "chain",
            "inet",
            NFT_TABLE,
            NFT_CHAIN_INPUT,
            "{",
            "type",
            "filter",
            "hook",
            "input",
            "priority",
            "0",
            ";",
            "}",
        ])
        .status()
        .context("Failed to create input chain")?;

    if !status.success() {
        return Err(anyhow!("Failed to create input chain"));
    }

    log::info!("Initialized nftables table and chains");
    Ok(())
}

/// Add rate limit rule for a cgroup
pub fn add_cgroup_rate_limit(
    cgroup_path: &str,
    rate_bytes_per_sec: u64,
    direction: Direction,
) -> Result<()> {
    let chain = match direction {
        Direction::Upload => NFT_CHAIN_OUTPUT,
        Direction::Download => NFT_CHAIN_INPUT,
    };

    // nftables doesn't directly support cgroup matching in the same way as TC
    // We'll use socket cgroup matching when available (kernel 4.10+)
    // For now, we'll use a simpler approach with meta cgroup matching

    // Convert bytes/sec to bits/sec for nftables
    let rate_bits = rate_bytes_per_sec * 8;

    // Create rule with rate limit
    // Format: nft add rule inet chadthrottle output_limit socket cgroupv2 level 1 "/path" limit rate 1000 kbytes/second
    let rule = format!(
        "socket cgroupv2 level 1 \"{}\" limit rate {} bytes/second",
        cgroup_path, rate_bytes_per_sec
    );

    let status = Command::new("nft")
        .args(&["add", "rule", "inet", NFT_TABLE, chain, &rule])
        .status()
        .context("Failed to add nftables rate limit rule")?;

    if !status.success() {
        return Err(anyhow!(
            "Failed to add rate limit rule for cgroup {}",
            cgroup_path
        ));
    }

    log::debug!(
        "Added nftables rate limit: {} bytes/sec for {}",
        rate_bytes_per_sec,
        cgroup_path
    );
    Ok(())
}

/// Add rate limit rule for a cgroup with traffic type filtering
pub fn add_cgroup_rate_limit_with_traffic_type(
    cgroup_path: &str,
    rate_bytes_per_sec: u64,
    direction: Direction,
    traffic_type: crate::process::TrafficType,
) -> Result<()> {
    use crate::process::TrafficType;

    let chain = match direction {
        Direction::Upload => NFT_CHAIN_OUTPUT,
        Direction::Download => NFT_CHAIN_INPUT,
    };

    // Build IP filter based on traffic type
    let ip_filter = match traffic_type {
        TrafficType::All => {
            // No IP filtering - apply to all traffic
            String::new()
        }
        TrafficType::Internet => {
            // Only throttle non-local IPs (internet traffic)
            // Exclude RFC1918 private ranges, loopback, link-local, etc.
            format!(
                "ip daddr != {{ 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.0/8, 169.254.0.0/16, 224.0.0.0/4, 240.0.0.0/4 }} \
                 ip6 daddr != {{ ::1, fe80::/10, fc00::/7, ff00::/8 }} "
            )
        }
        TrafficType::Local => {
            // Only throttle local network IPs
            format!("ip daddr {{ 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16 }} ")
        }
    };

    // Create rule with rate limit and optional IP filtering
    let rule = if ip_filter.is_empty() {
        format!(
            "socket cgroupv2 level 1 \"{}\" limit rate {} bytes/second",
            cgroup_path, rate_bytes_per_sec
        )
    } else {
        format!(
            "socket cgroupv2 level 1 \"{}\" {} limit rate {} bytes/second",
            cgroup_path, ip_filter, rate_bytes_per_sec
        )
    };

    let status = Command::new("nft")
        .args(&["add", "rule", "inet", NFT_TABLE, chain, &rule])
        .status()
        .context("Failed to add nftables rate limit rule")?;

    if !status.success() {
        return Err(anyhow!(
            "Failed to add rate limit rule for cgroup {} with traffic type {:?}",
            cgroup_path,
            traffic_type
        ));
    }

    log::info!(
        "Added nftables rate limit: {} bytes/sec for {} (traffic type: {:?})",
        rate_bytes_per_sec,
        cgroup_path,
        traffic_type
    );
    Ok(())
}

/// Remove all rules for a cgroup
pub fn remove_cgroup_rules(cgroup_path: &str, direction: Direction) -> Result<()> {
    let chain = match direction {
        Direction::Upload => NFT_CHAIN_OUTPUT,
        Direction::Download => NFT_CHAIN_INPUT,
    };

    // List rules and find ones matching our cgroup
    let output = Command::new("nft")
        .args(&["--handle", "list", "chain", "inet", NFT_TABLE, chain])
        .output()
        .context("Failed to list nftables rules")?;

    let rules_output = String::from_utf8_lossy(&output.stdout);

    // Parse output to find rule handles containing our cgroup path
    for line in rules_output.lines() {
        if line.contains(cgroup_path) && line.contains("# handle") {
            // Extract handle number
            if let Some(handle_str) = line.split("# handle ").nth(1) {
                if let Ok(handle) = handle_str.trim().parse::<u32>() {
                    // Delete rule by handle
                    let _ = Command::new("nft")
                        .args(&[
                            "delete",
                            "rule",
                            "inet",
                            NFT_TABLE,
                            chain,
                            "handle",
                            &handle.to_string(),
                        ])
                        .status();
                }
            }
        }
    }

    Ok(())
}

/// Cleanup nftables table
pub fn cleanup_nft_table() -> Result<()> {
    // Delete the entire table (ignore errors - may already be deleted by other backend)
    let result = Command::new("nft")
        .args(&["delete", "table", "inet", NFT_TABLE])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            log::info!("Cleaned up nftables table");
        }
        Ok(output) => {
            // Table already deleted or doesn't exist - this is fine
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such file or directory") {
                log::debug!("nftables table already cleaned up");
            } else {
                log::warn!("nftables cleanup warning: {}", stderr.trim());
            }
        }
        Err(e) => {
            log::debug!("nftables cleanup error (likely already cleaned): {}", e);
        }
    }

    Ok(())
}

/// Direction for rate limiting
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Upload,
    Download,
}

/// Add rate limit rule for a cgroup using CgroupHandle
pub fn add_cgroup_rate_limit_with_handle(
    handle: &CgroupHandle,
    rate_bytes_per_sec: u64,
    direction: Direction,
) -> Result<()> {
    let chain = match direction {
        Direction::Upload => NFT_CHAIN_OUTPUT,
        Direction::Download => NFT_CHAIN_INPUT,
    };

    // Build rule based on cgroup backend type (not string parsing)
    let rule = match handle.backend_type {
        CgroupBackendType::V2Nftables | CgroupBackendType::V2Ebpf => {
            // V2: socket cgroupv2 level 0 "path" limit rate over X bytes/second drop
            // level 0 = exact cgroup path match (most precise)
            // "over" keyword is critical - drops packets that EXCEED the rate limit
            // identifier is the path relative to /sys/fs/cgroup/
            format!(
                "socket cgroupv2 level 0 \"{}\" limit rate over {} bytes/second drop",
                handle.identifier, rate_bytes_per_sec
            )
        }
        CgroupBackendType::V1 => {
            // V1: meta cgroup classid limit rate over X bytes/second drop
            // identifier should be like "1:1"
            format!(
                "meta cgroup {} limit rate over {} bytes/second drop",
                handle.identifier, rate_bytes_per_sec
            )
        }
    };

    let status = Command::new("nft")
        .args(&["add", "rule", "inet", NFT_TABLE, chain, &rule])
        .status()
        .context("Failed to add nftables rate limit rule")?;

    if !status.success() {
        return Err(anyhow!(
            "Failed to add rate limit rule for cgroup (PID {})",
            handle.pid
        ));
    }

    log::debug!(
        "Added nftables rate limit: {} bytes/sec for PID {} (backend: {})",
        rate_bytes_per_sec,
        handle.pid,
        handle.backend_type
    );
    Ok(())
}

/// Add rate limit rule for a cgroup with traffic type filtering using CgroupHandle
pub fn add_cgroup_rate_limit_with_handle_and_traffic_type(
    handle: &CgroupHandle,
    rate_bytes_per_sec: u64,
    direction: Direction,
    traffic_type: crate::process::TrafficType,
) -> Result<()> {
    use crate::process::TrafficType;

    let chain = match direction {
        Direction::Upload => NFT_CHAIN_OUTPUT,
        Direction::Download => NFT_CHAIN_INPUT,
    };

    // Build IP filter based on traffic type
    let ip_filter = match traffic_type {
        TrafficType::All => {
            // No IP filtering - apply to all traffic
            String::new()
        }
        TrafficType::Internet => {
            // Only throttle non-local IPs (internet traffic)
            // Exclude RFC1918 private ranges, loopback, link-local, multicast, reserved
            format!(
                "ip daddr != {{ 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.0/8, 169.254.0.0/16, 224.0.0.0/4, 240.0.0.0/4 }} \
                 ip6 daddr != {{ ::1, fe80::/10, fc00::/7, ff00::/8 }} "
            )
        }
        TrafficType::Local => {
            // Only throttle local network IPs (RFC1918 + link-local)
            format!("ip daddr {{ 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16 }} ")
        }
    };

    // Build rule based on cgroup backend type
    let cgroup_match = match handle.backend_type {
        CgroupBackendType::V2Nftables | CgroupBackendType::V2Ebpf => {
            format!("socket cgroupv2 level 0 \"{}\"", handle.identifier)
        }
        CgroupBackendType::V1 => {
            format!("meta cgroup {}", handle.identifier)
        }
    };

    // Combine cgroup match, optional IP filter, and rate limit
    let rule = if ip_filter.is_empty() {
        format!(
            "{} limit rate over {} bytes/second drop",
            cgroup_match, rate_bytes_per_sec
        )
    } else {
        format!(
            "{} {} limit rate over {} bytes/second drop",
            cgroup_match, ip_filter, rate_bytes_per_sec
        )
    };

    let status = Command::new("nft")
        .args(&["add", "rule", "inet", NFT_TABLE, chain, &rule])
        .status()
        .context("Failed to add nftables rate limit rule with traffic type filter")?;

    if !status.success() {
        return Err(anyhow!(
            "Failed to add rate limit rule for cgroup (PID {}) with traffic type {:?}",
            handle.pid,
            traffic_type
        ));
    }

    log::info!(
        "Added nftables rate limit: {} bytes/sec for PID {} (backend: {}, traffic type: {:?})",
        rate_bytes_per_sec,
        handle.pid,
        handle.backend_type,
        traffic_type
    );
    Ok(())
}

/// Remove all rules for a cgroup using CgroupHandle
pub fn remove_cgroup_rules_with_handle(handle: &CgroupHandle, direction: Direction) -> Result<()> {
    let chain = match direction {
        Direction::Upload => NFT_CHAIN_OUTPUT,
        Direction::Download => NFT_CHAIN_INPUT,
    };

    // List rules and find ones matching our cgroup identifier
    let output = Command::new("nft")
        .args(&["--handle", "list", "chain", "inet", NFT_TABLE, chain])
        .output()
        .context("Failed to list nftables rules")?;

    let rules_output = String::from_utf8_lossy(&output.stdout);

    // Parse output to find rule handles containing our identifier
    // Match based on backend type for accurate detection
    for line in rules_output.lines() {
        let matches = match handle.backend_type {
            CgroupBackendType::V2Nftables | CgroupBackendType::V2Ebpf => {
                // V2: identifier is a path like "chadthrottle/pid_1234"
                // Line will contain: socket cgroupv2 "chadthrottle/pid_1234"
                line.contains(&handle.identifier)
            }
            CgroupBackendType::V1 => {
                // V1: identifier is classid like "1:1"
                // Line will contain: meta cgroup 1:1
                line.contains(&format!("cgroup {}", handle.identifier))
            }
        };

        if matches && line.contains("# handle") {
            // Extract handle number
            if let Some(handle_str) = line.split("# handle ").nth(1) {
                if let Ok(rule_handle) = handle_str.trim().parse::<u32>() {
                    // Delete rule by handle
                    let result = Command::new("nft")
                        .args(&[
                            "delete",
                            "rule",
                            "inet",
                            NFT_TABLE,
                            chain,
                            "handle",
                            &rule_handle.to_string(),
                        ])
                        .status();

                    if result.is_ok() {
                        log::debug!(
                            "Removed nftables rule handle {} for PID {}",
                            rule_handle,
                            handle.pid
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
