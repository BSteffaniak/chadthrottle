# BPF Cgroup Attachment Verification Steps

## Current Status

✅ eBPF program loads with correct `expected_attach_type: Some(Ingress)`  
✅ Cgroup file opens successfully  
❌ `bpf_link_create` syscall fails with **EINVAL (errno=22)**

## Possible Root Causes

Since the program metadata is correct, the EINVAL is likely caused by:

1. **Kernel configuration** - Missing BPF cgroup support
2. **Cgroup delegation** - User cgroups not properly delegated
3. **Kernel regression** - Bug in kernel 6.12.57's BPF subsystem
4. **Program structure** - Something wrong with the compiled BPF bytecode
5. **Conflicting programs** - Another BPF program blocking attachment

## Verification Steps

### Step 1: System Verification

Run the comprehensive verification script:

```bash
sudo ./verify_bpf_setup.sh
```

**What it checks:**

- ✓ Kernel version
- ✓ BPF kernel configuration (CONFIG_BPF, CONFIG_CGROUP_BPF, etc.)
- ✓ Cgroup v2 mount and setup
- ✓ User cgroup existence and permissions
- ✓ Existing BPF programs (conflicts?)
- ✓ Root privileges
- ✓ Potential conflicts (Docker, firewall, etc.)

**Look for:**

- Any `✗ FAIL` markers indicating missing kernel features
- Warnings about missing CONFIG\_\* options
- Existing BPF programs attached to user cgroups

---

### Step 2: Test with Root Cgroup

Try attaching to the root cgroup instead of user cgroups:

```bash
sudo CHADTHROTTLE_TEST_ROOT_CGROUP=1 RUST_LOG=debug ./target/release/chadthrottle 2>&1 | tee /tmp/root-cgroup-test.log
```

Then throttle a process via the TUI.

**Expected outcomes:**

**If it succeeds:**

```
WARN  🧪 TEST MODE: Using root cgroup "/sys/fs/cgroup"
DEBUG Program 'chadthrottle_ingress' loaded with expected_attach_type: Some(Ingress)
INFO  Successfully attached eBPF ingress program to "/sys/fs/cgroup"
```

→ **Conclusion:** User cgroup delegation issue. Solution: Attach to parent cgroups or enable delegation.

**If it fails:**

```
ERROR Failed to attach program to cgroup: `bpf_link_create` failed [errno=22]
Cgroup path: "/sys/fs/cgroup"
```

→ **Conclusion:** More fundamental issue (kernel, program structure, etc.)

---

### Step 3: Kernel Log Analysis

Check kernel logs for BPF-related errors:

```bash
# Check recent kernel messages
sudo dmesg | tail -100

# Look for BPF-specific errors
sudo dmesg | grep -i bpf

# Check for permission/capability errors
sudo dmesg | grep -i -E "bpf|capability|permission denied"
```

**Look for:**

- `bpf: Invalid argument` or similar errors
- Permission/capability denials
- BPF verifier errors
- Cgroup-related errors

---

### Step 4: Minimal BPF Test

Test if basic BPF cgroup attachment works on this kernel:

```bash
sudo ./test_minimal_bpf.sh
```

**What it does:**

- Creates a minimal cgroup_skb BPF program
- Compiles it with clang
- Tries to load and attach it with bpftool
- Tests attachment to root cgroup

**Expected outcomes:**

**If it works:**

```
✓ BPF program compiled
✓ Successfully attached to root cgroup!
```

→ **Conclusion:** Kernel supports cgroup BPF. Issue is specific to our program or Aya library.

**If it fails:**

```
✗ Failed to attach to root cgroup
```

→ **Conclusion:** Kernel BPF cgroup support issue. Check kernel config or version.

---

### Step 5: Check Existing BPF Programs

See if other programs are attached to the target cgroup:

```bash
# Install bpftool if not present
sudo apt install linux-tools-common linux-tools-$(uname -r)

# Check programs on user cgroup
sudo bpftool cgroup show /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/

# Check programs on root cgroup
sudo bpftool cgroup show /sys/fs/cgroup

# List all loaded BPF programs
sudo bpftool prog list | grep -i cgroup
```

**Look for:**

- `CGROUP_SKB` programs already attached
- Programs from systemd, Docker, or other tools
- Multiple programs on the same attach point

---

### Step 6: Manual Attachment Test with Aya

If the minimal test works, the issue is specific to our Aya-based program. Check:

1. **Aya version compatibility:**

   ```bash
   cargo tree | grep aya
   ```

   Currently using: `aya v0.13.1`

   Try downgrading to `aya v0.12.0` if needed.

