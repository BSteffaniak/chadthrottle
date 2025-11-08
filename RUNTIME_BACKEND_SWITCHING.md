# Runtime Backend Switching Implementation

## Overview

ChadThrottle now supports **interactive backend switching at runtime** via the TUI. Users can switch between different upload and download throttling backends on-the-fly without restarting the application.

## Key Design Decision: Non-Retroactive Backend Selection

**Important**: Switching backends only affects **future throttles**, not existing ones.

- When you throttle a process with the eBPF backend, it stays on eBPF until removed
- If you switch to `tc_htb` and throttle another process, it uses `tc_htb`
- Both throttles coexist peacefully on their respective backends
- This allows A/B testing of backends and prevents disruption of existing throttles

## How to Use

### 1. View Backend Information

Press **`b`** to open the backend information modal:

- Shows all available upload/download backends
- ⭐ Star indicates current **default** backend for new throttles
- ✅ Green checkmark = available backend
- ❌ Red X = unavailable backend
- Shows active throttle count per backend: `(3 active)`

### 2. Switch Backend

From the backend info modal, press **`Enter`** to open the backend selector:

- Use **`Tab`** to switch between Upload/Download selection
- Use **`↑↓`** or **`j/k`** to navigate between backends
- Press **`Enter`** to select a backend as the new default
- Press **`Esc`** to cancel

### 3. Apply Throttles

After switching backends:

- Press **`t`** on any process to throttle it
- The throttle will use the currently selected default backend
- Status message confirms: `✅ New throttles will use 'ebpf' backend`

## Architecture Changes

### ThrottleManager Refactoring

**Old Design** (single backend per direction):

```rust
pub struct ThrottleManager {
    upload_backend: Option<Box<dyn UploadThrottleBackend>>,
    download_backend: Option<Box<dyn DownloadThrottleBackend>>,
}
```

**New Design** (multiple concurrent backends):

```rust
pub struct ThrottleManager {
    // Pool of initialized backends (lazy-loaded)
    upload_backends: HashMap<String, Box<dyn UploadThrottleBackend>>,
    download_backends: HashMap<String, Box<dyn DownloadThrottleBackend>>,

    // Track which backend each PID uses
    upload_backend_map: HashMap<i32, String>,    // pid -> backend_name
    download_backend_map: HashMap<i32, String>,

    // Default backend for NEW throttles
    default_upload: Option<String>,
    default_download: Option<String>,
}
```

### New ThrottleManager Methods

```rust
// Set default backend for future throttles
pub fn set_default_upload_backend(&mut self, name: &str) -> Result<()>
pub fn set_default_download_backend(&mut self, name: &str) -> Result<()>

// Get backend usage statistics
pub fn get_active_backend_stats(&self) -> HashMap<String, usize>
pub fn get_pids_for_backend(&self, backend_name: &str) -> Vec<i32>
```

### Lazy Backend Initialization

Backends are only created when first used:

1. User selects `tc_htb` as default upload backend
2. No initialization happens yet
3. User throttles a process → `tc_htb` is initialized and throttle applied
4. Subsequent throttles reuse the already-initialized backend

### Routing to Correct Backend

When removing a throttle, the manager routes to the correct backend:

```rust
pub fn remove_throttle(&mut self, pid: i32) -> Result<()> {
    // Look up which backend created this throttle
    if let Some(backend_name) = self.upload_backend_map.remove(&pid) {
        if let Some(backend) = self.upload_backends.get_mut(&backend_name) {
            backend.remove_upload_throttle(pid)?;
        }
    }
    // Same for download...
}
```

## UI Components

### Backend Info Modal (`b` key)

- Shows all available backends with status
- ⭐ **[DEFAULT]** = Current default for new throttles
- Shows active throttle count: `ebpf (3 active)`
- Press **Enter** to switch backends

### Backend Selector Modal (`b` → `Enter`)

- Two-panel view: Upload | Download
- Navigation with Tab, ↑↓, j/k
- Visual indicators:
  - ⭐ Current default (yellow/bold)
  - ✅ Available (green)
  - ❌ Unavailable (grayed, non-selectable)
- Shows priority levels (Best, Better, Good, Fallback)
- Shows active throttle counts

## Configuration Persistence

Backend preferences are saved to `~/.config/chadthrottle/throttles.json`:

```json
{
  "preferred_upload_backend": "ebpf",
  "preferred_download_backend": "ebpf",
  "throttles": { ... }
}
```

On next startup, ChadThrottle automatically sets these as defaults.

## Example Workflow

```
1. Start ChadThrottle
   → eBPF auto-selected as default (highest priority)

2. Throttle Firefox (PID 1234)
   → Firefox uses eBPF backend

3. Press 'b' → see backend info
   ⭐ ebpf         [DEFAULT]    Priority: Best       (1 active)
   ✅ tc_htb       Available    Priority: Good       (0 active)

4. Press Enter → backend selector opens

5. Navigate to "tc_htb", press Enter
   → Status: "✅ New throttles will use 'tc_htb' backend"

6. Throttle Chrome (PID 5678)
   → Chrome uses tc_htb backend

7. Press 'b' again → see backend info
   ⭐ tc_htb       [DEFAULT]    Priority: Good       (1 active)
   ✅ ebpf         Available    Priority: Best       (1 active)

8. Both Firefox (eBPF) and Chrome (tc_htb) throttled simultaneously
```

## Benefits

1. **No Migration Risk**: Existing throttles are never touched
2. **True Multi-Backend**: Can run eBPF, tc_htb, ifb_tc all at once
3. **A/B Testing**: Compare backend performance side-by-side
4. **Lazy Loading**: Only initialize what you use
5. **Clean Separation**: Each backend manages its own throttles independently
6. **Safe Cleanup**: Backends cleanup when their last throttle is removed

## Technical Details

### Files Modified

1. `chadthrottle/src/backends/throttle/manager.rs` - Multi-backend support
2. `chadthrottle/src/backends/throttle/mod.rs` - Public backend creation functions
3. `chadthrottle/src/ui.rs` - Backend selector modal and state
4. `chadthrottle/src/main.rs` - Keyboard handlers for backend switching
5. `chadthrottle/src/keybindings.rs` - Updated documentation
6. `chadthrottle/src/config.rs` - Already had preference fields

### Cleanup Strategy

- Backends remain alive as long as they have active throttles
- On app exit, all backends cleanup (existing Drop implementation)
- Removing the last throttle doesn't cleanup the backend (it might be selected as default)

### Error Handling

- Invalid backend selection → rejected with error message
- Backend initialization failure → error returned to user
- Unavailable backends → grayed out in UI, can't be selected

## Future Enhancements

Potential improvements:

1. Show which specific processes use each backend in info modal
2. "Migrate all throttles to X backend" command
3. Backend performance metrics (latency, accuracy)
4. Per-process backend override in throttle dialog
5. Backend health monitoring and auto-fallback

## Compatibility

This feature is **fully backward compatible**:

- CLI mode (`--upload-backend`, `--download-backend`) still works
- Existing config files load correctly
- `--list-backends` shows all available backends
- All existing backends continue to work as before

The TUI simply adds an interactive layer on top of the existing backend selection system.
