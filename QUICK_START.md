# ChadThrottle v0.6.0 - Quick Start Guide

## Installation

```bash
# Build release version
cargo build --release

# Binary location
./target/release/chadthrottle
```

## Running

```bash
# Basic usage (auto-detects best backends)
sudo ./target/release/chadthrottle

# List available backends
./target/release/chadthrottle --list-backends

# With specific backends
sudo ./target/release/chadthrottle --upload-backend tc_htb --download-backend tc_police

# Restore saved throttles on startup
sudo ./target/release/chadthrottle --restore
```

## Keyboard Controls

| Key | Action |
|-----|--------|
| `↑/k` | Move selection up |
| `↓/j` | Move selection down |
| `t` | Throttle selected process |
| `r` | Remove throttle from process |
| `g` | Toggle bandwidth graph |
| `h/?` | Show help |
| `q/Esc` | Quit |

## Throttling a Process

1. Launch ChadThrottle: `sudo ./chadthrottle`
2. Navigate to process using ↑/↓ or j/k
3. Press `t` to open throttle dialog
4. Enter download limit (KB/s) or leave empty for unlimited
5. Press `Tab` to switch to upload field
6. Enter upload limit (KB/s) or leave empty for unlimited
7. Press `Enter` to apply

## Viewing Bandwidth Graphs

1. Select a process with ↑/↓
2. Press `g` to show bandwidth graph
3. Graph shows last 60 seconds of history
4. Press `g` again to close

## Configuration

**Config File:** `~/.config/chadthrottle/throttles.json`

**Auto-save:** Throttles are saved automatically on exit

**Restore:** Use `--restore` flag to restore saved throttles on startup

## Available Backends

### Upload (Egress)
- **tc_htb** (Good) - TC HTB, requires: TC + cgroups

### Download (Ingress)
- **ifb_tc** (Good) - IFB+TC HTB, requires: TC + cgroups + IFB module
- **tc_police** (Fallback) - TC Police, requires: TC only (no per-process filtering)

### Future
- **ebpf_cgroup** (Best) - eBPF cgroup (coming soon!)

## Troubleshooting

### "No backends available"
- Install `tc` (traffic control): `sudo apt install iproute2`
- Enable cgroups (required for tc_htb and ifb_tc)
- Use tc_police as fallback (works without cgroups, but global limit only)

### "Download throttling: Not available"
- If IFB unavailable, tc_police will be auto-selected
- tc_police applies global rate limit (not per-process)

### Permission Denied
- Must run with sudo: `sudo ./chadthrottle`
- Needs root for TC/cgroup operations

## Features

✅ Real-time network monitoring  
✅ Per-process bandwidth tracking  
✅ Upload/download throttling  
✅ Multiple backend support  
✅ Graceful degradation  
✅ Config persistence  
✅ Bandwidth history (60s)  
✅ Real-time graphs  
✅ Beautiful TUI  

## System Requirements

- Linux 3.10+ (4.10+ recommended for future eBPF support)
- Root/sudo access
- `tc` command (iproute2 package)
- Optional: cgroups (for per-process filtering)
- Optional: IFB module (for better download throttling)

## Examples

### Limit Firefox to 1MB/s download, 512KB/s upload
1. Run: `sudo ./chadthrottle`
2. Find Firefox in list
3. Press `t`
4. Enter: `1024` (download KB/s)
5. Tab, Enter: `512` (upload KB/s)
6. Press Enter

### Check bandwidth usage over time
1. Select process
2. Press `g` to view graph
3. See peak and average rates in title

### Persist throttles across restarts
1. Throttle processes as usual
2. Exit (`q`)
3. Throttles auto-saved
4. Next run: `sudo ./chadthrottle --restore`

## Documentation

- `SESSION_SUMMARY.md` - What's new in this version
- `TC_POLICE_IMPLEMENTATION.md` - TC Police backend details
- `EBPF_BACKENDS.md` - Future eBPF implementation plan
- `ARCHITECTURE.md` - Architecture overview

## Support

- GitHub: https://github.com/yourusername/ChadThrottle
- Issues: Report bugs and feature requests

---

**ChadThrottle** - Network throttling done right! 🔥
