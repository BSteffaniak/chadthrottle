// eBPF utility functions for loading and managing eBPF programs
#[cfg(feature = "throttle-ebpf")]
use anyhow::{Context, Result};
#[cfg(feature = "throttle-ebpf")]
use std::fs;
#[cfg(feature = "throttle-ebpf")]
use std::os::unix::io::AsRawFd;
#[cfg(feature = "throttle-ebpf")]
use std::path::{Path, PathBuf};
#[cfg(feature = "throttle-ebpf")]
use std::sync::OnceLock;
#[cfg(feature = "throttle-ebpf")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "throttle-ebpf")]
use aya::{
    Ebpf,
    maps::HashMap as BpfHashMap,
    programs::{CgroupAttachMode, CgroupSkb, CgroupSkbAttachType},
};
#[cfg(feature = "throttle-ebpf")]
use chadthrottle_common::{CgroupThrottleConfig, ThrottleStats, TokenBucket};

/// Global BPF configuration
#[cfg(feature = "throttle-ebpf")]
static BPF_CONFIG: OnceLock<BpfConfig> = OnceLock::new();

#[cfg(feature = "throttle-ebpf")]
#[derive(Debug, Clone)]
pub struct BpfConfig {
    pub attach_method: BpfAttachMethod,
}

#[cfg(feature = "throttle-ebpf")]
impl BpfConfig {
    pub fn new(attach_method: BpfAttachMethod) -> Self {
        Self { attach_method }
    }
}

/// Initialize global BPF configuration (call once at startup)
#[cfg(feature = "throttle-ebpf")]
pub fn init_bpf_config(config: BpfConfig) {
    BPF_CONFIG.get_or_init(|| config);
}

/// Get global BPF configuration
#[cfg(feature = "throttle-ebpf")]
pub fn get_bpf_config() -> BpfConfig {
    BPF_CONFIG.get().cloned().unwrap_or_else(|| {
        // Default config if not initialized
        BpfConfig::new(BpfAttachMethod::from_env_and_arg(None))
    })
}

