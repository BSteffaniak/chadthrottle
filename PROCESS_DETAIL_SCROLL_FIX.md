# Process Detail Tabs - Scroll Bounds Fix

## Issue

Process detail tabs (Overview, Connections, Traffic, System) were scrollable but had no bounds - they could scroll infinitely past content, just like the help modal initially did.

## Root Cause

- Tabs were using `.scroll((scroll_offset as u16, 0))` ✓
- BUT they never clamped `scroll_offset` to content bounds ❌
- Unlike modals which now clamp during rendering

## Solution: Option 1 (Implemented)

Changed all 4 detail tab functions to:

1. Accept `app: &mut AppState` instead of just `scroll_offset: usize`
2. Calculate content length after building text
3. Call `AppState::clamp_scroll()` to bound the offset
4. Update `app.detail_scroll_offset` with clamped value
5. Use clamped value for `.scroll()`

## Changes Made

### 1. Updated Function Signatures

**Before:**

```rust
fn draw_detail_overview(..., scroll_offset: usize) {}
fn draw_detail_connections(..., scroll_offset: usize) {}
fn draw_detail_traffic(..., scroll_offset: usize) {}
fn draw_detail_system(..., scroll_offset: usize) {}
```

**After:**

```rust
fn draw_detail_overview(..., app: &mut AppState) {}
fn draw_detail_connections(..., app: &mut AppState) {}
fn draw_detail_traffic(..., app: &mut AppState) {}
fn draw_detail_system(..., app: &mut AppState) {}
```

### 2. Added Scroll Clamping to Each Tab

**Pattern applied to all 4 tabs:**

```rust
// Build text content
let mut text = vec![];
// ... add lines to text ...

// Clamp scroll offset to content bounds
let content_lines = text.len();
let clamped_scroll = AppState::clamp_scroll(
    app.detail_scroll_offset,
    content_lines,
    area.height
);
app.detail_scroll_offset = clamped_scroll;

// Render with clamped scroll
let paragraph = Paragraph::new(text)
    .scroll((clamped_scroll as u16, 0))
    .block(...);
```

### 3. Updated Caller (draw_process_detail)

**Changed:**

- Function signature: `&AppState` → `&mut AppState`
- Cloned process to avoid borrow checker issues
- Removed `history` extraction (moved into Overview function)
- Passed `app` instead of `scroll_offset` to all tabs

**Before:**

```rust
fn draw_process_detail(..., app: &AppState) {
    let process = app.get_detail_process()?;  // borrows app
    let history = &app.history;               // borrows app
    draw_detail_overview(..., process, history, scroll_offset);
}
```

**After:**

```rust
fn draw_process_detail(..., app: &mut AppState) {
    let process = app.get_detail_process()?.clone();  // clone to avoid borrow
    draw_detail_overview(..., &process, app);         // pass mutable app
    // history extracted inside Overview function
}
```

### 4. Removed Manual Scrolling in Connections Tab

The Connections tab had old manual window slicing code that was incompatible with Paragraph's `.scroll()`:

**Removed:**

```rust
let visible_height = area.height.saturating_sub(10);
let start_idx = scroll_offset.min(sorted_conns.len().saturating_sub(1));
let end_idx = (start_idx + visible_height).min(sorted_conns.len());
for conn in &sorted_conns[start_idx..end_idx] { ... }
```

**Replaced with:**

```rust
for conn in &sorted_conns { ... }  // Render all, let Paragraph handle scrolling
```

Also removed pagination text that referenced `start_idx` and `end_idx`.

## Borrow Checker Challenges

**Challenge:** Can't borrow `app` immutably (for `process`, `history`) and mutably (to pass to tab functions) at the same time.

**Solutions Applied:**

1. Clone `process` before passing mutable `app` to tab functions
2. Move `history` extraction into `draw_detail_overview` function
3. Pass `&process` (reference to cloned value) to functions

## Files Modified

- `chadthrottle/src/ui.rs`:
  - `draw_process_detail` - Changed signature, cloned process
  - `draw_detail_overview` - Changed signature, added clamping
  - `draw_detail_connections` - Changed signature, added clamping, removed manual scrolling
  - `draw_detail_traffic` - Changed signature, added clamping
  - `draw_detail_system` - Changed signature, added clamping

## Result

✅ All 4 process detail tabs now have properly bounded scrolling
✅ Cannot scroll past content
✅ Consistent with modal behavior
✅ Build successful with no errors

## Testing

Test each tab:

1. Select a process, press Enter
2. Navigate tabs with Tab
3. Try scrolling past content with ↑↓
4. Verify it stops at content bounds
