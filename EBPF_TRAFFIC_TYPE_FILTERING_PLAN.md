# eBPF Traffic Type Filtering Implementation Plan

## Executive Summary

**Goal:** Add IP-based traffic type filtering (Internet Only / Local Only) to eBPF throttling backends.

**Impact:** Eliminates the need for backend compatibility modal when using eBPF with traffic type filtering.

**Estimated Effort:** 8-12 hours  
**Complexity:** Medium-High  
**Risk:** Medium (requires careful eBPF programming and testing)

---

## Current State Analysis

### What Works Now ✅

1. eBPF throttling for "All Traffic" type
2. Token bucket rate limiting in kernel space
3. Per-cgroup throttling with cgroup_skb programs
4. Separate upload (egress) and download (ingress) programs
5. Statistics tracking (packets/bytes total/dropped)

### What's Missing ❌

1. IP address extraction from packets in eBPF
2. IP classification logic (local vs internet) in eBPF
3. Traffic type configuration in `CgroupThrottleConfig`
4. Conditional throttling based on destination IP
5. `supports_traffic_type()` override returning `true`

### Current Architecture

**eBPF Programs:**

- `chadthrottle-ebpf/src/egress.rs` - Upload throttling (cgroup_skb/egress)
- `chadthrottle-ebpf/src/ingress.rs` - Download throttling (cgroup_skb/ingress)

**Shared Data Structures:** (`chadthrottle-common/src/lib.rs`)

- `CgroupThrottleConfig` - Configuration for each throttle
- `TokenBucket` - Rate limiting state
- `ThrottleStats` - Statistics

**Userspace Backends:**

- `backends/throttle/upload/linux/ebpf.rs` - Upload backend
- `backends/throttle/download/linux/ebpf.rs` - Download backend

**Traffic Classification:**

- `traffic_classifier.rs` - IP classification logic (userspace only)

---

## Design: IP Filtering in eBPF

### Challenge: IP Address Extraction

eBPF `cgroup_skb` programs work with `SkBuffContext` which provides limited packet access.

**Available in `SkBuffContext`:**

- `ctx.len()` - Packet length
- `ctx.load()` - Read bytes from packet at offset
- `ctx.data()` / `ctx.data_end()` - Direct packet access (with verifier checks)

**What we need:**

1. Parse Ethernet header (14 bytes)
2. Parse IP header (IPv4: 20+ bytes, IPv6: 40+ bytes)
3. Extract destination IP address
4. Classify as local vs internet
5. Apply throttling conditionally

### Approach: Packet Parsing in eBPF

**Option A: Direct Packet Access (Recommended)**

```rust
// egress.rs / ingress.rs
fn try_chadthrottle_egress(ctx: SkBuffContext) -> Result<i32, i64> {
    // ... existing code ...

    // NEW: Extract destination IP
    let dest_ip = extract_dest_ip(&ctx)?;

    // NEW: Classify traffic
    let traffic_category = classify_ip(&dest_ip);

    // NEW: Check if we should throttle this traffic type
    if !should_throttle(&config, traffic_category) {
        return Ok(1); // Allow without throttling
    }

    // Existing token bucket logic...
}
```

**Option B: BPF Helper Functions**
Use `bpf_skb_load_bytes()` to safely read packet data.

**Recommendation:** Use Option A (direct access) for better performance, with proper verifier checks.

---

## Data Structure Changes

### 1. Add Traffic Type to `CgroupThrottleConfig`

**File:** `chadthrottle-common/src/lib.rs`

**Current:**

```rust
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CgroupThrottleConfig {
    pub cgroup_id: u64,
    pub pid: u32,
    pub _padding: u32,
    pub rate_bps: u64,
    pub burst_size: u64,
}
```

**Updated:**

```rust
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CgroupThrottleConfig {
    pub cgroup_id: u64,
    pub pid: u32,
    pub traffic_type: u8, // 0 = All, 1 = Internet, 2 = Local
    pub _padding: [u8; 3], // Align to 4 bytes
    pub rate_bps: u64,
    pub burst_size: u64,
}
```

**Rationale:**

- Use `u8` instead of enum for C compatibility
- Add 3 bytes of padding to maintain 8-byte alignment
- Total size remains 32 bytes (cache-line friendly)