/// Get the cgroup ID for a given PID
#[cfg(feature = "throttle-ebpf")]
pub fn get_cgroup_id(pid: i32) -> Result<u64> {
    // DIAGNOSTIC: If CHADTHROTTLE_TEST_ROOT_CGROUP is set, always return root cgroup ID
    if std::env::var("CHADTHROTTLE_TEST_ROOT_CGROUP").is_ok() {
        log::warn!("🧪 TEST MODE: Using root cgroup ID (1) for all processes");
        return Ok(1);
    }

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
///
/// Returns the LEAF cgroup (the process's actual cgroup).
/// We use AllowMultiple mode to avoid conflicts with other BPF programs.
#[cfg(feature = "throttle-ebpf")]
pub fn get_cgroup_path(pid: i32) -> Result<PathBuf> {
    // DIAGNOSTIC: If CHADTHROTTLE_TEST_ROOT_CGROUP is set, use root cgroup for testing
    if std::env::var("CHADTHROTTLE_TEST_ROOT_CGROUP").is_ok() {
        let root_cgroup = PathBuf::from("/sys/fs/cgroup");
        log::warn!(
            "🧪 TEST MODE: Using root cgroup {:?} instead of process cgroup",
            root_cgroup
        );
        return Ok(root_cgroup);
    }

    let cgroup_path = format!("/proc/{}/cgroup", pid);
    let contents = fs::read_to_string(&cgroup_path)
        .with_context(|| format!("Failed to read {}", cgroup_path))?;

    // Parse cgroup v2 format: "0::/user.slice/user-1000.slice/user@1000.service/tmux-spawn-XXX.scope"
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
    Ebpf::load(program_bytes)
        .inspect_err(|e| log::error!("Failed to load eBPF program: {e}"))
        .context("Failed to load eBPF program")
}

/// Attach a cgroup SKB program using legacy method (bpf_prog_attach)
#[cfg(feature = "throttle-ebpf")]
pub fn attach_cgroup_skb_legacy(
    ebpf: &mut Ebpf,
    program_name: &str,
    cgroup_path: &Path,
    attach_type: CgroupSkbAttachType,
) -> Result<()> {
    use std::os::fd::{AsFd, BorrowedFd};

    let program: &mut CgroupSkb = ebpf
        .program_mut(program_name)
        .ok_or_else(|| anyhow::anyhow!("Program {} not found", program_name))?
        .try_into()
        .context("Program is not a CgroupSkb program")?;

    // Program should already be loaded in ensure_loaded()
    // This ensures userspace and kernel use the SAME map instances
    log::debug!(
        "Program '{}' expected_attach_type: {:?}",
        program_name,
        program.expected_attach_type()
    );

    // Get program FD (should already exist from ensure_loaded)
    let prog_fd = program
        .fd()
        .context("Program not loaded - call ensure_loaded() first")?;
    let prog_fd_borrowed: BorrowedFd = prog_fd.as_fd();
    let prog_fd_raw = prog_fd_borrowed.as_raw_fd();

    log::debug!(
        "Program FD obtained: {} (using legacy bpf_prog_attach)",
        prog_fd_raw
    );

    // Open cgroup
    let cgroup_file = fs::File::open(cgroup_path)
        .with_context(|| format!("Failed to open cgroup {:?}", cgroup_path))?;
    let cgroup_fd_raw = cgroup_file.as_raw_fd();

    log::debug!(
        "Opened cgroup file: {:?} (fd: {})",
        cgroup_path,
        cgroup_fd_raw
    );

    // Convert attach type to BPF constant
    // These are stable kernel ABI constants from linux/bpf.h
    const BPF_CGROUP_INET_INGRESS: u32 = 0;
    const BPF_CGROUP_INET_EGRESS: u32 = 1;

    let bpf_attach_type = match attach_type {
        CgroupSkbAttachType::Ingress => BPF_CGROUP_INET_INGRESS,
        CgroupSkbAttachType::Egress => BPF_CGROUP_INET_EGRESS,
    };

    // Call bpf_prog_attach syscall directly
    // According to kernel headers, BPF_PROG_ATTACH uses this struct layout:
    // union bpf_attr {
    //     struct { /* anonymous struct used by BPF_MAP_* commands */
    //         __u32 map_fd;
    //         __aligned_u64 key;
    //         union {
    //             __aligned_u64 value;
    //             __aligned_u64 next_key;
    //         };
    //         __u64 flags;
    //     };
    //     struct { /* anonymous struct used by BPF_PROG_LOAD command */
    //         ...
    //     };
    //     struct { /* anonymous struct used by BPF_PROG_ATTACH/DETACH commands */
    //         __u32 target_fd;     /* container object to attach to */
    //         __u32 attach_bpf_fd; /* eBPF program to attach */
    //         __u32 attach_type;
    //         __u32 attach_flags;
    //         ...
    //     };
    // };
    // The PROG_ATTACH struct starts at offset 0 in the union.

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct bpf_attr_attach {
        target_fd: u32,
        attach_bpf_fd: u32,
        attach_type: u32,
        attach_flags: u32,
    }

    const BPF_F_ALLOW_MULTI: u32 = 1 << 1; // = 2

    let mut attr = bpf_attr_attach {
        target_fd: cgroup_fd_raw as u32,
        attach_bpf_fd: prog_fd_raw as u32,
        attach_type: bpf_attach_type,
        attach_flags: BPF_F_ALLOW_MULTI,
    };

    log::warn!("🧪 Using LEGACY bpf_prog_attach method instead of bpf_link_create");
    log::debug!(
        "Calling bpf_prog_attach with: prog_fd={}, target_fd={}, attach_type={}, flags=BPF_F_ALLOW_MULTI",
        prog_fd_raw,
        cgroup_fd_raw,
        bpf_attach_type as u32
    );

    let ret = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            8, // BPF_PROG_ATTACH
            &attr as *const _ as *const libc::c_void,
            std::mem::size_of::<bpf_attr_attach>(),
        )
    };

    if ret < 0 {
        let errno = std::io::Error::last_os_error();
        log::error!(
            "bpf_prog_attach failed: {} (errno: {:?})",
            errno,
            errno.raw_os_error()
        );
        return Err(anyhow::anyhow!("bpf_prog_attach failed: {}", errno));
    }

    log::info!("✅ Successfully attached using legacy method!");
    Ok(())
}

