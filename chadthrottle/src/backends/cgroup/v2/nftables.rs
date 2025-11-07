//! Cgroup v2 backend for nftables
//!
//! This backend uses cgroup v2's unified hierarchy with nftables' socket cgroupv2
//! matcher. This is the recommended approach for modern Linux systems.
//!
//! # How it works
//!
//! 1. Create cgroup at `/sys/fs/cgroup/chadthrottle/<name>`
//! 2. Write PID to `cgroup.procs`
//! 3. nftables rule matches with: `socket cgroupv2 "/sys/fs/cgroup/chadthrottle/pid_1234"`
//! 4. Rate limit enforced by nftables limit + drop action
//!
//! # Requirements
//!
//! - Cgroup v2 unified hierarchy (mounted at `/sys/fs/cgroup/`)
//! - nftables with socket cgroupv2 support (kernel 4.10+)
//! - Root privileges to create cgroups and write PIDs

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::backends::cgroup::{CgroupBackend, CgroupBackendType, CgroupHandle};

/// Base path for cgroup v2 unified hierarchy
const CGROUP_V2_BASE: &str = "/sys/fs/cgroup";
const CHADTHROTTLE_CGROUP: &str = "chadthrottle";

/// Cgroup v2 backend for nftables
pub struct CgroupV2NftablesBackend {
    /// Base path for our cgroups
    base_path: PathBuf,
}

impl CgroupV2NftablesBackend {
    pub fn new() -> Result<Self> {
        let base_path = PathBuf::from(CGROUP_V2_BASE).join(CHADTHROTTLE_CGROUP);
        Ok(Self { base_path })
    }

    /// Get the cgroup path for a process
    fn get_cgroup_path(&self, name: &str) -> PathBuf {
        self.base_path.join(name)
    }

    /// Get the relative path from /sys/fs/cgroup/ (for nftables rule)
    fn get_relative_path(&self, name: &str) -> String {
        format!("{}/{}", CHADTHROTTLE_CGROUP, name)
    }
}

impl CgroupBackend for CgroupV2NftablesBackend {
    fn backend_type(&self) -> CgroupBackendType {
        CgroupBackendType::V2Nftables
    }

    fn is_available(&self) -> Result<bool> {
        let cgroup_v2_path = PathBuf::from(CGROUP_V2_BASE);

        // Check if cgroup v2 is mounted
        if !cgroup_v2_path.exists() || !cgroup_v2_path.is_dir() {
            return Ok(false);
        }

        // Check for cgroup.controllers file (indicates v2)
        let controllers_file = cgroup_v2_path.join("cgroup.controllers");
        if !controllers_file.exists() {
            return Ok(false);
        }

        // Check if we can create directories (permission check)
        // Don't actually create anything yet, just check parent is writable
        let test_path = self.base_path.clone();
        if let Some(parent) = test_path.parent() {
            if !parent.exists() {
                return Ok(false);
            }
        }

        // Check for nftables (we'll verify cgroupv2 support at runtime)
        let nft_check = std::process::Command::new("nft").arg("--version").output();

        if nft_check.is_err() {
            return Ok(false);
        }

        Ok(true)
    }

    fn unavailable_reason(&self) -> String {
        let cgroup_v2_path = PathBuf::from(CGROUP_V2_BASE);

        if !cgroup_v2_path.exists() {
            return format!(
                "Cgroup v2 not found at {}. System may still use cgroup v1.",
                CGROUP_V2_BASE
            );
        }

        let controllers_file = cgroup_v2_path.join("cgroup.controllers");
        if !controllers_file.exists() {
            return format!(
                "{} exists but cgroup.controllers not found. Not a valid cgroup v2 hierarchy.",
                CGROUP_V2_BASE
            );
        }

        // Check for nftables
        let nft_check = std::process::Command::new("nft").arg("--version").output();

        if nft_check.is_err() {
            return "nftables not found. Install nftables package.".to_string();
        }

        "Cgroup v2 and nftables available but access denied (need root permissions)".to_string()
    }

    fn create_cgroup(&self, pid: i32, name: &str) -> Result<CgroupHandle> {
        // Create base chadthrottle cgroup if needed
        fs::create_dir_all(&self.base_path).context(format!(
            "Failed to create base cgroup at {:?}",
            self.base_path
        ))?;

        // Create process-specific cgroup
        let cgroup_name = format!("pid_{}", pid);
        let cgroup_path = self.get_cgroup_path(&cgroup_name);

        // Remove if it already exists (leftover from previous run)
        if cgroup_path.exists() {
            let _ = fs::remove_dir(&cgroup_path);
        }

        fs::create_dir_all(&cgroup_path)
            .context(format!("Failed to create cgroup at {:?}", cgroup_path))?;

        // Add process to cgroup
        let procs_file = cgroup_path.join("cgroup.procs");
        fs::write(&procs_file, format!("{}", pid))
            .context(format!("Failed to add PID {} to cgroup", pid))?;

        let relative_path = self.get_relative_path(&cgroup_name);

        log::debug!(
            "Created cgroup v2 for PID {} ({}) at {:?}",
            pid,
            name,
            cgroup_path
        );

        Ok(CgroupHandle {
            pid,
            identifier: relative_path,
            backend_type: CgroupBackendType::V2Nftables,
        })
    }

    fn remove_cgroup(&self, handle: &CgroupHandle) -> Result<()> {
        let cgroup_name = format!("pid_{}", handle.pid);
        let cgroup_path = self.get_cgroup_path(&cgroup_name);

        if cgroup_path.exists() {
            // Try to remove the directory
            // Note: This will fail if processes are still in the cgroup
            if let Err(e) = fs::remove_dir(&cgroup_path) {
                log::warn!("Failed to remove cgroup directory {:?}: {}", cgroup_path, e);

                // Try to kill any remaining processes
                let procs_file = cgroup_path.join("cgroup.procs");
                if let Ok(contents) = fs::read_to_string(&procs_file) {
                    for line in contents.lines() {
                        if let Ok(remaining_pid) = line.trim().parse::<i32>() {
                            log::debug!(
                                "Found lingering PID {} in cgroup, attempting cleanup",
                                remaining_pid
                            );
                            // Don't kill - just log. The cgroup will be cleaned up by kernel eventually
                        }
                    }
                }
            } else {
                log::debug!("Removed cgroup v2 at {:?}", cgroup_path);
            }
        }

        Ok(())
    }

    fn get_filter_expression(&self, handle: &CgroupHandle) -> String {
        // Return the cgroup path for nftables socket cgroupv2 matcher
        // The path should be relative to /sys/fs/cgroup/
        format!("socket cgroupv2 \"{}\"", handle.identifier)
    }

    fn list_active_cgroups(&self) -> Result<Vec<CgroupHandle>> {
        let mut handles = Vec::new();

        if !self.base_path.exists() {
            return Ok(handles);
        }

        let entries = fs::read_dir(&self.base_path).context(format!(
            "Failed to read cgroup directory {:?}",
            self.base_path
        ))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            // Parse PID from directory name (format: pid_<pid>)
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(pid_str) = name.strip_prefix("pid_") {
                    if let Ok(pid) = pid_str.parse::<i32>() {
                        let relative_path = self.get_relative_path(name);
                        handles.push(CgroupHandle {
                            pid,
                            identifier: relative_path,
                            backend_type: CgroupBackendType::V2Nftables,
                        });
                    }
                }
            }
        }

        Ok(handles)
    }
}
