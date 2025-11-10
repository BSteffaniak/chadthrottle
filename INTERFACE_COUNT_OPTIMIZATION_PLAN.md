# Interface Process Count Optimization Plan

## Problem

The interface modal performs O(n×m) operations every frame (60 FPS):

- For each interface (n=10), iterate all processes twice (m=200+100)
- Total: 3,000 iterations per frame = 180,000 iterations per second
- Location: `ui.rs` lines 2790-2801

## Current Code (Inefficient)

```rust
// Calculate total count from unfiltered list (all processes using this interface)
let total_count = app
    .unfiltered_process_list
    .iter()
    .filter(|p| p.interface_stats.contains_key(&iface.name))
    .count();

// Calculate filtered count (how many are currently visible in the filtered view)
let filtered_count = app
    .process_list
    .iter()
    .filter(|p| p.interface_stats.contains_key(&iface.name))
    .count();
```

## Solution: Cache Counts in InterfaceInfo

### Step 1: Add count fields to InterfaceInfo

**File**: `chadthrottle/src/process.rs`

Find `InterfaceInfo` struct and add:

```rust
pub struct InterfaceInfo {
    pub name: String,
    pub total_download_rate: u64,
    pub total_upload_rate: u64,
    pub process_count: usize,
    // Add these new fields:
    pub total_process_count: usize,    // Processes in unfiltered list
    pub filtered_process_count: usize, // Processes in filtered list
}
```

### Step 2: Calculate counts when updating interface map

**File**: `chadthrottle/src/main.rs` (in update loop around line 1610-1750)

After getting `interface_map` from monitor.update(), calculate counts:

```rust
// After line: Ok((mut process_map, interface_map)) => {

// Calculate process counts for each interface
let mut interface_counts: HashMap<String, (usize, usize)> = HashMap::new();
for iface_name in interface_map.keys() {
    let total = app.unfiltered_process_list
        .iter()
        .filter(|p| p.interface_stats.contains_key(iface_name))
        .count();
    let filtered = app.process_list
        .iter()
        .filter(|p| p.interface_stats.contains_key(iface_name))
        .count();
    interface_counts.insert(iface_name.clone(), (total, filtered));
}

// Update interface_map with counts
let mut interface_map_with_counts = interface_map;
for (name, info) in interface_map_with_counts.iter_mut() {
    if let Some((total, filtered)) = interface_counts.get(name) {
        info.total_process_count = *total;
        info.filtered_process_count = *filtered;
    }
}

// Update app with the enriched interface map
app.update_interfaces(interface_map_with_counts);
```

### Step 3: Use cached counts in modal

**File**: `chadthrottle/src/ui.rs` lines 2790-2801

Replace expensive iteration with cached values:

```rust
// Use cached counts (calculated once per update, not per frame!)
let total_count = iface.total_process_count;
let filtered_count = iface.filtered_process_count;
```

## Performance Impact

### Before:

- 10 interfaces × (200 + 100) processes = 3,000 iterations
- 60 FPS = 180,000 iterations/second
- Happens while modal is open

### After:

- Counts calculated once per second during update
- Modal reads cached values (O(1) per interface)
- 10 interfaces × 2 lookups = 20 operations per frame
- 99.3% reduction in operations

## Implementation Notes

- Counts only need to be updated when process list changes (every ~1 second)
- Not every frame (60 times per second)
- This is separate from the critical fix (removing background draw)
- Can be done as follow-up optimization after critical fix

## Combined Impact

1. **Critical Fix**: Remove background process list draw
   - Eliminates hundreds of process renderings per second
   - Should fix multi-second lag

2. **Count Optimization**: Cache interface process counts
   - Eliminates thousands of iterations per second
   - Further improves modal responsiveness

Both fixes together should make the modal instantly responsive.
