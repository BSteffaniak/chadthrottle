# Building ChadThrottle

## Quick Start

ChadThrottle builds out-of-the-box on Linux and macOS with network monitoring support:

```bash
cargo build
```

That's it! No platform-specific flags needed.

## Platform Support

| Platform    | Default Build | Full Features             |
| ----------- | ------------- | ------------------------- |
| **Linux**   | ✅ Monitoring | `--features linux-full`   |
| **macOS**   | ✅ Monitoring | Monitoring only (for now) |
| **Windows** | ❌ Not yet    | Coming soon               |

## Building for Your Platform

### Linux - Monitoring Only

```bash
cargo build
```

This gives you network monitoring and process tracking.

### Linux - Full Features (Throttling)

To enable bandwidth throttling on Linux:

```bash
# All throttling features:
cargo build --features linux-full

# Or pick specific backends:
cargo build --features throttle-tc-htb,throttle-ifb-tc
```

Available Linux throttling features:

- `throttle-tc-htb` - TC HTB upload throttling
- `throttle-ifb-tc` - IFB+TC download throttling
- `throttle-tc-police` - TC Police download (no IFB needed)
- `throttle-nftables` - nftables-based throttling
- `throttle-ebpf` - eBPF cgroup throttling (requires xtask build)

### macOS

```bash
cargo build
```

Network monitoring works on macOS. Bandwidth throttling is not yet supported.

**Note:** If you get libiconv linking errors:

```bash
# With Nix (recommended):
nix-shell -p libiconv --run "cargo build"

# Or add to your Nix Darwin config:
environment.systemPackages = [ pkgs.libiconv ];
```

## Release Builds

### Standard Release

```bash
cargo build --release

# With throttling on Linux:
cargo build --release --features linux-full
```

### With eBPF Support

eBPF requires a two-step build process:

```bash
# 1. Build eBPF programs (requires nightly Rust)
cargo xtask build-ebpf

# 2. Build main binary with eBPF enabled
cargo build --features throttle-ebpf

# Or build everything at once:
cargo xtask build          # Debug build
cargo xtask build-release  # Release build
```

**Requirements for eBPF:**

- Nightly Rust with `rust-src` component
- `bpf-linker` installed
- Linux kernel 4.15+ with eBPF support

See [eBPF documentation](EBPF_IMPLEMENTATION.md) for details.

## Feature Flags Reference

### Core Features

- `monitor-pnet` - Network packet capture (default, cross-platform)

### Linux Throttling Features

- `linux-full` - All Linux features (convenience bundle)
- `throttle-tc-htb` - TC HTB upload throttling
- `throttle-ifb-tc` - IFB+TC download throttling
- `throttle-tc-police` - TC Police download throttling
- `throttle-nftables` - nftables throttling
- `throttle-ebpf` - eBPF cgroup throttling

### Cgroup Features (Linux)

- `cgroup-v1` - Cgroup v1 support
- `cgroup-v2-nftables` - Cgroup v2 with nftables
- `cgroup-v2-ebpf` - Cgroup v2 with eBPF

### Platform Bundles

- `linux-full` - All Linux features
- `macos-full` - All macOS features (same as default for now)

## Development

### Check Available Features

```bash
cargo build --list-backends
./target/debug/chadthrottle --list-backends
```

### Build with Verbose Output

```bash
cargo build --verbose
```

### Clean Build

```bash
cargo clean
cargo build
```

## Troubleshooting

### "pnet not found" Error

You're probably running `cargo build` with custom features. Make sure to include `monitor-pnet`:

```bash
cargo build --features monitor-pnet,throttle-tc-htb
```

Or use the convenience bundle:

```bash
cargo build --features linux-full
```

### "procfs not found" Error on Linux

This shouldn't happen - `procfs` is automatically included on Linux. If you see this:

```bash
cargo clean
cargo build
```

### macOS libiconv Linking Error

Add `libiconv` to your environment:

```bash
# Temporary:
nix-shell -p libiconv --run "cargo build"

# Permanent (add to Nix Darwin config):
environment.systemPackages = [ pkgs.libiconv ];
```

### eBPF Build Fails

Make sure you have:

1. Nightly Rust: `rustup toolchain install nightly --component rust-src`
2. bpf-linker: `cargo install bpf-linker`
3. Built eBPF programs first: `cargo xtask build-ebpf`

See [xtask README](xtask/README.md) for more details.

## CI/CD

### GitHub Actions Example

```yaml
name: Build

on: [push, pull_request]

jobs:
  build:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v3

      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable

      - name: Build
        run: cargo build --verbose

      - name: Test
        run: cargo test --verbose

      # Linux only: Build with full features
      - name: Build Linux Full
        if: matrix.os == 'ubuntu-latest'
        run: cargo build --features linux-full
```

## Cross-Compilation

### Linux to Linux (different arch)

```bash
# Install target:
rustup target add x86_64-unknown-linux-gnu

# Build:
cargo build --target x86_64-unknown-linux-gnu
```

### macOS to Linux (requires cross-compilation toolchain)

Cross-compilation for Linux targets from macOS is complex due to kernel dependencies. We recommend:

1. Building on Linux directly
2. Using Docker with Linux container
3. Using remote build server

## Next Steps

- See [ARCHITECTURE.md](ARCHITECTURE.md) for code structure
- See [README.md](README.md) for usage instructions
- See [CROSS_PLATFORM_PROGRESS.md](CROSS_PLATFORM_PROGRESS.md) for implementation status
