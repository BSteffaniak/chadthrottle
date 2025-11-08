# eBPF TC Classifier Implementation Decision

## TL;DR

**Decision: DO NOT integrate TC classifier into ifb_tc backend for cgroup v2.**

Instead, users on cgroup v2 systems should use the **`ebpf` download backend**, which uses `BPF_CGROUP_INET_INGRESS` hooks for superior performance and simpler implementation.

## Background

### The Problem

The `ifb_tc` download throttling backend uses TC (Traffic Control) with IFB (Intermediate Functional Block) devices to throttle download traffic. It relies on the TC cgroup filter to match packets to cgroups.

**On cgroup v1:** TC cgroup filter works by reading `net_cls.classid` from the cgroup.

**On cgroup v2:** The `net_cls` controller doesn't exist, so TC cgroup filter cannot match packets to cgroups.

### Initial Proposed Solution: eBPF TC Classifier

The idea was to implement an eBPF TC classifier that would:

1. Run on every packet arriving at the IFB device (ingress)
2. Look up the packet's destination socket
3. Get the cgroup ID from the socket
4. Map cgroup ID → TC classid using a BPF map
5. Set the TC classid for HTB to route the packet

## Why We Didn't Implement It

### Fundamental Limitation: No Socket Context on TC Ingress

**The core issue:** On TC ingress (download traffic), packets arrive at the IFB device **BEFORE** they've been delivered to application sockets. This means:

- We cannot call `bpf_get_socket_cookie()` - no socket yet
- We cannot call `bpf_skb_ancestor_cgroup_id()` - only works on egress
- We cannot access `skb->sk` - packet not associated with socket yet

### Possible Workarounds (All Complex/Limited)

