# ChadThrottle

A TUI network monitor and throttler for Linux - like NetLimiter but chad.

## Features

- Real-time per-process network bandwidth monitoring
- Per-process upload/download throttling (Linux)
- Multiple throttling backends (TC, nftables, eBPF)
- Interactive TUI with vim-style navigation
- CLI mode for scripting
- Interface filtering and traffic categorization
- Cross-platform monitoring (Linux, macOS, Windows)

## Installation

### From Source

```bash
# Basic monitoring (cross-platform)
cargo build --release

# Full Linux support (all throttling backends)
cargo build --release --features linux-full

# macOS
cargo build --release --features macos-full
```

### Cargo Features

| Feature              | Description                               |
| -------------------- | ----------------------------------------- |
| `monitor-pnet`       | Network monitoring via pnet (default)     |
| `throttle-tc-htb`    | TC HTB upload throttling                  |
| `throttle-ifb-tc`    | IFB+TC download throttling                |
| `throttle-tc-police` | TC Police download throttling (fallback)  |
| `throttle-nftables`  | nftables throttling                       |
| `throttle-ebpf`      | eBPF cgroup throttling (best performance) |
| `linux-full`         | All Linux throttling backends             |
| `macos-full`         | macOS support                             |

## Usage

### TUI Mode (Default)

```bash
# Run with default settings
chadthrottle

# Specify throttling backends
chadthrottle --upload-backend ebpf --download-backend nftables

# List available backends
chadthrottle --list-backends
```

### CLI Mode

Throttle a process without launching the TUI:

```bash
# Throttle download to 1 MB/s
chadthrottle --pid 1234 --download-limit 1M

# Throttle both directions with duration
chadthrottle --pid 1234 --download-limit 1M --upload-limit 500K --duration 60

# Limit formats: 500K, 1M, 1.5M, 2G
```

### Keybindings

| Key      | Action                                  |
| -------- | --------------------------------------- |
| `↑/k`    | Move selection up                       |
| `↓/j`    | Move selection down                     |
| `i`      | Toggle interface view                   |
| `l`      | Cycle traffic view (All/Internet/Local) |
| `Enter`  | View details (process or interface)     |
| `Tab`    | Switch tabs (in detail view)            |
| `Space`  | Toggle interface filter                 |
| `A`      | Toggle All/None interfaces              |
| `t`      | Throttle selected process               |
| `r`      | Remove throttle                         |
| `g`      | Toggle bandwidth graph                  |
| `f`      | Freeze/unfreeze sort order              |
| `b`      | View/switch backends                    |
| `h/?`    | Toggle help                             |
| `q/Esc`  | Quit (or close modal)                   |
| `Ctrl+C` | Force quit                              |

### CLI Options

```
chadthrottle [OPTIONS]

Options:
    --upload-backend <BACKEND>    Upload throttling backend
    --download-backend <BACKEND>  Download throttling backend
    --socket-mapper <BACKEND>     Socket mapper backend
    --list-backends               List available backends and exit
    --no-restore                  Don't restore saved throttles on startup
    --no-save                     Don't save throttles on exit
    --pid <PID>                   PID to throttle (CLI mode)
    --download-limit <LIMIT>      Download limit (e.g., "1M", "500K")
    --upload-limit <LIMIT>        Upload limit (e.g., "1M", "500K")
    --duration <SECONDS>          Duration to run throttle
    --bpf-attach-method <METHOD>  BPF attach method: auto, link, legacy
    -h, --help                    Print help
    -V, --version                 Print version
```

## Requirements

### Linux

- Root privileges (for throttling)
- For TC backends: `iproute2` package
- For nftables backend: `nftables` package
- For eBPF backend: Linux 5.8+ with BPF support

### macOS

- Monitoring works out of the box
- Throttling uses `dnctl` (requires root)

### Windows

- Monitoring only (no throttling support)

## License

MIT
