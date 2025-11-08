# Diagnostic Tests for EINVAL Issue

## What We Added

### 1. Enhanced Error Logging

- Shows program's `expected_attach_type` after loading
- Logs cgroup file opening success/failure
- Shows whether file was opened successfully

### 2. File Opening Fix

Changed from `File::open()` (read-only, O_RDONLY) to:

```rust
OpenOptions::new().read(true).write(true).open()  // O_RDWR
```

This might fix EINVAL if BPF attachment requires write access to the cgroup.

### 3. Root Cgroup Test Mode

Added `CHADTHROTTLE_TEST_ROOT_CGROUP` environment variable to test if the issue is specific to user cgroups.

## How to Test

### Test 1: Normal Mode (Enhanced Logging)

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle
```

**Look for these log lines when you throttle a process:**

```
DEBUG Program 'chadthrottle_ingress' loaded with expected_attach_type: Some(Ingress)
DEBUG Opened cgroup file: "/sys/fs/cgroup/..."
```

**Expected outcomes:**

- ✅ If `expected_attach_type: Some(Ingress)` → Program has correct attach type
- ✅ If "Opened cgroup file" → File opened successfully
- ✅ If no "Failed to open" error → O_RDWR fix worked
- ❌ If still get EINVAL → Issue is something else

### Test 2: Root Cgroup Test

```bash
sudo CHADTHROTTLE_TEST_ROOT_CGROUP=1 RUST_LOG=debug ./target/release/chadthrottle
```

**What this does:**

- Forces ALL processes to use root cgroup (`/sys/fs/cgroup`) instead of their actual cgroup
- This tests if the issue is specific to user-owned cgroups

**Look for:**

```
WARN  🧪 TEST MODE: Using root cgroup "/sys/fs/cgroup" instead of process cgroup
INFO  Attaching eBPF ingress program to cgroup (path: "/sys/fs/cgroup")
```

**Expected outcomes:**

- ✅ If attachment SUCCEEDS → Issue was user cgroup delegation/ownership
- ❌ If still fails with EINVAL → Issue is more fundamental

## What Each Outcome Means

### If Test 1 shows `expected_attach_type: None`

**Problem:** Section name not being parsed correctly by Aya
**Solution:** Check Aya version compatibility or use different section name format

### If Test 1 shows "Failed to open cgroup: Permission denied"

**Problem:** File permissions issue
**Solution:** Check cgroup ownership and permissions

### If Test 1 opens file OK but still EINVAL

**Problem:** BPF syscall parameter issue
**Solution:** Investigate kernel compatibility or BPF program structure

### If Test 2 succeeds but Test 1 fails

**Problem:** User cgroup delegation/ownership
**Solution:** Use a different cgroup strategy or enable proper delegation

### If both tests fail with EINVAL

**Problem:** Fundamental compatibility issue
**Solutions:**

- Check kernel BPF support: `zgrep BPF /proc/config.gz`
- Try a different Aya version
- Check if cgroup v2 is properly mounted
- Verify BPF programs compile correctly

## Run the Tests

Start chadthrottle with the appropriate mode, then use the TUI to throttle a process (like wget or sleep).

The detailed logs will show exactly where the failure occurs.
