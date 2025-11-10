# xtask

Build automation for ChadThrottle, particularly for building eBPF programs which require nightly Rust.

## Usage

Run from the workspace root:

```bash
# Build eBPF programs only
cargo xtask build-ebpf

# Build everything (eBPF + main crate) in debug mode
cargo xtask build

# Build everything in release mode
cargo xtask build-release

# Clean build artifacts
cargo xtask clean

# Show help
cargo xtask help
```

## Build Options

```bash
# Enable specific features (comma-separated)
cargo xtask build --features linux-full

# Disable default features
cargo xtask build --no-default-features --features throttle-tc-htb

# Combine options
cargo xtask build-release --features "throttle-tc-htb,throttle-ifb-tc"
```

Note: The `throttle-ebpf` feature is always included automatically.

## Requirements

- Rust nightly (auto-detected or auto-installed via rustup)
- `bpf-linker`: Install with `cargo install bpf-linker`
- Linux kernel 4.15+ with eBPF support

## Supported Environments

- Works with rustup (auto-installs nightly if needed)
- Works with NixOS (detects nightly automatically)
- Works with nix-shell and flakes