/// Detach a cgroup SKB program using legacy method (bpf_prog_detach)
#[cfg(feature = "throttle-ebpf")]
pub fn detach_cgroup_skb_legacy(
    cgroup_path: &Path,
    attach_type: CgroupSkbAttachType,
) -> Result<()> {
    use std::os::fd::AsRawFd;

    // Open cgroup
    let cgroup_file = fs::File::open(cgroup_path)
        .with_context(|| format!("Failed to open cgroup {:?} for detach", cgroup_path))?;
    let cgroup_fd_raw = cgroup_file.as_raw_fd();

    log::debug!(
        "Detaching from cgroup: {:?} (fd: {})",
        cgroup_path,
        cgroup_fd_raw
    );

    // Convert attach type to BPF constant
    const BPF_CGROUP_INET_INGRESS: u32 = 0;
    const BPF_CGROUP_INET_EGRESS: u32 = 1;

    let bpf_attach_type = match attach_type {
        CgroupSkbAttachType::Ingress => BPF_CGROUP_INET_INGRESS,
        CgroupSkbAttachType::Egress => BPF_CGROUP_INET_EGRESS,
    };

    // BPF_PROG_DETACH uses the same struct as BPF_PROG_ATTACH
    // but we don't need attach_bpf_fd or attach_flags for detach
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct bpf_attr_detach {
        target_fd: u32,
        attach_bpf_fd: u32, // Not used for detach, but required for struct layout
        attach_type: u32,
        attach_flags: u32, // Not used for detach
    }

    let attr = bpf_attr_detach {
        target_fd: cgroup_fd_raw as u32,
        attach_bpf_fd: 0, // Not used for detach
        attach_type: bpf_attach_type,
        attach_flags: 0, // Not used for detach
    };

    log::debug!(
        "Calling bpf_prog_detach with: target_fd={}, attach_type={}",
        cgroup_fd_raw,
        bpf_attach_type
    );

    let ret = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            9, // BPF_PROG_DETACH
            &attr as *const _ as *const libc::c_void,
            std::mem::size_of::<bpf_attr_detach>(),
        )
    };

    if ret < 0 {
        let errno = std::io::Error::last_os_error();
        log::warn!(
            "bpf_prog_detach failed: {} (errno: {:?}) - program may have already been detached",
            errno,
            errno.raw_os_error()
        );
        // Don't return error - program might already be detached, which is fine
        return Ok(());
    }

    log::debug!("✅ Successfully detached using legacy method");
    Ok(())
}

/// BPF attach method selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpfAttachMethod {
    /// Try bpf_link_create first, fallback to bpf_prog_attach on EINVAL
    Auto,
    /// Use modern bpf_link_create only
    Link,
    /// Use legacy bpf_prog_attach only
    Legacy,
}

impl BpfAttachMethod {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "link" => Self::Link,
            "legacy" => Self::Legacy,
            _ => Self::Auto, // default to auto
        }
    }

    pub fn from_env_and_arg(arg: Option<&str>) -> Self {
        // CLI arg takes precedence
        if let Some(method) = arg {
            return Self::from_str(method);
        }

        // Check environment variable (legacy support)
        if std::env::var("CHADTHROTTLE_USE_LEGACY_ATTACH").is_ok() {
            return Self::Legacy;
        }

        // Check new environment variable
        if let Ok(method) = std::env::var("CHADTHROTTLE_BPF_ATTACH_METHOD") {
            return Self::from_str(&method);
        }

        // Default to auto
        Self::Auto
    }
}

/// Attach a cgroup SKB program using configured method
#[cfg(feature = "throttle-ebpf")]
pub fn attach_cgroup_skb(
    ebpf: &mut Ebpf,
    program_name: &str,
    cgroup_path: &Path,
    attach_type: CgroupSkbAttachType,
) -> Result<()> {
    // Get attach method from global config
    let config = get_bpf_config();
    attach_cgroup_skb_with_method(
        ebpf,
        program_name,
        cgroup_path,
        attach_type,
        config.attach_method,
    )
}

/// Attach a cgroup SKB program with specified method
#[cfg(feature = "throttle-ebpf")]
pub fn attach_cgroup_skb_with_method(
    ebpf: &mut Ebpf,
    program_name: &str,
    cgroup_path: &Path,
    attach_type: CgroupSkbAttachType,
    method: BpfAttachMethod,
) -> Result<()> {
    match method {
        BpfAttachMethod::Legacy => {
            log::info!("Using legacy BPF attach method (bpf_prog_attach)");
            return attach_cgroup_skb_legacy(ebpf, program_name, cgroup_path, attach_type);
        }
        BpfAttachMethod::Link => {
            log::debug!("Using modern BPF attach method (bpf_link_create)");
            return attach_cgroup_skb_link(ebpf, program_name, cgroup_path, attach_type);
        }
        BpfAttachMethod::Auto => {
            log::debug!("Auto-detecting best BPF attach method");
            // Try modern method first
            match attach_cgroup_skb_link(ebpf, program_name, cgroup_path, attach_type) {
                Ok(_) => {
                    log::info!("✅ Successfully attached using modern method (bpf_link_create)");
                    return Ok(());
                }
                Err(e) => {
                    // Check if it's EINVAL (errno 22) by walking the error chain
                    let is_einval = {
                        let mut found = false;

                        // Walk the anyhow error chain
                        for cause in e.chain() {
                            // Check if this error is an io::Error with errno 22
                            if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
                                if io_err.raw_os_error() == Some(22) {
                                    log::debug!("Found io::Error with errno 22 in error chain");
                                    found = true;
                                    break;
                                }
                            }

                            // Also check string representation as fallback
                            let error_str = cause.to_string();
                            if error_str.contains("errno=22")
                                || error_str.contains("Invalid argument")
                                || error_str.contains("os error 22")
                            {
                                log::debug!("Found EINVAL in error string: {}", error_str);
                                found = true;
                                break;
                            }
                        }

                        found
                    };

                    if is_einval {
                        log::warn!(
                            "Modern attach failed with EINVAL, falling back to legacy method..."
                        );
                        return attach_cgroup_skb_legacy(
                            ebpf,
                            program_name,
                            cgroup_path,
                            attach_type,
                        );
                    } else {
                        // Other error, don't retry
                        log::error!("Modern attach failed with non-EINVAL error, not retrying");
                        return Err(e);
                    }
                }
            }
        }
    }
}

