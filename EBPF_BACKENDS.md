# eBPF Backend Implementation Plan

## Overview

ChadThrottle will support high-performance eBPF-based throttling backends that provide superior performance compared to traditional TC (Traffic Control) methods.

## Stub Files Created

The following stub files are ready for future implementation:

- `src/backends/throttle/upload/linux/ebpf_cgroup.rs` - Upload throttling via eBPF
- `src/backends/throttle/download/linux/ebpf_cgroup.rs` - Download throttling via eBPF (no IFB needed!)

## Benefits of eBPF Backends

### Performance
- **Lower Overhead**: eBPF programs run in kernel space with minimal overhead
- **No qdisc Manipulation**: Direct cgroup attachment, no TC qdisc setup needed
- **Better Scalability**: Handles high-throughput scenarios more efficiently

### Simplicity
- **No IFB Module**: Download throttling works without the IFB kernel module!
- **Native Integration**: Direct cgroup integration, no intermediate devices
- **Cleaner Setup**: Fewer moving parts, less can go wrong

### Priority
- **Priority Level**: `Best` (4) - Highest priority backend
- **Auto-Selected**: Will be automatically chosen when available

## Implementation Requirements

### 1. Dependencies

Add to `Cargo.toml`:

```toml
[dependencies]
# eBPF support
aya = { version = "0.11", features = ["async_tokio"] }
aya-log = "0.1"

[build-dependencies]
aya-gen = "0.1"
```

### 2. Separate eBPF Program Crate

Create `chadthrottle-ebpf/` subdirectory:

```
chadthrottle-ebpf/
├── Cargo.toml
├── src/
│   ├── main.rs           # eBPF program entry point
│   ├── egress.rs         # Upload (egress) rate limiting
│   └── ingress.rs        # Download (ingress) rate limiting
```

### 3. eBPF Program Logic

The eBPF programs will implement:

**Token Bucket Algorithm:**
- Per-cgroup token bucket for rate limiting
- Refill tokens at configured rate
- Drop/delay packets when bucket empty

**BPF Maps:**
- `rate_limits`: Per-cgroup rate limit configuration
- `token_buckets`: Per-cgroup token bucket state
- `stats`: Per-cgroup statistics (packets, bytes, drops)

### 4. Kernel Requirements

**Minimum Kernel Version:**
- Linux 4.10+ for `BPF_PROG_TYPE_CGROUP_SKB`
- Linux 4.15+ for better eBPF features

**Required Features:**
- `CONFIG_BPF=y`
- `CONFIG_BPF_SYSCALL=y`
- `CONFIG_CGROUP_BPF=y`
- cgroup2 filesystem mounted

**Capabilities:**
- `CAP_SYS_ADMIN` (root or specific capability)
- `CAP_BPF` (kernel 5.8+, optional)

## Implementation Steps

### Phase 1: Upload Throttling (BPF_CGROUP_INET_EGRESS)

1. **Create eBPF Program**
   ```rust
   // chadthrottle-ebpf/src/egress.rs
   #[cgroup_skb(egress)]
   pub fn egress_rate_limit(ctx: SkbContext) -> i32 {
       // Token bucket rate limiting logic
       // Check cgroup's token bucket
       // Allow or drop packet based on available tokens
   }
   ```

2. **Implement Rust Backend**
   - Load compiled eBPF bytecode using `aya`
   - Attach program to cgroup with `BPF_CGROUP_INET_EGRESS`
   - Update BPF maps with rate limits
   - Handle cleanup on detach

3. **Update Backend Selection**
   - Add feature flag: `throttle-ebpf-cgroup`
   - Register in `detect_upload_backends()`
   - Set priority to `Best`

### Phase 2: Download Throttling (BPF_CGROUP_INET_INGRESS)

1. **Create eBPF Program**
   ```rust
   // chadthrottle-ebpf/src/ingress.rs
   #[cgroup_skb(ingress)]
   pub fn ingress_rate_limit(ctx: SkbContext) -> i32 {
       // Token bucket rate limiting for downloads
   }
   ```

2. **Implement Rust Backend**
   - Similar to upload, but for ingress
   - No IFB device needed!
   - Simpler than TC-based download throttling

3. **Update Backend Selection**
   - Register in `detect_download_backends()`
   - Will be selected over IFB+TC when available

## Expected Performance

### Comparison: TC vs eBPF

| Metric | TC HTB | eBPF Cgroup |
|--------|--------|-------------|
| **CPU Overhead** | ~5-10% | ~1-2% |
| **Latency** | +2-5ms | <1ms |
| **Setup Time** | ~100ms | ~10ms |
| **Dependencies** | tc, cgroups, (IFB for download) | Just cgroups |
| **Kernel Version** | Any | 4.10+ |

## Testing Plan

1. **Unit Tests**
   - Test token bucket logic in eBPF
   - Test map updates
   - Test edge cases (zero rate, max rate)

2. **Integration Tests**
   - Test with actual network traffic
   - Verify rate limiting accuracy
   - Test multiple processes simultaneously
   - Stress test with high bandwidth

3. **Benchmark**
   - Compare CPU usage: TC vs eBPF
   - Measure latency impact
   - Test scalability (1 vs 100 throttled processes)

## Future Enhancements

### Alternative Download Method: eBPF XDP

```rust
// Even lower overhead than cgroup attachment
// Processes packets at NIC driver level
// Priority: Better (3) - Good for high-throughput scenarios
```

**Benefits:**
- Lowest possible latency
- Processes packets before network stack
- Best for 10Gbit+ networks

**Drawbacks:**
- Can't filter by process (only by IP/port)
- Requires more complex setup
- Not all NICs support XDP

## References

- [eBPF Documentation](https://ebpf.io/)
- [Aya Book](https://aya-rs.dev/)
- [Linux BPF Documentation](https://www.kernel.org/doc/html/latest/bpf/index.html)
- [BPF Cgroup Programs](https://lwn.net/Articles/708355/)

## Status

**Current:** Stub files created, ready for implementation
**Next Steps:** 
1. Add `aya` dependency
2. Create eBPF program crate
3. Implement token bucket algorithm
4. Test and benchmark

**Estimated Effort:** 2-3 weeks for full implementation and testing
