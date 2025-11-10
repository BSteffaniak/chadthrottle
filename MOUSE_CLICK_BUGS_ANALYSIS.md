# Mouse Click Selection - Bug Analysis

## Issues Identified

### Bug 1: Clicks Leak Through Modals to Process List Behind

**Symptom**: When a modal is open (interface modal, backend modal), clicking inside the modal also selects processes in the background list.

**Root Cause**:

- Line 1139 in `ui.rs`: When `InterfaceList` view mode is active, BOTH process list and modal are drawn
- Process list registers its clickable region FIRST (line 1389)
- Interface modal registers its region SECOND (line 2913)
- In click handler (main.rs:1601), we iterate regions and stop at FIRST match (line 1680)
- But we're checking if click is within bounds for BOTH regions
- Since regions can overlap, the FIRST region (process list) wins

**Solution**:

1. **Option A (Best)**: Check view mode/modal state BEFORE processing clicks
   - Only allow ProcessList clicks when `ViewMode::ProcessView`
   - Only allow modal clicks when modal is open
2. **Option B**: Iterate regions in REVERSE order (last added = highest priority)
   - Modals drawn last = checked first
   - But this is fragile

3. **Option C**: Add priority to ClickableRegionType
   - Modals = priority 10
   - Normal UI = priority 1
   - Sort by priority before checking

**Recommended Fix**: Option A - check state before processing region type

### Bug 2: Modal Clicks Don't Visibly Select Items

**Symptom**: Clicking on interface names or backend names doesn't move the selection cursor/highlight.

**Root Cause - Interface Modal**:

- Line 1651-1653 in main.rs: We only update `app.selected_interface_index`
- But interface modal likely uses a different state variable for cursor position
- Need to check what state variable controls the visual cursor

**Root Cause - Backend Modal**:

- Line 1673 in main.rs: We only update `app.backend_selected_index`
- Backend modal rendering uses this for selection (line 1939 ui.rs: `index == app.backend_selected_index`)
- So this SHOULD work... need to verify auto-scroll isn't fighting us

**Investigation Needed**:

- Check if interface modal uses `selected_interface_index` or something else
- Check if auto-scroll optimization is preventing selection update from being visible
- May need to update `last_interface_selection` and `last_backend_selection` when clicking

### Bug 3: Click Areas Are Off

**Symptom**:

- Process details tabs: Can click outside tab name and still select it
- Process list: Clicking one process selects the one above it

**Root Cause - Process List (Off by One)**:
Lines 1615-1618 in main.rs:

```rust
let relative_y = click_y.saturating_sub(region.area.y + 1);
let clicked_index = first_visible_index + relative_y as usize;
```

Problem: `region.area.y` is the TOP of the inner_list_area (which already accounts for border)

- We're adding +1 again, which shifts everything down
- Clicking row 0 calculates as row 1 (selects item above)

Need to understand:

- Does `inner_list_area.y` already include the border offset?
- Looking at line 1378-1382 in ui.rs:
  ```rust
  let inner_list_area = Rect {
      x: list_area.x + 1,
      y: list_area.y,        // NO offset! Y is NOT adjusted
      width: list_area.width.saturating_sub(2),
      height: list_area.height.saturating_sub(1),
  };
  ```
- So `inner_list_area.y == list_area.y` (no border adjustment for y)
- But the list widget renders WITH a border above it
- So we DO need some offset... but maybe not +1?

**Investigation**:

- Is there a header row in the list widget?
- Line 1331 shows header is drawn separately as Paragraph
- So list starts immediately after header
- Need to check if List widget adds its own header or spacing

**Root Cause - Process Detail Tabs (Click Range Too Wide)**:
Lines 3037-3042 in ui.rs:

```rust
let start_col = current_col;
let tab_text = format!("[{}]", tab_name);
let end_col = current_col + tab_text.width() as u16;

tab_ranges.push((start_col, end_col, tab_enum));
```

Problem: We're calculating position AFTER rendering, but:

1. We need to match EXACTLY what was rendered
2. Looking at lines 3005-3007 (rendering):
   ```rust
   spans.push(Span::raw("["));
   spans.push(Span::styled(tab_name.to_string(), style));
   spans.push(Span::raw("]"));
   ```
3. These are separate spans with a space between tabs (line 2993)

