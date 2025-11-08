# BPF Detach Fix - Summary

## Problem

eBPF programs were not being detached properly when throttles were removed or the application exited, causing programs to accumulate on cgroups.

### Root Cause

When using `BPF_F_ALLOW_MULTI` mode (which allows multiple BPF programs to be attached to the same cgroup), the `BPF_PROG_DETACH` syscall **requires** the program FD to be specified in the `attach_bpf_fd` field.

**Previous buggy code:**

```rust
let attr = bpf_attr_detach {
    target_fd: cgroup_fd_raw as u32,
    attach_bpf_fd: 0,  // ← BUG: Should specify which program to detach
    attach_type: bpf_attach_type,
    attach_flags: 0,
};
```

This caused the syscall to fail (return -1), but the code was returning `Ok()` anyway, assuming the failure meant "already detached". This led to program accumulation.

## The Fix

### Changes Made

#### 1. Updated `AttachedProgram` struct (download/ebpf.rs, upload/ebpf.rs)

Added `program_fd: i32` field to track the FD of each attached program:

```rust
struct AttachedProgram {
    cgroup_path: PathBuf,
    attach_type: CgroupSkbAttachType,
    program_fd: i32,  // NEW: Required for detaching with BPF_F_ALLOW_MULTI
}
```

#### 2. Updated `detach_cgroup_skb_legacy()` (linux_ebpf_utils.rs)

- Added `program_fd: i32` parameter
- Pass the program FD in `attach_bpf_fd` field
- Set `BPF_F_ALLOW_MULTI` flag (must match attach flags)
- Changed to return error on failure (instead of silently returning Ok)

```rust
pub fn detach_cgroup_skb_legacy(
    cgroup_path: &Path,
    attach_type: CgroupSkbAttachType,
    program_fd: i32,  // NEW parameter
) -> Result<()> {
    // ...
    const BPF_F_ALLOW_MULTI: u32 = 1 << 1;

    let attr = bpf_attr_detach {
        target_fd: cgroup_fd_raw as u32,
        attach_bpf_fd: program_fd as u32,  // FIX: Specify which program to detach
        attach_type: bpf_attach_type,
        attach_flags: BPF_F_ALLOW_MULTI,  // FIX: Must match attach flags
    };
    // ...
}
```

#### 3. Capture program FD when attaching (download/ebpf.rs, upload/ebpf.rs)

After calling `attach_cgroup_skb()`, we now get the program FD and store it:

```rust
let program_fd = {
    use std::os::fd::{AsFd, AsRawFd};
    let program: &CgroupSkb = ebpf
        .program("chadthrottle_ingress")
        .ok_or_else(|| anyhow::anyhow!("Program not found"))?
        .try_into()?;
    let prog_fd = program.fd()?;
    prog_fd.as_fd().as_raw_fd()
};

self.attached_programs.push(AttachedProgram {
    cgroup_path: cgroup_path.clone(),
    attach_type: CgroupSkbAttachType::Ingress,
    program_fd,  // Store the FD
});
```

#### 4. Pass program FD when detaching

Updated all calls to `detach_cgroup_skb_legacy()` to pass the program FD:

```rust
detach_cgroup_skb_legacy(
    &attached.cgroup_path,
    attached.attach_type,
    attached.program_fd,  // Pass the stored FD
)
```

## Files Modified

1. `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs`
   - Updated `detach_cgroup_skb_legacy()` signature and implementation
2. `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`
   - Updated `AttachedProgram` struct
   - Capture program FD when attaching
   - Pass program FD to detach in `remove_download_throttle()`
   - Pass program FD to detach in `cleanup()`

3. `chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`
   - Updated `AttachedProgram` struct
   - Capture program FD when attaching
   - Pass program FD to detach in `remove_upload_throttle()`
   - Pass program FD to detach in `cleanup()`

## Testing

To verify the fix works:

1. **Clean up any existing leftover programs:**

   ```fish
   set CGROUP "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/tmux-spawn-*.scope"
   for id in (sudo bpftool cgroup show "$CGROUP" | grep chadthrottle | awk '{print $1}')
       sudo bpftool cgroup detach "$CGROUP" ingress id $id
       sudo bpftool cgroup detach "$CGROUP" egress id $id
   end
   ```

2. **Run ChadThrottle and apply throttles:**

   ```bash
   sudo ./target/release/chadthrottle
   # Select a process, press 't', set limits, press Enter
   ```

3. **Remove throttle (press 'r'):**
   - The program should be detached immediately
   - Check with: `sudo bpftool cgroup show "$CGROUP" | grep chadthrottle`
   - Should show NO programs attached

4. **Quit the app (press 'q'):**
   - All programs should be detached during cleanup
   - Check with: `sudo bpftool cgroup show "$CGROUP" | grep chadthrottle`
   - Should show NO programs attached

## Expected Behavior After Fix

✅ Programs are properly detached when removing throttles (press 'r')
✅ Programs are properly detached on application exit
✅ No program accumulation
✅ Cleanup logs show successful detachment

## References

- Linux kernel BPF documentation: `BPF_PROG_DETACH` requires `attach_bpf_fd` when using `BPF_F_ALLOW_MULTI`
- Related issue: Programs were accumulating because detach syscall was failing silently
