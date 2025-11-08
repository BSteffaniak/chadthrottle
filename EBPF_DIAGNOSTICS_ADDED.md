# eBPF Throttling Diagnostics Added

## Summary

Added comprehensive diagnostics to identify why eBPF download throttling isn't working and fixed the annoying ifb_tc log spam.

## Changes Made

### 1. Enhanced ThrottleStats with Diagnostic Fields

**File:** `chadthrottle-common/src/lib.rs`

Added new fields to track eBPF program behavior:

- `program_calls` - Counts how many times the eBPF program runs
- `config_misses` - Counts lookups where cgroup not found in config map
- `cgroup_id_seen` - Shows the actual cgroup ID the eBPF program sees
- `_reserved` - Reserved for future use

### 2. Updated eBPF Ingress Program

**File:** `chadthrottle-ebpf/src/ingress.rs`

Modified to populate diagnostic fields:

- Increments `program_calls` on every invocation
- Stores actual `cgroup_id_seen` for verification
- Increments `config_misses` when cgroup not throttled
- Tracks stats even for non-throttled cgroups

### 3. Added BPF Stats Logging Method

**File:** `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`

New `log_throttle_stats()` method that:

- Reads stats from BPF maps
- Detects if eBPF program is being called
- Detects if eBPF is receiving packets
- Detects if eBPF is dropping packets
- Logs warnings for each failure mode
- Shows token bucket state

### 4. Added Bandwidth vs Throttle Logging

**File:** `chadthrottle/src/main.rs`

Added periodic logging (every 5 seconds) that shows:

- Actual download speed for throttled processes
- Configured throttle limit
- Ratio (actual/limit)
- Status: ✅ THROTTLED, ⚠️ OVER LIMIT, or ⚠️ THROTTLE NOT WORKING

Example output:

```
INFO PID 12345 (wget) download: actual=5.2 MB/s, limit=80 KB/s, ratio=65.00x ⚠️ THROTTLE NOT WORKING
```

### 5. Fixed ifb_tc Log Spam

**File:** `chadthrottle/src/backends/throttle/download/linux/ifb_tc.rs`

Changed from `log::debug!()` with multi-line message to:

- `log::trace!()` with single-line message
- Only visible with `RUST_LOG=trace`
- Prevents spam in normal DEBUG logging

### 6. Added human_readable() Helper

**File:** `chadthrottle/src/main.rs`

Helper function to format bytes (e.g., "1.5 MB", "500 KB")

## Testing Instructions

### Run with Enhanced Logging

```bash
cd /home/braden/ChadThrottle
sudo RUST_LOG=debug ./target/release/chadthrottle 2>&1 | tee /tmp/chadthrottle-diagnostics.log
```

### Test Scenario

1. Start ChadThrottle (command above)
2. In another terminal, start wget:
   ```bash
   wget http://speedtest.tele2.net/100MB.zip -O /dev/null
   ```
3. In ChadThrottle UI, throttle the wget process to 80 KB/s download
4. Watch the logs for diagnostic output

### Expected Log Output

**Every 5 seconds, you should see:**

```
INFO PID 12345 (wget) download: actual=X MB/s, limit=80 KB/s, ratio=Y.YYx [STATUS]
```

**If eBPF is NOT being called:**

```
WARN ⚠️  PID 12345 cgroup 25722: eBPF program NOT BEING CALLED! (program_calls=0, ...)
```

**If eBPF is called but no packets:**

```
WARN ⚠️  PID 12345 cgroup 25722: eBPF program called but NO PACKETS! (program_calls=123, ...)
```

**If eBPF sees packets but not dropping:**

```
WARN ⚠️  PID 12345 cgroup 25722: eBPF NOT DROPPING PACKETS! packets_total=5000, packets_dropped=0, ...
```

**If working correctly:**

```
INFO eBPF stats PID 12345 cgroup 25722: program_calls=5234, packets=5000, dropped=4500 (90.0%), ...
INFO PID 12345 (wget) download: actual=78.5 KB/s, limit=80 KB/s, ratio=0.98x ✅ THROTTLED
```

## Diagnostic Interpretation

### Scenario 1: "THROTTLE NOT WORKING" + "program NOT BEING CALLED"

**Problem:** eBPF program not attached or not being triggered

**Causes:**

- Program failed to attach to cgroup
- Wrong cgroup path
- BPF_CGROUP_INET_INGRESS not supported

**Fix:** Check if program attached successfully in earlier logs

### Scenario 2: "THROTTLE NOT WORKING" + "program called but NO PACKETS"

**Problem:** eBPF program runs but doesn't see wget's packets

**Causes:**

- wget in different cgroup than expected
- Packets bypass the cgroup hook
- Wrong cgroup ID in config map

**Fix:** Check `cgroup_id_seen` vs expected cgroup ID

### Scenario 3: "THROTTLE NOT WORKING" + "NOT DROPPING PACKETS"

**Problem:** eBPF sees packets but token bucket not working

**Causes:**

- Token bucket logic bug
- Token bucket always returns true (allow)
- Rate calculation error

**Fix:** Check token bucket implementation in `ingress.rs`

### Scenario 4: "THROTTLE NOT WORKING" + No eBPF logs at all

**Problem:** eBPF backend not being used

**Causes:**

- Backend not available
- Wrong backend selected
- Feature not enabled

**Fix:** Check backend status at startup

## Files Modified

- `chadthrottle-common/src/lib.rs` - Added diagnostic fields
- `chadthrottle-ebpf/src/ingress.rs` - Populate diagnostics
- `chadthrottle/src/backends/throttle/download/linux/ebpf.rs` - Stats logging
- `chadthrottle/src/backends/throttle/manager.rs` - Stats method stub
- `chadthrottle/src/backends/throttle/download/linux/ifb_tc.rs` - Fix log spam
- `chadthrottle/src/main.rs` - Bandwidth logging + human_readable()

## Next Steps

1. Run the test scenario above
2. Capture full logs to `/tmp/chadthrottle-diagnostics.log`
3. Search logs for the diagnostic patterns above
4. Share relevant log excerpts showing the failure mode
5. We'll use that to identify and fix the root cause

## Build Command

```bash
cargo +nightly xtask build-release
```

Binary location: `/home/braden/ChadThrottle/target/release/chadthrottle`
