# Interface Filtering Feature with Session Persistence

## Overview

Added comprehensive interface filtering to ChadThrottle's TUI with session persistence, allowing users to filter the process list to show only processes using specific network interfaces. Filter preferences are automatically saved and restored across sessions.

## Features Implemented

### 1. Three-State Filter System

**Filter States:**

- **`None`** (default) → Show ALL processes (no filtering)
- **`Some([])`** → Show NOTHING (empty filter - explicit hide all)
- **`Some(["wlan0", "eth0"])`** → Show only processes using these interfaces

This design makes filter state explicit and handles all edge cases cleanly.

### 2. Session Persistence

**Config File Storage:**

```json
{
  "throttles": {},
  "auto_restore": true,
  "preferred_upload_backend": "tc_htb",
  "preferred_download_backend": "ifb_tc",
  "filtered_interfaces": ["wlan0", "eth0"]
}
```

**Behavior:**

- Filter preferences saved to `~/.config/chadthrottle/throttles.json`
- Automatically loaded on startup
- Automatically saved on exit (unless `--no-save` flag used)
- Also saved immediately when filter is applied

### 3. Interactive Filter Selector UI

**Modal Interface:**

```
┌─ Filter by Network Interface ──────────────────┐
│                                                 │
│ Filter by Network Interface                    │
│                                                 │
│ Current: wlan0, eth0                           │
│                                                 │
│ Select which interfaces to show:               │
│                                                 │
│   ▶ [✓] wlan0        (5 processes)             │
│     [✓] eth0         (2 processes)             │
│     [ ] lo           (3 processes)             │
│     [ ] docker0      (1 process)               │
│                                                 │
│ [↑↓] Navigate  [Space] Toggle  [A] All         │
│ [N] None  [Enter] Apply  [Esc] Cancel          │
└─────────────────────────────────────────────────┘
```

**Features:**

- Multi-select interface list
- Shows process count per interface
- Current filter state displayed at top
- Real-time checkbox updates

### 4. Keyboard Controls

**Opening Filter Selector:**

- **`Shift+F`** or **`F`** (capital) - Open interface filter modal

**Within Filter Modal:**

- **`↑/↓` or `k/j`** - Navigate interfaces
- **`Space`** - Toggle selected interface on/off
- **`A`** - Select all interfaces (clears filter → show all)
- **`N`** - Deselect all (empty filter → show nothing)
- **`Enter`** - Apply filter and close
- **`Esc`** - Cancel and close without applying

### 5. Visual Filter Indicators

**Status Bar Updates:**

No filter (default):

```
Monitoring 15 process(es) on 3 interface(s)
```

Active filter:

```
FILTER: wlan0, eth0 | Monitoring 8 process(es) on 3 interface(s)
```

Empty filter:

```
FILTER: None (showing 0 processes) | Monitoring 0 process(es) on 3 interface(s)
```

Multiple interfaces (truncated):

```
FILTER: wlan0, eth0 +2 more | Monitoring 12 process(es) on 3 interface(s)
```

**Color Coding:**

- Active filter: **Cyan** with bold text
- Empty filter: **Red** with bold text (warning indicator)
- Normal status: Gray text

### 6. Smart Filtering Logic

**Process Inclusion Rules:**

- Process is shown if it uses **ANY** of the filtered interfaces
- A process using multiple interfaces (e.g., wlan0 AND docker0) is shown if ANY of its interfaces match
- In process view, all interface stats for the process are still visible
- Interface view is never filtered (always shows all interfaces)

**Example:**

- Filter: `["wlan0"]`
- Firefox uses: wlan0, lo
- Result: Firefox is shown (uses wlan0)
- Stats displayed: Both wlan0 and lo stats for Firefox

### 7. Filter State Transitions

| Current State | User Action         | New State          | Result         |
| ------------- | ------------------- | ------------------ | -------------- |
| `None`        | Select some, apply  | `Some([selected])` | Shows filtered |
| `None`        | Press N, apply      | `Some([])`         | Shows nothing  |
| `Some([...])` | Press A, apply      | `None`             | Shows all      |
| `Some([...])` | Deselect all, apply | `Some([])`         | Shows nothing  |
| `Some([])`    | Press A, apply      | `None`             | Shows all      |
| `Some([])`    | Select some, apply  | `Some([selected])` | Shows filtered |

## Implementation Details

### Files Modified

1. **`config.rs`**
   - Added `filtered_interfaces: Option<Vec<String>>` field
   - Updated serialization/deserialization
   - Added default implementation

2. **`ui.rs`**
   - Added `InterfaceFilterSelector` struct
   - Added `InterfaceFilterItem` struct
   - Added filter state to `AppState`
   - Implemented `update_filter_selector()` - Populate modal with interfaces
   - Implemented `apply_interface_filters()` - Apply selected filters
   - Implemented `filter_select_all()`, `filter_deselect_all()`, `filter_toggle_selected()`
   - Implemented `filter_select_next()`, `filter_select_previous()` - Navigation
   - Implemented `apply_process_filter()` - Filter process list logic
   - Added `draw_interface_filter_selector()` - Modal UI rendering
   - Updated `draw_status_bar()` - Show filter status
   - Updated `update_processes()` - Apply filter before rendering