### 2. Define Traffic Type Constants

**File:** `chadthrottle-common/src/lib.rs`

```rust
// Traffic type values for eBPF
pub const TRAFFIC_TYPE_ALL: u8 = 0;
pub const TRAFFIC_TYPE_INTERNET: u8 = 1;
pub const TRAFFIC_TYPE_LOCAL: u8 = 2;
```

---

## eBPF Implementation

### Phase 1: IP Address Extraction

**Files:** `chadthrottle-ebpf/src/egress.rs`, `chadthrottle-ebpf/src/ingress.rs`

**New functions:**

```rust
/// IP address representation (IPv4 or IPv6)
#[derive(Clone, Copy)]
enum IpAddr {
    V4([u8; 4]),
    V6([u8; 16]),
}

/// Extract destination IP from packet
/// Returns None if packet is not IP or parsing fails
#[inline(always)]
fn extract_dest_ip(ctx: &SkBuffContext) -> Option<IpAddr> {
    // Get direct packet access
    let data = ctx.data();
    let data_end = ctx.data_end();

    // Check minimum Ethernet header size
    if unsafe { data.add(14) } > data_end {
        return None;
    }

    // Read EtherType (bytes 12-13)
    let ethertype = unsafe {
        u16::from_be_bytes([
            *(data.add(12)),
            *(data.add(13)),
        ])
    };

    match ethertype {
        0x0800 => extract_ipv4_dest(ctx, 14), // IPv4
        0x86DD => extract_ipv6_dest(ctx, 14), // IPv6
        _ => None, // Not an IP packet
    }
}

#[inline(always)]
fn extract_ipv4_dest(ctx: &SkBuffContext, offset: usize) -> Option<IpAddr> {
    let data = ctx.data();
    let data_end = ctx.data_end();

    // Check IPv4 header minimum size (20 bytes)
    if unsafe { data.add(offset + 20) } > data_end {
        return None;
    }

    // Destination IP is at bytes 16-19 of IPv4 header
    let dest_ip = unsafe {
        [
            *(data.add(offset + 16)),
            *(data.add(offset + 17)),
            *(data.add(offset + 18)),
            *(data.add(offset + 19)),
        ]
    };

    Some(IpAddr::V4(dest_ip))
}

#[inline(always)]
fn extract_ipv6_dest(ctx: &SkBuffContext, offset: usize) -> Option<IpAddr> {
    let data = ctx.data();
    let data_end = ctx.data_end();

    // Check IPv6 header size (40 bytes minimum)
    if unsafe { data.add(offset + 40) } > data_end {
        return None;
    }

    // Destination IP is at bytes 24-39 of IPv6 header
    let mut dest_ip = [0u8; 16];
    for i in 0..16 {
        dest_ip[i] = unsafe { *(data.add(offset + 24 + i)) };
    }

    Some(IpAddr::V6(dest_ip))
}
```

### Phase 2: IP Classification Logic

**Files:** `chadthrottle-ebpf/src/egress.rs`, `chadthrottle-ebpf/src/ingress.rs`

**Port logic from `traffic_classifier.rs` to eBPF:**