/// Attach using modern bpf_link_create method
#[cfg(feature = "throttle-ebpf")]
fn attach_cgroup_skb_link(
    ebpf: &mut Ebpf,
    program_name: &str,
    cgroup_path: &Path,
    attach_type: CgroupSkbAttachType,
) -> Result<()> {
    let program: &mut CgroupSkb = ebpf
        .program_mut(program_name)
        .ok_or_else(|| anyhow::anyhow!("Program {} not found", program_name))?
        .try_into()
        .context("Program is not a CgroupSkb program")?;

    // Program should already be loaded in ensure_loaded()
    // This ensures userspace and kernel use the SAME map instances
    log::debug!(
        "Program '{}' expected_attach_type: {:?}",
        program_name,
        program.expected_attach_type()
    );

    // Verify program FD is valid (should already exist from ensure_loaded)
    match program.fd() {
        Ok(prog_fd) => {
            use std::os::fd::AsFd;
            let raw_fd = prog_fd.as_fd().as_raw_fd();
            log::debug!("Program FD obtained: valid (raw fd: {})", raw_fd);
        }
        Err(e) => {
            log::error!("Failed to get program FD: {}", e);
            return Err(anyhow::anyhow!(
                "Program not loaded - call ensure_loaded() first: {}",
                e
            ));
        }
    }

    // Open cgroup file (read-only is sufficient for BPF attachment)
    let cgroup_file = fs::File::open(cgroup_path)
        .with_context(|| format!("Failed to open cgroup {:?}", cgroup_path))?;

    log::debug!(
        "Opened cgroup file: {:?} (fd: {})",
        cgroup_path,
        cgroup_file.as_raw_fd()
    );

    // Use AllowMultiple mode to allow multiple BPF programs on the same cgroup
    // This is CRITICAL because systemd or other tools may have already attached programs
    // Single mode would fail silently if another program exists
    // AllowMultiple sets BPF_F_ALLOW_MULTI flag which enables program stacking
    log::debug!(
        "Attempting to attach with: attach_type={:?}, mode=AllowMultiple, cgroup_fd={}",
        attach_type,
        cgroup_file.as_raw_fd()
    );

    let attach_result = program.attach(&cgroup_file, attach_type, CgroupAttachMode::AllowMultiple);

    // Log detailed attach result before error handling
    match &attach_result {
        Ok(link_id) => {
            log::debug!("Attachment succeeded! Link ID: {:?}", link_id);
        }
        Err(e) => {
            log::error!("Attachment failed immediately: {}", e);
        }
    }

    attach_result
        .inspect_err(|e| {
            // Extract the underlying OS error if available
            let error_details = if let Some(source) = std::error::Error::source(&e) {
                format!("{} (source: {})", e, source)
            } else {
                format!("{}", e)
            };

            // Try to extract errno information
            let errno_info = if let Some(source) = std::error::Error::source(&e) {
                if let Some(io_err) = source.downcast_ref::<std::io::Error>() {
                    if let Some(errno) = io_err.raw_os_error() {
                        format!(" [errno={}]", errno)
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            log::error!(
                "Failed to attach program to cgroup: {}{}\nCgroup path: {:?}\nAttach type: {:?}\nMode: AllowMultiple",
                error_details,
                errno_info,
                cgroup_path,
                attach_type
            );
        })
        .context("Failed to attach program to cgroup")?;

    log::debug!(
        "Attached {} to {:?} with mode: AllowMultiple (BPF_F_ALLOW_MULTI - allows coexistence)",
        program_name,
        cgroup_path
    );

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

/// Get current time in nanoseconds since UNIX epoch
/// This is used to initialize the token bucket timestamp to match what the eBPF program expects
#[cfg(feature = "throttle-ebpf")]
pub fn get_current_time_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before UNIX epoch")
        .as_nanos() as u64
}