3. **`main.rs`**
   - Added event handling for filter modal (Space, A, N, Enter, Esc, navigation)
   - Added `Shift+F` keybinding to open filter
   - Load filter from config on startup
   - Save filter to config on exit
   - Save filter immediately when applied

4. **`keybindings.rs`**
   - Added `Shift+F` keybinding definition
   - Updated status bar keybindings
   - Updated help screen

### Filter Application Flow

1. **User opens filter:** `Shift+F`
2. **System populates modal:** Reads current `interface_list` and `active_interface_filters`
3. **User makes selections:** Space to toggle, A for all, N for none
4. **User applies:** Enter
5. **System updates state:**
   - Determines new filter state (None, Some([]), or Some([...]))
   - Updates `app.active_interface_filters`
   - Saves to config immediately
6. **Process list updates:** Next refresh applies filter via `apply_process_filter()`

### Persistence Flow

**Startup:**

```rust
// Load config
let config = Config::load();

// Restore filter
if let Some(filters) = config.filtered_interfaces {
    app.active_interface_filters = Some(filters);
}
```

**Runtime:**

```rust
// When filter applied
app.apply_interface_filters();
config.filtered_interfaces = app.active_interface_filters.clone();
config.save(); // Immediate save
```

**Shutdown:**

```rust
// Save all state including filter
config.filtered_interfaces = app.active_interface_filters.clone();
config.save();
```

## Use Cases

### 1. Focus on Physical Network

- Filter to `wlan0` and `eth0` only
- Hides Docker, loopback, and VPN traffic
- Clean view of actual internet traffic

### 2. Monitor VPN Usage

- Filter to `tun0` or `wg0`
- See only processes using VPN tunnel
- Verify VPN is actually being used

### 3. Debug Docker Networking

- Filter to `docker0` and `veth*`
- Focus on container traffic
- Troubleshoot container networking issues

### 4. Isolate Loopback Traffic

- Filter to `lo` only
- See local-only processes
- Identify services communicating locally

### 5. Multi-Interface Comparison

- Filter to multiple interfaces
- Compare traffic patterns
- Identify which apps prefer which interface

### 6. Hide Everything (Debug Mode)

- Select "None" (empty filter)
- Verify filtering works
- Clean slate for testing

## Example User Flows

### Flow 1: First-Time Filtering

1. User starts ChadThrottle → sees all processes (default `None`)
2. Presses `Shift+F` → filter modal opens, all checkboxes checked
3. Unchecks `docker0` and `lo` → only `wlan0` and `eth0` remain
4. Presses Enter → filter applied, saved to config
5. Status bar shows: `FILTER: wlan0, eth0 | Monitoring 8 process(es)`
6. Exits and restarts → filter automatically restored

### Flow 2: Clearing Filter

1. User has `Some(["wlan0"])` active
2. Presses `Shift+F` → sees `wlan0` checked, others unchecked
3. Presses `A` → all interfaces checked
4. Presses Enter → `None` (show all), saved
5. Status bar returns to normal

### Flow 3: Temporary No-Show

1. User has `None` (show all)
2. Presses `Shift+F` → all checked
3. Presses `N` → all unchecked
4. Presses Enter → `Some([])`, process list empty
5. Status bar shows: `FILTER: None (showing 0 processes)` in red

## Testing

Build and run:

```bash
cargo build --release
sudo ./target/release/chadthrottle
```

**Test checklist:**

- [ ] Filter to single interface → only shows those processes
- [ ] Filter to multiple interfaces → shows combined processes
- [ ] Select all → clears filter, shows everything
- [ ] Deselect all → shows nothing
- [ ] Exit and restart → filter persists
- [ ] Change filter → new filter saved immediately
- [ ] Status bar shows correct filter state
- [ ] Process using multiple interfaces shown if ANY match

## Technical Notes

### Why Option<Vec<String>>?

Alternative designs considered:

1. **Empty Vec = show all:** Ambiguous, can't distinguish "no filter" from "empty filter"
2. **Separate bool flag:** Two pieces of state to keep in sync, error-prone
3. **Option<Vec<String>>:** Clean three-state system, explicit semantics

### Filter vs View Mode

**Important distinction:**

- **Filter:** Affects ProcessView only, persists across sessions
- **Interface View (i key):** Different view mode, not a filter
- Interface List always shows all interfaces regardless of filter
- Filter only applies when in ProcessView mode

### Performance

- Filter applied once per process list update (1Hz)
- O(n\*m) complexity where n=processes, m=interfaces per process
- Negligible overhead even with 100s of processes
- No impact on packet capture performance

## Future Enhancements

Potential additions:

- Quick filter presets ("Physical only", "VPN only", etc.)
- Regex support for interface names
- Filter by interface type (WiFi, Ethernet, Virtual)
- Visual indicator in interface list showing filtered interfaces
- Filter history/favorites
- Auto-include new interfaces option
