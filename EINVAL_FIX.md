# eBPF EINVAL Fix - Expected Attach Type Mismatch

## Problem

eBPF program attachment was failing with:

```
ERROR Failed to attach program to cgroup: `bpf_link_create` failed (source: Invalid argument (os error 22)) [errno=22]
```

## Root Cause

**errno=22 (EINVAL)** - Invalid argument

The eBPF programs were compiled with the generic `#[cgroup_skb]` macro, which creates programs with **no expected_attach_type**. However, when attaching them to cgroups, we were specifying specific attach types (`Ingress` for download, `Egress` for upload).

The kernel's `bpf_link_create` syscall rejects this with EINVAL because:

- Program's `expected_attach_type` = `None` (from generic `#[cgroup_skb]`)
- Attachment `attach_type` = `BPF_CGROUP_INET_INGRESS` or `BPF_CGROUP_INET_EGRESS`
- These must match for cgroup_skb programs

## The Fix

### Changed Files

#### 1. `chadthrottle-ebpf/src/ingress.rs` (line 96)

```diff
-#[cgroup_skb]
+#[cgroup_skb(ingress)]
 pub fn chadthrottle_ingress(ctx: SkBuffContext) -> i32 {
```

**Effect:** Program now compiled with section name `cgroup_skb/ingress` and `expected_attach_type = BPF_CGROUP_INET_INGRESS`

#### 2. `chadthrottle-ebpf/src/egress.rs` (line 96)

```diff
-#[cgroup_skb]
+#[cgroup_skb(egress)]
 pub fn chadthrottle_egress(ctx: SkBuffContext) -> i32 {
```

**Effect:** Program now compiled with section name `cgroup_skb/egress` and `expected_attach_type = BPF_CGROUP_INET_EGRESS`

#### 3. Enhanced Error Logging (`linux_ebpf_utils.rs`)

Added detailed errno logging to help diagnose future attachment failures:

- Shows full error chain
- Displays errno value (e.g., errno=22)
- Logs cgroup path, attach type, and mode

#### 4. Fixed Misleading Log Message (`download/linux/ebpf.rs`)

Changed "parent cgroup" → "cgroup" to accurately reflect that we attach to the process's leaf cgroup.

## Verification

Check compiled eBPF object section names:

```bash
readelf -S target/bpfel-unknown-none/debug/chadthrottle-ingress | grep cgroup
# Shows: cgroup_skb/ingress

readelf -S target/bpfel-unknown-none/debug/chadthrottle-egress | grep cgroup
# Shows: cgroup_skb/egress
```

## Expected Result

With this fix:

1. ✅ Programs compile with correct expected_attach_type
2. ✅ Kernel accepts `bpf_link_create` with matching attach types
3. ✅ eBPF programs attach successfully to cgroups
4. ✅ Throttling actually works!

## Testing

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle
```

You should now see:

```
INFO  Attaching eBPF ingress program to cgroup (path: "...")
DEBUG Attached chadthrottle_ingress to "..." with mode: AllowMultiple
```

Instead of the previous EINVAL error.

## Common errno Values Reference

For future debugging:

- `errno=1` (EPERM) - Permission denied
- `errno=16` (EBUSY) - Resource busy
- `errno=22` (EINVAL) - Invalid argument ← **This was our issue**
- `errno=28` (ENOSPC) - No space (too many programs)
- `errno=95` (EOPNOTSUPP) - Operation not supported

## Related Documentation

- Aya eBPF macros: https://docs.rs/aya-ebpf-macros/latest/aya_ebpf_macros/
- Linux BPF cgroup attachment: https://docs.kernel.org/bpf/prog_cgroup_sysctl.html
- BPF_PROG_TYPE_CGROUP_SKB: https://docs.kernel.org/bpf/prog_cgroup_skb.html
