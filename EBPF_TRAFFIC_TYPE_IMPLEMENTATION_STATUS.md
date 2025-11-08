# eBPF Traffic Type Filtering - Implementation Status

## Date

November 8, 2025

## Completed Work ✅

### Phase 1: Data Structures (COMPLETE)

**File:** `chadthrottle-common/src/lib.rs`

**Changes Made:**

1. ✅ Added traffic type constants:
   - `TRAFFIC_TYPE_ALL = 0`
   - `TRAFFIC_TYPE_INTERNET = 1`
   - `TRAFFIC_TYPE_LOCAL = 2`

2. ✅ Updated `CgroupThrottleConfig` struct:
   - Added `traffic_type: u8` field
   - Changed `_padding` from `u32` to `[u8; 3]` for proper alignment
   - Updated `new()` constructor to initialize `traffic_type` to `TRAFFIC_TYPE_ALL`

3. ✅ Verified struct size remains 32 bytes (cache-line friendly)
4. ✅ Successfully compiled

### Phase 5: Userspace Backend Updates (COMPLETE)

**Files:**

- `chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`
- `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`

**Changes Made:**

1. ✅ Import traffic type constants from common crate
2. ✅ Convert `TrafficType` enum to u8 in both backends
3. ✅ Pass `traffic_type_value` to `CgroupThrottleConfig`
4. ✅ Update struct instantiation with new field
5. ✅ Added `supports_traffic_type()` override returning `true` to both backends

**Result:** Userspace code is ready and will compile once eBPF programs are fixed

## In Progress Work 🚧

### Phases 2-4: eBPF Program Updates (BLOCKED)

**Files:**

- `chadthrottle-ebpf/src/egress.rs`
- `chadthrottle-ebpf/src/ingress.rs`

**What Was Attempted:**

1. ✅ Added `IpAddr` and `TrafficCategory` enums
2. ✅ Implemented IP classification functions (IPv4 and IPv6)
3. ✅ Added conditional throttling logic to check traffic type
4. ❌ **BLOCKED:** Packet data extraction

**Blocking Issue:**
The aya_ebpf `SkBuffContext` API doesn't provide a straightforward way to read packet data that satisfies both:

- The eBPF verifier's strict requirements
- The bpf-linker's limitations (no complex error handling, no aggregate returns, etc.)

**Attempted Approaches:**

1. `ctx.data()` / `ctx.data_end()` - Methods don't exist on `SkBuffContext`
2. `ctx.load()` with `.ok()?` - Causes LLVM errors about stack arguments and aggregate returns
3. `ctx.ptr_at()` - Method doesn't exist in this version of aya_ebpf

**Current State:**

- IP classification logic is implemented ✅
- Conditional throttling logic is implemented ✅
- Packet parsing is NOT working ❌
- eBPF programs do NOT compile ❌

## What Needs to Be Done 🔧

### Option 1: Research Correct aya_ebpf Packet Access API

**Effort:** 2-3 hours

Need to:

1. Check aya_ebpf documentation/examples for packet parsing
2. Find the correct API for reading packet bytes
3. Implement IP extraction using that API
4. Ensure it passes eBPF verifier

**Resources to check:**

- https://github.com/aya-rs/aya/tree/main/examples
- aya_ebpf documentation
- Existing cgroup_skb examples in aya

### Option 2: Disable Traffic Type Filtering for Now

**Effort:** 30 minutes

1. Remove IP extraction code from eBPF programs
2. Keep `traffic_type` field but ignore it in eBPF (always throttle)
3. Keep userspace changes (pass traffic_type, return `supports_traffic_type() = true`)
4. Document as "planned feature" for future implementation

**Result:**

- eBPF programs compile and work (but ignore traffic_type)
- Userspace thinks feature is supported but it doesn't actually filter
- Modal won't show, but filtering won't work either
- NOT RECOMMENDED - creates false impression

### Option 3: Keep Modal for eBPF (Revert supports_traffic_type)

**Effort:** 5 minutes

1. Remove `supports_traffic_type()` override from eBPF backends
2. They default to "only supports All" (current behavior)
3. Modal shows when user selects Internet/Local with eBPF backend
4. User can switch to nftables for upload traffic type filtering

**Result:**

- Honest about capabilities
- Upload + Internet/Local → modal offers nftables switch
- Download + Internet/Local → modal offers "convert to All" only
- Same UX as before this session for eBPF backend

## Recommendation

**OPTION 1** is the right approach, but requires finding the correct aya_ebpf API.

**OPTION 3** is the pragmatic short-term solution - be honest that eBPF doesn't support traffic type filtering yet.

**Immediate Next Steps:**

1. Research aya_ebpf packet access APIs (see examples, documentation)
2. If found quickly (< 1 hour), implement and complete
3. If not found quickly, go with Option 3 (revert supports_traffic_type override)

## Files Modified So Far

**Completed & Working:**

- ✅ `chadthrottle-common/src/lib.rs`
- ✅ `chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`
- ✅ `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`

**In Progress (Broken):**

- ❌ `chadthrottle-ebpf/src/egress.rs`
- ❌ `chadthrottle-ebpf/src/ingress.rs`

## Time Spent

- Phase 1 (data structures): 30 minutes ✅
- Phase 5 (userspace backends): 45 minutes ✅
- Phases 2-4 (eBPF packet parsing): 2+ hours ❌ (BLOCKED)

**Total:** ~3+ hours so far

## Lessons Learned

1. eBPF packet parsing APIs are not well documented in aya
2. eBPF verifier and bpf-linker have strict requirements
3. Complex Rust error handling (`Result`, `Option` with `?`) doesn't work in eBPF
4. Should have researched packet access API first before implementing
5. TC classifier programs might be easier than cgroup_skb for packet inspection

## Alternative Approach (For Future)

Instead of modifying existing cgroup_skb programs, could:

1. Use TC (traffic control) BPF programs which have better packet access
2. Attach to TC egress/ingress hooks
3. Use `bpf_skb_load_bytes()` helper directly
4. Might be more compatible with aya's current API

This would be a larger refactoring but might be more straightforward for packet inspection.