```rust
/// Traffic category
#[derive(PartialEq)]
enum TrafficCategory {
    Internet = 0,
    Local = 1,
}

/// Classify IPv4 address as local or internet
#[inline(always)]
fn classify_ipv4(ip: &[u8; 4]) -> TrafficCategory {
    // Private ranges (RFC 1918)
    // 10.0.0.0/8
    if ip[0] == 10 {
        return TrafficCategory::Local;
    }

    // 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
    if ip[0] == 172 && (ip[1] >= 16 && ip[1] <= 31) {
        return TrafficCategory::Local;
    }

    // 192.168.0.0/16
    if ip[0] == 192 && ip[1] == 168 {
        return TrafficCategory::Local;
    }

    // Loopback: 127.0.0.0/8
    if ip[0] == 127 {
        return TrafficCategory::Local;
    }

    // Link-local: 169.254.0.0/16
    if ip[0] == 169 && ip[1] == 254 {
        return TrafficCategory::Local;
    }

    // Broadcast: 255.255.255.255
    if ip[0] == 255 && ip[1] == 255 && ip[2] == 255 && ip[3] == 255 {
        return TrafficCategory::Local;
    }

    // Unspecified: 0.0.0.0
    if ip[0] == 0 && ip[1] == 0 && ip[2] == 0 && ip[3] == 0 {
        return TrafficCategory::Local;
    }

    // Everything else is internet
    TrafficCategory::Internet
}

/// Classify IPv6 address as local or internet
#[inline(always)]
fn classify_ipv6(ip: &[u8; 16]) -> TrafficCategory {
    // Loopback: ::1
    if ip[0..15].iter().all(|&b| b == 0) && ip[15] == 1 {
        return TrafficCategory::Local;
    }

    // Unspecified: ::
    if ip.iter().all(|&b| b == 0) {
        return TrafficCategory::Local;
    }

    // Link-local: fe80::/10
    if ip[0] == 0xfe && (ip[1] & 0xc0) == 0x80 {
        return TrafficCategory::Local;
    }

    // Unique local: fc00::/7
    if (ip[0] & 0xfe) == 0xfc {
        return TrafficCategory::Local;
    }

    // Everything else is internet
    TrafficCategory::Internet
}

/// Classify IP address
#[inline(always)]
fn classify_ip(ip: &IpAddr) -> TrafficCategory {
    match ip {
        IpAddr::V4(ipv4) => classify_ipv4(ipv4),
        IpAddr::V6(ipv6) => classify_ipv6(ipv6),
    }
}
```

### Phase 3: Conditional Throttling

**Files:** `chadthrottle-ebpf/src/egress.rs`, `chadthrottle-ebpf/src/ingress.rs`

**Update main throttling logic:**

```rust
fn try_chadthrottle_egress(ctx: SkBuffContext) -> Result<i32, i64> {
    const KEY: u64 = THROTTLE_KEY;

    let mut stats = match unsafe { CGROUP_STATS.get(&KEY) } {
        Some(s) => *s,
        None => ThrottleStats::new(),
    };

    stats.program_calls = stats.program_calls.saturating_add(1);

    let config = match unsafe { CGROUP_CONFIGS.get(&KEY) } {
        Some(cfg) => cfg,
        None => {
            stats.config_misses = stats.config_misses.saturating_add(1);
            unsafe { CGROUP_STATS.insert(&KEY, &stats, 0)?; }
            return Ok(1); // Allow
        }
    };

    // NEW: Check if we need to filter by traffic type
    if config.traffic_type != TRAFFIC_TYPE_ALL {
        // Extract destination IP
        let dest_ip = match extract_dest_ip(&ctx) {
            Some(ip) => ip,
            None => {
                // Can't parse IP - allow packet (fail open)
                return Ok(1);
            }
        };

        // Classify traffic
        let category = classify_ip(&dest_ip);

        // Check if we should throttle this packet
        let should_throttle = match config.traffic_type {
            TRAFFIC_TYPE_INTERNET => category == TrafficCategory::Internet,
            TRAFFIC_TYPE_LOCAL => category == TrafficCategory::Local,
            _ => true, // TRAFFIC_TYPE_ALL or unknown - throttle everything
        };

        if !should_throttle {
            // This traffic type should not be throttled - allow
            return Ok(1);
        }
    }

    // Original throttling logic continues...
    let packet_size = ctx.len() as u64;
    // ... token bucket algorithm ...
}
```

---

## Userspace Backend Changes

### Update Upload Backend

**File:** `chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`

**Changes:**

1. **Pass traffic type to eBPF config:**

```rust
fn throttle_upload(
    &mut self,
    pid: i32,
    process_name: String,
    limit_bytes_per_sec: u64,
    traffic_type: crate::process::TrafficType, // NEW parameter
) -> Result<()> {
    // ... existing cgroup setup ...

    // Convert TrafficType to u8 for eBPF
    let traffic_type_value = match traffic_type {
        crate::process::TrafficType::All => TRAFFIC_TYPE_ALL,
        crate::process::TrafficType::Internet => TRAFFIC_TYPE_INTERNET,
        crate::process::TrafficType::Local => TRAFFIC_TYPE_LOCAL,
    };

    let config = CgroupThrottleConfig {
        cgroup_id,
        pid: pid as u32,
        traffic_type: traffic_type_value, // NEW field
        _padding: [0; 3],
        rate_bps: limit_bytes_per_sec,
        burst_size: limit_bytes_per_sec * 2,
    };

    // ... rest of existing code ...
}
```