The issue: We're calculating as if rendering happened differently than it actually did

- We calculate with `format!("[{}]", tab_name)` all at once
- But we actually render as 3 separate spans: `"["` + `tab_name` + `"]"`
- The spans might have different positions than we calculate

**Better approach**: Track actual column positions WHILE building spans

## Fixes Required

### Fix 1: Prevent Modal Click Leakthrough

**File**: `main.rs` lines 1608-1677

Add view mode checks:

```rust
match &region.region_type {
    ui::ClickableRegionType::ProcessList { .. } => {
        // Only handle process list clicks in ProcessView mode
        if app.view_mode != ui::ViewMode::ProcessView {
            continue; // Skip this region
        }
        // ... existing code ...
    }
    ui::ClickableRegionType::InterfaceModal { .. } => {
        // Only handle interface modal clicks when modal is open
        if app.view_mode != ui::ViewMode::InterfaceList {
            continue;
        }
        // ... existing code ...
    }
    ui::ClickableRegionType::BackendModal { .. } => {
        // Only handle backend modal clicks when modal is open
        if !app.show_backend_info {
            continue;
        }
        // ... existing code ...
    }
    // ProcessDetailTabs handled by ViewMode::ProcessDetail check
}
```

### Fix 2: Make Modal Selections Visible

**File**: `main.rs` lines 1638-1656, 1657-1676

For interface modal:

```rust
app.selected_interface_index = Some(clicked_index);
app.last_interface_selection = Some(clicked_index); // Add this
// May also need to update interface_list_state if it exists
```

For backend modal:

```rust
app.backend_selected_index = item_index;
app.last_backend_selection = item_index; // Add this
```

### Fix 3a: Fix Process List Click Offset

**File**: `main.rs` lines 1614-1618

Need to determine correct offset:

```rust
// Current (wrong):
let relative_y = click_y.saturating_sub(region.area.y + 1);

// Possible fix (need to test):
let relative_y = click_y.saturating_sub(region.area.y);
// Or if there's a header:
let relative_y = click_y.saturating_sub(region.area.y + header_height);
```

Investigation needed: Check if List widget adds internal padding/header

### Fix 3b: Fix Process Detail Tab Click Ranges

**File**: `ui.rs` lines 3018-3055

Rewrite to track actual positions:

```rust
// Build spans and track positions simultaneously
let mut tab_ranges = Vec::new();
let base_col = area.x + 2; // Account for border and padding
let mut current_col = base_col;

// Skip past "Process Name (PID XXX)  " part
for span in &spans {
    current_col += span.content.width() as u16;
}

// Now add tabs and track their positions
let tabs = ["Overview", "Connections", "Traffic", "System"];
for (i, tab_name) in tabs.iter().enumerate() {
    let tab_enum = match i { ... };

    if i > 0 {
        spans.push(Span::raw(" "));
        current_col += 1; // Space before tab
    }

    spans.push(Span::raw("["));
    let bracket_start = current_col;
    current_col += 1;

    spans.push(Span::styled(tab_name.to_string(), style));
    let text_start = current_col;
    let text_end = current_col + tab_name.width() as u16;
    current_col = text_end;

    spans.push(Span::raw("]"));
    current_col += 1;

    // Store click range for just the tab text (not brackets/spaces)
    tab_ranges.push((text_start, text_end, tab_enum));
}
```

Wait, this approach won't work because we've already built the spans above this code!

**Better approach**: Move position tracking DURING span building (lines 2970-3008)

## Testing Plan

After fixes:

1. **Modal Leakthrough**:
   - Open interface modal, click on interface - should NOT select background process
   - Open backend modal, click on backend - should NOT select background process
2. **Modal Selection Visibility**:
   - Click on interface name - cursor should move to that interface
   - Click on backend name - selection should move to that backend
3. **Click Position Accuracy**:
   - Click on each process - should select THAT process, not one above/below
   - Click on tab name - should only select when clicking on text, not spaces

## Priority

1. **HIGH**: Fix modal leakthrough (most annoying bug)
2. **HIGH**: Fix process list offset (makes clicking unreliable)
3. **MEDIUM**: Fix modal selection visibility (confusing but workable)
4. **LOW**: Fix tab click ranges (minor annoyance, still mostly works)
