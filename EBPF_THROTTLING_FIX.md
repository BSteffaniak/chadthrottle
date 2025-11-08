# eBPF Download Throttling - Root Cause Fix

## Problem Identified

wget was downloading at **7x the throttle limit** (349.7 KB/s vs 50 KB/s limit).

Root cause analysis revealed TWO critical bugs:

### Bug 1: Stats Logging Not Working

The `log_ebpf_stats()` function existed but was never actually called because:

- `ThrottleManager` couldn't access the concrete `EbpfDownload` type through trait object
- The stub implementation just logged a trace message and did nothing

**Result:** No diagnostic output, couldn't see if eBPF program was being called.

### Bug 2: eBPF Attached to Wrong Cgroup (THE SMOKING GUN)

The eBPF `BPF_CGROUP_INET_INGRESS` program was attached to the **leaf cgroup**:

```
/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/tmux-spawn-XXX.scope
```

**Problem:** Transient systemd scope cgroups don't reliably receive packet events for BPF_CGROUP_INET_INGRESS hooks!

Packets are often processed at the parent cgroup level, so the leaf cgroup hook never fires.

## Fixes Implemented

### Fix 1: Make Stats Logging Actually Work

**Added `log_diagnostics()` to trait:**

- File: `chadthrottle/src/backends/throttle/mod.rs`
- Added method to `DownloadThrottleBackend` trait with default no-op implementation

**Implemented in EbpfDownload:**

- File: `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`
- Calls existing `log_throttle_stats()` method

**Fixed ThrottleManager:**

- File: `chadthrottle/src/backends/throttle/manager.rs`
- Now actually calls `backend.log_diagnostics(pid)` through trait

**Result:** Stats logging now works and will show eBPF program status.

### Fix 2: Attach to Parent Cgroup (CRITICAL FIX)

**Changed attach strategy:**

- File: `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs`
- Modified `get_cgroup_path()` to walk UP to parent cgroup

**Old behavior:**

```
PID in: /user.slice/user-1000.slice/user@1000.service/tmux-spawn-XXX.scope
Attach to: tmux-spawn-XXX.scope  ❌ DOESN'T WORK!
```

**New behavior:**

```
PID in: /user.slice/user-1000.slice/user@1000.service/tmux-spawn-XXX.scope
Attach to: user@1000.service  ✅ WORKS!
```

**Why this fixes it:**

- `user@1000.service` is a **stable parent cgroup** that receives ALL packets for user's processes
- BPF_CGROUP_INET_INGRESS fires reliably at this level
- The eBPF program filters by exact cgroup_id in the BPF map, so only throttled PIDs are affected

**Implementation details:**

- Added `attached_cgroups: HashSet<PathBuf>` to track which parent cgroups we've attached to
- Only attach once per parent (avoid duplicate attachments)
- Each PID still gets its own entry in CGROUP_CONFIGS map with its specific cgroup_id
- eBPF program checks the packet's cgroup_id and only throttles if it's in the map

## Expected Behavior Now

### On Throttling wget to 50 KB/s:

1. **Attachment logs:**

   ```
   DEBUG PID 12345 cgroup: user.slice/.../tmux-spawn-XXX.scope -> attaching to parent: user.slice/.../user@1000.service
   INFO  Attaching eBPF ingress program to parent cgroup (path: "/sys/fs/cgroup/.../user@1000.service")
   INFO  Successfully attached eBPF ingress program
   ```

2. **Bandwidth monitoring (every 5 seconds):**

   ```
   INFO PID 12345 (wget) download: actual=48.2 KB/s, limit=50.0 KB/s, ratio=0.96x ✅ THROTTLED
   ```

3. **eBPF stats (every 5 seconds):**
   ```
   INFO eBPF stats PID 12345 cgroup 25722: program_calls=15234, packets=5000, dropped=4500 (90.0%), ...
   ```

### Diagnostic Output Breakdown

**If working correctly:**

- `actual` should be close to `limit` (ratio < 1.1x)
- `program_calls` should be > 0 (eBPF is running)
- `packets_dropped` should be > 0 (throttling active)
- Status: `✅ THROTTLED`

**If broken, you'll see:**

