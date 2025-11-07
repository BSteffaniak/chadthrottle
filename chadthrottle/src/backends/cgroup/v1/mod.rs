//! Cgroup v1 backend implementation using net_cls controller
//!
//! This backend uses the legacy cgroup v1 net_cls controller which tags
//! packets with a classid that can be matched by TC filters and iptables.
//!
//! # How it works
//!
//! 1. Create cgroup at `/sys/fs/cgroup/net_cls/chadthrottle/<name>`
//! 2. Write PID to `cgroup.procs`
//! 3. Write classid to `net_cls.classid` (e.g., 0x10001 for 1:1)
//! 4. TC filters match on classid: `filter match cgroup`
//!
//! # Requirements
//!
//! - Cgroup v1 hierarchy mounted (usually at `/sys/fs/cgroup/`)
//! - net_cls controller available
//! - Root privileges to create cgroups and write PIDs

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::backends::cgroup::{CgroupBackend, CgroupBackendType, CgroupHandle};

/// Base path for net_cls cgroup controller
const CGROUP_V1_BASE: &str = "/sys/fs/cgroup/net_cls";
const CHADTHROTTLE_CGROUP: &str = "chadthrottle";

/// Cgroup v1 backend using net_cls controller
pub struct CgroupV1Backend {
    /// Base path for our cgroups
    base_path: PathBuf,
    /// Track allocated classids to avoid collisions
    /// Maps PID -> classid (e.g., 0x10001 for 1:1)
    allocated_classids: Mutex<HashMap<i32, u32>>,
    /// Next available classid (incremented for each process)
    next_classid: Mutex<u32>,
}

impl CgroupV1Backend {
    pub fn new() -> Result<Self> {
        let base_path = PathBuf::from(CGROUP_V1_BASE).join(CHADTHROTTLE_CGROUP);
        Ok(Self {
            base_path,
            allocated_classids: Mutex::new(HashMap::new()),
            next_classid: Mutex::new(1), // Start at 1:1 (0x10001)
        })
    }

    /// Convert classid number to hex format (e.g., 1 -> 0x10001 for major:minor 1:1)
    fn classid_to_hex(classid: u32) -> u32 {
        // Major = 1, minor = classid
        // Format: 0xMMMMmmmm where MMMM=major (1), mmmm=minor (classid)
        0x10000 | classid
    }

    /// Convert classid to TC filter format (e.g., "1:1")
    fn classid_to_tc_format(classid: u32) -> String {
        format!("1:{}", classid)
    }

    /// Allocate a new classid for a process
    fn allocate_classid(&self, pid: i32) -> Result<u32> {
        let mut allocated = self.allocated_classids.lock().unwrap();
        let mut next = self.next_classid.lock().unwrap();

        // Check if already allocated
        if let Some(&classid) = allocated.get(&pid) {
            return Ok(classid);
        }

        // Allocate new classid
        let classid = *next;
        *next += 1;
        allocated.insert(pid, classid);

        Ok(classid)
    }

    /// Free a classid when process is removed
    fn free_classid(&self, pid: i32) {
        let mut allocated = self.allocated_classids.lock().unwrap();
        allocated.remove(&pid);
    }

    /// Get the cgroup path for a process
    fn get_cgroup_path(&self, name: &str) -> PathBuf {
        self.base_path.join(name)
    }
}

impl CgroupBackend for CgroupV1Backend {
    fn backend_type(&self) -> CgroupBackendType {
        CgroupBackendType::V1
    }

    fn is_available(&self) -> Result<bool> {
        // Check if net_cls controller exists
        let net_cls_path = PathBuf::from(CGROUP_V1_BASE);
        if !net_cls_path.exists() {
            return Ok(false);
        }

        // Check if we can read it
        if !net_cls_path.is_dir() {
            return Ok(false);
        }

        // Try to read a controller file to verify access
        let test_file = net_cls_path.join("cgroup.procs");
        if !test_file.exists() {
            return Ok(false);
        }

        Ok(true)
    }

