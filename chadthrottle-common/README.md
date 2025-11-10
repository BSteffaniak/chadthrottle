# chadthrottle-common

Shared data structures and constants for ChadThrottle's eBPF-based network rate limiting.

This `no_std` library defines the communication interface between userspace and eBPF programs, providing data types that can be shared via BPF maps.

## Features

- **`userspace`**: Enables `aya::Pod` trait implementations for userspace programs. Disabled by default for eBPF compatibility.

## Data Structures

### `TokenBucket`

Token bucket state for rate limiting, shared between userspace and eBPF via BPF maps.

```rust
pub struct TokenBucket {
    pub capacity: u64,       // Maximum tokens (rate limit in bytes/sec)
    pub tokens: u64,         // Current tokens available
    pub last_update_ns: u64, // Last update timestamp in nanoseconds
    pub rate_bps: u64,       // Rate limit in bytes per second
}
```

### `CgroupThrottleConfig`

Configuration for throttling a specific cgroup.

```rust
pub struct CgroupThrottleConfig {
    pub cgroup_id: u64,      // cgroup inode number for matching
    pub pid: u32,            // PID of the process
    pub traffic_type: u8,    // Traffic type to throttle (0=All, 1=Internet, 2=Local)
    pub rate_bps: u64,       // Rate limit in bytes per second
    pub burst_size: u64,     // Burst size in bytes
}
```

### `ThrottleStats`

Statistics for a throttled cgroup.

```rust
pub struct ThrottleStats {
    pub packets_total: u64,    // Total packets seen
    pub bytes_total: u64,      // Total bytes seen
    pub packets_dropped: u64,  // Packets dropped due to rate limiting
    pub bytes_dropped: u64,    // Bytes dropped due to rate limiting
    pub program_calls: u64,    // eBPF program invocation count
    pub config_misses: u64,    // Config map lookup failures
    pub cgroup_id_seen: u64,   // Cgroup ID observed by eBPF
}
```

## Traffic Type Constants

```rust
pub const TRAFFIC_TYPE_ALL: u8 = 0;
pub const TRAFFIC_TYPE_INTERNET: u8 = 1;
pub const TRAFFIC_TYPE_LOCAL: u8 = 2;
```

## Usage

### In eBPF Programs

```rust
use chadthrottle_common::{TokenBucket, CgroupThrottleConfig, TRAFFIC_TYPE_ALL};

// Use in BPF map definitions
// TokenBucket and other types are #[repr(C)] for ABI compatibility
```

### In Userspace Programs

Enable the `userspace` feature in your `Cargo.toml`:

```toml
[dependencies]
chadthrottle-common = { path = "../chadthrottle-common", features = ["userspace"] }
```

Then use the types with Aya:

```rust
use chadthrottle_common::{TokenBucket, CgroupThrottleConfig, ThrottleStats};

// Types implement aya::Pod when userspace feature is enabled
// Can be used with Aya's HashMap, Array, and other map types
```

## Design

All types are:

- `#[repr(C)]` for stable ABI across userspace/kernel boundary
- Plain old data (POD) with primitive fields only
- `Clone + Copy` for efficient use in BPF maps
- `no_std` compatible for eBPF environments
