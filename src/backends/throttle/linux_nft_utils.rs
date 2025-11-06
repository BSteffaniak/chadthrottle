// Shared utilities for Linux nftables operations

use anyhow::{Context, Result, anyhow};
use std::process::Command;

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
    // Delete the entire table
    let _ = Command::new("nft")
        .args(&["delete", "table", "inet", NFT_TABLE])
        .status();

    log::info!("Cleaned up nftables table");
    Ok(())
}

/// Direction for rate limiting
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Upload,
    Download,
}