2. **Program verification:**

   ```bash
   # Check our compiled program structure
   readelf -S target/bpfel-unknown-none/release/chadthrottle-ingress

   # Should show:
   # [ 3] cgroup_skb/ingress PROGBITS ...
   ```

3. **BPF verifier logs:**
   Enable BPF verifier logging in the code to see why the kernel rejects the program.

---

### Step 7: Kernel Configuration Deep Dive

If previous tests fail, check detailed kernel BPF configuration:

```bash
# Check all BPF-related configs
zgrep BPF /proc/config.gz 2>/dev/null || grep BPF /boot/config-$(uname -r)

# Critical configs that MUST be enabled:
# CONFIG_BPF=y
# CONFIG_BPF_SYSCALL=y
# CONFIG_CGROUP_BPF=y
# CONFIG_BPF_JIT=y (recommended)
```

**If any are missing:**

- Kernel doesn't support cgroup BPF
- Need to recompile kernel or use a different kernel

---

## Diagnostic Decision Tree

```
Start
  |
  +--> Run verify_bpf_setup.sh
         |
         +--> All checks pass?
               |
               +--> YES: System OK, proceed to Step 2
               |
               +--> NO: Fix missing kernel configs/features
                        └─> Recompile kernel or use different distro

  +--> Test with root cgroup (Step 2)
         |
         +--> Attachment succeeds?
               |
               +--> YES: User cgroup delegation issue
               |         └─> Attach to parent cgroups instead
               |
               +--> NO: Proceed to Step 3

  +--> Check kernel logs (Step 3)
         |
         +--> BPF errors found?
               |
               +--> YES: Investigate specific error
               |
               +--> NO: Proceed to Step 4

  +--> Minimal BPF test (Step 4)
         |
         +--> Works?
               |
               +--> YES: Issue is in our program/Aya
               |         └─> Check program structure, try older Aya
               |
               +--> NO: Kernel BPF support broken
                        └─> Check config, try different kernel
```

---

## Quick Diagnostic Commands

Run all these in sequence for a quick check:

```bash
# 1. Verify kernel BPF config
echo "=== Kernel BPF Config ==="
zgrep "CONFIG_BPF=\|CONFIG_CGROUP_BPF=" /proc/config.gz 2>/dev/null || \
  grep "CONFIG_BPF=\|CONFIG_CGROUP_BPF=" /boot/config-$(uname -r)

# 2. Check cgroup v2
echo -e "\n=== Cgroup v2 ==="
cat /sys/fs/cgroup/cgroup.controllers

# 3. Test root cgroup attachment
echo -e "\n=== Root Cgroup Test ==="
sudo CHADTHROTTLE_TEST_ROOT_CGROUP=1 timeout 5 ./target/release/chadthrottle 2>&1 | \
  grep -E "TEST MODE|expected_attach_type|Successfully attached|Failed to attach"

# 4. Check kernel logs
echo -e "\n=== Recent Kernel BPF Logs ==="
sudo dmesg | grep -i bpf | tail -20

# 5. List existing BPF programs
echo -e "\n=== Existing BPF Programs ==="
sudo bpftool prog list | grep -i cgroup | head -10
```

---

## Expected Resolution

Based on the diagnostic results:

### Scenario A: User Cgroup Issue

**Symptoms:** Root cgroup works, user cgroup fails  
**Solution:** Modify code to attach to parent cgroup or enable cgroup delegation

### Scenario B: Kernel Config Missing

**Symptoms:** verify*bpf_setup.sh shows missing CONFIG*\*  
**Solution:** Enable kernel features or use different kernel/distro

### Scenario C: Kernel Bug/Regression

**Symptoms:** Everything looks OK but still fails  
**Solution:** Try different kernel version (downgrade from 6.12.57)

### Scenario D: Aya Compatibility

**Symptoms:** Minimal test works, our program fails  
**Solution:** Try older Aya version or investigate program structure

### Scenario E: Program Structure Issue

**Symptoms:** Manual bpftool load works, Aya-based doesn't  
**Solution:** Enable BPF verifier logs, check what kernel rejects

---

## Next Steps

1. **Run:** `sudo ./verify_bpf_setup.sh` → Share output
2. **Run:** Root cgroup test → Report if it works
3. **Run:** `sudo dmesg | grep -i bpf` → Share any errors
4. **Provide:** Output of all quick diagnostic commands

This will pinpoint the exact cause of the EINVAL error.
