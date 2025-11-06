# ChadThrottle Quick Start

## Build and Run

```bash
# Build the project
cd /home/braden/ChadThrottle
cargo build --release

# Run it (requires sudo for network monitoring)
sudo ./target/release/chadthrottle
```

## What You'll See

The TUI will display:
- **PID** - Process ID
- **Process** - Process name
- **Download** - Current download rate (with colors)
- **Upload** - Current upload rate (with colors)
- **T** - Throttle indicator (⚡ if throttled)

## Testing Network Activity

To test the monitoring, open another terminal and generate some network traffic:

```bash
# Download a file
curl -O https://speed.hetzner.de/100MB.bin

# Or ping continuously
ping -c 100 google.com

# Or use wget
wget https://speed.hetzner.de/100MB.bin
```

You should see these processes appear in ChadThrottle with their bandwidth usage!

## Current Limitations

**Phase 1 (Current):**
- ✅ Real-time packet capture with 100% accuracy
- ✅ Per-process bandwidth tracking (TCP & UDP, IPv4 & IPv6)
- ✅ Pure Rust, no external dependencies
- ✅ Beautiful TUI with process list
- ✅ Keyboard navigation
- ⚠️ Throttling features are placeholders (coming soon!)

**Coming Soon (Phase 2):**
- Interactive throttle dialog
- Launch processes with bandwidth limits
- Apply throttling to running processes

## Troubleshooting

### "Permission denied" errors
Run with sudo: `sudo ./target/release/chadthrottle`

### No processes showing up
Make sure there's active network traffic. Try running `curl` or `wget` in another terminal.

**Note:** ChadThrottle now uses packet capture for 100% accurate bandwidth tracking! Every byte is counted as it flows through your network interface.

### "trickle not found" warning
Install trickle for throttling support:
```bash
sudo apt install trickle  # Ubuntu/Debian
```

## Next Steps

1. **Try the navigation**: Use arrow keys or j/k to navigate
2. **Open help**: Press `h` to see all keyboard shortcuts
3. **Check out the code**: Explore `src/` to understand how it works
4. **Contribute**: Add throttling features, improve UI, add graphs!

## Example Session

```
┌──────────────────────────────────────────────────┐
│ 🔥 ChadThrottle v0.1.0 - Network Monitor        │
├──────────────────────────────────────────────────┤
│ PID    Process              ↓Download   ↑Upload │
│ 1234 ▶ firefox              2.3 MB/s    150 KB/s│
│ 5678   curl                 8.1 MB/s    1.2 KB/s│
│ 9012   discord              45 KB/s     12 KB/s │
├──────────────────────────────────────────────────┤
│ [↑↓] Navigate  [h] Help  [q] Quit               │
└──────────────────────────────────────────────────┘
```

Enjoy your chad-level network monitoring! 🔥
