# eBPF Legacy Attach Cleanup Fix

## Summary

Implemented proper cleanup for the legacy `bpf_prog_attach` method to ensure BPF programs are properly detached when ChadThrottle exits.

## Problem

When using the legacy `bpf_prog_attach` syscall (fallback for systems where `bpf_link_create` fails), programs were not being detached on cleanup. This caused:

1. **Program Accumulation**: Each run of ChadThrottle attached a new BPF program instance without removing the old ones
2. **Resource Leaks**: Programs remained attached even after ChadThrottle exited
3. **Multiple Instances**: 9+ duplicate programs attached to the same cgroup (as observed in diagnostics)
4. **Map Confusion**: Each program instance had its own set of maps, causing throttling to fail

### Why Modern Method Auto-Cleans But Legacy Doesn't

- **`bpf_link_create` (modern)**: Creates a link FD that automatically detaches when the FD is closed
- **`bpf_prog_attach` (legacy)**: Directly attaches to cgroup, requires explicit `bpf_prog_detach` syscall

## Solution Implemented

### 1. Added `detach_cgroup_skb_legacy()` Function

**File**: `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs`

Implemented the `BPF_PROG_DETACH` syscall (command 9) to explicitly detach programs:

```rust
pub fn detach_cgroup_skb_legacy(
    cgroup_path: &Path,
    attach_type: CgroupSkbAttachType,
) -> Result<()>
```

This mirrors the `attach_cgroup_skb_legacy()` function but calls `BPF_PROG_DETACH` instead of `BPF_PROG_ATTACH`.

### 2. Added Attachment Tracking

**Files**:

- `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`
- `chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`

Added tracking structures to remember which programs were attached:

```rust
#[derive(Debug, Clone)]
struct AttachedProgram {
    cgroup_path: PathBuf,
    attach_type: CgroupSkbAttachType,
}

pub struct EbpfDownload {
    // ... existing fields ...
    attached_programs: Vec<AttachedProgram>,
}
```

### 3. Store Attachments When Attaching

When a program is successfully attached, we now store it:

```rust
self.attached_programs.push(AttachedProgram {
    cgroup_path: cgroup_path.clone(),
    attach_type: CgroupSkbAttachType::Ingress, // or Egress
});
```

### 4. Proper Cleanup Implementation

Modified the `cleanup()` function to explicitly detach all programs:

```rust
fn cleanup(&mut self) -> Result<()> {
    // Remove throttles (clear maps)
    for pid in pids {
        let _ = self.remove_download_throttle(pid);
    }

    // Detach all attached programs (CRITICAL for legacy attach method)
    for attached in &self.attached_programs {
        detach_cgroup_skb_legacy(&attached.cgroup_path, attached.attach_type)?;
    }
    self.attached_programs.clear();

    // Drop the eBPF instance
    self.ebpf = None;
    // ...
}
```

## Changes Made

### Modified Files

1. **`chadthrottle/src/backends/throttle/linux_ebpf_utils.rs`**
   - Added `detach_cgroup_skb_legacy()` function
   - Implements `BPF_PROG_DETACH` syscall

2. **`chadthrottle/src/backends/throttle/download/linux/ebpf.rs`**
   - Added `AttachedProgram` struct
   - Added `attached_programs: Vec<AttachedProgram>` field
   - Modified attachment code to track programs
   - Modified `cleanup()` to detach all programs

3. **`chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`**
   - Same changes as download backend
   - Applied to egress (upload) programs

## Testing

### Manual Test

First, clean up existing leftover programs:

```bash
# List programs attached to your cgroup
sudo bpftool cgroup show /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/tmux-spawn-*.scope

# Detach each one (replace IDs with actual ones)
for id in 1577 1604 1612 1632 1640 1648 1656 1664 1672; do
    sudo bpftool cgroup detach /sys/fs/cgroup/... ingress id $id
done
```

### Automated Test

Run the provided test script:

```bash
./test_cleanup.sh
```

This script:

1. Cleans up any existing programs
2. Runs ChadThrottle with wget for 10 seconds
3. Verifies all programs are detached after exit
4. Returns exit code 0 if successful, 1 if programs leaked

### Expected Results

**Before the fix**:

```bash
$ sudo bpftool cgroup show <path>
ID       AttachType      AttachFlags     Name
1577     cgroup_inet_ingress multi       chadthrottle_in
1604     cgroup_inet_ingress multi       chadthrottle_in
1612     cgroup_inet_ingress multi       chadthrottle_in
...  (9+ programs!)
```

**After the fix**:

```bash
$ sudo bpftool cgroup show <path>
(no output - all programs properly detached)
```

## Compatibility

This fix ensures ChadThrottle properly cleans up on ANY system, regardless of whether:

- Modern `bpf_link_create` is supported
- Legacy `bpf_prog_attach` fallback is used
- Kernel version (as long as cgroup BPF is supported)

The cleanup now works correctly in both paths:

- **Modern path**: Link FDs auto-detach (existing behavior, unchanged)
- **Legacy path**: Explicit `bpf_prog_detach` calls (NEW, now working)

## Next Steps

With proper cleanup in place, the next issue to investigate is why the BPF programs aren't executing (no `run_cnt`, no stats). This is a separate issue from the cleanup problem and may be related to:

1. Traffic not flowing through the cgroup ingress hook
2. `BPF_CGROUP_INET_INGRESS` limitations on certain network configurations
3. Kernel version compatibility issues

But now at least we won't accumulate zombie programs!

## References

- Linux kernel BPF syscall documentation: https://man7.org/linux/man-pages/man2/bpf.2.html
- `BPF_PROG_ATTACH` command: 8
- `BPF_PROG_DETACH` command: 9
- Both use the same `bpf_attr` struct layout
