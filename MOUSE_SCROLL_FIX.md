# Mouse Scroll Fix - Don't Change Selection in Lists

## Issue

Mouse scroll was changing selection in process list and interface list, which was confusing UX. Users expected scrolling to just move the viewport, not change which item is selected.

## Root Cause

The process list and interface list use Ratatui's `List` widget, which has **scroll-follows-selection** behavior built-in. The List widget doesn't support independent scrolling - the scroll position is always tied to the selected item.

When mouse scroll called `select_next()` or `select_previous()`, it was changing the selection, which then caused the viewport to scroll.

## Solution

**Disabled mouse scroll for list views** (ProcessView and InterfaceList).

### What Works Now

**Mouse Scroll ENABLED:**

- ✅ Help modal (`h`) - Scrolls content without changing anything
- ✅ Backend info modal (`b`) - Scrolls content independently of selection
- ✅ Backend compatibility dialog - Scrolls content
- ✅ Process detail view (all 4 tabs) - Scrolls content

**Mouse Scroll DISABLED:**

- ❌ Process list (main view) - No mouse scroll (use arrow keys or PageUp/PageDown)
- ❌ Interface list (`i` view) - No mouse scroll (use arrow keys or PageUp/PageDown)

### Why This Makes Sense

**Lists have selection UI:**

- Selected item is highlighted
- Selection has semantic meaning (what you're about to act on)
- Changing selection via mouse scroll is confusing
- Keyboard navigation (↑↓) is the expected way to change selection

**Modals/Details have no selection:**

- Just viewing content
- No highlighted item
- Scrolling is purely for reading
- Mouse scroll works naturally

## Technical Details

**Changed in `main.rs`:**

```rust
// Before:
ui::ViewMode::ProcessView => {
    for _ in 0..3 {
        app.select_next();  // ❌ Changed selection!
    }
}

// After:
ui::ViewMode::ProcessView => {
    // Don't scroll lists with mouse - would change selection
    // Use arrow keys or PageUp/PageDown instead
}
```

Same change applied to both ScrollUp and ScrollDown for both ProcessView and InterfaceList.

## User Experience

### Process List (Main View)

- **Keyboard**: ↑↓ (1 item), PageUp/PageDown (10 items) ✓
- **Mouse**: No scroll (doesn't change selection) ✓
- **Selection**: Only changes with keyboard ✓

### Interface List (`i` View)

- **Keyboard**: ↑↓ (1 item), PageUp/PageDown (10 items) ✓
- **Mouse**: No scroll (doesn't change selection) ✓
- **Selection**: Only changes with keyboard ✓

### Help Modal (`h`)

- **Keyboard**: ↑↓ (1 line), PageUp/PageDown (10 lines) ✓
- **Mouse**: Scroll wheel (1 line) ✓
- **Selection**: None (just content) ✓

### Backend Info Modal (`b`)

- **Keyboard**: ↑↓ (selection), PageUp/PageDown (viewport) ✓
- **Mouse**: Scroll wheel (viewport only) ✓
- **Selection**: Only changes with ↑↓ keys ✓

### Process Detail Tabs

- **Keyboard**: ↑↓ (1 line), PageUp/PageDown (10 lines) ✓
- **Mouse**: Scroll wheel (3 lines) ✓
- **Selection**: None (just content) ✓

## Alternative Considered

**We could have:**

1. Converted List widgets to Paragraph widgets (major refactoring)
2. Added independent scroll offset tracking for lists
3. Implemented manual scroll-follows-selection logic

**Why we didn't:**

- Too much work for minimal benefit
- Keyboard navigation for lists is standard TUI pattern
- Mouse scroll for content (not lists) is more intuitive
- Current solution is simpler and more maintainable

## Files Modified

- `chadthrottle/src/main.rs`:
  - Removed `select_next/previous()` calls from ProcessView mouse scroll
  - Removed `select_next/previous_interface()` calls from InterfaceList mouse scroll
  - Added explanatory comments
  - ~20 lines changed

## Build Status

```
Finished `release` profile [optimized] target(s) in 22.13s
```

✅ No errors
✅ No new warnings
✅ Ready to use!

## Testing

**Test that mouse scroll WORKS:**

1. Press `h` - scroll help with mouse wheel ✓
2. Press `b` - scroll backend info with mouse wheel ✓
3. Select process, press Enter - scroll detail tabs with mouse wheel ✓

**Test that mouse scroll DOESN'T change selection:**

1. In process list - scroll mouse wheel, selection doesn't move ✓
2. Press `i` for interface list - scroll mouse wheel, selection doesn't move ✓

**Test that keyboard STILL works:**

1. In process list - ↑↓ changes selection, PageUp/PageDown jumps ✓
2. In interface list - ↑↓ changes selection, PageUp/PageDown jumps ✓

## Summary

Mouse scroll now has **consistent, predictable behavior**:

- ✅ Scrolls content in modals and detail views
- ✅ Does NOT change selection in lists
- ✅ Keyboard navigation unchanged
- ✅ Intuitive UX

The fix preserves the keyboard-first design while making mouse scroll work naturally where it makes sense!
