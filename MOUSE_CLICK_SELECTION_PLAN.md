# Mouse Click Selection Implementation Plan

## Overview

Add mouse click selection support across the UI to allow users to select items by clicking them, without triggering their associated actions (no drill-down).

## Current Mouse Handling

- Location: `main.rs` lines 1528-1598
- Currently handles: ScrollUp, ScrollDown
- Ignores: All click events (line 1595-1597)

## Root Cause of Lag (Critical - Fix First!)

**Before implementing mouse clicks, must fix performance issue:**

- Location: `ui.rs` lines 1106-1110
- Problem: Draws entire process list (200+ processes) 60x/second behind modal
- Impact: Multi-second lag when interface modal is open
- Fix: Remove `draw_process_list()` call when in InterfaceList mode

## Implementation Plan

### Phase 1: Fix Critical Performance Issue (URGENT)

**File**: `chadthrottle/src/ui.rs` lines 1106-1110

**Current Code**:

```rust
ViewMode::InterfaceList => {
    // Draw process list in background so we can see live updates
    draw_process_list(f, chunks[1], app);
    // Draw interface modal overlay on top
    draw_interface_modal(f, f.area(), app);
}
```

**Fixed Code**:

```rust
ViewMode::InterfaceList => {
    // Don't draw process list in background - it's hidden and causes lag
    // Just draw the interface modal
    draw_interface_modal(f, f.area(), app);
}
```

### Phase 2: Mouse Click Selection - Process List

**Target**: Click on process to select it (don't drill down)

**Key Information**:

- Process list rendered at: `ui.rs` line 1348
- Uses `List` widget with `render_stateful_widget`
- Inner area: `Rect { x: list_area.x + 1, y: list_area.y, width: w-2, height: h-1 }`
- Each process = 1 line in the list
- List starts after header area

**Implementation**:

1. In `main.rs` mouse event handler, add `MouseEventKind::Down(MouseButton::Left)`
2. Get mouse coordinates: `mouse.column`, `mouse.row`
3. Check if click is within process list area (need to pass area bounds or calculate)
4. Calculate which list item was clicked: `clicked_index = mouse.row - list_area.y`
5. Update `app.selected_index` and `app.list_state.select(Some(clicked_index))`
6. Only do this when `ViewMode::ProcessView`

**Challenges**:

- Need to know the list area bounds in main.rs (currently only known in ui.rs)
- Solution: Store clickable regions in AppState, update during draw

### Phase 3: Mouse Click Selection - Process Detail Tabs

**Target**: Click on tab name to switch tabs

**Key Information**:

- Tabs rendered in header at: `ui.rs` lines 2915-2943
- Tab names: "[Overview]", "[Connections]", "[Traffic]", "[System]"
- Tabs are in a single line with spaces between them
- Format: "Process Name (PID 1234) [Overview] [Connections] [Traffic] [System]"

**Tab Positions** (approximate):

- Assume header starts at column 1 after "Process Name (PID XXXX) "
- Each tab: "[TabName]" with space between
- Overview: ~column 30-40
- Connections: ~column 42-56
- Traffic: ~column 58-68
- System: ~column 70-80

**Implementation**:

1. Detect click in ProcessDetail view mode
2. Check if `mouse.row == detail_area.y + 1` (header line with tabs)
3. Calculate which tab based on mouse.column:
   - Parse header text to find "[" positions
   - Match column to tab boundaries
4. Update `app.detail_tab` to selected tab

**Challenges**:

- Tab positions vary based on process name length
- Solution: Store tab click regions when rendering header

### Phase 4: Mouse Click Selection - Interface List

**Target**: Click on interface in the interface filter modal

**Key Information**:

- Interface modal at: `ui.rs` lines 2733-2868
- Uses `Paragraph` widget (not List)
- Modal area: `centered_rect(70, 60, area)` (line 2828)
- Interfaces start at line: header_lines = 7 (line 2832)
- Each interface = 1 line

**Implementation**:

1. Detect click when `ViewMode::InterfaceList`
2. Check if click within modal bounds
3. Calculate line: `line = mouse.row - modal_area.y`
4. If line >= header_lines: `clicked_index = line - header_lines`
5. Update `app.selected_interface_index = Some(clicked_index)`

### Phase 5: Mouse Click Selection - Backend Modal

**Target**: Click on backend item to select it

**Key Information**:

- Backend modal at: `ui.rs` lines 1848-2392
- Uses `Paragraph` widget with mixed content (headers + backends)
- Modal area: `centered_rect(80, 80, area)` (line 2363)
- Items include group headers (not selectable) and backends (selectable)
- Line tracking: `current_line` and `selected_line` (lines 1863-1900)

**Implementation**:

1. Detect click when `show_backend_info == true`
2. Check if click within modal bounds
3. Calculate line: `line = mouse.row - backend_area.y + scroll_offset`
4. Map line to backend item index (skip group headers)
5. Update `app.backend_selected_index = mapped_index`

**Challenges**:

- Need to map visual line to item index (accounting for group headers)
- Solution: Build line-to-index map during rendering, store in AppState

## Data Structures Needed

### AppState additions:

```rust
pub struct ClickableRegion {
    pub area: Rect,
    pub region_type: ClickableRegionType,
}

pub enum ClickableRegionType {
    ProcessList { start_index: usize, items_visible: usize },
    ProcessDetailTab { tab_positions: Vec<(u16, u16, ProcessDetailTab)> }, // (start_col, end_col, tab)
    InterfaceModal { header_lines: usize },
    BackendModal { line_to_index: HashMap<usize, usize> }, // visual_line -> item_index
}

// Add to AppState:
pub clickable_regions: Vec<ClickableRegion>
```

## Testing Strategy

1. Test performance fix first - modal should be responsive
2. Test each click selection independently
3. Test edge cases:
   - Click outside bounds (should ignore)
   - Click on headers/non-selectable items (should ignore)
   - Click while scrolled (account for scroll offset)
4. Test that clicks don't trigger drill-down actions

## Files to Modify

1. `chadthrottle/src/ui.rs` - Line 1108 (CRITICAL FIX)
2. `chadthrottle/src/ui.rs` - Add clickable region tracking to draw functions
3. `chadthrottle/src/main.rs` - Add MouseEventKind::Down handler
4. `chadthrottle/src/ui.rs` - AppState struct (add clickable_regions field)

## Implementation Order

1. **CRITICAL**: Fix performance issue (remove background process list draw)
2. **HIGH**: Add process list click selection (most common use case)
3. **MEDIUM**: Add process detail tab click selection
4. **MEDIUM**: Add interface modal click selection
5. **LOW**: Add backend modal click selection (less frequently used)

## Notes

- Mouse coordinates in crossterm are 0-based
- Ratatui Rect coordinates are also 0-based
- List widget handles its own borders, need to account for that
- Paragraph widgets in modals have borders defined in Block
- Scroll offset must be added to calculate actual item position
