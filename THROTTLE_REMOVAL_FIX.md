# Throttle Removal Bug Fix

## 🐛 The Bug

**Throttles persisted even after removal!**

When you pressed 'r' to remove a throttle or quit the TUI:

- ✅ UI showed throttle was removed
- ✅ Cleanup code was called
- ❌ **The process remained throttled!**

## Root Cause

**Two critical bugs in `remove_download_throttle()` and `remove_upload_throttle()`:**

### Bug #1: Wrong Map Key

**The code used `cgroup_id` to remove from maps, but maps were using fixed key `0`!**

```rust
// INSERTION (in throttle_download) - CORRECT:
const MAP_KEY: u64 = 0;
config_map.insert(MAP_KEY, config, 0)?;  // ✅ Uses key 0

// REMOVAL (in remove_download_throttle) - WRONG:
config_map.remove(&cgroup_id);  // ❌ Tries to remove key 25722
```

**What happened:**

1. Config inserted at map[0]
2. Remove tries to delete map[25722]
3. **Nothing is removed!** (wrong key)
4. BPF program keeps reading config from map[0]
5. **Throttling continues!**

### Bug #2: Program Never Detached

**The removal code never detached the BPF program!**

```rust
// OLD CODE:
fn remove_download_throttle(&mut self, pid: i32) -> Result<()> {
    // Remove from maps (with wrong key)
    config_map.remove(&cgroup_id);  // ❌ Wrong key

    // TODO: detach program?
    // Never actually detached!  // ❌ BPF program stays attached
}
```

**Result:** Even if maps were cleared correctly, the BPF program remained attached to the cgroup, continuing to throttle traffic.

## The Fix

### Part 1: Use Correct Map Key

**Changed to use `MAP_KEY = 0` for all map operations:**

```rust
// FIXED CODE:
const MAP_KEY: u64 = 0;  // Match what we used in insert

config_map.remove(&MAP_KEY);  // ✅ Removes correct key
bucket_map.remove(&MAP_KEY);  // ✅ Removes correct key
stats_map.remove(&MAP_KEY);   // ✅ Removes correct key
```

### Part 2: Detach BPF Program

**Added explicit program detachment:**

```rust
// Get cgroup path for this PID
let cgroup_path = get_cgroup_path(pid)?;

// Find the attached program for this cgroup
if let Some(pos) = self.attached_programs.iter().position(|p| {
    &p.cgroup_path == &cgroup_path
}) {
    let attached = self.attached_programs.remove(pos);

    // Detach the program!
    detach_cgroup_skb_legacy(&attached.cgroup_path, attached.attach_type)?;

    // Remove from tracking
    self.attached_cgroups.remove(&attached.cgroup_path);
}
```

### Part 3: Proper Cleanup Flow

**Complete removal now does:**

1. ✅ Remove config from map[0] (not map[cgroup_id])
2. ✅ Remove bucket from map[0]
3. ✅ Remove stats from map[0]
4. ✅ Detach BPF program from cgroup
5. ✅ Remove from `attached_programs` list
6. ✅ Remove from `attached_cgroups` set
7. ✅ **Throttling stops immediately!**

## Files Modified

### Download Backend

**`chadthrottle/src/backends/throttle/download/linux/ebpf.rs`**

- Changed `remove_download_throttle()` (lines 427-476)
- Use `MAP_KEY = 0` instead of `cgroup_id` for map removals
- Added `get_cgroup_path()` call to find program to detach
- Added BPF program detachment logic
- Added cleanup of tracking structures

### Upload Backend

**`chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`**

- Changed `remove_upload_throttle()` (lines 342-391)
- Same fixes as download backend
- Use `MAP_KEY = 0` for map removals
- Added BPF program detachment

## Testing

### Before Fix

```bash
# Start a download
wget http://example.com/largefile.iso

# Throttle it
sudo ./chadthrottle
# Press 't', set limit to 30K

# Try to remove throttle
# Press 'r'

# Result:
❌ UI shows "Throttle removed"
❌ But download still throttled at 30 KB/s!
❌ BPF program still attached (bpftool shows it)
❌ Maps still have config at key 0
```

### After Fix

```bash
# Start a download
wget http://example.com/largefile.iso

# Throttle it
sudo ./chadthrottle
# Press 't', set limit to 30K

# Remove throttle
# Press 'r'

# Result:
✅ UI shows "Throttle removed"
✅ Download immediately returns to full speed!
✅ BPF program detached (bpftool confirms)
✅ Maps cleared (config removed from map[0])
```

### Verification Commands

**While throttled:**

```bash
sudo bpftool cgroup show /sys/fs/cgroup/path/to/cgroup
# Shows: chadthrottle_in program attached
```

**After pressing 'r':**

```bash
sudo bpftool cgroup show /sys/fs/cgroup/path/to/cgroup
# Shows: (empty - program detached!)
```

## How to Use

### Remove Throttle via TUI

1. Select throttled process
2. Press `r`
3. ✅ Throttle removed immediately

### Remove Throttle via Quit

1. Press `q` or `Esc` or `Ctrl+C`
2. ✅ All throttles cleaned up automatically

### Prevent Auto-Restore

```bash
# Don't restore throttles on next startup
./chadthrottle --no-restore

# Don't save throttles on exit
./chadthrottle --no-save
```

## Why This Matters

**This was a critical production bug:**

1. **User Experience:** Users couldn't remove throttles they applied
2. **Process Management:** Throttled processes stayed throttled even after "removal"
3. **System Impact:** BPF programs accumulated, never being cleaned up
4. **Resource Leak:** Multiple program instances could stack up

**After this fix:**

- ✅ Throttle removal works immediately
- ✅ BPF programs properly detached
- ✅ No resource leaks
- ✅ System returns to normal state

## Related Fixes

This fix builds on previous fixes:

1. **Map Instance Fix** - Ensured program loads once, not per-attach
2. **Cleanup Fix** - Added proper detachment on TUI exit
3. **Cgroup ID Fix** - Use fixed key instead of `bpf_get_current_cgroup_id()`
4. **This Fix** - Removal now uses correct key and detaches programs

All four fixes were necessary for proper throttling functionality!

## Impact Summary

| Operation                   | Before Fix           | After Fix            |
| --------------------------- | -------------------- | -------------------- |
| Apply throttle              | ✅ Works             | ✅ Works             |
| Remove throttle (press 'r') | ❌ Broken            | ✅ Works             |
| Quit TUI (press 'q')        | ❌ Throttles persist | ✅ Cleaned up        |
| Map cleanup                 | ❌ Wrong key used    | ✅ Correct key       |
| Program detach              | ❌ Never detached    | ✅ Properly detached |
| Resource cleanup            | ❌ Leaked            | ✅ Complete          |

## Build & Test

```bash
# Build with fix
cargo xtask build-release

# Binary at:
./target/release/chadthrottle

# Test removal:
# 1. Start a download (wget, curl, browser)
# 2. Run: sudo ./target/release/chadthrottle
# 3. Press 't' to throttle a process
# 4. Press 'r' to remove throttle
# 5. Verify download speed returns to normal immediately!
```

## Lesson Learned

**Always match your insert and remove keys!**

When using fixed keys for maps:

- Insert with key 0 → Remove with key 0
- NOT: Insert with key 0 → Remove with key cgroup_id

This bug was silent because:

- Errors were ignored (`let _ = config_map.remove(...)`)
- BPF program kept running with old config
- No visible error message

The fix adds logging to make it obvious when cleanup happens!
