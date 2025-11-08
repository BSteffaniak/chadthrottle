#![no_std]
#![no_main]

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}

use aya_ebpf::{
    bindings::BPF_F_NO_PREALLOC,
    helpers::bpf_ktime_get_ns,
    macros::{cgroup_skb, map},
    maps::HashMap,
    programs::SkBuffContext,
};
use chadthrottle_common::{CgroupThrottleConfig, ThrottleStats, TokenBucket};

/// Maximum number of throttled cgroups (configurable)
const MAX_CGROUPS: u32 = 4096;

/// Fixed map key for single-cgroup programs
/// Since we attach one BPF program instance per cgroup, each program
/// only needs to handle one throttle config. Using a fixed key simplifies
/// the logic and avoids issues with bpf_get_current_cgroup_id() returning
/// the wrong cgroup ID in softirq context.
const THROTTLE_KEY: u64 = 0;

/// Map: cgroup_id -> TokenBucket
/// Stores token bucket state for each throttled cgroup
#[map]
static CGROUP_BUCKETS: HashMap<u64, TokenBucket> =
    HashMap::with_max_entries(MAX_CGROUPS, BPF_F_NO_PREALLOC);

/// Map: cgroup_id -> CgroupThrottleConfig
/// Stores configuration for each throttled cgroup
#[map]
static CGROUP_CONFIGS: HashMap<u64, CgroupThrottleConfig> =
    HashMap::with_max_entries(MAX_CGROUPS, BPF_F_NO_PREALLOC);

/// Map: cgroup_id -> ThrottleStats
/// Stores statistics for each throttled cgroup
#[map]
static CGROUP_STATS: HashMap<u64, ThrottleStats> =
    HashMap::with_max_entries(MAX_CGROUPS, BPF_F_NO_PREALLOC);

/// Token bucket algorithm implementation
/// Returns true if packet should be allowed, false if dropped
#[inline(always)]
fn token_bucket_allow(bucket: &mut TokenBucket, packet_size: u64, now_ns: u64) -> bool {
    // Handle first-time initialization when last_update_ns = 0
    // This avoids clock mismatch between userspace (wall clock) and kernel (monotonic clock)
    if bucket.last_update_ns == 0 {
        bucket.last_update_ns = now_ns;
        // First packet - allow if we have enough tokens (initial burst)
        if bucket.tokens >= packet_size {
            bucket.tokens = bucket.tokens.saturating_sub(packet_size);
            return true;
        } else {
            return false;
        }
    }

    // Calculate time elapsed since last update in nanoseconds
    let elapsed_ns = now_ns.saturating_sub(bucket.last_update_ns);

    // Calculate tokens to add: (elapsed_ns * rate_bps) / 1_000_000_000
    // eBPF doesn't support 128-bit math, so we must be very careful
    // We'll do the calculation in a way that avoids large intermediate values

    // Convert elapsed time to seconds (as a small fraction)
    // elapsed_ns / 1_000_000_000 would give us seconds, but we need to keep precision
    // So we'll work with elapsed_ns / 1000 (microseconds) and rate_bps / 1000 (KB/s)

    let elapsed_us = elapsed_ns / 1000; // microseconds elapsed

    // Now calculate: (elapsed_us * rate_bps) / 1_000_000
    // But even this can overflow for large values
    // So we'll divide first if possible
    let tokens_to_add = if elapsed_us < 1_000_000 {
        // Short time period - safe to multiply first
        let product = elapsed_us.wrapping_mul(bucket.rate_bps);
        product / 1_000_000
    } else {
        // Long time period - divide elapsed_us first
        let seconds = elapsed_us / 1_000_000;
        seconds.wrapping_mul(bucket.rate_bps)
    };

    // Add new tokens, capped at capacity
    bucket.tokens = bucket.tokens.saturating_add(tokens_to_add);
    if bucket.tokens > bucket.capacity {
        bucket.tokens = bucket.capacity;
    }

    // Update last update time
    bucket.last_update_ns = now_ns;

    // Check if we have enough tokens
    if bucket.tokens >= packet_size {
        bucket.tokens = bucket.tokens.saturating_sub(packet_size);
        true
    } else {
        false
    }
}

/// eBPF program for egress (upload) traffic throttling
#[cgroup_skb(egress)]
pub fn chadthrottle_egress(ctx: SkBuffContext) -> i32 {
    match try_chadthrottle_egress(ctx) {
        Ok(ret) => ret,
        Err(_) => 1, // Allow on error
    }
}

fn try_chadthrottle_egress(ctx: SkBuffContext) -> Result<i32, i64> {
    // CRITICAL: Use fixed key instead of bpf_get_current_cgroup_id()
    //
    // Why? bpf_get_current_cgroup_id() returns the cgroup of the "current task",
    // which in cgroup_skb programs running in softirq context is the kernel thread,
    // NOT the process that will send the packet!
    //
    // Since we attach one BPF program instance per cgroup (in userspace),
    // each program only handles packets for its specific cgroup. We use a
    // fixed key and userspace inserts the config with this same key.
    const KEY: u64 = THROTTLE_KEY;

    // Get or create statistics (need to track program calls even if not throttled)
    let mut stats = match unsafe { CGROUP_STATS.get(&KEY) } {
        Some(s) => *s,
        None => ThrottleStats::new(),
    };

    // Update diagnostic fields
    stats.program_calls = stats.program_calls.saturating_add(1);

    // Check if this cgroup is being throttled
    let config = match unsafe { CGROUP_CONFIGS.get(&KEY) } {
        Some(cfg) => cfg,
        None => {
            // Not throttled - increment config miss counter and allow
            stats.config_misses = stats.config_misses.saturating_add(1);
            unsafe {
                CGROUP_STATS.insert(&KEY, &stats, 0)?;
            }
            return Ok(1); // Allow
        }
    };

    // Get packet size
    let packet_size = ctx.len() as u64;

    // Get or create token bucket
    let mut bucket = match unsafe { CGROUP_BUCKETS.get(&KEY) } {
        Some(b) => *b,
        None => {
            // Initialize new bucket
            let now_ns = unsafe { bpf_ktime_get_ns() };
            TokenBucket {
                capacity: config.burst_size,
                tokens: config.burst_size, // Start with full bucket
                last_update_ns: now_ns,
                rate_bps: config.rate_bps,
            }
        }
    };

    // Get current time
    let now_ns = unsafe { bpf_ktime_get_ns() };

    // Apply token bucket algorithm
    let allow = token_bucket_allow(&mut bucket, packet_size, now_ns);

    // Update bucket in map
    unsafe {
        CGROUP_BUCKETS.insert(&KEY, &bucket, 0)?;
    }

    // Update traffic statistics
    stats.packets_total = stats.packets_total.saturating_add(1);
    stats.bytes_total = stats.bytes_total.saturating_add(packet_size);

    if !allow {
        stats.packets_dropped = stats.packets_dropped.saturating_add(1);
        stats.bytes_dropped = stats.bytes_dropped.saturating_add(packet_size);
    }

    // Store the actual cgroup ID from config for diagnostics
    stats.cgroup_id_seen = config.cgroup_id;

    unsafe {
        CGROUP_STATS.insert(&KEY, &stats, 0)?;
    }

    // Return verdict: 1 = allow, 0 = drop
    Ok(if allow { 1 } else { 0 })
}

#[unsafe(no_mangle)]
#[unsafe(link_section = "license")]
pub static LICENSE: [u8; 4] = *b"GPL\0";
