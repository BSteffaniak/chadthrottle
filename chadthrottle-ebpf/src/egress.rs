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
use chadthrottle_common::{
    CgroupThrottleConfig, ThrottleStats, TokenBucket, TRAFFIC_TYPE_ALL, TRAFFIC_TYPE_INTERNET,
    TRAFFIC_TYPE_LOCAL,
};

/// Maximum number of throttled cgroups (configurable)
const MAX_CGROUPS: u32 = 4096;

/// Fixed map key for single-cgroup programs
/// Since we attach one BPF program instance per cgroup, each program
/// only needs to handle one throttle config. Using a fixed key simplifies
/// the logic and avoids issues with bpf_get_current_cgroup_id() returning
/// the wrong cgroup ID in softirq context.
const THROTTLE_KEY: u64 = 0;

/// Check if packet should be throttled based on traffic type filtering
/// Returns true if packet should be throttled, false if it should be allowed
///
/// NOTE: This is a simplified implementation to satisfy the BPF verifier.
/// - IPv4 with 2-byte precision + IPv6 with 1-byte detection
/// - Uses minimal stack (separate 2-byte and 1-byte buffers)
/// - IPv4: Precise RFC 1918 + link-local detection
/// - IPv6: Basic fe80::/10 and fc00::/7 detection
#[inline(always)]
fn should_throttle_packet(ctx: &SkBuffContext, traffic_type: u8) -> bool {
    // Early return for "All" traffic - most common case
    if traffic_type == TRAFFIC_TYPE_ALL {
        return true;
    }

    // Try IPv4 first (more common)
    // Check packet is large enough for IPv4 (14 byte ethernet + 20 byte IP = 34 minimum)
    if ctx.len() >= 34 {
        // Read TWO bytes of destination IP address (offset 30 in packet)
        // Offset: 14 (ethernet) + 16 (IP header to dest field) = 30
        let mut two_bytes = [0u8, 0u8];
        if ctx.load_bytes(30, &mut two_bytes).is_ok() {
            let first = two_bytes[0];
            let second = two_bytes[1];

            // Precise classification based on first TWO octets of IPv4 address:
            // 10.x.x.x         = RFC 1918 private (all of it)
            // 127.x.x.x        = loopback (all of it)
            // 169.254.x.x      = link-local (RFC 3927, ONLY this specific range!)
            // 172.16-31.x.x    = RFC 1918 private (ONLY this specific range!)
            // 192.168.x.x      = RFC 1918 private (ONLY this specific range!)
            let is_ipv4_local = first == 10
                || first == 127
                || (first == 169 && second == 254)
                || (first == 172 && second >= 16 && second <= 31)
                || (first == 192 && second == 168);

            // IPv4 path - return immediately
            return match traffic_type {
                TRAFFIC_TYPE_INTERNET => !is_ipv4_local,
                TRAFFIC_TYPE_LOCAL => is_ipv4_local,
                _ => true,
            };
        }
    }

    // Try IPv6 (destination at offset 38, minimum packet size 54)
    // Packet structure: 14 (ethernet) + 40 (IPv6 header) = 54 minimum
    if ctx.len() >= 54 {
        // Read first byte of IPv6 destination address (offset 38)
        let mut ipv6_first = [0u8];
        if ctx.load_bytes(38, &mut ipv6_first).is_ok() {
            // Simplified IPv6 local detection (first byte only):
            // fe80::/10 = link-local (fe80-febf, first byte == 0xfe)
            // fc00::/7  = unique local (fc00-fdff, (first_byte & 0xfe) == 0xfc)
            let is_ipv6_local = ipv6_first[0] == 0xfe || (ipv6_first[0] & 0xfe) == 0xfc;

            // IPv6 path - return immediately
            return match traffic_type {
                TRAFFIC_TYPE_INTERNET => !is_ipv6_local,
                TRAFFIC_TYPE_LOCAL => is_ipv6_local,
                _ => true,
            };
        }
    }

    // Couldn't parse as IPv4 or IPv6, throttle it (fail closed)
    true
}

#[inline(always)]
fn should_throttle_ipv4(ctx: &SkBuffContext, traffic_type: u8) -> bool {
    if ctx.len() < 34 {
        return true;
    }

    let mut dest_ip = [0u8, 0u8, 0u8, 0u8];
    if ctx.load_bytes(30, &mut dest_ip).is_err() {
        return true;
    }

    let is_local = is_ipv4_local(&dest_ip);

    match traffic_type {
        TRAFFIC_TYPE_INTERNET => !is_local, // Throttle if internet
        TRAFFIC_TYPE_LOCAL => is_local,     // Throttle if local
        _ => true,
    }
}

#[inline(always)]
fn should_throttle_ipv6(ctx: &SkBuffContext, traffic_type: u8) -> bool {
    if ctx.len() < 54 {
        return true;
    }

    let mut dest_ip = [
        0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8,
    ];
    if ctx.load_bytes(38, &mut dest_ip).is_err() {
        return true;
    }

    let is_local = is_ipv6_local(&dest_ip);

    match traffic_type {
        TRAFFIC_TYPE_INTERNET => !is_local,
        TRAFFIC_TYPE_LOCAL => is_local,
        _ => true,
    }
}

#[inline(always)]
fn is_ipv4_local(ip: &[u8; 4]) -> bool {
    // Private ranges (RFC 1918)
    if ip[0] == 10 || (ip[0] == 172 && ip[1] >= 16 && ip[1] <= 31) || (ip[0] == 192 && ip[1] == 168)
    {
        return true;
    }
    // Loopback, link-local, broadcast, unspecified
    if ip[0] == 127
        || (ip[0] == 169 && ip[1] == 254)
        || (ip[0] == 255 && ip[1] == 255 && ip[2] == 255 && ip[3] == 255)
        || (ip[0] == 0 && ip[1] == 0 && ip[2] == 0 && ip[3] == 0)
    {
        return true;
    }
    false
}

#[inline(always)]
fn is_ipv6_local(ip: &[u8; 16]) -> bool {
    // NOTE: We intentionally skip ::1 (loopback) and :: (unspecified) checks
    // to avoid loops that the BPF verifier rejects. These are edge cases rarely
    // seen in actual network traffic. The important local ranges are covered below.

    // Link-local fe80::/10
    if ip[0] == 0xfe && (ip[1] & 0xc0) == 0x80 {
        return true;
    }
    // Unique local fc00::/7
    if (ip[0] & 0xfe) == 0xfc {
        return true;
    }
    false
}

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

    // Check if we should throttle this packet based on traffic type
    if !should_throttle_packet(&ctx, config.traffic_type) {
        // This traffic type should not be throttled - allow
        return Ok(1);
    }

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
