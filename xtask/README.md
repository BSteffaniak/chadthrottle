# xtask

Build automation for ChadThrottle, providing commands to build eBPF programs and the main project.

## Description

The `xtask` binary provides build automation for ChadThrottle, particularly for building eBPF programs which require nightly Rust. It handles nightly toolchain detection, eBPF compilation with `bpf-linker`, and feature management.

## Prerequisites

**Required:**

- Rust nightly toolchain with `rust-src` component
- `bpf-linker` for eBPF compilation: `cargo install bpf-linker`

**Automatic Setup:**

The tool will automatically detect or install nightly Rust via rustup if needed.

## Usage

Run from the workspace root:

```bash
cargo xtask <COMMAND> [OPTIONS]
```

### Commands

- `build-ebpf` - Build eBPF programs only (requires nightly + bpf-linker)
- `build` - Build everything (eBPF + main crate) in debug mode
- `build-release` - Build everything in release mode
- `clean` - Clean all build artifacts
- `help` - Show help message

### Options

- `--features <FEATURES>` - Enable specific features (comma-separated)
- `--no-default-features` - Disable default features

### Examples

```bash
# Default build with monitor-pnet + throttle-ebpf
cargo xtask build

# Build with all Linux backends enabled
cargo xtask build --features linux-full

# Build with specific throttle backends
cargo xtask build --features "throttle-tc-htb,throttle-ifb-tc"

# Build without monitor-pnet, but with TC HTB backend
cargo xtask build --no-default-features --features throttle-tc-htb

# Production build with all Linux features
cargo xtask build-release --features linux-full

# Just rebuild eBPF programs
cargo xtask build-ebpf
```

## Notes

- `throttle-ebpf` feature is always included (required for eBPF programs)
- Default features: `monitor-pnet` (can disable with `--no-default-features`)
- Supports rustup, NixOS, nix-shell, and flakes environments
- Automatically detects nightly Rust or installs it via rustup