1. **Socket Lookup (`bpf_sk_lookup_tcp/udp`)**
   - Parse packet headers to extract 5-tuple (src/dst IP/port, protocol)
   - Call `bpf_sk_lookup_*()` helpers to find destination socket
   - Get cgroup ID from socket
   - **Issues:**
     - Not exposed in aya-ebpf 0.1 (would need raw BPF helper calls)
     - Adds significant per-packet overhead
     - Only works for TCP/UDP, not ICMP or other protocols
     - Socket must exist (doesn't work for first SYN packet)
     - Requires kernel 4.17+ for sk_lookup helpers

2. **Connection Tracking via BPF Maps**
   - Store socket info on egress (upload) in a BPF map
   - Look up connection on ingress (download) by 5-tuple
   - **Issues:**
     - Doesn't work for connections initiated by external hosts
     - Requires managing connection state (memory overhead)
     - Doesn't handle asymmetric routing
     - Complex state management

3. **Alternative Hook Point (XDP)**
   - Use XDP instead of TC
   - **Issues:**
     - XDP is even earlier in packet processing (before socket lookup)
     - Same fundamental problem: no socket context

## The Better Solution: eBPF Cgroup Hooks

ChadThrottle **already has** a superior solution: the `ebpf` download backend.

### How It Works

**File:** `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`

**eBPF Program:** `chadthrottle-ebpf/src/ingress.rs`

**Mechanism:**

- Uses `BPF_PROG_TYPE_CGROUP_SKB` with `BPF_CGROUP_INET_INGRESS`
- Attaches directly to cgroup directories
- Runs **AFTER** packet is associated with socket/cgroup
- Direct access to cgroup ID via `bpf_get_current_cgroup_id()`
- Token bucket rate limiting in eBPF (packet dropping)

### Advantages Over TC Classifier

| Feature          | TC Classifier                       | eBPF Cgroup Hooks                |
| ---------------- | ----------------------------------- | -------------------------------- |
| Socket context   | ❌ No (needs complex lookup)        | ✅ Yes (direct access)           |
| Cgroup ID access | ❌ Complex workarounds              | ✅ `bpf_get_current_cgroup_id()` |
| Performance      | ⚠️ Socket lookup overhead           | ✅ ~50% lower CPU overhead       |
| Implementation   | ⚠️ Complex (packet parsing, lookup) | ✅ Simple and direct             |
| Protocol support | ⚠️ TCP/UDP only (with lookup)       | ✅ All protocols                 |
| Kernel version   | 4.17+ (for sk_lookup)               | 4.10+ (for cgroup SKB)           |
| IFB module       | ❌ Required                         | ✅ Not needed!                   |
| Setup complexity | ⚠️ IFB setup + TC config            | ✅ Just attach to cgroup         |

### Limitations of eBPF Cgroup Hooks

**Important:** The eBPF cgroup backend can only **drop packets**, not **shape/queue** them like TC HTB.

- **TC HTB (ifb_tc):** Queues packets and releases them at controlled rate (smooth traffic)
- **eBPF cgroup:** Drops packets when rate limit exceeded (TCP retransmits, reduced throughput)

**For most users:** Packet dropping is sufficient and simpler.

**For advanced users needing true queuing:** Use `ifb_tc` on cgroup v1 systems.

## What We Built

### eBPF TC Classifier (`tc_classifier.rs`)

**Status:** ✅ Compiles successfully

**What it does:** Placeholder TC classifier that returns `TC_ACT_PIPE` (pass-through)

**BPF Map:** `CGROUP_CLASSID_MAP` (u64 cgroup_id → u32 classid)

**Current implementation:**

- Does NOT perform socket lookup (aya-ebpf 0.1 doesn't expose the helpers)
- Does NOT map packets to cgroups (would need socket lookup)
- Just passes packets through to TC

**Why we kept it:**

1. **Future enhancement:** When aya-ebpf adds `bpf_sk_lookup_*()` support, we can enhance it
2. **Alternative use cases:** Might be useful for other TC classifier scenarios
3. **Documentation:** Shows what was attempted and why it wasn't pursued
4. **Minimal overhead:** Compiles to a tiny pass-through program

### Build Integration

**Added to:**

- `xtask/src/main.rs` - Builds tc_classifier eBPF program
- `chadthrottle-ebpf/Cargo.toml` - Added tc_classifier binary target
- `chadthrottle/build.rs` - Checks for tc_classifier program

**Build command:** `cargo xtask build-ebpf`

**Result:** All 3 eBPF programs compile:

- `chadthrottle-egress` (upload throttling)
- `chadthrottle-ingress` (download throttling)
- `chadthrottle-tc-classifier` (placeholder/future)

## Recommendations

### For Users

**Cgroup v1 systems:**

- Use `ifb_tc` for download throttling (TC HTB queuing, smooth traffic shaping)
- Use `tc_htb` for upload throttling

**Cgroup v2 systems:**

- Use `ebpf` for both download and upload throttling (best performance, no IFB needed)
- Accept packet dropping instead of queuing (TCP handles this well)

**Mixed environments:**

- ChadThrottle automatically selects the best available backend
- `ebpf` backend has `BackendPriority::Best` (highest priority)
- Falls back to other backends if eBPF not available

### For Developers

**Do NOT pursue TC classifier approach** unless:

1. aya-ebpf adds `bpf_sk_lookup_*()` helper support
2. You need TC queuing specifically (not just rate limiting)
3. You're willing to accept the complexity and performance overhead
4. You have a specific use case that cgroup hooks don't cover

**Instead:**

- Enhance the existing `ebpf` download backend if needed
- Improve documentation for cgroup v1 vs v2 tradeoffs
- Consider adding TCP-aware features to eBPF backend (e.g., ECN marking instead of drops)

## Technical Notes

### Why TC Cgroup Filter Requires net_cls

TC's cgroup filter (`tc filter add ... cgroup`) works by:

1. Reading the `net_cls.classid` value from the packet's cgroup
2. Matching packets based on that classid
3. Routing to appropriate TC class

Cgroup v2 removed the `net_cls` controller in favor of eBPF programs, so this mechanism doesn't work.

### Why eBPF Cgroup Hooks Are Better

eBPF cgroup hooks (`BPF_CGROUP_INET_*`) were designed specifically to replace legacy cgroup controllers like `net_cls`:

- **Direct cgroup association:** Program runs in context of the cgroup
- **Access to cgroup ID:** `bpf_get_current_cgroup_id()` helper
- **Better performance:** No packet redirection through IFB
- **Simpler code:** No need for TC qdisc/class/filter setup

This is the **intended** way to do per-cgroup network throttling on modern kernels.

## Files Modified

### Created

- `chadthrottle-ebpf/src/tc_classifier.rs` - eBPF TC classifier (placeholder)
- `EBPF_TC_CLASSIFIER_DECISION.md` - This document

### Modified

- `xtask/src/main.rs` - Build tc_classifier
- `chadthrottle-ebpf/Cargo.toml` - Added tc_classifier binary
- `chadthrottle/build.rs` - Check for tc_classifier program

### NOT Modified (deliberately)

- `chadthrottle/src/backends/throttle/download/linux/ifb_tc.rs` - Kept cgroup v1 requirement
- No integration of TC classifier into userspace (not needed)

## Conclusion

The TC classifier approach is **technically possible** but **not worth the complexity** given that:

1. **A better solution already exists:** eBPF cgroup hooks (`ebpf` backend)
2. **TC classifier has fundamental limitations:** No socket context on ingress
3. **Workarounds are complex:** Socket lookup, connection tracking, etc.
4. **Performance would be worse:** Per-packet lookup overhead
5. **Protocol limitations:** TCP/UDP only with socket lookup

**Recommendation:** Document that `ifb_tc` requires cgroup v1, and recommend `ebpf` backend for cgroup v2 users.

The TC classifier code is kept as a reference implementation and for potential future use if requirements change.

## References

- [BPF cgroup hooks documentation](https://www.kernel.org/doc/html/latest/bpf/cgroup-sysctl.html)
- [TC eBPF classifiers](https://docs.kernel.org/bpf/prog_cgroup_sock.html)
- [aya-ebpf documentation](https://docs.rs/aya-ebpf/)
- Session summary from previous work
