#![no_std]
#![no_main]

use aya_ebpf::{
    bindings::BPF_F_NO_PREALLOC,
    helpers::{bpf_get_current_cgroup_id, bpf_ktime_get_ns},
    macros::{cgroup_skb, map},
    maps::HashMap,
    programs::SkBuffContext,
};
use chadthrottle_common::{CgroupThrottleConfig, ThrottleStats, TokenBucket};

/// Maximum number of throttled cgroups (configurable)
const MAX_CGROUPS: u32 = 4096;

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
    // Calculate time elapsed since last update in nanoseconds
    let elapsed_ns = now_ns.saturating_sub(bucket.last_update_ns);

    // Convert elapsed time to seconds (with nanosecond precision)
    // tokens_to_add = (elapsed_ns * rate_bps) / 1_000_000_000
    // To avoid overflow, we calculate: (elapsed_ns / 1000) * rate_bps / 1_000_000
    let elapsed_us = elapsed_ns / 1000;
    let tokens_to_add = (elapsed_us * bucket.rate_bps) / 1_000_000;

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
#[cgroup_skb]
pub fn chadthrottle_egress(ctx: SkBuffContext) -> i32 {
    match try_chadthrottle_egress(ctx) {
        Ok(ret) => ret,
        Err(_) => 1, // Allow on error
    }
}

fn try_chadthrottle_egress(ctx: SkBuffContext) -> Result<i32, i64> {
    // Get current cgroup ID
    let cgroup_id = unsafe { bpf_get_current_cgroup_id() };

    // Check if this cgroup is being throttled
    let config = match unsafe { CGROUP_CONFIGS.get(&cgroup_id) } {
        Some(cfg) => cfg,
        None => return Ok(1), // Not throttled, allow
    };

    // Get packet size
    let packet_size = ctx.len() as u64;

    // Get or create token bucket
    let mut bucket = match unsafe { CGROUP_BUCKETS.get(&cgroup_id) } {
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
        CGROUP_BUCKETS.insert(&cgroup_id, &bucket, 0)?;
    }

    // Update statistics
    let mut stats = match unsafe { CGROUP_STATS.get(&cgroup_id) } {
        Some(s) => *s,
        None => ThrottleStats::new(),
    };

    stats.packets_total = stats.packets_total.saturating_add(1);
    stats.bytes_total = stats.bytes_total.saturating_add(packet_size);

    if !allow {
        stats.packets_dropped = stats.packets_dropped.saturating_add(1);
        stats.bytes_dropped = stats.bytes_dropped.saturating_add(packet_size);
    }

    unsafe {
        CGROUP_STATS.insert(&cgroup_id, &stats, 0)?;
    }

    // Return verdict: 1 = allow, 0 = drop
    Ok(if allow { 1 } else { 0 })
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
