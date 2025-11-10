# Backend Modal Click Bug - Root Cause Analysis

## Problem Statement

The backend modal still has two issues:

1. Clicks leak through to the process list behind it
2. Clicking on backend items doesn't move the radio button selection

## Issue 1: Click Leakthrough

### Root Cause

The fix I applied was INCOMPLETE. I added this check:

```rust
ui::ClickableRegionType::ProcessList { .. } => {
    // Only handle process list clicks in ProcessView mode
    if app.view_mode != ui::ViewMode::ProcessView {
        continue;
    }
    // ...
}
```

**But this doesn't work for backend modal because:**

1. Backend modal is a FLAG (`show_backend_info`), NOT a view mode
2. When backend modal is open, `view_mode` is still `ProcessView` (or whatever it was)
3. Draw sequence (ui.rs:1133-1180):
   ```
   - Draw based on view_mode (ProcessView draws process_list at line 1135)
   - Draw backend modal IF show_backend_info is true (line 1177-1180)
   ```
4. So both process list AND backend modal register clickable regions
5. My check `if app.view_mode != ProcessView` evaluates to FALSE
6. Process list click handler STILL RUNS even when backend modal is open!

### Correct Fix Needed

The ProcessList click handler needs to check BOTH:

1. View mode is ProcessView, AND
2. No modals are covering it

```rust
ui::ClickableRegionType::ProcessList { .. } => {
    // Only handle process list clicks when it's actually visible
    // (not covered by any modal)
    if app.view_mode != ui::ViewMode::ProcessView
        || app.show_backend_info           // Backend modal blocks it
        || app.show_help                   // Help modal blocks it
        || app.show_throttle_dialog        // Throttle dialog blocks it
        || app.show_graph                  // Graph modal blocks it
        || app.show_backend_compatibility_dialog  // Compat dialog blocks it
    {
        continue;
    }
    // ... process list click handling ...
}
```

## Issue 2: Backend Selection Not Visible

### Current Code (main.rs:1696-1698)

```rust
app.backend_selected_index = item_index;
// Update tracking field for auto-scroll optimization
app.last_backend_selection = item_index;
```

This LOOKS correct, but let me verify what the rendering code checks...

### Backend Rendering Code (ui.rs:1939)

```rust
let is_selected = index == app.backend_selected_index;
```

This should work! Let me check the auto-scroll code...

### Auto-Scroll Code (ui.rs:2401-2410)

```rust
// Auto-scroll to keep selected backend visible (only when selection changes)
let selection_changed = app.backend_selected_index != app.last_backend_selection;
if selection_changed {
    if let Some(line) = selected_line {
        app.backend_info_scroll_offset =
            AppState::scroll_to_line(app.backend_info_scroll_offset, line, backend_area.height);
    }
    app.last_backend_selection = app.backend_selected_index;
}
```

Wait! There's a problem here:

**The auto-scroll optimization updates `last_backend_selection` AFTER scrolling (line 2410)**

So the flow is:

1. User clicks → sets `backend_selected_index = 5` and `last_backend_selection = 5`
2. Next frame renders → checks if `backend_selected_index (5) != last_backend_selection (5)`
3. Selection hasn't "changed" from auto-scroll's perspective!
4. Auto-scroll doesn't run
5. Selection change IS visible, but auto-scroll might not keep it in view

Actually wait, re-reading the click handler... we ARE setting both fields. Let me trace through this more carefully.

### Detailed Flow Analysis

**When user clicks on backend:**

1. Click handler (main.rs:1696-1698):

   ```rust
   app.backend_selected_index = item_index;  // e.g., 5
   app.last_backend_selection = item_index;  // e.g., 5
   ```

2. Next frame render (ui.rs:2401-2410):

   ```rust
   let selection_changed = app.backend_selected_index != app.last_backend_selection;
   // selection_changed = (5 != 5) = FALSE
   ```

3. Auto-scroll doesn't run because selection_changed is false

4. But the visual selection SHOULD still change because:
   - Rendering checks `index == app.backend_selected_index` (line 1939)
   - We DID update `backend_selected_index`
   - So the radio button and highlighting should update

### Hypothesis

The issue might be that:

1. Selection IS changing visually
2. But auto-scroll isn't keeping it in view
3. So if you click on an item that's off-screen, it gets selected but you don't see it

OR:

The click calculation is wrong and we're selecting the wrong item!

### Testing Needed

Need to determine:

1. Is the CORRECT backend being selected (check with logging/debugging)?
2. Is the selection visible if the clicked item is already on screen?
3. Is auto-scroll the problem (selection happens but scrolls away)?

## Correct Fixes Required

### Fix 1: Prevent Process List Click When Modals Open

**File**: `main.rs` lines ~1609-1625

**Current (incomplete)**:

```rust
ui::ClickableRegionType::ProcessList { .. } => {
    // Only handle process list clicks in ProcessView mode
    if app.view_mode != ui::ViewMode::ProcessView {
        continue;
    }
    // ...
}
```

**Fixed**:

```rust
ui::ClickableRegionType::ProcessList { .. } => {
    // Only handle process list clicks when it's actually visible
    // (not covered by any overlays/modals)
    if app.view_mode != ui::ViewMode::ProcessView
        || app.show_backend_info
        || app.show_help
        || app.show_throttle_dialog
        || app.show_graph
        || app.show_backend_compatibility_dialog
    {
        continue;
    }
    // ... existing code ...
}
```

### Fix 2: Backend Selection Visibility

**Option A**: Don't update `last_backend_selection` in click handler

- Let the auto-scroll code update it
- This way selection change IS detected on next frame

```rust
// In main.rs click handler:
app.backend_selected_index = item_index;
// DON'T update last_backend_selection here
```

**Option B**: Force auto-scroll to run when clicking

- Update both fields but force scroll to selected item

```rust
// In main.rs click handler:
app.backend_selected_index = item_index;
app.last_backend_selection = item_index;
// Force scroll to ensure item is visible
// (Need to calculate selected_line here - complex)
```

**Option C (RECOMMENDED)**: Update last_backend_selection BEFORE rendering

- In click handler, set last to OLD value temporarily
- Let render code detect the change and scroll
- Then render code updates last to new value

Actually, simplest fix:

```rust
// In main.rs click handler - DON'T update last_backend_selection
app.backend_selected_index = item_index;
// Remove: app.last_backend_selection = item_index;
```

This way next frame will see the change and auto-scroll.

## Summary

**Backend Modal Has TWO Separate Issues:**

1. **Click Leakthrough**: Process list check is incomplete
   - Need to check for ALL modals, not just view_mode
   - Fix: Add checks for all modal flags

2. **Selection Visibility**: Might be auto-scroll optimization issue
   - Setting both fields at once prevents change detection
   - Fix: Don't update `last_backend_selection` in click handler
   - Let auto-scroll code detect change and update it

Both fixes are simple one-line changes.
