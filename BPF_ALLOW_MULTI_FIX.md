# eBPF Attachment Mode Fix - BPF_F_ALLOW_MULTI

## Critical Bug Found

The previous fix (parent cgroup attachment) was correct, but the eBPF program still wasn't running because of **attachment mode**.

### The Smoking Gun

```
WARN  ⚠️  PID 1955888 cgroup 25722: No stats in BPF map (eBPF not initialized?)
```

The eBPF program attached successfully, but the `CGROUP_STATS` map remained empty - meaning **the program never executed even once**.

## Root Cause

**File:** `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs:174`

```rust
program.attach(&cgroup_file, attach_type, CgroupAttachMode::Single)
```

**Problem:** `CgroupAttachMode::Single` means:

- Only ONE BPF program can be attached to the cgroup
- If another program is already attached, ours is **silently ignored**
- Systemd, docker, or other tools often pre-attach BPF programs to user cgroups

**Result:** Our program attached to the kernel data structures but **never actually ran** because another program had priority.

## The Fix

Changed attachment mode from `Single` to `AllowMultiple`:

```rust
program.attach(&cgroup_file, attach_type, CgroupAttachMode::AllowMultiple)
```

**What this does:**

- Sets the `BPF_F_ALLOW_MULTI` flag
- Allows **multiple BPF programs** to be attached to the same cgroup
- Programs execute in **stack order** (all programs run, not just first)
- Our throttling program will run **even if systemd has other programs attached**

## Why This Was Silent

The `attach()` call **succeeded** but the program was placed in a "dormant" state:

- Kernel accepted the attachment
- But didn't execute it because Single mode was in conflict
- No error returned (this is a kernel behavior)
- Stats maps never populated because code never ran

This is why we saw:

- ✅ "Successfully attached eBPF ingress program"
- ❌ No stats in BPF map
- ❌ No throttling

## Expected Behavior Now

### Before Fix (AllowMultiple)

```
INFO  Successfully attached eBPF ingress program
WARN  ⚠️  No stats in BPF map (eBPF not initialized?)
INFO  PID 1955888 (wget) download: actual=342.7 KB/s, limit=50.0 KB/s, ratio=6.85x ⚠️ THROTTLE NOT WORKING
```

### After Fix (AllowMultiple)

```
INFO  Successfully attached eBPF ingress program
DEBUG Attached chadthrottle_ingress with mode: AllowMultiple (BPF_F_ALLOW_MULTI - allows coexistence)
INFO  PID 1955888 (wget) download: actual=48.5 KB/s, limit=50.0 KB/s, ratio=0.97x ✅ THROTTLED
INFO  eBPF stats: program_calls=5234, packets=5000, dropped=4500 (90.0%), ...
```

## Technical Details

### BPF Program Attachment Modes

| Mode            | Flag                   | Behavior                   | Use Case                               |
| --------------- | ---------------------- | -------------------------- | -------------------------------------- |
| `Single`        | 0                      | Only one program allowed   | Exclusive control (rare)               |
| `AllowOverride` | `BPF_F_ALLOW_OVERRIDE` | Child cgroups can override | Hierarchical policies                  |
| `AllowMultiple` | `BPF_F_ALLOW_MULTI`    | Multiple programs stack    | **Most common** (this is what we need) |

### Why AllowMultiple is Standard

Modern Linux systems have **many** BPF programs attached to cgroups:

- **systemd** - Resource accounting, monitoring
- **Docker/Podman** - Container networking
- **Kubernetes** - Network policies
- **Security tools** - AppArmor, SELinux extensions
- **Monitoring** - Performance tools, observability

Using `Single` mode means our program **conflicts** with all of these and gets silently disabled.

Using `AllowMultiple` means our program **coexists** and runs alongside them.

### Program Execution Order

With `AllowMultiple`, all attached programs run in **attachment order**:

1. Systemd's programs (attached at boot)
2. Docker's programs (attached when containers start)
3. **Our throttling program** (attached when we throttle)
4. Other tools

Each program can:

- Allow packet (return 1) → next program runs
- Drop packet (return 0) → packet dropped, no further programs run

Our program drops packets when over limit, so throttling works correctly.

## Testing

Run the same test as before:

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle 2>&1 | tee /tmp/chadthrottle-multi-test.log
```

Then throttle wget. You should now see:

1. **Attachment log includes mode:**

   ```
   DEBUG Attached chadthrottle_ingress to "..." with mode: AllowMultiple (BPF_F_ALLOW_MULTI)
   ```

2. **Stats map is populated:**

   ```
   INFO eBPF stats PID X cgroup Y: program_calls=1234, packets=1000, dropped=900, ...
   ```

3. **Throttling works:**
   ```
   INFO PID X (wget) download: actual=48 KB/s, limit=50 KB/s, ratio=0.96x ✅ THROTTLED
   ```

## Why Previous Tests Failed

### Test 1: First attempt (leaf cgroup)

- ❌ Wrong cgroup (leaf scope, packets not routed there)
- ❌ Single mode (would have failed anyway)

### Test 2: Parent cgroup fix

- ✅ Right cgroup (user@1000.service receives packets)
- ❌ Single mode (conflicted with systemd, silently disabled)

### Test 3: This fix (parent + AllowMultiple)

- ✅ Right cgroup
- ✅ AllowMultiple mode
- ✅ **Should work!**

## Files Changed

**Single file modified:**

- `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs`
  - Line 177: `CgroupAttachMode::Single` → `CgroupAttachMode::AllowMultiple`
  - Added debug logging showing attachment mode

## Alternative Diagnostic

If this STILL doesn't work, we can check what programs are attached:

```bash
# List all BPF programs (if bpftool exists)
sudo bpftool prog list | grep cgroup

# Or check cgroup.procs
cat /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/cgroup.procs

# Check if wget is in the cgroup
ps aux | grep wget
cat /proc/<wget_pid>/cgroup
```

## Next Steps if Still Broken

If `AllowMultiple` still doesn't work, there are two remaining possibilities:

1. **Kernel doesn't support BPF_F_ALLOW_MULTI** (very old kernel)
   - Check: `uname -r` (need 4.15+ for reliable multi-attach)

2. **Packets genuinely not reaching cgroup hook** (routing issue)
   - This would be a fundamental kernel issue
   - Fall back to TC eBPF classifier approach

But `AllowMultiple` should fix it - this is a **very common issue** with BPF programs on modern systems.

## Build Command

```bash
cargo +nightly xtask build-release
```

Binary: `/home/braden/ChadThrottle/target/release/chadthrottle`

---

**This SHOULD finally work!** The combination of:

1. Parent cgroup attachment (user@1000.service)
2. AllowMultiple mode (BPF_F_ALLOW_MULTI)
3. Proper stats logging

...means we'll either see throttling work, or get clear diagnostics showing exactly what's wrong.