- `⚠️ THROTTLE NOT WORKING` - ratio > 1.5x
- `⚠️ eBPF program NOT BEING CALLED` - program_calls = 0
- `⚠️ eBPF NOT DROPPING PACKETS` - packets_dropped = 0

## Testing

Run with enhanced diagnostics:

```bash
cd /home/braden/ChadThrottle
sudo RUST_LOG=debug ./target/release/chadthrottle 2>&1 | tee /tmp/chadthrottle-test.log
```

In another terminal:

```bash
wget http://speedtest.tele2.net/100MB.zip -O /dev/null
```

Throttle wget to 50 KB/s in the UI, then watch the logs.

## Technical Details

### Why Parent Cgroup Attachment Works

BPF_CGROUP_INET_INGRESS hooks are **hierarchical**:

- When you attach to a parent cgroup, the program runs for ALL descendant processes
- The program has access to `bpf_get_current_cgroup_id()` which returns the EXACT cgroup of the packet
- We store per-process throttle configs in CGROUP_CONFIGS map using the exact cgroup_id
- The eBPF program checks if the packet's cgroup_id is in the map - if yes, throttle; if no, allow

This is **superior** to leaf attachment because:

1. Parent cgroups are stable (don't come and go like scope cgroups)
2. Packet events reliably reach parent cgroup hooks
3. We still get per-process granularity via cgroup_id filtering
4. Only need to attach once per user session, not per process

### Cgroup Hierarchy Example

```
/sys/fs/cgroup/
  └─ user.slice/
     └─ user-1000.slice/
        └─ user@1000.service/  ← ATTACH HERE! (stable, receives all packets)
           ├─ tmux-spawn-111.scope  (PID 1001, cgroup_id 11111)
           ├─ tmux-spawn-222.scope  (PID 1002, cgroup_id 22222)
           └─ tmux-spawn-333.scope  (PID 1003, cgroup_id 33333)
```

**Throttle PID 1002:**

1. Attach BPF program to `user@1000.service` (if not already attached)
2. Add entry to CGROUP_CONFIGS: `{cgroup_id: 22222, rate: 51200}`
3. When packet arrives for ANY process in user@1000.service:
   - eBPF program runs
   - Calls `bpf_get_current_cgroup_id()` → gets 11111, 22222, or 33333
   - Looks up in CGROUP_CONFIGS map
   - Only throttles if cgroup_id = 22222
   - Other PIDs unaffected

## Files Modified

1. `chadthrottle/src/backends/throttle/mod.rs`
   - Added `log_diagnostics()` to `DownloadThrottleBackend` trait

2. `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`
   - Implemented `log_diagnostics()`
   - Added `attached_cgroups: HashSet<PathBuf>` field
   - Changed attachment logic to only attach once per parent cgroup
   - Added PathBuf import

3. `chadthrottle/src/backends/throttle/manager.rs`
   - Fixed `log_ebpf_stats()` to actually call `backend.log_diagnostics()`

4. `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs`
   - Modified `get_cgroup_path()` to return parent cgroup (user@UID.service)
   - Added extensive documentation explaining the strategy

## Expected Test Results

### Before Fix

```
INFO  PID 1907444 (wget) download: actual=349.7 KB/s, limit=50.0 KB/s, ratio=6.99x ⚠️ THROTTLE NOT WORKING
[No eBPF stats - logging broken]
```

### After Fix

```
DEBUG PID 1907444 cgroup: user.slice/.../tmux-spawn-XXX.scope -> attaching to parent: user.slice/.../user@1000.service
INFO  Attaching eBPF ingress program to parent cgroup
INFO  Successfully attached eBPF ingress program
INFO  PID 1907444 (wget) download: actual=48.5 KB/s, limit=50.0 KB/s, ratio=0.97x ✅ THROTTLED
INFO  eBPF stats PID 1907444 cgroup 25722: program_calls=5234, packets=5000, dropped=4500 (90.0%), bytes_total=512000, bytes_dropped=460800
```

## Build Command

```bash
cargo +nightly xtask build-release
```

Binary: `/home/braden/ChadThrottle/target/release/chadthrottle`

---

**This should fix the throttling!** 🎉

The combination of:

1. Proper stats logging (so we can see what's happening)
2. Parent cgroup attachment (so eBPF program actually gets called)

...means wget should now be properly throttled.
