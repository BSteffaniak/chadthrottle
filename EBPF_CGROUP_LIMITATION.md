# eBPF Cgroup SKB Throttling Limitation

## Critical Discovery

**`BPF_PROG_TYPE_CGROUP_SKB` programs CANNOT drop packets - they are for observation/accounting ONLY.**

## The Problem

Our current eBPF implementation uses:

- `#[cgroup_skb]` macro
- `BPF_CGROUP_INET_INGRESS` and `BPF_CGROUP_INET_EGRESS` attach points
- Returns `1` (allow) or `0` (drop)

**However:** The return value is IGNORED by the kernel. These program types are for:

- Packet accounting
- Statistics collection
- Observability
- **NOT for packet filtering/dropping**

## Evidence

1. **Kernel Documentation**: cgroup SKB programs described as "inspect or filter" but the "filter" is misleading
2. **Program Behavior**:
   - ✅ Programs load successfully
   - ✅ Attach to cgroups successfully
   - ✅ Execute on every packet
   - ❌ Return values don't affect packet flow
   - ❌ Packets always pass through regardless of return value

3. **Testing Results**:
   - Token bucket algorithm implemented correctly
   - Clock synchronization fixed
   - Programs execute and update maps
   - **But no actual throttling occurs**

## Why We Have This Issue

The Linux kernel has multiple eBPF program types:

| Program Type               | Can Drop Packets? | Attachment Point       |
| -------------------------- | ----------------- | ---------------------- |
| `BPF_PROG_TYPE_CGROUP_SKB` | ❌ NO             | Cgroup                 |
| `BPF_PROG_TYPE_SCHED_CLS`  | ✅ YES            | TC (network interface) |
| `BPF_PROG_TYPE_XDP`        | ✅ YES            | Network driver         |
| `BPF_PROG_TYPE_SK_SKB`     | ⚠️ Socket-level   | Socket                 |

## Current Workaround

**USE THE TC BACKENDS THAT ALREADY WORK:**

```bash
# Download throttling - use tc_police
sudo chadthrottle --download-backend tc_police

# Upload throttling - use tc_htb
sudo chadthrottle --upload-backend tc_htb

# Both together
sudo chadthrottle --download-backend tc_police --upload-backend tc_htb
```

These backends:

- ✅ Actually drop packets
- ✅ Enforce rate limits correctly
- ✅ Are production-ready
- ✅ Don't have the cgroup SKB limitation

## Proper Fix Options

### Option 1: TC-based eBPF (Recommended)

Migrate to `BPF_PROG_TYPE_SCHED_CLS` (TC classifier):

**Pros:**

- ✅ Can actually drop packets (`TC_ACT_SHOT`)
- ✅ Still uses eBPF
- ✅ Good performance

**Cons:**

- ❌ Attaches to network interface, not cgroup
- ❌ Harder to do per-process throttling
- ❌ Requires mapping packets back to processes (netfilter conntrack or similar)

### Option 2: Disable eBPF Backend

Mark eBPF backend as unavailable until properly implemented:

```rust
fn is_available() -> bool {
    // cgroup SKB programs cannot drop packets
    // TODO: Implement TC-based eBPF throttling
    false
}
```

### Option 3: Hybrid Approach

Use cgroup SKB for **accounting** + TC for **enforcement**:

- eBPF cgroup programs track per-process bandwidth
- TC programs enforce global/interface limits
- Combine statistics from both

## Recommendation

**For now: Use TC backends (`tc_police`/`tc_htb`)**

These are:

- Already implemented
- Already working
- Production-ready
- More reliable than eBPF cgroup approach

The eBPF cgroup backend should be marked as "experimental" or disabled until we can implement a proper TC-based eBPF solution that can actually enforce throttling.

## Files to Modify

To disable eBPF backend:

1. `src/backends/throttle/download/linux/ebpf.rs` - Set `is_available()` to return `false`
2. `src/backends/throttle/upload/linux/ebpf.rs` - Set `is_available()` to return `false`
3. `src/backends/throttle/mod.rs` - Update backend priority/availability logic

## References

- Kernel docs: BPF_PROG_TYPE_CGROUP_SKB for observation only
- Working TC implementations: tc_police.rs, tc_htb.rs
- This issue discovered after fixing: loading, alignment, clock sync

---

**Status**: eBPF cgroup throttling **fundamentally cannot work** with current approach.  
**Solution**: Use TC backends that actually work.
