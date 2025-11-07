// eBPF utility functions for loading and managing eBPF programs
#[cfg(feature = "throttle-ebpf")]
use anyhow::{Context, Result};
#[cfg(feature = "throttle-ebpf")]
use std::fs;
#[cfg(feature = "throttle-ebpf")]
use std::path::{Path, PathBuf};

#[cfg(feature = "throttle-ebpf")]
use aya::{
    Ebpf,
    maps::HashMap as BpfHashMap,
    programs::{CgroupSkb, CgroupSkbAttachType},
};
#[cfg(feature = "throttle-ebpf")]
use chadthrottle_common::{CgroupThrottleConfig, ThrottleStats, TokenBucket};

/// Get the cgroup ID for a given PID
#[cfg(feature = "throttle-ebpf")]
pub fn get_cgroup_id(pid: i32) -> Result<u64> {
    let cgroup_path = format!("/proc/{}/cgroup", pid);
    let contents = fs::read_to_string(&cgroup_path)
        .with_context(|| format!("Failed to read {}", cgroup_path))?;

    // Parse cgroup v2 format: "0::/system.slice/some-service.service"
    // We need to get the inode number of the cgroup directory
    let cgroup_line = contents
        .lines()
        .find(|line| line.starts_with("0::"))
        .ok_or_else(|| anyhow::anyhow!("No cgroup v2 entry found for PID {}", pid))?;

    let cgroup_rel_path = cgroup_line
        .strip_prefix("0::")
        .ok_or_else(|| anyhow::anyhow!("Invalid cgroup v2 format"))?;

    // Get the inode of the cgroup directory
    let cgroup_full_path = if cgroup_rel_path.is_empty() {
        PathBuf::from("/sys/fs/cgroup")
    } else {
        PathBuf::from("/sys/fs/cgroup").join(cgroup_rel_path.trim_start_matches('/'))
    };

    let metadata = fs::metadata(&cgroup_full_path)
        .with_context(|| format!("Failed to get metadata for {:?}", cgroup_full_path))?;

    // Get inode number using std::os::unix::fs::MetadataExt
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.ino())
}

/// Find the cgroup path to attach to for a given PID
#[cfg(feature = "throttle-ebpf")]
pub fn get_cgroup_path(pid: i32) -> Result<PathBuf> {
    let cgroup_path = format!("/proc/{}/cgroup", pid);
    let contents = fs::read_to_string(&cgroup_path)
        .with_context(|| format!("Failed to read {}", cgroup_path))?;

    // Parse cgroup v2 format
    let cgroup_line = contents
        .lines()
        .find(|line| line.starts_with("0::"))
        .ok_or_else(|| anyhow::anyhow!("No cgroup v2 entry found for PID {}", pid))?;

    let cgroup_rel_path = cgroup_line
        .strip_prefix("0::")
        .ok_or_else(|| anyhow::anyhow!("Invalid cgroup v2 format"))?;

    let cgroup_full_path = if cgroup_rel_path.is_empty() {
        PathBuf::from("/sys/fs/cgroup")
    } else {
        PathBuf::from("/sys/fs/cgroup").join(cgroup_rel_path.trim_start_matches('/'))
    };

    Ok(cgroup_full_path)
}

/// Check if the system supports eBPF and cgroup v2
#[cfg(feature = "throttle-ebpf")]
pub fn check_ebpf_support() -> bool {
    // Check if cgroup v2 is mounted
    if !Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        log::debug!("cgroup v2 not mounted");
        return false;
    }

    // Check kernel version (need 4.10+ for cgroup SKB programs)
    if let Ok(contents) = fs::read_to_string("/proc/version") {
        if let Some(version_str) = contents.split_whitespace().nth(2) {
            if let Some(major_minor) = version_str.split('.').take(2).collect::<Vec<_>>().get(0..2)
            {
                if let (Ok(major), Ok(minor)) =
                    (major_minor[0].parse::<u32>(), major_minor[1].parse::<u32>())
                {
                    if major < 4 || (major == 4 && minor < 10) {
                        log::debug!(
                            "Kernel version {}.{} is too old for eBPF cgroup SKB (need 4.10+)",
                            major,
                            minor
                        );
                        return false;
                    }
                }
            }
        }
    }

    true
}

/// Load eBPF program from embedded bytes
#[cfg(feature = "throttle-ebpf")]
pub fn load_ebpf_program(program_bytes: &[u8]) -> Result<Ebpf> {
    Ebpf::load(program_bytes).context("Failed to load eBPF program")
}

/// Attach a cgroup SKB program
#[cfg(feature = "throttle-ebpf")]
pub fn attach_cgroup_skb(
    ebpf: &mut Ebpf,
    program_name: &str,
    cgroup_path: &Path,
    attach_type: CgroupSkbAttachType,
) -> Result<()> {
    use aya::programs::CgroupAttachMode;

    let program: &mut CgroupSkb = ebpf
        .program_mut(program_name)
        .ok_or_else(|| anyhow::anyhow!("Program {} not found", program_name))?
        .try_into()
        .context("Program is not a CgroupSkb program")?;

    program.load().context("Failed to load program")?;

    let cgroup_file = fs::File::open(cgroup_path)
        .with_context(|| format!("Failed to open cgroup {:?}", cgroup_path))?;

    program
        .attach(&cgroup_file, attach_type, CgroupAttachMode::Single)
        .context("Failed to attach program to cgroup")?;

    Ok(())
}

/// Get a BPF map by name
#[cfg(feature = "throttle-ebpf")]
pub fn get_bpf_map<'a, K, V>(
    ebpf: &'a mut Ebpf,
    map_name: &str,
) -> Result<BpfHashMap<&'a mut aya::maps::MapData, K, V>>
where
    K: aya::Pod,
    V: aya::Pod,
{
    let map = ebpf
        .map_mut(map_name)
        .ok_or_else(|| anyhow::anyhow!("Map {} not found", map_name))?;

    Ok(BpfHashMap::try_from(map)?)
}
