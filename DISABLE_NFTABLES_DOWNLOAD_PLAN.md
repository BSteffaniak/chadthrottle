# Plan: Disable nftables Download Backend

## Overview

Since nftables `socket cgroupv2` cannot work on INPUT chain (kernel limitation), we need to disable the nftables download backend and guide users to use alternatives.

## Changes Required

### 1. Disable nftables Download Backend

**File:** `src/backends/throttle/download/linux/nftables.rs`

**Change in `is_available()` method:**

```rust
fn is_available() -> bool {
    // nftables with socket cgroupv2 only works on OUTPUT chain
    // Cannot throttle download (INPUT chain) due to kernel limitation
    // Use ifb_tc or tc_police backends for download throttling
    false
}
```

**Alternative: Add helpful message:**

```rust
fn is_available() -> bool {
    false  // See unavailable_reason()
}

fn unavailable_reason(&self) -> String {
    "nftables download throttling not supported: socket cgroupv2 only works on OUTPUT chain.\n\
     Use ifb_tc (recommended) or tc_police backends for download throttling.\n\
     nftables upload throttling works fine.".to_string()
}
```

### 2. Keep nftables Upload Backend (Should Work)

**File:** `src/backends/throttle/upload/linux/nftables.rs`

**No changes needed** - upload uses OUTPUT chain where `socket cgroupv2` works.

### 3. Update Documentation

**Add to comments in `download/linux/nftables.rs`:**

```rust
//! nftables Download Throttling Backend
//!
//! # LIMITATION - DISABLED
//!
//! This backend is DISABLED because nftables `socket cgroupv2` expression
//! only works on OUTPUT chain, not INPUT chain. This is a kernel/netfilter
//! limitation.
//!
//! Download (ingress) traffic arrives on INPUT chain BEFORE socket association,
//! so the kernel cannot determine which cgroup the packet belongs to at INPUT
//! hook time.
//!
//! ## Alternatives for Download Throttling:
//!
//! 1. **ifb_tc** (Recommended) - Uses IFB device to redirect ingress to egress
//! 2. **tc_police** - Simple per-interface rate limiting (no per-process)
//! 3. **eBPF TC** (Future) - TC classifier with BPF_PROG_TYPE_SCHED_CLS
//!
//! ## Why Upload Works:
//!
//! Upload traffic uses OUTPUT chain where socket association is already known,
//! so `socket cgroupv2` matching works correctly.
```

### 4. Verify IFB Module Availability

**Check if user has IFB support:**

User can run:

```bash
# Try to load IFB module
sudo modprobe ifb numifbs=1

# Check if it loaded
lsmod | grep ifb

# Or check if IFB devices can be created
sudo ip link add name ifb_test type ifb
sudo ip link del ifb_test
```

If `modprobe ifb` fails or `ip link add type ifb` fails, IFB is not available.

**Fallback options if IFB not available:**

1. **tc_police** - Works without IFB but NO per-process support
2. Compile kernel with IFB support
3. Use different kernel

### 5. Update README/Documentation

**Add section explaining backend capabilities:**

```markdown
## Throttling Backend Capabilities

### Download (Ingress) Throttling:

| Backend   | Per-Process | Cgroup v1 | Cgroup v2  | Notes                                 |
| --------- | ----------- | --------- | ---------- | ------------------------------------- |
| ifb_tc    | ✅ Yes      | ✅ Yes    | 🔄 Partial | Requires IFB kernel module            |
| tc_police | ❌ No       | N/A       | N/A        | Per-interface only, no cgroups        |
| nftables  | ❌ Disabled | ❌ No     | ❌ No      | socket cgroupv2 doesn't work on INPUT |
| ebpf      | 🔄 Future   | ❌ No     | 🔄 Planned | Needs TC classifier implementation    |

### Upload (Egress) Throttling:

| Backend  | Per-Process | Cgroup v1   | Cgroup v2  | Notes                           |
| -------- | ----------- | ----------- | ---------- | ------------------------------- |
| tc_htb   | ✅ Yes      | ✅ Yes      | 🔄 Partial | Default, widely available       |
| nftables | ✅ Yes      | 🔄 Untested | ✅ Yes     | Modern, works with cgroup v2    |
| ebpf     | 🔄 Future   | ❌ No       | 🔄 Planned | Best performance when available |

**Recommended combinations:**

- **Cgroup v2 systems:** `--upload-backend nftables --download-backend ifb_tc`
- **Cgroup v1 systems:** `--upload-backend tc_htb --download-backend ifb_tc`
- **No IFB module:** `--upload-backend tc_htb --download-backend tc_police` (no per-process download)
```

## Testing After Changes

### 1. Verify nftables download is disabled:

```bash
sudo ./target/release/chadthrottle --list-backends
```

**Expected output:**

```
Download Backends:
  ifb_tc              [priority: Good] ✅ available
  tc_police           [priority: Fallback] ✅ available
  nftables_download   [priority: Better] ❌ unavailable

Upload Backends:
  tc_htb              [priority: Good] ✅ available
  nftables_upload     [priority: Better] ✅ available
```

### 2. Test that ifb_tc works for download:

```bash
# Start download
wget http://speedtest.tele2.net/100MB.zip -O /dev/null

# Throttle with ifb_tc
sudo ./target/release/chadthrottle --download-backend ifb_tc

# Throttle the wget process (press 'd')
# Should throttle correctly ✅
```

### 3. Test that nftables works for upload:

```bash
# Start upload (need a server)
scp /tmp/largefile user@server:/tmp/

# Throttle with nftables
sudo ./target/release/chadthrottle --upload-backend nftables

# Throttle the scp process (press 'u')
# Should throttle correctly ✅
```

## Summary of Changes

**Files to modify:**

1. ✅ `src/backends/throttle/download/linux/nftables.rs` - Set `is_available() = false`
2. ✅ Add documentation comments explaining why
3. ✅ Optional: Add `unavailable_reason()` with helpful message
4. ✅ Update README with backend capability matrix

**Files NOT modified:**

- `src/backends/throttle/upload/linux/nftables.rs` - Keep enabled, should work
- All other backends - No changes needed

**Testing required:**

- ✅ Verify nftables download shows as unavailable
- ✅ Verify ifb_tc download works (if IFB module available)
- ✅ Verify nftables upload works (future test)

## IFB Module Check Commands

User should run:

```bash
# Check if IFB module exists
modinfo ifb

# Try to load it
sudo modprobe ifb numifbs=1

# Verify it's loaded
lsmod | grep ifb

# Test device creation
sudo ip link add name ifb_test type ifb && echo "IFB works!" && sudo ip link del ifb_test
```

If all succeed: ✅ IFB available, ifb_tc will work
If any fail: ❌ IFB not available, must use tc_police (no per-process)
