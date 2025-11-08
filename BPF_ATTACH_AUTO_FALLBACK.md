# BPF Attach Method Auto-Fallback

## Overview

ChadThrottle now supports automatic fallback between modern and legacy BPF attachment methods, ensuring compatibility across different kernel versions and configurations.

## Problem Solved

Different kernel versions and configurations support different BPF attachment syscalls:

- **Modern method** (`bpf_link_create`): Kernel 5.7+, creates managed BPF links
- **Legacy method** (`bpf_prog_attach`): Kernel 4.10+, direct program attachment

Some systems (including certain NixOS configurations) may have `bpf_link_create` fail with EINVAL even on supported kernels, requiring the legacy method.

## Solution: Auto-Fallback

The default behavior (`--bpf-attach-method auto`) tries the modern method first, and automatically falls back to the legacy method if EINVAL is encountered.

## Configuration

### 1. CLI Argument (Recommended)

```bash
# Auto mode (default) - tries modern, falls back to legacy on EINVAL
sudo ./chadthrottle --bpf-attach-method auto

# Force legacy method
sudo ./chadthrottle --bpf-attach-method legacy

# Force modern method (may fail on some systems)
sudo ./chadthrottle --bpf-attach-method link
```

### 2. Environment Variables

```bash
# New way (preferred)
export CHADTHROTTLE_BPF_ATTACH_METHOD=auto  # or: link, legacy
sudo ./chadthrottle

# Old way (deprecated but still supported)
export CHADTHROTTLE_USE_LEGACY_ATTACH=1
sudo ./chadthrottle
```

Priority order:

1. `--bpf-attach-method` CLI argument (highest)
2. `CHADTHROTTLE_USE_LEGACY_ATTACH` env var (legacy compatibility)
3. `CHADTHROTTLE_BPF_ATTACH_METHOD` env var
4. Default: `auto`

### 3. Future: XDG Config File

(Planned for future release)

```toml
# ~/.config/chadthrottle/config.toml
[bpf]
attach_method = "auto"  # or: "link", "legacy"
```

## How It Works

### Auto Mode (Default)

1. **Try Modern**: Attempts `bpf_link_create`
   - ✅ Success → Uses modern method
   - ❌ EINVAL → Falls back to legacy
   - ❌ Other error → Propagates error (no fallback)

2. **Fallback to Legacy**: Uses `bpf_prog_attach`
   - ✅ Success → Uses legacy method
   - ❌ Error → Propagates error

### Log Output

**Successful Modern Attach:**

```
DEBUG ... Auto-detecting best BPF attach method
INFO  ... ✅ Successfully attached using modern method (bpf_link_create)
```

**Auto-Fallback to Legacy:**

```
DEBUG ... Auto-detecting best BPF attach method
DEBUG ... Found io::Error with errno 22 in error chain
WARN  ... Modern attach failed with EINVAL, falling back to legacy method...
INFO  ... ✅ Successfully attached using legacy method!
```

**Explicit Legacy:**

```
INFO  ... Using legacy BPF attach method (bpf_prog_attach)
INFO  ... ✅ Successfully attached using legacy method!
```

## Testing

### Test Auto-Fallback (TUI Mode)

```bash
# Rebuild
cargo +nightly xtask build-release

# Run in TUI mode - should auto-fallback on systems that need it
sudo RUST_LOG=debug ./target/release/chadthrottle

# Look for the fallback message in logs:
# "Modern attach failed with EINVAL, falling back to legacy method..."
```

### Test Auto-Fallback (CLI Mode)

```bash
# Test with a process
sudo RUST_LOG=debug ./target/release/chadthrottle \
    --pid $$ \
    --download-limit 1M \
    --download-backend ebpf \
    --duration 5

# Should see fallback in output
```

### Test Explicit Methods

```bash
# Force legacy (always works on kernel 4.10+)
sudo RUST_LOG=debug ./target/release/chadthrottle \
    --pid $$ \
    --download-limit 1M \
    --download-backend ebpf \
    --bpf-attach-method legacy \
    --duration 5

# Force modern (may fail on some systems)
sudo RUST_LOG=debug ./target/release/chadthrottle \
    --pid $$ \
    --download-limit 1M \
    --download-backend ebpf \
    --bpf-attach-method link \
    --duration 5
```

