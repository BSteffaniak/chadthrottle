# Backend Name Consistency Fix

## Problem

The backend info modal was showing incorrect ⭐ stars for active backends:

**User reported:**

```
Upload Backends:
  ⭐ ebpf            [DEFAULT]     ✅ Correct

Download Backends:
  ✅ ebpf            Available     ❌ WRONG - should show ⭐ on tc_police!
  ✅ tc_police       Available

Configuration:
  Preferred Download:   tc_police (tc_police_download selected)
```

The config showed "tc_police_download selected" but UI compared against "tc_police", so the match failed.

## Root Cause

**Naming mismatch** between backend detection list and backend `name()` methods:

| Backend           | Detection List | Backend name()       | Match? |
| ----------------- | -------------- | -------------------- | ------ |
| eBPF Upload       | "ebpf"         | "ebpf"               | ✅     |
| eBPF Download     | "ebpf"         | "ebpf"               | ✅     |
| nftables Upload   | "nftables"     | "nftables_upload"    | ❌     |
| nftables Download | "nftables"     | "nftables_download"  | ❌     |
| tc_htb            | "tc_htb"       | "tc_htb_upload"      | ❌     |
| ifb_tc            | "ifb_tc"       | "ifb_tc_download"    | ❌     |
| tc_police         | "tc_police"    | "tc_police_download" | ❌     |

### Why This Happened

The backend `name()` methods had "\_upload" or "\_download" suffixes, but the detection list (what users see and select) had simple names without suffixes.

When the UI tried to match:

```rust
let is_active = backend_info.active_download.as_ref() == Some(name);
// "tc_police_download" == "tc_police" → false ❌
```

## The Fix

Changed all backend `name()` methods to match the detection list names (removed suffixes).

### Files Modified

**Download Backends:**

1. `chadthrottle/src/backends/throttle/download/linux/tc_police.rs`
   - `"tc_police_download"` → `"tc_police"` ✅

2. `chadthrottle/src/backends/throttle/download/linux/ifb_tc.rs`
   - `"ifb_tc_download"` → `"ifb_tc"` ✅

3. `chadthrottle/src/backends/throttle/download/linux/nftables.rs`
   - `"nftables_download"` → `"nftables"` ✅

**Upload Backends:** 4. `chadthrottle/src/backends/throttle/upload/linux/nftables.rs`

- `"nftables_upload"` → `"nftables"` ✅

5. `chadthrottle/src/backends/throttle/upload/linux/tc_htb.rs`
   - `"tc_htb_upload"` → `"tc_htb"` ✅

### After Fix

All backend names now match:

| Backend           | Detection List | Backend name() | Match? |
| ----------------- | -------------- | -------------- | ------ |
| eBPF Upload       | "ebpf"         | "ebpf"         | ✅     |
| eBPF Download     | "ebpf"         | "ebpf"         | ✅     |
| nftables Upload   | "nftables"     | "nftables"     | ✅     |
| nftables Download | "nftables"     | "nftables"     | ✅     |
| tc_htb            | "tc_htb"       | "tc_htb"       | ✅     |
| ifb_tc            | "ifb_tc"       | "ifb_tc"       | ✅     |
| tc_police         | "tc_police"    | "tc_police"    | ✅     |

## Impact

### Before Fix

```
Download Backends:
  ✅ ebpf            Available
  ✅ tc_police       Available     ← No ⭐ even though it's active!

Configuration:
  Preferred Download: tc_police (tc_police_download selected)
```

### After Fix

```
Download Backends:
  ✅ ebpf            Available
  ⭐ tc_police       [DEFAULT]     ← ⭐ correctly shows active backend!

Configuration:
  Preferred Download: tc_police (tc_police selected)
```

## Testing

```bash
# 1. Build and run
cargo build --release
sudo ./target/release/chadthrottle

# 2. Press 'b' to view backends
# → ⭐ should be on the correct default backend

# 3. Switch backend: Press Enter → select different backend → Enter
# 4. Press 'b' again
# → ⭐ should move to the newly selected backend ✅

# 5. Quit and restart
# → ⭐ should persist on the selected backend ✅
```

## Config File Changes

### Before

```json
{
  "preferred_upload_backend": "tc_htb_upload",
  "preferred_download_backend": "tc_police_download"
}
```

### After

```json
{
  "preferred_upload_backend": "tc_htb",
  "preferred_download_backend": "tc_police"
}
```

**Note:** Old config files with "_upload"/"\_download" suffixes will still work! The `select_\*\_backend()` functions accept the old names and create backends successfully. New selections will save the simplified names.

## Build Status

✅ **Compiled successfully**  
✅ **Binary:** `/home/braden/ChadThrottle/target/release/chadthrottle` (4.4 MB)

## Related Issues

This fix resolves:

- ✅ Incorrect ⭐ placement in backend info modal
- ✅ "[DEFAULT]" label showing on wrong backend
- ✅ Confusion between displayed name and actual backend name
- ✅ Config file showing verbose names like "tc_police_download"

## Breaking Changes

**None!** Fully backward compatible:

- Old config files with suffixed names still work
- CLI args like `--upload-backend tc_htb_upload` still work
- New selections simply save cleaner names

## Summary

**The Problem:** Backend names had unnecessary "\_upload"/"\_download" suffixes  
**The Cause:** Inconsistency between detection list and backend implementation  
**The Fix:** Removed suffixes to match detection list names  
**The Result:** ⭐ stars now appear on the correct backends in the UI! ✅
