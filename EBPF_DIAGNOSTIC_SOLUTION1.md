# eBPF Diagnostic: Solution 1 - Traffic Type Bypass

## Status: ⏳ AWAITING TEST RESULTS

## What Was Changed

**Files Modified:**

- `chadthrottle-ebpf/src/ingress.rs:35-45`
- `chadthrottle-ebpf/src/egress.rs:35-45`

**Before (All packet parsing enabled):**

```rust
#[inline(always)]
fn should_throttle_packet(ctx: &SkBuffContext, traffic_type: u8) -> bool {
    if traffic_type == TRAFFIC_TYPE_ALL {
        return true;
    }

    // Get ethertype to determine IP version
    let mut ethtype_buf = [0u8, 0u8];
    if ctx.load_bytes(12, &mut ethtype_buf).is_err() {
        return true;
    }
    let ethertype = u16::from_be_bytes(ethtype_buf);

    match ethertype {
        0x0800 => should_throttle_ipv4(ctx, traffic_type),
        0x86DD => should_throttle_ipv6(ctx, traffic_type),
        _ => true,
    }
}
```

**After (Diagnostic bypass):**

```rust
#[inline(always)]
fn should_throttle_packet(_ctx: &SkBuffContext, _traffic_type: u8) -> bool {
    // TEMPORARY DIAGNOSTIC: Always throttle regardless of traffic type
    // This bypasses packet parsing to isolate verifier issues
    // TODO: Re-enable traffic type filtering once verifier issues are resolved
    true
}
```

## What This Tests

This diagnostic bypass removes ALL of the following from the eBPF program execution:

1. **Packet parsing**: No `ctx.load_bytes()` calls
2. **Buffer allocations**: No `ethtype_buf`, `dest_ip` arrays on stack
3. **Match statements**: No complex control flow
4. **Function calls**: No calls to `should_throttle_ipv4/ipv6`, `is_ipv4/ipv6_local`
5. **IP classification**: No local vs internet detection

**What remains:**

- Token bucket algorithm (still active)
- Map operations (CGROUP_CONFIGS, CGROUP_BUCKETS, CGROUP_STATS)
- Packet size checks (`ctx.len()`)
- Basic arithmetic and comparisons

## Expected Outcomes

### Outcome A: ✅ SUCCESS - Programs Load

**Log shows:**

```
✅ Loaded chadthrottle_ingress program into kernel (maps created)
✅ Loaded chadthrottle_egress program into kernel (maps created)
```

**Conclusion:**

- The verifier rejection was caused by **packet parsing/classification code**
- Possible culprits:
  - Stack usage from buffer arrays
  - Packet bounds checking complexity
  - Match statement nesting
  - Inline function complexity

**Next Steps:**

1. Implement Solution 2: Simplified packet parsing with minimal stack
2. Try reading only first byte of IPs for basic classification
3. Consider IPv4-only filtering (drop IPv6 support)

### Outcome B: ❌ FAILURE - Still Rejected

**Log shows:**

```
❌ Failed to load chadthrottle_ingress program into kernel
```

**Conclusion:**

- Problem is NOT in packet parsing
- Must be in:
  - Token bucket arithmetic (division, overflow checks)
  - Map operations (HashMap access patterns)
  - Stats tracking logic
  - Something fundamental about cgroup_skb programs

**Next Steps:**

1. Simplify token bucket to bare minimum
2. Remove stats tracking entirely
3. Test with single map instead of three
4. Last resort: Abandon traffic type filtering entirely

## How to Test

### Prerequisites

- Running as root (or with CAP_BPF capability)
- eBPF feature enabled in build

### Test Command

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle
```

### In the TUI

1. Select any process
2. Press `d` (download) or `u` (upload)
3. Enter limit (e.g., `1M`)
4. Select traffic type (any - will be ignored due to bypass)
5. Press Enter

### Check Results

```bash
grep "Loaded chadthrottle" /tmp/chadthrottle_debug.log
```

**Success:**

```
INFO  chadthrottle::backends::throttle::download::linux::ebpf > ✅ Loaded chadthrottle_ingress program into kernel (maps created)
INFO  chadthrottle::backends::throttle::upload::linux::ebpf   > ✅ Loaded chadthrottle_egress program into kernel (maps created)
```

**Failure:**

```
WARN  chadthrottle > Failed to apply throttle: Failed to load chadthrottle_ingress program into kernel
```

## Build Information

**Binary:** `target/release/chadthrottle`
**Build Time:** 2025-11-08 15:49:23
**eBPF Bytecode Size:** 4448 bytes
**Program Size:** 2584 bytes (323 instructions)

**Removed from previous version:**

- Packet parsing logic (but still in binary, just not called)
- ~30-50 bytes of stack arrays
- ~10-20 conditional branches

**Note:** Even though the functions aren't called, the compiler may still include them in the bytecode. The key difference is the **verifier only analyzes executed code paths**.

## Reverted Functionality

With this diagnostic bypass active:

**Disabled:**

- ❌ Traffic type filtering (Internet/Local/All all behave the same)
- ❌ IP classification
- ❌ Per-packet type differentiation

**Still Working:**

- ✅ Throttling (token bucket rate limiting)
- ✅ Per-process isolation (cgroup-based)
- ✅ Upload/download separate limits
- ✅ Statistics tracking
- ✅ Burst handling

**User Impact:**

- If traffic type is set to "Internet only" → throttles ALL traffic anyway
- If traffic type is set to "Local only" → throttles ALL traffic anyway
- Effectively acts like "All" traffic type for all selections

## Code Quality Notes

This is a **DIAGNOSTIC TEMPORARY WORKAROUND** for testing only.

**DO NOT SHIP THIS TO PRODUCTION.**

Once we identify the root cause, we need to:

1. Revert this change
2. Implement a proper fix
3. Re-enable traffic type filtering
4. Test thoroughly

## Related Documentation

- `EBPF_VERIFIER_FIX.md` - First loop removal attempt
- `EBPF_VERIFIER_FIX_V2.md` - Second loop removal attempt (simplified IPv6)
- `EBPF_TRAFFIC_FILTERING_COMPLETE.md` - Original implementation
- `ARCHITECTURE.md` - System architecture

## Test Script

A test script is provided: `test_solution1.sh`

```bash
./test_solution1.sh
```

This script provides testing instructions and expected outcomes.
