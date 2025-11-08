# eBPF Section Name Fix - Root Cause Resolved

## Problem Discovered

During diagnostic testing, we found:

```
DEBUG Program 'chadthrottle_ingress' loaded with expected_attach_type: None
```

Even though we had changed the eBPF macros to `#[cgroup_skb(ingress)]` and verified the section name in the source binaries, the **embedded** binaries still had the old section name!

## Root Cause

**The embedded eBPF binaries in OUT_DIR were stale!**

### What Happened:

1. ✅ We changed eBPF source: `#[cgroup_skb]` → `#[cgroup_skb(ingress)]`
2. ✅ We rebuilt eBPF programs: `cargo +nightly xtask build-ebpf`
3. ✅ Source binaries had correct section: `cgroup_skb/ingress`
4. ❌ **BUT** we used `cargo build` instead of `cargo xtask build-release`
5. ❌ `build.rs` didn't copy the new binaries to OUT_DIR
6. ❌ Main binary embedded **old** binaries with section `cgroup/skb`

### The Files:

**Source eBPF binaries** (correct):

```
target/bpfel-unknown-none/release/chadthrottle-ingress
Section: cgroup_skb/ingress  ✅
```

**Embedded binaries** (was stale):

```
target/release/build/chadthrottle-*/out/chadthrottle-ingress
Section: cgroup/skb  ❌ (old version)
```

**Main binary** (embeds the OUT_DIR version):

```
target/release/chadthrottle
Uses: OUT_DIR/chadthrottle-ingress (the stale one!)
```

## The Fix

**ALWAYS use `cargo +nightly xtask build-release` for the complete build!**

This ensures:

1. eBPF programs are built with nightly toolchain
2. Source binaries are created in `target/bpfel-unknown-none/release/`
3. `build.rs` copies them to OUT_DIR
4. Main binary embeds the correct, up-to-date eBPF programs

### Build Process:

```bash
# ❌ WRONG - Doesn't embed updated eBPF programs
cargo +nightly xtask build-ebpf --release
cargo build --release

# ✅ CORRECT - Complete build with proper embedding
cargo +nightly xtask build-release
```

## Verification

After fixing the build process, we verified:

**Embedded binary now has correct section:**

```bash
$ readelf -W -S target/release/build/chadthrottle-*/out/chadthrottle-ingress | grep cgroup
  [ 3] cgroup_skb/ingress PROGBITS ...
```

**When loaded, Aya will now set:**

```rust
CgroupSkb {
    expected_attach_type: Some(CgroupSkbAttachType::Ingress),  // ✅
}
```

## Expected Test Result

Now when you run:

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle
```

And throttle a process, you should see:

```
DEBUG Program 'chadthrottle_ingress' loaded with expected_attach_type: Some(Ingress)  ✅
DEBUG Opened cgroup file: "/sys/fs/cgroup/..."
DEBUG Attached chadthrottle_ingress to "..." with mode: AllowMultiple
INFO  Successfully attached eBPF ingress program
```

**No more EINVAL!** The kernel will accept the attachment because:

- Program's `expected_attach_type` = `Some(Ingress)`
- Attachment `attach_type` = `Ingress`
- **They match!** ✅

## Changes Made

### 1. eBPF Programs (`chadthrottle-ebpf/src/`)

```diff
-#[cgroup_skb]
+#[cgroup_skb(ingress)]
 pub fn chadthrottle_ingress(...)

-#[cgroup_skb]
+#[cgroup_skb(egress)]
 pub fn chadthrottle_egress(...)
```

### 2. Build Process

- **Cleaned all build artifacts:** `cargo +nightly xtask clean`
- **Used proper build command:** `cargo +nightly xtask build-release`
- **Verified embedded binaries:** `readelf` shows correct sections

### 3. Additional Diagnostics (kept for debugging)

- Log `expected_attach_type` after program load
- Log cgroup file opening
- Test mode for root cgroup (via `CHADTHROTTLE_TEST_ROOT_CGROUP`)

### 4. Reverted O_RDWR Change

- Changed back to `File::open()` (O_RDONLY)
- O_RDWR caused permission denied on user cgroups
- O_RDONLY is sufficient for BPF attachment

## Why This Matters

The kernel's `bpf_link_create` syscall validates:

```c
if (prog->expected_attach_type != attr->attach_type) {
    return -EINVAL;  // ← This was our error!
}
```

With `expected_attach_type = None` and `attach_type = Ingress`:

- ❌ `None != Ingress` → **EINVAL** (errno=22)

With `expected_attach_type = Some(Ingress)` and `attach_type = Ingress`:

- ✅ `Ingress == Ingress` → **Success!**

## Next Steps

1. **Test the fix:**

   ```bash
   sudo RUST_LOG=debug ./target/release/chadthrottle
   ```

2. **Verify expected_attach_type:**
   Look for: `loaded with expected_attach_type: Some(Ingress)`

3. **Confirm throttling works:**
   Throttle wget/curl and verify actual download speed matches the limit

## Related Issues

- `EINVAL_FIX.md` - Initial section name fix in eBPF source
- `EINVAL_DIAGNOSTICS_ADDED.md` - Diagnostic enhancements
- `DIAGNOSTIC_TESTS.md` - Testing instructions

## Lesson Learned

**Always use the provided build system (`xtask`) for complex multi-stage builds!**

The xtask build process handles:

- Nightly toolchain selection
- eBPF compilation with correct flags
- Binary copying to OUT_DIR
- Main crate compilation with proper cfg flags

Don't try to shortcut with `cargo build` directly!
