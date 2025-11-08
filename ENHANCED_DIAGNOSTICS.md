# Enhanced BPF Attachment Diagnostics

## What Was Added

Added comprehensive logging to diagnose why `bpf_link_create` returns EINVAL:

### 1. Program FD Validation

```rust
// Verifies the program is loaded and has a valid file descriptor
match program.fd() {
    Ok(prog_fd) => log::debug!("Program FD obtained: valid (raw fd: {})", raw_fd),
    Err(e) => return Err("Program FD invalid"),
}
```

### 2. Cgroup FD Logging

```rust
log::debug!("Opened cgroup file: {:?} (fd: {})", cgroup_path, fd);
```

### 3. Attachment Parameters

```rust
log::debug!("Attempting to attach with: attach_type={:?}, mode=AllowMultiple, cgroup_fd={}", ...);
```

### 4. Immediate Result Logging

```rust
match &attach_result {
    Ok(link_id) => log::debug!("Attachment succeeded! Link ID: {:?}", link_id),
    Err(e) => log::error!("Attachment failed immediately: {}", e),
}
```

## Key Discovery from bpftool

Systemd already has cgroup_skb programs attached:

```bash
$ sudo bpftool cgroup show /sys/fs/cgroup
ID       AttachType      AttachFlags     Name
1526     cgroup_inet_ingress multi           sd_fw_ingress
1525     cgroup_inet_egress multi           sd_fw_egress
```

**Critical:** Systemd IS using `multi` (AllowMultiple) flag, so that's not blocking us!

## What to Look For

When you run the new binary and throttle a process, look for:

### If Program FD is Invalid:

```
ERROR Failed to get program FD: ...
```

→ **Problem:** Program didn't load into kernel properly

### If Attachment Parameters Look Wrong:

```
DEBUG Attempting to attach with: attach_type=Ingress, mode=AllowMultiple, cgroup_fd=X
```

Compare the `cgroup_fd` value - should be > 0

### If Attachment Fails Immediately:

```
ERROR Attachment failed immediately: ...
ERROR Failed to attach program to cgroup: `bpf_link_create` failed [errno=22]
```

Look at what comes BEFORE this - did we get a valid program FD? Valid cgroup FD?

## Testing

```bash
sudo ./test_with_diagnostics.sh
```

Then throttle a process via the TUI.

## Expected Log Sequence

**If everything is set up correctly:**

```
DEBUG Program 'chadthrottle_ingress' loaded with expected_attach_type: Some(Ingress)
DEBUG Program FD obtained: valid (raw fd: 7)
DEBUG Opened cgroup file: "/sys/fs/cgroup/..." (fd: 8)
DEBUG Attempting to attach with: attach_type=Ingress, mode=AllowMultiple, cgroup_fd=8
```

Then either:

- ✅ `DEBUG Attachment succeeded! Link ID: ...`
- ❌ `ERROR Attachment failed immediately: ...`

## Next Steps Based on Results

### Scenario A: Program FD is Invalid

- Program not loading correctly into kernel
- Check BPF verifier logs
- May need different Aya version

### Scenario B: All FDs Valid, Still EINVAL

- Something wrong with how Aya calls `bpf_link_create`
- Aya version bug with kernel 6.12.57
- Try legacy attach method (prog_attach instead of link_create)

### Scenario C: Attachment Succeeds!

- Problem was intermittent or fixed by rebuild
- Document what changed

## Files Modified

- `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs`
  - Added `AsRawFd` import
  - Added `AsFd` import for ProgramFd
  - Added program FD validation with logging
  - Added cgroup FD logging
  - Added attachment parameter logging
  - Added immediate result logging

## Testing Script

Created `test_with_diagnostics.sh` for easy testing.

## Hypothesis

Given that:

1. Systemd's programs work fine with `multi` flag
2. Kernel config is correct
3. We're using AllowMultiple mode
4. Root cgroup test also fails

The issue is likely:

- **Aya library bug** with how it calls `bpf_link_create` on kernel 6.12.57
- **Program FD issue** - program not fully loaded before attach attempt
- **Syscall parameter mismatch** - Aya passing wrong values

The new diagnostics will reveal which one it is!
