# Network Interface Visibility Feature

## Overview

Added comprehensive network interface visibility to ChadThrottle's TUI, allowing users to view and analyze network traffic on a per-interface basis.

## Features Implemented

### 1. Multi-Interface Packet Capture

- **Previous behavior**: Only captured packets on a single "best" network interface
- **New behavior**: Captures packets on ALL active network interfaces simultaneously
- Each interface has its own dedicated capture thread
- Tracks which interface each packet came from

### 2. Data Structures

#### New Types in `process.rs`:

- `InterfaceStats`: Per-interface statistics for a process
  - `download_rate`, `upload_rate`
  - `total_download`, `total_upload`
- `InterfaceInfo`: Information about a network interface
  - `name`: Interface name (e.g., "eth0", "wlan0")
  - `mac_address`: Optional MAC address
  - `ip_addresses`: List of IPv4/IPv6 addresses
  - `is_up`, `is_loopback`: Status flags
  - `total_download_rate`, `total_upload_rate`: Aggregate bandwidth
  - `process_count`: Number of processes using this interface

- `InterfaceMap`: HashMap of interface name → InterfaceInfo

#### Updated `ProcessInfo`:

- Added `interface_stats`: HashMap mapping interface name to per-interface stats
- Allows seeing which interfaces a process is using and how much bandwidth on each

### 3. Three View Modes

#### ProcessView (Default)

- Shows all processes with network activity
- Press **'i'** to switch to interface view

#### InterfaceList

- Shows all network interfaces with:
  - Status indicator: ✓ (up), ✗ (down), ⟲ (loopback)
  - Interface name
  - IP addresses (up to 2 displayed)
  - Download/upload rates
  - Number of processes using the interface
- Press **↑/↓** to navigate
- Press **Enter** to drill into interface details
- Press **'i'** to return to process view

#### InterfaceDetail

- Shows all processes using the selected interface
- Displays per-interface bandwidth stats for each process
- Press **Esc** to return to interface list

### 4. Keyboard Shortcuts

| Key     | Action                                            |
| ------- | ------------------------------------------------- |
| `i`     | Toggle between Process View and Interface List    |
| `↑/↓`   | Navigate selection (works in all views)           |
| `Enter` | View interface details (when in Interface List)   |
| `Esc`   | Go back (from Interface Detail to Interface List) |
| `q`     | Quit (or go back in Interface Detail view)        |

All existing shortcuts (t, r, g, f, b, h) continue to work in Process View.

### 5. UI Updates

**Status Bar**: Now shows both process count and interface count

```
Monitoring 15 process(es) on 3 interface(s)
```

**Interface List Display**:

```
┌─ Network Interfaces [Press 'i' for process view | Enter to view details] ─┐
│   Interface    IP Address                     DL Rate    UL Rate    Processes│
│ ▶ ✓ wlan0      192.168.1.50                   1.2 MB/s   500 KB/s   5 proc   │
│   ✓ eth0       10.0.0.15                      100 KB/s   50 KB/s    2 proc   │
│   ⟲ lo         127.0.0.1                      50 KB/s    50 KB/s    3 proc   │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Interface Detail Display**:

```
┌─ Interface: wlan0 [Press Esc to go back] ──────────────────────────────────┐
│ PID     Process              DL Rate    UL Rate    Total DL   Total UL   Status│
│   1234  firefox              800 KB/s   300 KB/s   15.2 MB    3.5 MB      │
│   5678  spotify              400 KB/s   200 KB/s   8.1 MB     2.1 MB      │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Implementation Details

### Network Monitor Changes (`monitor.rs`)

1. **Multi-threaded Capture**:
   - `find_all_interfaces()`: Returns all active interfaces (not just one)
   - `capture_packets_on_interface()`: Per-interface capture thread
   - Each packet is tagged with its interface name

2. **Tracking Data**:
   - `interface_bandwidth`: Interface-level statistics
   - `process_interface_bandwidth`: Per-process, per-interface statistics
   - Allows answering questions like:
     - "What's the total bandwidth on eth0?"
     - "How much is Firefox using on wlan0 specifically?"

3. **Return Value**:
   - `update()` now returns `(ProcessMap, InterfaceMap)` tuple
   - Provides both process-centric and interface-centric views of the same data

### UI State Changes (`ui.rs`)

1. **New AppState fields**:
   - `view_mode`: Tracks current view (ProcessView/InterfaceList/InterfaceDetail)
   - `interface_list`: Cached list of interfaces for display
   - `interface_list_state`: Selection state for interface list
   - `selected_interface_name`: Interface being viewed in detail mode

2. **New methods**:
   - `toggle_view_mode()`: Switch between views
   - `update_interfaces()`: Update interface list from network monitor
   - `select_next_interface()`, `select_previous_interface()`: Navigation
   - `enter_interface_detail()`, `exit_interface_detail()`: Drill-down

## Use Cases

### Network Troubleshooting

- Identify which interface is saturated
- See if traffic is going through VPN tunnel vs direct connection
- Monitor container bridge interfaces separately from physical interfaces

### VPN Analysis

- Compare bandwidth on physical interface (e.g., wlan0) vs VPN tunnel (e.g., tun0)
- Verify traffic is actually going through VPN

### Container/Docker Monitoring

- See traffic on docker0 bridge separately
- Monitor individual container veth interfaces

### WiFi vs Ethernet Comparison

- Compare performance between wired and wireless connections
- Identify which applications prefer which interface

### Multi-Interface Systems

- Systems with multiple NICs can see per-NIC utilization
- Useful for routers, servers, or laptops with both WiFi and Ethernet

## Testing

Build with:

```bash
cargo build --release
```

Run with root privileges (required for packet capture):

```bash
sudo ./target/release/chadthrottle
```

Press **'i'** to toggle between process view and interface view.

## Future Enhancements

Potential additions:

- Filter process view by interface
- Sort interfaces by bandwidth
- Show interface type (Ethernet, WiFi, Loopback, VPN, etc.)
- Historical graphs per interface
- Interface-specific throttling
