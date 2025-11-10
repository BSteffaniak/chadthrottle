# chadthrottle-common

Shared data structures for communication between eBPF and userspace programs in the ChadThrottle network rate limiter.

## Description

This crate provides `#![no_std]` compatible data structures that are shared between eBPF programs and userspace control programs. These structures are designed to be safely transmitted via BPF maps and maintain consistent memory layouts across both environments.

## Features

- **`userspace`** - Enables `aya::Pod` trait implementations for userspace BPF map interactions. Disabled by default.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
chadthrottle-common = { path = "../chadthrottle-common" }

# For userspace programs that interact with BPF maps:
chadthrottle-common = { path = "../chadthrottle-common", features = ["userspace"] }
```

## Usage

### Data Structures

#### `TokenBucket`

Token bucket state for rate limiting, shared between userspace and eBPF via BPF maps.

```rust
pub struct TokenBucket {
    pub capacity: u64,        // Maximum tokens (bytes per second)
    pub tokens: u64,          // Current tokens available
    pub last_update_ns: u64,  // Last update timestamp in nanoseconds
    pub rate_bps: u64,        // Rate limit in bytes per second
}
```

#### `CgroupThrottleConfig`

Configuration for throttling a specific cgroup.

```rust
pub struct CgroupThrottleConfig {
    pub cgroup_id: u64,     // cgroup inode number for matching
    pub pid: u32,           // PID of the process
    pub traffic_type: u8,   // Traffic type to throttle (see constants below)
    pub rate_bps: u64,      // Rate limit in bytes per second
    pub burst_size: u64,    // Maximum burst size in bytes
}
```

**Traffic type constants:**

- `TRAFFIC_TYPE_ALL` (0) - Throttle all traffic
- `TRAFFIC_TYPE_INTERNET` (1) - Throttle internet traffic only
- `TRAFFIC_TYPE_LOCAL` (2) - Throttle local traffic only

#### `ThrottleStats`

Statistics for a throttled cgroup.

```rust
pub struct ThrottleStats {
    pub packets_total: u64,   // Total packets seen
    pub bytes_total: u64,     // Total bytes seen
    pub packets_dropped: u64, // Packets dropped due to rate limiting
    pub bytes_dropped: u64,   // Bytes dropped due to rate limiting
    pub program_calls: u64,   // Number of eBPF program invocations
    pub config_misses: u64,   // Cgroup config lookup failures
    pub cgroup_id_seen: u64,  // Actual cgroup ID observed by eBPF
}
```

### Example

```rust
use chadthrottle_common::{CgroupThrottleConfig, TRAFFIC_TYPE_INTERNET};

let config = CgroupThrottleConfig {
    cgroup_id: 12345,
    pid: 1000,
    traffic_type: TRAFFIC_TYPE_INTERNET,
    _padding: [0, 0, 0],
    rate_bps: 1_000_000,  // 1 MB/s
    burst_size: 5_000_000, // 5 MB burst
};
```

## License

This project is part of ChadThrottle. See the root repository for license information.