## Implementation Details

### Error Chain Walking

The fallback logic properly detects EINVAL by walking the `anyhow::Error` chain:

```rust
for cause in e.chain() {
    // Check for io::Error with raw_os_error() == 22
    if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
        if io_err.raw_os_error() == Some(22) {
            // Found EINVAL, trigger fallback
        }
    }
}
```

This handles errors wrapped by `.context()` properly.

### Global Configuration

Configuration is initialized once at startup using `std::sync::OnceLock`:

```rust
static BPF_CONFIG: OnceLock<BpfConfig> = OnceLock::new();

// In main()
init_bpf_config(BpfConfig::new(attach_method));

// Anywhere in the code
let config = get_bpf_config();
```

This makes the configuration available to both TUI and CLI modes without passing it through multiple layers.

## Compatibility

### Kernel Requirements

- **Minimum**: Linux 4.10+ (for cgroup_skb programs)
- **Modern method**: Linux 5.7+ (for bpf_link_create)
- **Auto mode**: Works on all kernels 4.10+

### Known Issues

Some systems with kernel 5.7+ may still require legacy attach due to:

- Kernel configuration (missing `CONFIG_BPF_SYSCALL` features)
- Security modules (SELinux, AppArmor) blocking `bpf_link_create`
- Custom kernel patches

**Auto mode handles all these cases automatically.**

## Migration Guide

### From `CHADTHROTTLE_USE_LEGACY_ATTACH=1`

**Old way:**

```bash
export CHADTHROTTLE_USE_LEGACY_ATTACH=1
sudo ./chadthrottle
```

**New way (equivalent):**

```bash
sudo ./chadthrottle --bpf-attach-method legacy
# OR
export CHADTHROTTLE_BPF_ATTACH_METHOD=legacy
sudo ./chadthrottle
```

**Recommended (auto-fallback):**

```bash
# Just remove the env var, auto mode is default
sudo ./chadthrottle
```

### No Action Required

If you don't set any environment variables or CLI args, auto-fallback is enabled by default. The system will "just work" on all compatible kernels.

## Troubleshooting

### Issue: "Failed to attach program to cgroup: ... errno=22"

**Solution**: The auto-fallback should handle this. If you still see this error:

1. Check logs for fallback message
2. Try explicit legacy: `--bpf-attach-method legacy`
3. Enable debug logs: `RUST_LOG=debug`

### Issue: Auto mode doesn't fallback

**Possible causes**:

1. Error is not EINVAL (errno 22) - auto only falls back on EINVAL
2. Legacy method also fails - check kernel version (need 4.10+)
3. Cgroup v2 not mounted - check `/sys/fs/cgroup/cgroup.controllers`

**Debug**:

```bash
sudo RUST_LOG=debug ./chadthrottle 2>&1 | grep -E "attach|EINVAL|fallback"
```

### Issue: "Modern attach failed with non-EINVAL error, not retrying"

This means the modern method failed with an error OTHER than EINVAL. Auto-fallback only triggers on EINVAL. The error will be displayed - fix the underlying issue.

## Future Enhancements

1. **XDG Config File Support** - Persistent configuration
2. **Per-Backend Configuration** - Different methods for upload/download
3. **Runtime Method Switching** - Change method without restart
4. **Automatic Method Detection** - Probe kernel capabilities on startup

## Related Files

- `chadthrottle/src/backends/throttle/linux_ebpf_utils.rs` - Core implementation
- `chadthrottle/src/main.rs` - Configuration initialization
- `test_ebpf_cli.sh` - Test script with examples

## See Also

- [EINVAL_FIX.md](EINVAL_FIX.md) - Original EINVAL investigation
- [BPF_ALLOW_MULTI_FIX.md](BPF_ALLOW_MULTI_FIX.md) - BPF_F_ALLOW_MULTI flag fix
- [CLI_MODE_ADDED.md](CLI_MODE_ADDED.md) - CLI mode documentation
