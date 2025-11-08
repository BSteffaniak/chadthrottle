# ChadThrottle Quick Reference Card

## TUI Mode (Interactive)

```bash
sudo ./target/release/chadthrottle
```

**Keys:** `↑↓` navigate, `t` throttle, `r` remove, `q` quit, `h` help

---

## CLI Mode (Non-Interactive)

### Basic Usage

```bash
# Throttle a process
sudo ./target/release/chadthrottle --pid <PID> --download-limit <LIMIT> --upload-limit <LIMIT>

# Examples
sudo ./target/release/chadthrottle --pid 1234 --download-limit 1M --upload-limit 500K
sudo ./target/release/chadthrottle --pid $$ --download-limit 1.5M --duration 60
```

### Bandwidth Format

- `500K` = 500 KB/s
- `1M` = 1 MB/s
- `1.5M` = 1.5 MB/s
- `1G` = 1 GB/s

### Options

```bash
--pid <PID>                 # Process to throttle (required for CLI mode)
--download-limit <LIMIT>    # Download limit (e.g., "1M")
--upload-limit <LIMIT>      # Upload limit (e.g., "500K")
--duration <SECONDS>        # Run for N seconds (default: until Ctrl+C)
--download-backend <NAME>   # Force specific download backend
--upload-backend <NAME>     # Force specific upload backend
--list-backends             # List available backends
```

---

## Testing eBPF Backend

### Standard Method (bpf_link_create)

```bash
sudo RUST_LOG=debug ./target/release/chadthrottle \
    --pid $$ \
    --download-limit 1M \
    --download-backend ebpf-cgroup \
    --duration 10
```

### Legacy Method (bpf_prog_attach)

```bash
sudo RUST_LOG=debug CHADTHROTTLE_USE_LEGACY_ATTACH=1 ./target/release/chadthrottle \
    --pid $$ \
    --download-limit 1M \
    --download-backend ebpf-cgroup \
    --duration 10
```

### Use Test Scripts

```bash
sudo ./test_ebpf_cli.sh           # eBPF-specific test
sudo ./test_cli_mode.sh           # General CLI test
```

---

## Checking if It Works

### Check BPF Programs

```bash
# List all BPF programs
sudo bpftool prog list

# Show cgroup attachments
sudo bpftool cgroup tree /sys/fs/cgroup

# Show specific program
sudo bpftool prog show id <ID>
```

### Check Logs

Look for these in output:

- ✅ "Throttle applied successfully!"
- ✅ "Attached chadthrottle_ingress"
- ❌ Avoid: "EINVAL" errors

---

## Common Use Cases

### Throttle Firefox

```bash
# Find PID
FIREFOX_PID=$(pgrep -o firefox)

# Throttle it
sudo ./target/release/chadthrottle \
    --pid $FIREFOX_PID \
    --download-limit 2M
```

### Throttle wget/curl Download

```bash
# Start download
wget https://speed.hetzner.de/100MB.bin &
WGET_PID=$!

# Throttle it
sudo ./target/release/chadthrottle \
    --pid $WGET_PID \
    --download-limit 500K
```

### Time-Limited Throttle

```bash
# Throttle for 5 minutes
sudo ./target/release/chadthrottle \
    --pid 1234 \
    --download-limit 1M \
    --duration 300
```

---

## Backends

### List Available

```bash
./target/release/chadthrottle --list-backends
```

### Common Backends

**Upload:**

- `tc-htb` - Traffic control (standard)
- `ebpf-cgroup` - eBPF cgroup (experimental)

**Download:**

- `ifb-tc` - IFB + traffic control (requires IFB module)
- `ebpf-cgroup` - eBPF cgroup (experimental)
- `tc-police` - TC police (alternative)

---

## Environment Variables

```bash
RUST_LOG=debug                        # Detailed logging
RUST_LOG=info                         # Standard logging
CHADTHROTTLE_USE_LEGACY_ATTACH=1     # Use legacy BPF attach
CHADTHROTTLE_TEST_ROOT_CGROUP=1      # Use root cgroup (testing)
```

---

## Troubleshooting

### Permission Denied

→ Run with `sudo`

### No backends available

→ Check: `tc` installed, cgroups enabled, IFB module loaded

### EINVAL on eBPF attach

→ Try: `CHADTHROTTLE_USE_LEGACY_ATTACH=1`

### Download throttling not working

→ Enable IFB: `sudo modprobe ifb numifbs=1`

---

## Build

```bash
# Standard build
cargo build --release

# eBPF build (requires nightly)
cargo +nightly xtask build-release
```

---

## Quick Test

```bash
# 1. Build
cargo +nightly xtask build-release

# 2. Test with current shell
sudo ./target/release/chadthrottle --pid $$ --download-limit 1M --duration 5

# 3. Success if you see:
#    ✅ Throttle applied successfully!
#    (waits 5 seconds)
#    ✅ Throttle removed successfully!
```

---

**Documentation:** See `README.md`, `QUICKSTART.md`, `CLI_MODE_ADDED.md`, `SESSION_COMPLETE.md`

**Version:** 0.6.0