2. **Override `supports_traffic_type()`:**

```rust
impl UploadThrottleBackend for EbpfUpload {
    // ... existing methods ...

    fn supports_traffic_type(&self, _traffic_type: crate::process::TrafficType) -> bool {
        true // eBPF supports all traffic types with IP filtering
    }
}
```

### Update Download Backend

**File:** `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`

**Apply same changes as upload backend.**

---

## Implementation Phases

### Phase 1: Data Structure Updates (1-2 hours)

**Files:**

- `chadthrottle-common/src/lib.rs`

**Tasks:**

1. Add `traffic_type: u8` to `CgroupThrottleConfig`
2. Update padding to `_padding: [u8; 3]`
3. Add traffic type constants
4. Update `new()` constructor

**Validation:**

- Compile both userspace and eBPF code
- Verify struct size remains 32 bytes
- Check alignment with `#[repr(C)]`

### Phase 2: IP Extraction in eBPF (2-3 hours)

**Files:**

- `chadthrottle-ebpf/src/egress.rs`
- `chadthrottle-ebpf/src/ingress.rs`

**Tasks:**

1. Implement `IpAddr` enum
2. Implement `extract_dest_ip()`
3. Implement `extract_ipv4_dest()`
4. Implement `extract_ipv6_dest()`

**Testing:**

- Test with IPv4 packets
- Test with IPv6 packets
- Test with non-IP packets (should return None)
- Verify eBPF verifier accepts the code

### Phase 3: IP Classification in eBPF (1-2 hours)

**Files:**

- `chadthrottle-ebpf/src/egress.rs`
- `chadthrottle-ebpf/src/ingress.rs`

**Tasks:**

1. Implement `TrafficCategory` enum
2. Implement `classify_ipv4()`
3. Implement `classify_ipv6()`
4. Implement `classify_ip()`

**Testing:**

- Unit tests for each IP range
- Compare results with userspace `traffic_classifier.rs`
- Test edge cases (0.0.0.0, 255.255.255.255, ::1, etc.)

### Phase 4: Conditional Throttling Logic (1-2 hours)

**Files:**

- `chadthrottle-ebpf/src/egress.rs`
- `chadthrottle-ebpf/src/ingress.rs`

**Tasks:**

1. Add IP extraction before throttling check
2. Add classification logic
3. Add conditional throttling based on config
4. Handle parsing failures (fail open)

**Testing:**

- Test "All Traffic" still works (no regression)
- Test "Internet Only" filters local traffic
- Test "Local Only" filters internet traffic
- Test packet parsing failures don't break throttling

### Phase 5: Userspace Backend Updates (1-2 hours)

**Files:**

- `chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`
- `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`

**Tasks:**

1. Pass `traffic_type` parameter to config
2. Convert `TrafficType` enum to u8
3. Override `supports_traffic_type()` to return `true`

**Testing:**

- Test UI shows eBPF as compatible with all traffic types
- Test modal doesn't appear for eBPF backend
- Test traffic type selection works end-to-end

### Phase 6: Integration Testing (2-3 hours)

**Scenarios:**

1. Throttle upload with "Internet Only" - verify local traffic unaffected
2. Throttle download with "Local Only" - verify internet traffic unaffected
3. Switch between traffic types dynamically
4. Test with real applications (browser, SSH, file transfers)
5. Performance testing (ensure no significant overhead)

---

## Files to Modify

### eBPF Programs (BPF/kernel space)

1. `chadthrottle-ebpf/src/egress.rs` - Add IP filtering to upload
2. `chadthrottle-ebpf/src/ingress.rs` - Add IP filtering to download

### Shared Data Structures

3. `chadthrottle-common/src/lib.rs` - Update `CgroupThrottleConfig`

### Userspace Backends

4. `chadthrottle/src/backends/throttle/upload/linux/ebpf.rs` - Upload backend
5. `chadthrottle/src/backends/throttle/download/linux/ebpf.rs` - Download backend

### Total: 5 files

---

## Potential Challenges

### 1. eBPF Verifier Strictness

**Challenge:** eBPF verifier may reject packet parsing code if bounds checks are insufficient.

