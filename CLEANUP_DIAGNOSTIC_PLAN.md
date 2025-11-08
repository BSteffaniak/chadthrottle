# Cleanup Diagnostic Plan

## Current Situation

- Throttles are being removed (BPF maps cleared)
- But programs might still be attached to cgroups
- Need to determine why detach isn't working

## Diagnostic Steps

### 1. Run with full logging

```bash
RUST_LOG=debug sudo -E ./target/release/chadthrottle
```

### 2. Throttle a process, then quit

Look for these log messages:

- "Detaching BPF program from cgroup: ..." (should show cgroup path, id, fd)
- "Calling bpf_prog_detach with: ..." (should show target_fd, program_fd, flags)
- "✅ Successfully detached..." OR "bpf_prog_detach failed: ..." with errno

### 3. Check what's actually attached

```bash
set CGROUP "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/tmux-spawn-*.scope"
sudo bpftool cgroup show "$CGROUP"
```

Note:

- Program IDs (first column)
- Program names (should be chadthrottle_in or chadthrottle_eg)
- Attach type (ingress vs egress)

### 4. Check program details

For each program ID:

```bash
sudo bpftool prog show id <ID>
```

Note the FD if shown.

### 5. Manual detach test

Try manually detaching with the program ID:

```bash
sudo bpftool cgroup detach "$CGROUP" ingress id <ID>
```

Does this work?

## Possible Issues

### Issue A: FD vs ID confusion

- We're passing program FD to BPF_PROG_DETACH
- But maybe it needs program ID instead?
- Program ID != Program FD

### Issue B: FD becomes invalid

- FD stored as integer when program attached
- FD might be closed before we try to detach
- Reused for something else

### Issue C: Wrong syscall parameters

- BPF_F_ALLOW_MULTI flag might not be right for detach
- attach_flags field might need different value

### Issue D: Cgroup path mismatch

- Stored cgroup_path might not match actual attached cgroup
- Process might have moved to different cgroup

## Next Steps Based on Findings

**If logs show "Successfully detached" but programs still attached:**

- Detach syscall returning success but not actually working
- Might need to use program ID instead of FD
- Or might need different flags

**If logs show "bpf_prog_detach failed":**

- Note the errno (e.g., ENOENT, EINVAL, EPERM)
- This tells us what's wrong

**If no detach attempts in logs:**

- Cleanup() might not be running
- Or attached_programs might be empty when it runs
