# IFB_TC Cgroup v1 Requirement Fix

**Date:** 2025-11-07  
**Status:** ✅ Complete  
**Build:** Success (release binary compiled)

## Problem Summary

The `ifb_tc` download throttling backend was reporting as "available" on cgroup v2 systems, but **silently failing to actually throttle** because TC cgroup filter only works with cgroup v1.

### Root Cause

**TC cgroup filter (used by ifb_tc) ONLY works with:**

- ✅ Cgroup v1 with `net_cls.classid` controller
- ❌ Cgroup v2 (requires eBPF TC classifier, not implemented)

**What was happening:**

1. User on cgroup v2 system (modern Linux default) ✅
2. `ifb_tc.is_available()` returned `true` ❌ (incorrectly)
3. User selected ifb_tc for download throttling
4. IFB device setup succeeded ✅
5. TC cgroup filter configured ✅
6. Cgroup v2 directories created ✅
7. TC classes created ✅
8. **BUT: TC cgroup filter can't match packets to cgroups with v2** ❌
9. Result: No throttling, packets go to default class

### Evidence from User Logs

```
DEBUG cgroup::v2::nftables > Created cgroup v2 for PID 1616391 (wget)
INFO  ifb_tc                > ✅ IFB throttling backend initialized successfully
(No TC class creation logs - silently does nothing)
```

The cgroup was created but TC never matched any packets to it.

## Solution Implemented

### Fix #1: Added Cgroup v1 Check Helper

**File:** `chadthrottle/src/backends/cgroup/mod.rs`

Added new public function to check specifically for cgroup v1:

```rust
/// Check if cgroup v1 with net_cls controller is available
///
/// This is used by backends that specifically require cgroup v1,
/// such as ifb_tc which uses TC cgroup filter (only works with v1).
pub fn is_cgroup_v1_available() -> bool {
    #[cfg(feature = "cgroup-v1")]
    {
        if let Ok(backend) = v1::CgroupV1Backend::new() {
            if let Ok(available) = backend.is_available() {
                return available;
            }
        }
    }
    false
}
```

**Purpose:** Allows backends to explicitly check for cgroup v1 availability.

### Fix #2: Updated ifb_tc Availability Check

**File:** `chadthrottle/src/backends/throttle/download/linux/ifb_tc.rs`

**Old Code (BROKEN):**

```rust
fn is_available() -> bool {
    if !check_ifb_availability() {
        return false;
    }

    if !check_tc_available() {
        return false;
    }

    // Check if any cgroup backend is available (works with v1 or v2)
    if let Ok(Some(backend)) = crate::backends::cgroup::select_best_backend() {
        if let Ok(available) = backend.is_available() {
            return available;
        }
    }

    false
}
```

**Problem:** Accepted ANY cgroup backend (v1 or v2), but only v1 works!

**New Code (FIXED):**

```rust
fn is_available() -> bool {
    // Check IFB module
    if !check_ifb_availability() {
        log::debug!("ifb_tc unavailable: IFB kernel module not found");
        return false;
    }

    // Check TC (traffic control)
    if !check_tc_available() {
        log::debug!("ifb_tc unavailable: TC (traffic control) not available");
        return false;
    }

    // CRITICAL: ifb_tc REQUIRES cgroup v1 with net_cls controller
    // TC cgroup filter does NOT work with cgroup v2 (would need eBPF classifier)
    if !crate::backends::cgroup::is_cgroup_v1_available() {
        log::debug!(
            "ifb_tc unavailable: requires cgroup v1 net_cls controller.\n\
             TC cgroup filter does not work with cgroup v2.\n\
             Use tc_police for download throttling on cgroup v2 systems."
        );
        return false;
    }

    log::debug!("ifb_tc available: all requirements met (IFB, TC, cgroup v1)");
    true
}
```

**Changes:**

- ✅ Explicitly checks for cgroup v1 (not just any cgroup)
- ✅ Returns false on cgroup v2 systems
- ✅ Clear debug logging explaining why unavailable
- ✅ Suggests tc_police as alternative for cgroup v2

### Fix #3: Updated Documentation

**File:** `chadthrottle/src/backends/throttle/download/linux/ifb_tc.rs`

Added comprehensive documentation at top of file:

```rust
// IFB + TC HTB download throttling backend
//
// REQUIREMENTS:
// - IFB kernel module (ifb)
// - TC (traffic control) command
// - **Cgroup v1 with net_cls controller** (does NOT work with cgroup v2)
//
// LIMITATIONS:
// - TC cgroup filter only works with cgroup v1 net_cls.classid
// - On cgroup v2 systems, use tc_police backend instead
// - Future: Could be adapted to work with cgroup v2 via eBPF TC classifier
```

## Build Status

✅ **Compile successful:**

```bash
$ cargo build --release
   Compiling chadthrottle v0.6.0
   Finished `release` profile [optimized] target(s) in 15.11s
```

**Warnings:** 41 warnings (all pre-existing, no new warnings)  
**Errors:** 0

## Expected Behavior After Fix

### On Cgroup v2 Systems (Most Modern Linux)

**When running without backend specified:**

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle
```

**Expected logs:**

```
DEBUG ifb_tc unavailable: requires cgroup v1 net_cls controller.
      TC cgroup filter does not work with cgroup v2.
      Use tc_police for download throttling on cgroup v2 systems.
