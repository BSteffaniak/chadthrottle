# Auto-Scroll for Selection in Modals - Complete

## Issue

Interface modal and Backend info modal weren't scrolling when using Up/Down to navigate selections.

**Root Cause:**

- Up/Down keys moved the **selection cursor** (which item is highlighted)
- PageUp/PageDown moved the **scroll offset** (viewport position)
- These were **independent** - you could select an off-screen item without seeing it!
- The Paragraph widget didn't auto-scroll to keep selection visible

## Solution: Auto-Scroll to Follow Selection

Implemented automatic scroll adjustment to keep the selected item visible when navigating with Up/Down.

### New Helper Method

Added `scroll_to_line()` to `AppState`:

```rust
/// Adjust scroll offset to ensure a specific line is visible
/// Returns the adjusted scroll offset
pub fn scroll_to_line(
    current_scroll: usize,
    target_line: usize,
    visible_height: u16,
) -> usize {
    let usable_height = visible_height.saturating_sub(3) as usize;

    // If target is above visible area, scroll up to show it
    if target_line < current_scroll {
        return target_line;
    }

    // If target is below visible area, scroll down to show it
    let last_visible_line = current_scroll + usable_height.saturating_sub(1);
    if target_line > last_visible_line {
        return target_line.saturating_sub(usable_height.saturating_sub(1));
    }

    // Target is already visible
    current_scroll
}
```

**How it works:**

- If target line is above viewport → scroll up to show it at top
- If target line is below viewport → scroll down to show it at bottom
- If target is already visible → don't change scroll

### Interface Modal Changes

**Before:**

- Selection moved with Up/Down
- Scroll only changed with PageUp/PageDown
- Selected item could be off-screen

**After:**

```rust
// Auto-scroll to keep selected interface visible
// Header lines: blank + title + blank + filter state + blank + "Select interfaces..." + blank = 7 lines
let header_lines = 7;
if let Some(selected_idx) = app.selected_interface_index {
    let selected_line = header_lines + selected_idx;
    app.interface_modal_scroll_offset = AppState::scroll_to_line(
        app.interface_modal_scroll_offset,
        selected_line,
        modal_area.height,
    );
}
```

**Logic:**

1. Calculate which line the selected interface is on (header_lines + index)
2. Call `scroll_to_line()` to adjust scroll if needed
3. Continue with existing clamp logic

### Backend Info Modal Changes

More complex because backend items include group headers interspersed with backends.

**Added line tracking:**

```rust
// Track line numbers for auto-scroll
let mut current_line = text.len();  // Start after initial lines
let mut selected_line: Option<usize> = None;

for (index, item) in app.backend_items.iter().enumerate() {
    match item {
        BackendSelectorItem::GroupHeader(_) => {
            if index > 0 {
                current_line += 1;  // Blank line before header
            }
            // Add header line
            current_line += 1;
        }
        BackendSelectorItem::Backend { ... } => {
            if index == app.backend_selected_index {
                selected_line = Some(current_line);
            }
            // Add backend line
            current_line += 1;
        }
    }
}

// Auto-scroll to selected backend
if let Some(line) = selected_line {
    app.backend_info_scroll_offset = AppState::scroll_to_line(
        app.backend_info_scroll_offset,
        line,
        backend_area.height,
    );
}
```

**Logic:**

1. Track line number while building text
2. Record line number when we encounter selected backend
3. After text is built, scroll to that line if needed

## Behavior Now

### Interface Modal (press `i`)

- **Up/Down**: Navigate selection + auto-scroll to keep visible
- **PageUp/PageDown**: Scroll 10 lines without changing selection
- **Selected item always visible** ✓

### Backend Info Modal (press `b`)

- **Up/Down**: Navigate selection + auto-scroll to keep visible
- **PageUp/PageDown**: Scroll 10 lines without changing selection
- **Selected backend always visible** ✓

## Technical Notes

### Why clone in backend modal?

- Fixed unused variable warning for `group` field by changing to `group: _`

### Interaction with existing scroll bounds

1. First: Auto-scroll to selection (if any)
2. Then: Clamp to content bounds
3. This ensures selected item is visible AND scroll stays within valid range

## Files Modified

- `chadthrottle/src/ui.rs`:
  - Added `scroll_to_line()` helper method
  - Updated `draw_interface_modal()` with auto-scroll
  - Updated `draw_backend_info()` with line tracking and auto-scroll

## Result

✅ Both modals now auto-scroll when navigating with Up/Down
✅ Selected items always visible
✅ PageUp/PageDown still work for manual scrolling
✅ Scroll properly bounded to content
✅ Build successful

## Testing

**Interface Modal:**

1. Press `i` to open interface modal
2. Use Up/Down to navigate - watch scroll follow selection
3. Try PageUp/PageDown for manual scrolling
4. Verify selected interface always visible

**Backend Info Modal:**

1. Press `b` to open backend modal
2. Use Up/Down to navigate backends - watch scroll follow selection
3. Try PageUp/PageDown for manual scrolling
4. Verify selected backend always visible
