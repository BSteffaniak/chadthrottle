# xtask Feature Support

The `cargo xtask` build system now supports passing custom features to the main crate build.

## Quick Reference

```bash
# Default build (monitor-pnet + throttle-ebpf from defaults)
cargo xtask build

# Build with all Linux backends
cargo xtask build --features linux-full

# Build with specific backends
cargo xtask build --features "throttle-tc-htb,throttle-ifb-tc"

# Disable defaults, use only specific features
cargo xtask build --no-default-features --features throttle-tc-htb

# Release build with features
cargo xtask build-release --features linux-full
```

## How It Works

### Feature Priority Chain

1. **Always included**: `throttle-ebpf` (mandatory for eBPF program compatibility)
2. **Default features**: From `chadthrottle/Cargo.toml` (`monitor-pnet`)
3. **User features**: Specified via `--features`
4. **No defaults flag**: `--no-default-features` disables step 2

### Examples

| Command                                                                             | Resulting Features                           |
| ----------------------------------------------------------------------------------- | -------------------------------------------- |
| `cargo xtask build`                                                                 | `throttle-ebpf` + defaults (`monitor-pnet`)  |
| `cargo xtask build --features linux-full`                                           | `throttle-ebpf,linux-full` + defaults        |
| `cargo xtask build --no-default-features`                                           | `throttle-ebpf` only                         |
| `cargo xtask build --no-default-features --features "throttle-tc-htb,monitor-pnet"` | `throttle-ebpf,throttle-tc-htb,monitor-pnet` |

### Available Features

See `chadthrottle/Cargo.toml` for all features:

**Monitor Backends:**

- `monitor-pnet` (cross-platform, default)

**Cgroup Backends:**

- `cgroup-v1` - Legacy cgroup v1 support
- `cgroup-v2-nftables` - Cgroup v2 with nftables
- `cgroup-v2-ebpf` - Cgroup v2 with eBPF (future)

**Throttle Backends (Linux):**

- `throttle-tc-htb` - TC HTB upload throttling
- `throttle-ifb-tc` - IFB+TC download throttling
- `throttle-tc-police` - TC Police download (fallback)
- `throttle-nftables` - nftables-based throttling
- `throttle-ebpf` - eBPF cgroup throttling (**always enabled**)

**Feature Bundles:**

- `linux-full` - All Linux backends enabled
- `macos-full` - All macOS backends enabled

## Implementation Details

### Key Design Decisions

1. **`throttle-ebpf` is mandatory**: The xtask build system always builds eBPF programs first, so the main crate MUST include `throttle-ebpf` to load them.

2. **Smart deduplication**: If you specify `--features throttle-ebpf,other-feature`, it won't duplicate `throttle-ebpf` in the command.

3. **Cargo-style syntax**: Uses the same `--features` and `--no-default-features` flags as cargo for consistency.

### Code Structure

```rust
// BuildOptions struct holds parsed arguments
struct BuildOptions {
    features: Option<String>,
    no_default_features: bool,
}

// Parses --features and --no-default-features from args
fn parse_build_args(args: &[String]) -> BuildOptions

// Ensures throttle-ebpf is always in the feature list
fn build_feature_list(opts: &BuildOptions) -> String

// Updated to accept BuildOptions
fn build_main(release: bool, opts: &BuildOptions) -> Result<()>
```

## Testing

Run the included test script to verify all feature combinations work:

```bash
./test_features.sh
```

Expected output:

```
=== Test 1: Default build (should show throttle-ebpf) ===
  → Using features: throttle-ebpf

=== Test 2: Custom features (should show throttle-ebpf,linux-full) ===
  → Using features: throttle-ebpf,linux-full

=== Test 3: Multiple features (should show throttle-ebpf,throttle-tc-htb,throttle-ifb-tc) ===
  → Using features: throttle-ebpf,throttle-tc-htb,throttle-ifb-tc

=== Test 4: No default features (should show throttle-ebpf,throttle-tc-htb) ===
  → Disabling default features
  → Using features: throttle-ebpf,throttle-tc-htb

=== Test 5: Deduplication test (should not duplicate throttle-ebpf) ===
  → Using features: throttle-ebpf,throttle-tc-htb
```

## Common Use Cases

### Development: Build with all backends for testing

```bash
cargo xtask build --features linux-full
```

### Production: Minimal build (monitoring only, no throttling)

```bash
cargo xtask build --no-default-features --features monitor-pnet
```

### Platform-specific: macOS build

```bash
cargo xtask build --features macos-full
```

### Custom: Specific backend combination

```bash
cargo xtask build --features "throttle-tc-htb,throttle-ifb-tc,throttle-nftables"
```

## Troubleshooting

**Q: I specified `--no-default-features` but still see `throttle-ebpf`**  
A: This is expected! `throttle-ebpf` is ALWAYS included because xtask builds the eBPF programs and the main crate needs to load them.

**Q: Can I disable eBPF entirely?**  
A: No, not through xtask. The whole point of xtask is to build the eBPF programs + main crate together. If you want a build without eBPF support, use `cargo build` directly in the `chadthrottle/` directory.

**Q: What's the difference between `cargo build` and `cargo xtask build`?**  
A: `cargo xtask build` first compiles the eBPF programs (requires nightly), then builds the main crate with `throttle-ebpf` enabled. Regular `cargo build` skips the eBPF step and uses whatever features you specify.

## See Also

- `cargo xtask help` - Full help text
- `chadthrottle/Cargo.toml` - Complete feature definitions
- `EBPF_IMPLEMENTATION.md` - eBPF backend documentation
- `BUILD.md` - General build instructions
