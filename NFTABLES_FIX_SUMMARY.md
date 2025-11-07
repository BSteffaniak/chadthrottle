# nftables Throttling Fix - Critical "over" Keyword

## The Problem

The nftables `limit` statement was created incorrectly:

### ❌ BROKEN (what we had):

```
socket cgroupv2 level 0 "path" limit rate 51200 bytes/second drop
```

**Why this didn't work:**

- `limit rate` without `over` means "match WITHIN the limit"
- Packets within limit: rule matches, but `drop` conflicts with `limit` (passes through)
- Packets over limit: `limit` fails, rule doesn't match (passes through)
- **Result: ALL packets pass through - NO THROTTLING!**

### ✅ FIXED (correct syntax):

```
socket cgroupv2 level 0 "path" limit rate over 51200 bytes/second drop
                                             ^^^^
```

**Why this works:**

- `limit rate over` means "match packets that EXCEED the limit"
- Packets within limit: rule doesn't match (passes through) ✓
- Packets over limit: rule matches, `drop` action executes ✓
- **Result: Throttling works!**

## What Changed

**File:** `src/backends/throttle/linux_nft_utils.rs`

**Added the `over` keyword to both v1 and v2 rule generation:**

```rust
CgroupBackendType::V2Nftables | CgroupBackendType::V2Ebpf => {
    format!(
        "socket cgroupv2 level 0 \"{}\" limit rate over {} bytes/second drop",
        //                                             ^^^^
        handle.identifier, rate_bytes_per_sec
    )
}
CgroupBackendType::V1 => {
    format!(
        "meta cgroup {} limit rate over {} bytes/second drop",
        //                            ^^^^
        handle.identifier, rate_bytes_per_sec
    )
}
```

## Testing Instructions

### 1. Start a download to throttle:

```bash
wget http://speedtest.tele2.net/100MB.zip -O /dev/null
```

### 2. Start ChadThrottle with nftables backend:

```bash
sudo ./target/release/chadthrottle
```

### 3. Throttle the wget process:

- Navigate to the wget process in the TUI
- Press 'd' to set download limit (e.g., 50 KB/s = 50000 bytes/sec)

### 4. Verify the correct rule was created:

```bash
sudo nft list ruleset | grep -A 2 chadthrottle
```

**Expected output:**

```
table inet chadthrottle {
    chain input_limit {
        socket cgroupv2 level 0 "chadthrottle/pid_XXXXX" limit rate over 51200 bytes/second drop
        #                                                             ^^^^
    }
}
```

**Key verification points:**

- ✅ Contains `limit rate over` (NOT just `limit rate`)
- ✅ Has `socket cgroupv2 level 0`
- ✅ Path is quoted
- ✅ Ends with `drop`

### 5. Verify bandwidth is actually limited:

Watch the wget download speed in its terminal - should be throttled to approximately your limit (e.g., ~50 KB/s).

### 6. Verify cgroup was created:

```bash
ls -la /sys/fs/cgroup/chadthrottle/
cat /sys/fs/cgroup/chadthrottle/pid_XXXXX/cgroup.procs
```

Should show the wget PID in cgroup.procs.

### 7. Test cleanup:

Exit chadthrottle (Ctrl+C or 'q') and verify:

```bash
sudo nft list ruleset | grep chadthrottle  # Should be empty
ls /sys/fs/cgroup/chadthrottle/            # Should be empty or not exist
```

## Summary of All Fixes Applied

1. ✅ **Import CgroupBackendType** - Added to use enum for type detection
2. ✅ **Fix rule generation** - Use `backend_type` instead of string parsing
3. ✅ **Fix rule removal** - Match based on backend type
4. ✅ **Silent cleanup errors** - Downgrade duplicate delete errors to DEBUG
5. ✅ **Add level parameter** - `socket cgroupv2 level 0`
6. ✅ **Add "over" keyword** - `limit rate over` to drop exceeding packets ← **Critical fix!**

## Build Status

✅ **Compiled successfully**

- Binary: `target/release/chadthrottle` (4.1M)
- Warnings: 49 (none critical)
- Errors: 0

## Expected Behavior

**Before all fixes:**

```
❌ Error: Could not parse integer (meta cgroup path)
❌ Error: unexpected quoted string, expecting level
❌ No throttling even when rules created
❌ Multiple cleanup errors
```

**After all fixes:**

```
✅ INFO: Selected cgroup backend: cgroup-v2-nftables (good)
✅ DEBUG: Created cgroup v2 for PID XXXXX
✅ DEBUG: Added nftables rate limit (backend: cgroup-v2-nftables)
✅ Throttling actually works!
✅ INFO: Cleaned up nftables table (once, no errors)
```

---

**The nftables backend should now work correctly on cgroup v2 systems!** 🚀
