#![no_std]

/// Token bucket state for rate limiting
/// This struct is shared between userspace and eBPF programs via BPF maps
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TokenBucket {
    /// Maximum tokens (equals rate limit in bytes per second)
    pub capacity: u64,
    /// Current tokens available
    pub tokens: u64,
    /// Last update timestamp in nanoseconds
    pub last_update_ns: u64,
    /// Rate limit in bytes per second
    pub rate_bps: u64,
}

// SAFETY: TokenBucket is a plain old data type with all u64 fields
#[cfg(feature = "userspace")]
unsafe impl aya::Pod for TokenBucket {}

impl TokenBucket {
    pub const fn new() -> Self {
        Self {
            capacity: 0,
            tokens: 0,
            last_update_ns: 0,
            rate_bps: 0,
        }
    }
}

/// Configuration for a cgroup throttle
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CgroupThrottleConfig {
    /// cgroup inode number for matching
    pub cgroup_id: u64,
    /// PID of the process
    pub pid: u32,
    /// Padding for alignment
    pub _padding: u32,
    /// Rate limit in bytes per second (sustained rate)
    pub rate_bps: u64,
    /// Burst size in bytes (maximum tokens, allows short bursts above rate)
    pub burst_size: u64,
}

// SAFETY: CgroupThrottleConfig is a plain old data type with all primitive fields
#[cfg(feature = "userspace")]
unsafe impl aya::Pod for CgroupThrottleConfig {}

impl CgroupThrottleConfig {
    pub const fn new() -> Self {
        Self {
            cgroup_id: 0,
            pid: 0,
            _padding: 0,
            rate_bps: 0,
            burst_size: 0,
        }
    }
}

/// Statistics for a throttled cgroup
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ThrottleStats {
    /// Total packets seen
    pub packets_total: u64,
    /// Total bytes seen
    pub bytes_total: u64,
    /// Packets dropped due to rate limiting
    pub packets_dropped: u64,
    /// Bytes dropped due to rate limiting
    pub bytes_dropped: u64,
}

// SAFETY: ThrottleStats is a plain old data type with all u64 fields
#[cfg(feature = "userspace")]
unsafe impl aya::Pod for ThrottleStats {}

impl ThrottleStats {
    pub const fn new() -> Self {
        Self {
            packets_total: 0,
            bytes_total: 0,
            packets_dropped: 0,
            bytes_dropped: 0,
        }
    }
}
