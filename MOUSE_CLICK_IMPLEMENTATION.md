# Mouse Click Selection - Implementation Guide

## Goal

Allow users to click on UI elements to select them (without triggering drill-down actions).

## Implementation Steps

### Step 1: Add Data Structures for Click Tracking

**File**: `chadthrottle/src/ui.rs`

Add after the `TrafficViewMode` enum (around line 83):

```rust
#[derive(Debug, Clone)]
pub struct ClickableRegion {
    pub area: Rect,
    pub region_type: ClickableRegionType,
}

#[derive(Debug, Clone)]
pub enum ClickableRegionType {
    ProcessList {
        first_visible_index: usize, // Index of first visible item in list
        visible_count: usize,        // Number of visible items
    },
    ProcessDetailTabs {
        // Column ranges for each tab: (start_col, end_col, tab)
        tab_ranges: Vec<(u16, u16, ProcessDetailTab)>,
    },
    InterfaceModal {
        header_lines: usize, // Number of header lines before interface list starts
    },
    BackendModal {
        // Maps visual line (accounting for scroll) to backend item index
        line_to_item: std::collections::HashMap<usize, usize>,
    },
}
```

Add to `AppState` struct (around line 60):

```rust
pub struct AppState {
    // ... existing fields ...
    pub last_interface_selection: Option<usize>,
    pub last_backend_selection: usize,
    // Add this new field:
    pub clickable_regions: Vec<ClickableRegion>,
}
```

Initialize in `AppState::new()` (around line 326):

```rust
impl AppState {
    pub fn new() -> Self {
        // ... existing initialization ...
        Self {
            // ... existing fields ...
            last_interface_selection: None,
            last_backend_selection: 0,
            clickable_regions: Vec::new(), // Add this
        }
    }
}
```

### Step 2: Track Process List Click Region

**File**: `chadthrottle/src/ui.rs`

In `draw_process_list()` function, after rendering the list (around line 1348), add:

```rust
f.render_stateful_widget(list, inner_list_area, &mut app.list_state);

// Store clickable region for mouse selection
let first_visible = app.list_state.offset();
let visible_count = inner_list_area.height.saturating_sub(1) as usize; // -1 for potential scrollbar
app.clickable_regions.push(ClickableRegion {
    area: inner_list_area,
    region_type: ClickableRegionType::ProcessList {
        first_visible_index: first_visible,
        visible_count,
    },
});
```

### Step 3: Track Process Detail Tab Click Regions

**File**: `chadthrottle/src/ui.rs`

In `draw_detail_header_with_tabs()` function (around line 2910-2951), after building the header, add:

```rust
let header_widget = Paragraph::new(Line::from(spans)).block(
    Block::default()
        .borders(Borders::ALL)
        .title("Process Details"),
);
f.render_widget(header_widget, area);

// Calculate tab click regions
let mut tab_ranges = Vec::new();
let base_col = area.x + 2; // Account for border and padding
let mut current_col = base_col;

// Skip past "Process Name (PID XXX)  " part
let prefix = format!(" {}  ", process.name);
current_col += prefix.width() as u16;
current_col += format!("(PID {})  ", process.pid).width() as u16;

let tabs = ["Overview", "Connections", "Traffic", "System"];
for (i, tab_name) in tabs.iter().enumerate() {
    let tab_enum = match i {
        0 => ProcessDetailTab::Overview,
        1 => ProcessDetailTab::Connections,
        2 => ProcessDetailTab::Traffic,
        3 => ProcessDetailTab::System,
        _ => ProcessDetailTab::Overview,
    };

    // Tab format: "[TabName]" with space after (except last)
    let start_col = current_col;
    let tab_text = format!("[{}]", tab_name);
    let end_col = current_col + tab_text.width() as u16;

    tab_ranges.push((start_col, end_col, tab_enum));

    current_col = end_col;
    if i < tabs.len() - 1 {
        current_col += 1; // Space between tabs
    }
}

// Store clickable region (find app reference - may need to pass as parameter)
// This will need app to be passed to draw_detail_header_with_tabs
```

**Note**: Need to modify `draw_detail_header_with_tabs` signature to accept `app: &mut AppState` parameter.

### Step 4: Track Interface Modal Click Region

**File**: `chadthrottle/src/ui.rs`

In `draw_interface_modal()` function, after rendering (around line 2860), add:

```rust
f.render_widget(Clear, modal_area);
f.render_widget(widget, modal_area);

// Store clickable region for mouse selection
app.clickable_regions.push(ClickableRegion {
    area: modal_area,
    region_type: ClickableRegionType::InterfaceModal {
        header_lines: 7, // Same as used in auto-scroll calculation
    },
});
```

### Step 5: Track Backend Modal Click Region

**File**: `chadthrottle/src/ui.rs`

In `draw_backend_info()` function, build line-to-index map while iterating items (around line 1867):

```rust
let mut current_line = text.len();
let mut selected_line: Option<usize> = None;
let mut line_to_item: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();

// Render all items (group headers and backends) with radio buttons
for (index, item) in app.backend_items.iter().enumerate() {
    match item {
        BackendSelectorItem::GroupHeader(group) => {
            // ... existing code ...
            current_line += 1;
            // Group headers are not clickable, don't add to map
        }
        BackendSelectorItem::Backend { ... } => {
            // ... existing code ...

            // Map this line to the backend item index
            line_to_item.insert(current_line, index);

            if is_selected {
                selected_line = Some(current_line);
            }

            // ... rest of existing code ...
            current_line += 1;
        }
    }
}
```

After rendering (around line 2392), store the clickable region:

