# Interactive Backend Switching - Quick Guide

## Visual User Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                    ChadThrottle TUI                             │
│                                                                 │
│  PID     Process              ↓Rate      ↑Rate                 │
│  1234    firefox              1.5 MB/s   500 KB/s   ⚡          │
│  5678    chrome               800 KB/s   200 KB/s              │
│                                                                 │
│  [↑↓] Navigate  [t] Throttle  [b] Backends  [h] Help           │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ Press 'b'
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│              ChadThrottle - Backends                            │
│                                                                 │
│  Upload Backends:                                               │
│    ⭐ ebpf            [DEFAULT]    Priority: Best    (1 active) │
│    ✅ tc_htb          Available    Priority: Good    (0 active) │
│                                                                 │
│  Download Backends:                                             │
│    ⭐ ebpf            [DEFAULT]    Priority: Best    (1 active) │
│    ✅ ifb_tc          Available    Priority: Good    (0 active) │
│    ❌ nftables        Unavailable  Priority: Better             │
│                                                                 │
│  [Enter] Switch backends  [b/Esc] Close                         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ Press Enter
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  Backend Selection                              │
│                                                                 │
│  Select Default Upload Backend                                  │
│                                                                 │
│    ⭐ ebpf            [CURRENT DEFAULT]  Priority: Best         │
│  ▶ ✅ tc_htb          Available          Priority: Good         │
│    ❌ nftables        (unavailable)      Priority: Better       │
│                                                                 │
│  [Tab] Switch Upload/Download  [↑↓] Navigate                   │
│  [Enter] Select  [Esc] Cancel                                   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ Select tc_htb, Press Enter
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ChadThrottle TUI                             │
│                                                                 │
│  PID     Process              ↓Rate      ↑Rate                 │
│  1234    firefox              1.5 MB/s   500 KB/s   ⚡          │
│  5678    chrome               800 KB/s   200 KB/s              │
│                                                                 │
│  ✅ New throttles will use 'tc_htb' backend                     │
└─────────────────────────────────────────────────────────────────┘
```

## Key Bindings

| Key           | Context            | Action                     |
| ------------- | ------------------ | -------------------------- |
| `b`           | Main TUI           | Open backend info modal    |
| `Enter`       | Backend info modal | Switch to backend selector |
| `Tab`         | Backend selector   | Toggle Upload/Download     |
| `↑↓` or `j/k` | Backend selector   | Navigate backends          |
| `Enter`       | Backend selector   | Select backend as default  |
| `Esc`         | Any modal          | Close modal                |

## Keyboard Shortcuts Summary

### Main TUI

- `↑↓` or `j/k` - Navigate process list
- `t` - Throttle selected process
- `r` - Remove throttle from selected process
- `f` - Freeze/unfreeze sort order
- `g` - Toggle bandwidth graph
- `b` - **View/switch backends** ⭐ NEW
- `h` or `?` - Show help
- `q` or `Esc` - Quit
- `Ctrl+C` - Force quit

### Backend Info Modal (`b`)

- `Enter` - Switch to backend selector
- `b`, `q`, or `Esc` - Close modal

### Backend Selector Modal (`b` → `Enter`)

- `Tab` - Toggle between Upload/Download
- `↑↓` or `j/k` - Navigate backends (skips unavailable ones)
- `Enter` - Select highlighted backend as default
- `Esc` - Cancel and close

## What Happens When You Switch Backends?

### Before Switch

```
Firefox (PID 1234) → eBPF backend → Throttled to 1 MB/s
```

### Switch to tc_htb

```
Status: ✅ New throttles will use 'tc_htb' backend
```

### After Switch - Add New Throttle

```
Firefox (PID 1234) → eBPF backend  → Throttled to 1 MB/s  (unchanged)
Chrome  (PID 5678) → tc_htb backend → Throttled to 500 KB/s (new)
```

### Key Points

- ✅ **Firefox stays on eBPF** - existing throttles are NOT migrated
- ✅ **Chrome uses tc_htb** - new throttles use the selected backend
- ✅ **Both work simultaneously** - multiple backends coexist
- ✅ **Each backend manages its own throttles** - clean separation
- ✅ **Removing Firefox throttle** - routes to eBPF backend
- ✅ **Removing Chrome throttle** - routes to tc_htb backend

## Backend Status Indicators

| Symbol       | Meaning                                         |
| ------------ | ----------------------------------------------- |
| ⭐           | Current default backend for new throttles       |
| ✅           | Available backend (can be selected)             |
| ❌           | Unavailable backend (cannot be selected)        |
| `[DEFAULT]`  | This backend is currently the default           |
| `(3 active)` | Number of processes throttled with this backend |

## Example Session

```bash
# 1. Start ChadThrottle
sudo chadthrottle

# 2. Navigate to Firefox process
#    Press ↓ or j to select it

# 3. Throttle Firefox with default backend (eBPF)
#    Press 't', enter limits, press Enter
#    → Firefox is now throttled using eBPF

# 4. Check backend info
#    Press 'b'
#    → See: ⭐ ebpf [DEFAULT] (1 active)

# 5. Switch to tc_htb backend
#    Press Enter (from backend info)
#    Press Tab to select "Upload Backends"
#    Navigate to "tc_htb" with ↓
#    Press Enter
#    → Status: "New throttles will use 'tc_htb' backend"

# 6. Throttle Chrome with new backend
#    Navigate to Chrome process
#    Press 't', enter limits, press Enter
#    → Chrome is now throttled using tc_htb

# 7. Check backend info again
#    Press 'b'
#    → See: ⭐ tc_htb [DEFAULT] (1 active)
#           ✅ ebpf   Available (1 active)
#    → Both backends are active!
```

## Tips

1. **Test Before Committing**: Try a new backend on a test process before switching fully
2. **Check Active Counts**: The `(X active)` indicator shows which backends are in use
3. **Priority Matters**: Higher priority backends are generally better (Best > Better > Good > Fallback)
4. **Availability First**: Only available backends can be selected (unavailable ones are grayed out)
5. **Config Persists**: Your backend choice is saved and restored on next launch
6. **CLI Still Works**: Use `--upload-backend` and `--download-backend` flags for automation

## Troubleshooting

### "Backend 'X' is not available"

- Backend requires specific kernel modules or tools
- Run `chadthrottle --list-backends` to see availability reasons
- Example: `ifb_tc` requires `modprobe ifb`

### "No throttling backends available"

- No backend is selected as default
- Press 'b' → Enter to select an available backend
- Check logs for backend initialization errors

### Backend selector shows no available backends

- All backends failed initialization
- Check system requirements (tc, eBPF support, kernel modules)
- Run with `RUST_LOG=debug` for detailed diagnostics

## Advanced Usage

### Compare Backend Performance

1. Throttle Process A with eBPF
2. Switch to tc_htb
3. Throttle Process B with tc_htb
4. Use 'g' to view bandwidth graphs for each
5. Compare accuracy and latency

### Migrate Throttles (Manual)

1. Press 'r' to remove throttle from Process A
2. Switch backend with 'b' → Enter
3. Press 't' to re-throttle Process A with new backend

This gives you control over when migration happens.
