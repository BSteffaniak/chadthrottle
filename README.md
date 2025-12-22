# 🔥 ChadThrottle

**A blazingly fast TUI network monitor and throttler for Linux** - like NetLimiter for Windows or Snail for macOS, but more chad.

## Features

- 📊 **Real-time network monitoring** - See which processes are using bandwidth (100% accurate)
- ⚡ **Per-process bandwidth throttling** - Limit bandwidth using cgroups + tc
- 🔄 **Bidirectional throttling** - Both upload AND download limits (requires IFB module)
- 🌐 **IPv4 + IPv6 support** - Full dual-stack throttling
- 🎨 **Beautiful TUI** - Built with Ratatui for a slick terminal interface
- 🚀 **Fast & lightweight** - Written in Rust for maximum performance
- 🔧 **No external dependencies** - Pure Rust with kernel APIs only
- 💪 **Production ready** - Accurate packet capture and rate limiting
- 🛡️ **Graceful degradation** - Upload throttling works even without IFB

## Installation

### Prerequisites

**Required:**

- Linux kernel 2.6.29+ with cgroups support
- `tc` (traffic control) - usually part of `iproute2` package
- Root access for packet capture and traffic control

**Optional (for download throttling):**

- `ifb` kernel module for bidirectional throttling
- Without IFB: Upload throttling still works

### Build from source

```bash
cd chadthrottle

# Default build (monitoring only):
cargo build --release

# With all Linux throttling backends (recommended):
cargo build --release --features linux-full

# Or with specific backends:
cargo build --release --features "throttle-tc-htb,throttle-ifb-tc"

sudo cp target/release/chadthrottle /usr/local/bin/
```

**Note:** The default build includes monitoring only. To enable throttling features, you must explicitly enable cargo features (see above).

## Usage

ChadThrottle supports two modes: **TUI mode** (interactive) and **CLI mode** (non-interactive).

### TUI Mode (Interactive)

Start the interactive terminal UI:

```bash
sudo chadthrottle
```

**Note:** Requires root/sudo for full network monitoring capabilities.

#### Keyboard Shortcuts

**Navigation:**
- `↑`/`k` - Move selection up
- `↓`/`j` - Move selection down
- `i` - Toggle interface view
- `l` - Cycle traffic view (All/Internet/Local)
- `Enter` - View details (process or interface)
- `Tab` - Switch tabs (in detail view)
- `Space` - Toggle interface filter (in interface list)
- `A` - Toggle All/None interfaces (in interface list)

**Actions:**
- `t` - Throttle selected process (opens dialog)
- `r` - Remove throttle from selected process
- `g` - Toggle bandwidth graph
- `f` - Freeze/unfreeze sort order

**System:**
- `b` - View/switch backends
- `h`/`?` - Toggle help
- `q`/`Esc` - Quit (or close modal if open)
- `Ctrl+C` - Force quit (always exits)

**In Throttle Dialog:**

- `Tab` - Switch between download/upload fields
- `0-9` - Enter limit in KB/s
- `Backspace` - Delete character
- `Enter` - Apply throttle
- `Esc` - Cancel

### CLI Mode (Non-Interactive)

Throttle a specific process without the TUI:

```bash
# Throttle both download and upload
sudo chadthrottle --pid 1234 --download-limit 1M --upload-limit 500K

# Throttle only download
sudo chadthrottle --pid 1234 --download-limit 1.5M

# Throttle for a specific duration (30 seconds)
sudo chadthrottle --pid 1234 --download-limit 1M --duration 30

# Use specific backends
sudo chadthrottle --pid 1234 --download-limit 1M --upload-backend tc_htb --download-backend ebpf
```

**Bandwidth limit formats:**

- `500K` or `500KB` = 500 KB/s
- `1M` or `1MB` = 1 MB/s
- `1.5M` = 1.5 MB/s
- `1G` or `1GB` = 1 GB/s

**CLI mode features:**