```rust
f.render_widget(Clear, backend_area);
f.render_widget(backend_widget, backend_area);

// Store clickable region for mouse selection
app.clickable_regions.push(ClickableRegion {
    area: backend_area,
    region_type: ClickableRegionType::BackendModal { line_to_item },
});
```

### Step 6: Add MouseButton Import

**File**: `chadthrottle/src/main.rs` line 144-147

Change:

```rust
event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEvent, MouseEventKind,
},
```

To:

```rust
event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
},
```

### Step 7: Clear Clickable Regions at Start of Each Frame

**File**: `chadthrottle/src/ui.rs`

In `draw_ui_with_backend_info()` function (around line 1086), add at the start:

```rust
pub fn draw_ui_with_backend_info(
    f: &mut Frame,
    app: &mut AppState,
    backend_info: Option<&BackendInfo>,
) {
    // Clear clickable regions from previous frame
    app.clickable_regions.clear();

    // ... rest of existing code ...
}
```

### Step 8: Add Mouse Click Handler

**File**: `chadthrottle/src/main.rs`

In the mouse event handler (around line 1595), replace:

```rust
_ => {
    // Ignore other mouse events (clicks, moves, etc.)
}
```

With:

```rust
MouseEventKind::Down(MouseButton::Left) => {
    // Handle left mouse click for selection
    let click_x = mouse.column;
    let click_y = mouse.row;

    // Check each clickable region
    for region in &app.clickable_regions {
        // Check if click is within region bounds
        if click_x >= region.area.x
            && click_x < region.area.x + region.area.width
            && click_y >= region.area.y
            && click_y < region.area.y + region.area.height
        {
            match &region.region_type {
                ui::ClickableRegionType::ProcessList {
                    first_visible_index,
                    visible_count,
                } => {
                    // Calculate which list item was clicked
                    // Account for border (y+1) and header row
                    let relative_y = click_y.saturating_sub(region.area.y + 1);
                    let clicked_index = first_visible_index + relative_y as usize;

                    // Validate index is within bounds
                    if clicked_index < app.process_list.len() {
                        app.selected_index = Some(clicked_index);
                        app.list_state.select(Some(clicked_index));
                    }
                }
                ui::ClickableRegionType::ProcessDetailTabs { tab_ranges } => {
                    // Find which tab was clicked based on column
                    for (start_col, end_col, tab) in tab_ranges {
                        if click_x >= *start_col && click_x < *end_col {
                            app.detail_tab = *tab;
                            app.reset_detail_scroll(); // Reset scroll when switching tabs
                            break;
                        }
                    }
                }
                ui::ClickableRegionType::InterfaceModal { header_lines } => {
                    // Calculate which interface was clicked
                    // Account for border and header lines
                    let relative_y =
                        click_y.saturating_sub(region.area.y + 1);

                    if relative_y >= *header_lines as u16 {
                        let clicked_index =
                            (relative_y - *header_lines as u16) as usize;

                        // Validate index is within bounds
                        if clicked_index < app.interface_list.len() {
                            app.selected_interface_index = Some(clicked_index);
                        }
                    }
                }
                ui::ClickableRegionType::BackendModal { line_to_item } => {
                    // Calculate which line was clicked (accounting for scroll)
                    let relative_y =
                        click_y.saturating_sub(region.area.y + 1);
                    let visual_line =
                        relative_y as usize + app.backend_info_scroll_offset;

                    // Look up which backend item this line corresponds to
                    if let Some(&item_index) = line_to_item.get(&visual_line) {
                        // Make sure it's a backend (not a group header)
                        if matches!(
                            app.backend_items.get(item_index),
                            Some(ui::BackendSelectorItem::Backend { .. })
                        ) {
                            app.backend_selected_index = item_index;
                        }
                    }
                }
            }

            // Found matching region, stop searching
            break;
        }
    }
}
_ => {
    // Ignore other mouse events (moves, etc.)
}
```

## Testing Checklist

1. **Process List**:
   - [ ] Click on process selects it (visible highlight changes)
   - [ ] Click doesn't drill down into process detail
   - [ ] Click outside list doesn't change selection
   - [ ] Click works when list is scrolled

2. **Process Detail Tabs**:
   - [ ] Click on each tab switches to that tab
   - [ ] Only clicks on tab text work (not spaces between)
   - [ ] Tab content updates correctly

3. **Interface Modal**:
   - [ ] Click on interface selects it (cursor moves)
   - [ ] Click doesn't toggle checkbox (Space still does that)
   - [ ] Click on header area doesn't select anything
   - [ ] Click works when modal is scrolled

4. **Backend Modal**:
   - [ ] Click on backend selects it
   - [ ] Click on group header does nothing
   - [ ] Click works when modal is scrolled

5. **General**:
   - [ ] All existing keyboard navigation still works
   - [ ] Mouse scroll still works
   - [ ] No crashes or panics

## Notes

- Mouse coordinates are 0-based in crossterm
- Ratatui Rect coordinates are also 0-based
- Need to account for borders (+1 for y typically)
- Need to account for scroll offsets in modals
- `unicode_width` trait provides `.width()` method for calculating text widths

## Files Modified Summary

1. `chadthrottle/src/ui.rs`:
   - Add ClickableRegion and ClickableRegionType structs
   - Add clickable_regions field to AppState
   - Clear regions at start of each frame
   - Store regions in each draw function

2. `chadthrottle/src/main.rs`:
   - Add MouseButton import
   - Add MouseEventKind::Down(MouseButton::Left) handler
   - Implement click logic for all region types