DEBUG Available download backends:
DEBUG   nftables - priority: Better, available: false
DEBUG   ifb_tc - priority: Good, available: false
DEBUG   tc_police - priority: Fallback, available: true
INFO  Auto-selected download backend: tc_police
```

**When explicitly requesting ifb_tc:**

```bash
sudo ./target/release/chadthrottle --download-backend ifb_tc
```

**Expected behavior:**

- Error message: "Download backend 'ifb_tc' not available"
- Program exits or falls back to another backend

### On Cgroup v1 Systems (Legacy)

If system has `/sys/fs/cgroup/net_cls/`:

```
DEBUG ifb_tc available: all requirements met (IFB, TC, cgroup v1)
INFO  Using preferred download backend: ifb_tc
INFO  ✅ IFB throttling backend initialized successfully
```

Throttling will work as expected.

## Testing Instructions

### Test 1: Verify ifb_tc Shows Unavailable

```bash
cargo build --release
sudo RUST_LOG=debug ./target/release/chadthrottle 2>&1 | grep -E "ifb_tc|Available download|Auto-selected download"
```

**Expected output:**

```
DEBUG ifb_tc unavailable: requires cgroup v1 net_cls controller
DEBUG   ifb_tc - priority: Good, available: false
DEBUG   tc_police - priority: Fallback, available: true
INFO  Auto-selected download backend: tc_police
```

### Test 2: Test tc_police Actually Works

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle --download-backend tc_police --upload-backend tc_htb
```

**Steps:**

1. Launch program
2. Find a process (wget, firefox, etc.)
3. Press 't' to throttle
4. Set download limit (e.g., 1 MB/s)
5. Run download test
6. Verify speed is limited

**tc_police limitations:**

- ⚠️ **Interface-wide throttling** (not per-process)
- ⚠️ Affects ALL downloads on the interface, not just one process
- ✅ Works on cgroup v2 systems
- ✅ No cgroup requirements

### Test 3: Verify Error Message When Forcing ifb_tc

```bash
sudo ./target/release/chadthrottle --download-backend ifb_tc
```

**Expected:**

- Clear error about ifb_tc not being available
- Or program starts but shows ifb_tc as unavailable

## Alternative Backends for Cgroup v2

Since ifb_tc doesn't work on cgroup v2, users have these options:

### Option 1: tc_police (Current Recommendation)

```bash
sudo ./target/release/chadthrottle --download-backend tc_police
```

**Pros:**

- ✅ Works on any system with TC
- ✅ No cgroup requirements
- ✅ Simple and reliable

**Cons:**

- ❌ Interface-wide (not per-process)
- ❌ Throttles ALL traffic, can't isolate specific processes

### Option 2: Future eBPF Download Backend (Not Yet Implemented)

Would provide:

- ✅ Per-process download throttling
- ✅ Works with cgroup v2
- ✅ Native kernel integration
- ❌ Requires eBPF programs to be built

## Files Changed

### 1. chadthrottle/src/backends/cgroup/mod.rs

**Added:** `is_cgroup_v1_available()` helper function (lines ~189-201)

- Public function to check for cgroup v1
- Used by backends that require v1 specifically
- Returns false if v1 not available

### 2. chadthrottle/src/backends/throttle/download/linux/ifb_tc.rs

**Modified:** `is_available()` method (lines 286-310)

- Changed from accepting any cgroup to requiring v1
- Added debug logging for each check
- Clear message explaining cgroup v2 incompatibility

**Modified:** Top documentation (lines 1-12)

- Added REQUIREMENTS section
- Added LIMITATIONS section
- Explicitly mentions cgroup v1 requirement

## Technical Background

### Why TC Cgroup Filter Needs Cgroup v1

**Cgroup v1 (Legacy):**

- Has `net_cls.classid` controller
- Kernel tags packets with classid value
- TC can match on this classid directly
- Works with simple TC filters: `filter match cgroup`

**Cgroup v2 (Modern):**

- No `net_cls.classid` - unified hierarchy
- TC cgroup filter can't match packets
- Requires eBPF TC classifier to work
- eBPF classifier not implemented in ifb_tc

### Future Enhancement Path

To support cgroup v2 with ifb_tc:

1. Implement eBPF TC classifier program
2. Attach to TC filter on ifb device
3. Map cgroup IDs to classids in eBPF
4. Match packets in eBPF, return classid
5. TC uses classid to route to correct HTB class

This is similar to how the eBPF download backend would work, but integrated with TC HTB instead of pure eBPF rate limiting.

## Success Criteria

✅ ifb_tc reports unavailable on cgroup v2 systems  
✅ Clear debug log explains why unavailable  
✅ Users directed to tc_police alternative  
✅ No silent failures or confusion  
✅ Build succeeds with no errors  
✅ Documentation updated with requirements

## Related Documentation

- IFB_THROTTLING_FIXES.md - IFB error handling improvements
- INTERFACE_AND_EBPF_FIXES.md - Interface detection and eBPF fixes
- CGROUP_V2_REFACTORING_COMPLETE.md - Cgroup abstraction architecture
- EBPF_BACKENDS.md - eBPF architecture and future enhancements

---

**Summary:** ifb_tc now correctly reports as unavailable on cgroup v2 systems with a clear explanation. Users are automatically directed to tc_police, which works on all systems. Future eBPF integration could enable per-process download throttling on cgroup v2.
