# Modal 'q' Key Consistency Fix

## Issue

The 'q' key doesn't close all modals consistently. Some modals support 'q' to close, others only support 'Esc'.

## Current State Analysis

### Modals That Support 'q' ✅

1. **Help Dialog** (line 614)
   - ANY key closes it (including 'q')
2. **Graph Modal** (line 755)
   - `KeyCode::Char('g') | KeyCode::Char('q') | KeyCode::Esc`
3. **Backend Info Modal** (line 744)
   - `KeyCode::Char('b') | KeyCode::Char('q') | KeyCode::Esc`
4. **Backend Compatibility Dialog** (line 911)
   - `KeyCode::Esc | KeyCode::Char('q')`

5. **Main App Exit** (line 1028)
   - `KeyCode::Char('q') | KeyCode::Esc`

### Modals That DON'T Support 'q' ❌

1. **Throttle Dialog** (line 924)
   - Only `KeyCode::Esc`
   - **This is the inconsistency!**

2. **Backend Selector** (line 622)
   - Only `KeyCode::Esc`
   - **This is also inconsistent!**

## Expected Behavior

**User expectation:** 'q' should close ANY modal/dialog in the application.

This is a common UI pattern:

- 'q' = quit/close the current context
- 'Esc' = escape/cancel the current context
- Both should do the same thing for modals

## Fix Required

### Location 1: Throttle Dialog Handler

**File:** `chadthrottle/src/main.rs`
**Line:** ~924

**Current code:**

```rust
if app.show_throttle_dialog {
    match key.code {
        KeyCode::Esc => {
            app.show_throttle_dialog = false;
            app.throttle_dialog.reset();
        }
        // ... other keys
    }
}
```

**Should be:**

```rust
if app.show_throttle_dialog {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.show_throttle_dialog = false;
            app.throttle_dialog.reset();
        }
        // ... other keys
    }
}
```

### Location 2: Backend Selector Handler

**File:** `chadthrottle/src/main.rs`
**Line:** ~622

**Current code:**

```rust
if app.show_backend_selector {
    match key.code {
        KeyCode::Esc => {
            app.show_backend_selector = false;
        }
        // ... other keys
    }
}
```

**Should be:**

```rust
if app.show_backend_selector {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.show_backend_selector = false;
        }
        // ... other keys
    }
}
```

## Testing

After applying the fix, verify:

1. **Throttle Dialog:**

   ```
   - Press 't' to open throttle dialog
   - Press 'q' → should close dialog
   - Press 't' again
   - Press 'Esc' → should still close dialog
   ```

2. **Backend Selector:**

   ```
   - Press 'b' to open backend selector
   - Press 'q' → should close selector
   - Press 'b' again
   - Press 'Esc' → should still close selector
   ```

3. **Backend Compatibility Modal:**
   ```
   - Trigger modal (eBPF + "Internet Only")
   - Press 'q' → should close modal (already works)
   - Trigger again
   - Press 'Esc' → should close modal (already works)
   ```

## Impact

- **Low risk change** - Just adds an alternate key binding
- **No behavior change** - Existing 'Esc' functionality remains
- **Improves UX** - Makes all modals consistent
- **2 lines changed** - Minimal code modification

## Priority

**Medium** - This is a UX polish issue, not a bug. Users can still close dialogs with 'Esc', but the inconsistency is confusing.

## Related Files

- `chadthrottle/src/main.rs` (lines ~622 and ~924)

## Implementation Notes

When making the change:

1. Search for exact line: `KeyCode::Esc => {` in throttle dialog section
2. Change to: `KeyCode::Esc | KeyCode::Char('q') => {`
3. Repeat for backend selector section
4. Rebuild and test both modals
