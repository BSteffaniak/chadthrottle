# chadthrottle-common

Shared data structures for ChadThrottle's userspace and eBPF components.

## Description

This `no_std` library defines C-compatible types used to communicate between ChadThrottle's userspace application and its eBPF programs. All structures are designed for safe sharing via BPF maps.

## Features

- **default**: No features enabled
- **userspace**: Enables `aya::Pod` trait implementations for BPF map integration

## Public API

### TokenBucket

Token bucket state for rate limiting, shared between userspace and eBPF via BPF maps:

```rust
pub struct TokenBucket {
    pub capacity: u64,
    pub tokens: u64,
    pub last_update_ns: u64,
    pub rate_bps: u64,
}
```

### CgroupThrottleConfig

Configuration for throttling a cgroup:

```rust
pub struct CgroupThrottleConfig {
    pub cgroup_id: u64,
    pub pid: u32,
    pub traffic_type: u8,
    pub rate_bps: u64,
    pub burst_size: u64,
}
```

**Traffic type constants:**

- `TRAFFIC_TYPE_ALL` (0): Throttle all traffic
- `TRAFFIC_TYPE_INTERNET` (1): Throttle internet traffic only
- `TRAFFIC_TYPE_LOCAL` (2): Throttle local traffic only

### ThrottleStats

Statistics for a throttled cgroup:

```rust
pub struct ThrottleStats {
    pub packets_total: u64,
    pub bytes_total: u64,
    pub packets_dropped: u64,
    pub bytes_dropped: u64,
    pub program_calls: u64,
    pub config_misses: u64,
    pub cgroup_id_seen: u64,
}
```

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
chadthrottle-common = { path = "../chadthrottle-common" }
```

For userspace applications that need `aya::Pod` trait implementations:

```toml
[dependencies]
chadthrottle-common = { path = "../chadthrottle-common", features = ["userspace"] }
```

For eBPF programs (no features needed):

```toml
[dependencies]
chadthrottle-common = { path = "../chadthrottle-common", default-features = false }
```

## License

MIT