- Applies throttle immediately
- Runs until Ctrl+C (or `--duration` expires)
- Automatically removes throttle on exit
- Perfect for scripts and automation

## Architecture

```
ChadThrottle
├── src/
│   ├── main.rs       # Entry point and TUI event loop
│   ├── monitor.rs    # Network monitoring with packet capture
│   ├── ui.rs         # Ratatui UI components
│   ├── process.rs    # Process data structures
│   └── backends/     # Pluggable backend implementations
└── Cargo.toml
```

## How It Works

### Monitoring (Packet Capture with pnet)

ChadThrottle uses **accurate packet-level tracking** to monitor network usage per process:

1. **Raw Packet Capture**: Uses `pnet` library to capture packets directly from network interfaces via `AF_PACKET` sockets (Linux kernel API)
2. **Packet Parsing**: Parses Ethernet → IP (v4/v6) → TCP/UDP headers to extract connection information
3. **Socket Inode Mapping**: Scans `/proc/[pid]/fd/` and `/proc/net/*` to map connections to PIDs
4. **Accurate Byte Counting**: Tracks every packet's size and attributes it to the correct process

**Key advantages:**

- ✅ **100% accurate** - Counts every byte sent and received
- ✅ **No external dependencies** - Pure Rust using kernel APIs directly
- ✅ **Single static binary** - No need for libpcap or other C libraries
- ✅ **Real-time tracking** - Captures packets as they flow through the network

### Throttling (cgroups + TC + IFB)

ChadThrottle implements accurate **bidirectional** per-process throttling using:

1. **Linux cgroups (net_cls)** - Tags all packets from a process
2. **TC (Traffic Control) HTB** - Rate limits upload (egress)
3. **IFB (Intermediate Functional Block)** - Redirects download (ingress) → treats as egress
4. **IPv4 + IPv6 support** - Full dual-stack throttling
5. **Guaranteed limits** - Kernel-enforced, no way to bypass

**How to use:**

- Select a process and press `t`
- Enter download/upload limits in KB/s (leave empty for unlimited)
- Press Enter to apply
- Look for ⚡ indicator on throttled processes
- Press `r` to remove throttle

**Throttling capabilities:**

- ✅ **Upload throttling** - Always works (TC HTB on main interface)
- ✅ **Download throttling** - Requires IFB module (TC HTB on IFB device)
- ✅ **IPv4 + IPv6** - Both protocols fully supported
- 🛡️ **Graceful fallback** - Upload-only if IFB unavailable

**Note:** If IFB module is not available, ChadThrottle will:

- Show a warning when you try to set download limits
- Apply upload throttling only
- Continue working normally for monitoring and upload limits

## Roadmap

- [x] Real-time network monitoring TUI with packet capture
- [x] 100% accurate per-process bandwidth tracking
- [x] Pure Rust implementation with no external C dependencies
- [x] Process list with bandwidth usage
- [x] Interactive throttle dialog
- [x] Bidirectional throttling (upload + download)
- [x] IPv4 + IPv6 support
- [x] Graceful degradation without IFB
- [x] Apply throttling to existing processes (cgroups)
- [x] Bandwidth usage graphs
- [x] Save/load throttle profiles
- [ ] Per-connection throttling
- [ ] Domain whitelist/blacklist
- [x] eBPF-based throttling (alternative to IFB)

## Why "ChadThrottle"?

Because monitoring network activity and throttling bandwidth at the process level on Linux should be as chad as it is on Windows and macOS. No more complicated tc commands or iptables rules - just a clean TUI that gets the job done.

## Contributing

Pull requests welcome! This is an early-stage project with lots of room for improvement.

## License

MIT

## See Also

- [NetLimiter](https://www.netlimiter.com/) - Windows network monitor/throttler
- [Snail](https://www.snail-app.com/) - macOS network throttler
- [bandwhich](https://github.com/imsnif/bandwhich) - Terminal bandwidth utilization tool
- [trickle](https://github.com/mariusae/trickle) - Userspace bandwidth shaper
