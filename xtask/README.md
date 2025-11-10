# xtask

Build automation for ChadThrottle, providing commands to build eBPF programs and the main application with proper toolchain management.

## Description

This is an internal build tool for the ChadThrottle workspace. It handles the complex build requirements for eBPF programs, which require nightly Rust and specific build configurations.

## Usage

The xtask binary is invoked through cargo from the workspace root:

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

### Build Options

You can pass additional build options for the main crate:

```bash
# Enable specific features (throttle-ebpf is always included)
cargo xtask build --features linux-full

# Disable default features
cargo xtask build --no-default-features --features throttle-tc-htb

# Multiple features (comma-separated)
cargo xtask build --features "throttle-tc-htb,throttle-ifb-tc"
```

## Requirements

**For eBPF builds:**

- Rust nightly toolchain with rust-src component
- `bpf-linker`: Install with `cargo install bpf-linker`

The tool automatically detects or installs nightly Rust when needed, supporting:

- rustup (auto-installs nightly if needed)
- NixOS environments (auto-detects nightly)
- nix-shell and flakes

## License

This package is part of the ChadThrottle project and follows the same license.