**Solution:**

- Use explicit bounds checks before every memory access
- Use `data_end` pointer comparison consistently
- Keep parsing logic simple and linear
- Test incremental changes with verifier

### 2. Packet Parsing Edge Cases

**Challenge:** Non-IP packets, fragmented packets, tunneled traffic.

**Solution:**

- Fail open (allow) on parsing errors
- Only parse standard Ethernet + IP headers
- Document limitations (tunnels/VPNs may not be classified correctly)
- Add statistics for parsing failures

### 3. Performance Impact

**Challenge:** IP parsing adds overhead to every packet.

**Solution:**

- Use `#[inline(always)]` for hot path functions
- Keep classification logic branchless where possible
- Benchmark before/after (expect <5% overhead)
- Consider performance counters in stats

### 4. IPv6 Complexity

**Challenge:** IPv6 headers can have extension headers, making parsing complex.

**Solution:**

- Only parse base IPv6 header (no extension headers)
- Extension headers are rare in practice
- Document limitation
- Can enhance later if needed

---

## Testing Strategy

### Unit Tests

- Test IP extraction with sample packets
- Test classification logic with known IPs
- Test traffic type filtering logic

### Integration Tests

1. **All Traffic (existing behavior)**
   - Should work exactly as before
   - No regression

2. **Internet Only**
   - Browse website (should throttle)
   - Ping local server (should NOT throttle)
   - SSH to local machine (should NOT throttle)

3. **Local Only**
   - Browse website (should NOT throttle)
   - File transfer to local NAS (should throttle)
   - Local dev server access (should throttle)

### Performance Tests

- Benchmark packet processing time
- Measure CPU overhead
- Test high-throughput scenarios
- Compare with nftables backend

---

## Success Criteria

✅ eBPF backends support all three traffic types  
✅ `supports_traffic_type()` returns `true` for eBPF  
✅ Backend compatibility modal never shows for eBPF  
✅ Traffic filtering works correctly for IPv4 and IPv6  
✅ Performance overhead <5%  
✅ No crashes or verifier rejections  
✅ Existing "All Traffic" functionality unchanged

---

## Future Enhancements

1. **Statistics Enhancement**
   - Track internet vs local traffic separately
   - Add counters for classification decisions
   - Expose via `get_stats()`

2. **Advanced Filtering**
   - Port-based filtering
   - Protocol-based filtering (TCP/UDP/ICMP)
   - Custom IP range lists

3. **Optimization**
   - Cache classification results for frequently seen IPs
   - Use BPF maps for IP range lookups (if many ranges)

4. **Documentation**
   - Add architecture diagram showing packet flow
   - Document limitations (tunnels, VPNs)
   - Performance characteristics

---

## Risk Mitigation

| Risk                        | Likelihood | Impact | Mitigation                               |
| --------------------------- | ---------- | ------ | ---------------------------------------- |
| Verifier rejects code       | Medium     | High   | Incremental testing, simple logic        |
| Performance degradation     | Low        | Medium | Benchmarking, optimization               |
| Parsing bugs                | Medium     | High   | Extensive testing, fail-open strategy    |
| IPv6 edge cases             | Medium     | Low    | Document limitations, basic IPv6 support |
| Regression in existing code | Low        | High   | Comprehensive testing, feature flags     |

---

## Timeline Estimate

**Total: 8-12 hours**

- Phase 1: Data structures (1-2 hours)
- Phase 2: IP extraction (2-3 hours)
- Phase 3: Classification (1-2 hours)
- Phase 4: Throttling logic (1-2 hours)
- Phase 5: Userspace updates (1-2 hours)
- Phase 6: Testing (2-3 hours)

**Can be split across multiple sessions:**

- Session 1 (4 hours): Phases 1-3
- Session 2 (4 hours): Phases 4-6

---

## Conclusion

This implementation is **feasible and well-scoped**. The eBPF infrastructure is already in place, and we're adding a focused feature (IP-based filtering) using established packet parsing techniques.

**Key Benefits:**

- Eliminates modal for eBPF users (best UX)
- Consistent with nftables implementation
- Maintains eBPF performance advantages
- Future-proof design (can add more filters later)

**Next Step:** Implement Phase 1 (data structures) as a starting point.
