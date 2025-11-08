# EINVAL Diagnostic Enhancements - Session Summary

## Problem

errno=22 (EINVAL) persists when attaching eBPF cgroup programs, even after fixing the expected_attach_type issue.

## Root Cause Hypotheses

Based on the EINVAL error from `bpf_link_create`, we identified 4 potential causes:

1. **Program's expected_attach_type not being set correctly** - Even with correct section names, metadata might not be parsed
2. **Cgroup delegation/ownership issues** - User-owned cgroups (UID 1000) might have delegation restrictions
3. **File descriptor flags issue** - Opening cgroup with O_RDONLY instead of O_RDWR
4. **Kernel compatibility** - BPF subsystem changes in kernel 6.12.57

## Changes Made

### 1. Enhanced Program Inspection Logging (`linux_ebpf_utils.rs:141-146`)

Added logging to show the program's `expected_attach_type` after loading:

```rust
// Log the program's expected_attach_type after loading
log::debug!(
    "Program '{}' loaded with expected_attach_type: {:?}",
    program_name,
    program.expected_attach_type()
);
```

**Purpose:** Verify that the eBPF program's metadata is correctly parsed from the ELF section name.

**What to look for:**

- ✅ `expected_attach_type: Some(Ingress)` or `Some(Egress)` = Correct
- ❌ `expected_attach_type: None` = Section name not parsed correctly

### 2. Fixed Cgroup File Opening Mode (`linux_ebpf_utils.rs:148-156`)

Changed from read-only to read+write:

```rust
// Before:
let cgroup_file = fs::File::open(cgroup_path)?;

// After:
let cgroup_file = fs::OpenOptions::new()
    .read(true)
    .write(true)  // O_RDWR instead of O_RDONLY
    .open(cgroup_path)?;
```

**Purpose:** BPF attachment might require write permissions on the cgroup directory.

**Why this matters:** Some kernel operations require O_RDWR even when they conceptually only "read" from the file.

### 3. Root Cgroup Test Mode (`linux_ebpf_utils.rs`)

Added environment variable `CHADTHROTTLE_TEST_ROOT_CGROUP` to bypass user cgroups:

**In `get_cgroup_id()`:**

```rust
if std::env::var("CHADTHROTTLE_TEST_ROOT_CGROUP").is_ok() {
    log::warn!("🧪 TEST MODE: Using root cgroup ID (1) for all processes");
    return Ok(1);
}
```

**In `get_cgroup_path()`:**

```rust
if std::env::var("CHADTHROTTLE_TEST_ROOT_CGROUP").is_ok() {
    let root_cgroup = PathBuf::from("/sys/fs/cgroup");
    log::warn!("🧪 TEST MODE: Using root cgroup {:?} instead of process cgroup", root_cgroup);
    return Ok(root_cgroup);
}
```

**Purpose:** Test if the issue is specific to user-owned cgroups vs root-owned cgroups.

**Why this matters:** In cgroup v2, delegation from systemd to user sessions might have restrictions that prevent root from attaching BPF programs to user-owned cgroups without explicit delegation setup.

## How to Use

### Normal Test (with enhanced logging):

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle
```

Then throttle a process via the TUI.

### Root Cgroup Test:

```bash
sudo CHADTHROTTLE_TEST_ROOT_CGROUP=1 RUST_LOG=debug ./target/release/chadthrottle
```

Then throttle a process via the TUI.

## Expected Test Results

### Scenario 1: O_RDWR Fix Solves It ✅

```
DEBUG Program 'chadthrottle_ingress' loaded with expected_attach_type: Some(Ingress)
DEBUG Opened cgroup file: "/sys/fs/cgroup/user.slice/..."
DEBUG Attached chadthrottle_ingress to "..." with mode: AllowMultiple
INFO  Successfully attached eBPF ingress program to "..."
```

**Conclusion:** File permissions were the issue. Fix is complete!

### Scenario 2: expected_attach_type is None ❌

```
DEBUG Program 'chadthrottle_ingress' loaded with expected_attach_type: None
ERROR Failed to attach program to cgroup: ... [errno=22]
```

**Conclusion:** Section name not being parsed. Need to investigate Aya compatibility or use different section format.

### Scenario 3: User Cgroup Fails, Root Cgroup Succeeds

**Normal mode:**

```
DEBUG Opened cgroup file: "/sys/fs/cgroup/user.slice/..."
ERROR Failed to attach program to cgroup: ... [errno=22]
```

**Test mode:**

```
WARN  🧪 TEST MODE: Using root cgroup "/sys/fs/cgroup"
DEBUG Opened cgroup file: "/sys/fs/cgroup"
INFO  Successfully attached eBPF ingress program to "/sys/fs/cgroup"
```

**Conclusion:** User cgroup delegation issue. Need to either:

- Enable cgroup delegation in systemd
- Use a different cgroup attachment strategy
- Attach to parent cgroups instead of leaf cgroups

### Scenario 4: Both Fail ❌

Both normal and test mode fail with EINVAL.

**Conclusion:** More fundamental issue. Investigate:

- Kernel BPF configuration
- Aya library version compatibility
- BPF program structure
- Kernel version regression

## Files Modified

1. `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs`
   - Added expected_attach_type logging
   - Changed file opening to O_RDWR
   - Added root cgroup test mode

2. `DIAGNOSTIC_TESTS.md` - User-facing test instructions

3. `EINVAL_DIAGNOSTICS_ADDED.md` - This document

## Next Steps

1. **Run Test 1** (normal mode) and capture the full output
2. **Check the logs** for `expected_attach_type` value
3. **If still EINVAL**, run Test 2 (root cgroup mode)
4. **Analyze results** based on the scenarios above
5. **Report findings** - the detailed logs will pinpoint the exact issue

## Related Documents

- `EINVAL_FIX.md` - Initial fix attempt (attach type specification)
- `EBPF_THROTTLING_FIX.md` - Earlier debugging session
- `BPF_ALLOW_MULTI_FIX.md` - AllowMultiple mode fix
- `DIAGNOSTIC_TESTS.md` - Test instructions for users
