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

**Current Features:**

- ✅ Real-time packet capture with 100% accuracy
- ✅ Per-process bandwidth tracking (TCP & UDP, IPv4 & IPv6)
- ✅ **Upload throttling** (always works)
- ✅ **Download throttling** (requires IFB kernel module - see [IFB_SETUP.md](IFB_SETUP.md))
- ✅ IPv4 + IPv6 dual-stack support
- ✅ Automatic IFB device management for download throttling
- ✅ Graceful degradation (upload-only if IFB unavailable)
- ✅ Pure Rust, no external dependencies
- ✅ Beautiful TUI with process list
- ✅ Interactive throttle dialog
- ✅ Dynamic throttle management

**How to Throttle:**

1. Run with sudo: `sudo ./target/release/chadthrottle`
2. Generate some traffic (e.g., `curl` in another terminal)
3. Select the process you want to throttle
4. Press `t` to open throttle dialog
5. Enter limits in KB/s (e.g., 500 for download, 100 for upload)
6. Press Enter to apply
7. Watch the ⚡ indicator appear!
8. Press `r` to remove the throttle

**Example:**

```bash
# Terminal 1
sudo ./target/release/chadthrottle

# Terminal 2
curl -O https://speed.hetzner.de/100MB.bin

# In ChadThrottle:
# - Select curl process
# - Press 't'
# - Enter: Download: 500, Upload: (leave empty)
# - Press Enter
# - Curl is now limited to 500 KB/s!
```

## Troubleshooting

### "Permission denied" errors

Run with sudo: `sudo ./target/release/chadthrottle`

### No processes showing up

Make sure there's active network traffic. Try running `curl` or `wget` in another terminal.

**Note:** ChadThrottle now uses packet capture for 100% accurate bandwidth tracking! Every byte is counted as it flows through your network interface.

### Download throttling not working (upload works)

**Symptom:** You can throttle upload but download limits don't apply

**Cause:** IFB kernel module not available

**Fix:** See [IFB_SETUP.md](IFB_SETUP.md) for setup instructions

**Quick test:**

```bash
sudo modprobe ifb numifbs=1
ip link show type ifb  # Should show ifb0
```

**Note:** Upload throttling works without IFB. Only download throttling requires it.

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