    fn unavailable_reason(&self) -> String {
        let net_cls_path = PathBuf::from(CGROUP_V1_BASE);

        if !net_cls_path.exists() {
            return format!(
                "Cgroup v1 net_cls controller not found at {}. Your system may use cgroup v2.",
                CGROUP_V1_BASE
            );
        }

        if !net_cls_path.is_dir() {
            return format!("{} exists but is not a directory", CGROUP_V1_BASE);
        }

        "Cgroup v1 net_cls controller exists but is not accessible (permission denied?)".to_string()
    }

    fn create_cgroup(&self, pid: i32, name: &str) -> Result<CgroupHandle> {
        // Allocate classid
        let classid = self.allocate_classid(pid)?;
        let classid_hex = Self::classid_to_hex(classid);

        // Create base chadthrottle cgroup if needed
        fs::create_dir_all(&self.base_path)
            .context("Failed to create chadthrottle cgroup directory")?;

        // Create process-specific cgroup
        let cgroup_name = format!("{}_{}", name, pid);
        let cgroup_path = self.get_cgroup_path(&cgroup_name);
        fs::create_dir_all(&cgroup_path)
            .context(format!("Failed to create cgroup at {:?}", cgroup_path))?;

        // Set classid
        let classid_file = cgroup_path.join("net_cls.classid");
        fs::write(&classid_file, format!("{}", classid_hex))
            .context(format!("Failed to write classid to {:?}", classid_file))?;

        // Add process to cgroup
        let procs_file = cgroup_path.join("cgroup.procs");
        fs::write(&procs_file, format!("{}", pid))
            .context(format!("Failed to add PID {} to cgroup", pid))?;

        log::debug!(
            "Created cgroup v1 for PID {} at {:?} with classid {}",
            pid,
            cgroup_path,
            Self::classid_to_tc_format(classid)
        );

        Ok(CgroupHandle {
            pid,
            identifier: Self::classid_to_tc_format(classid),
            backend_type: CgroupBackendType::V1,
        })
    }

    fn remove_cgroup(&self, handle: &CgroupHandle) -> Result<()> {
        // Find the cgroup directory (we need to scan for it since handle only has classid)
        let entries = fs::read_dir(&self.base_path).context(format!(
            "Failed to read cgroup directory {:?}",
            self.base_path
        ))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Check if this is a cgroup for our PID
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(&format!("_{}", handle.pid)) {
                    // Try to remove the directory
                    if let Err(e) = fs::remove_dir(&path) {
                        log::warn!("Failed to remove cgroup directory {:?}: {}", path, e);
                    } else {
                        log::debug!("Removed cgroup v1 at {:?}", path);
                    }
                    break;
                }
            }
        }

        // Free the classid
        self.free_classid(handle.pid);

        Ok(())
    }

    fn get_filter_expression(&self, handle: &CgroupHandle) -> String {
        // For TC filters, we match on the classid
        // The TC command looks like: tc filter add ... handle 1: cgroup
        // And TC automatically matches packets from cgroups with net_cls.classid set
        format!("classid {}", handle.identifier)
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

            // Parse PID from directory name (format: <name>_<pid>)
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(pid_str) = name.split('_').last() {
                    if let Ok(pid) = pid_str.parse::<i32>() {
                        // Read classid
                        let classid_file = path.join("net_cls.classid");
                        if let Ok(classid_hex_str) = fs::read_to_string(&classid_file) {
                            if let Ok(classid_hex) = classid_hex_str.trim().parse::<u32>() {
                                // Extract minor from 0xMMMMmmmm format
                                let classid = classid_hex & 0xFFFF;
                                handles.push(CgroupHandle {
                                    pid,
                                    identifier: Self::classid_to_tc_format(classid),
                                    backend_type: CgroupBackendType::V1,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(handles)
    }
}
