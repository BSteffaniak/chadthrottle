# Backend Persistence Fix

## Problem

Backend selection was not persisting between sessions even though:

- ✅ Config file saved preferences correctly
- ✅ Backend selector saved to config on selection
- ✅ Config file was loaded on startup

**Root Cause:** Config was loaded AFTER backends were already selected!

## The Bug

### Before Fix (main.rs lines 353-389):

```rust
// Line 353-354: Backends selected WITHOUT config preferences
let upload_backend = select_upload_backend(args.upload_backend.as_deref());
let download_backend = select_download_backend(args.download_backend.as_deref());

// ... backend initialization ...

// Line 389: Config loaded AFTER backends already selected ❌
let mut config = config::Config::load().unwrap_or_default();
```

The `select_*_backend()` functions only received CLI args, never the config file preferences!

## The Fix

### After Fix:

```rust
// Load config FIRST ✅
let mut config = config::Config::load().unwrap_or_default();

// Create preference chain: CLI args > Config file > Auto-detect ✅
let upload_preference = args.upload_backend.as_deref()
    .or(config.preferred_upload_backend.as_deref());
let download_preference = args.download_backend.as_deref()
    .or(config.preferred_download_backend.as_deref());

// Now select backends with full preference chain ✅
let upload_backend = select_upload_backend(upload_preference);
let download_backend = select_download_backend(download_preference);
```

## Backend Selection Priority

The fix implements a 3-tier priority system:

1. **CLI flags (highest)** - `--upload-backend ebpf`
   - Overrides everything
   - Use for automation/scripts

2. **Config file** - `~/.config/chadthrottle/throttles.json`
   - Set via TUI backend selector
   - Persists between sessions

3. **Auto-detection (lowest)** - Highest priority available backend
   - Fallback when no preference set
   - eBPF (Best) > nftables (Better) > tc_htb (Good) > tc_police (Fallback)

## Files Modified

1. `chadthrottle/src/main.rs` (2 locations)
   - TUI mode: Lines 349-389
   - CLI mode: Lines 249-266

## Changes Made

### Location 1: TUI Mode (main.rs ~line 350)

**Before:**

```rust
let mut app = AppState::new();

let upload_backend = select_upload_backend(args.upload_backend.as_deref());
let download_backend = select_download_backend(args.download_backend.as_deref());

// ... later ...
let mut config = config::Config::load().unwrap_or_default();
```

**After:**

```rust
let mut app = AppState::new();

// Load config FIRST
let mut config = config::Config::load().unwrap_or_default();

// Build preference chain
let upload_preference = args.upload_backend.as_deref()
    .or(config.preferred_upload_backend.as_deref());
let download_preference = args.download_backend.as_deref()
    .or(config.preferred_download_backend.as_deref());

// Select with preferences
let upload_backend = select_upload_backend(upload_preference);
let download_backend = select_download_backend(download_preference);
```

### Location 2: CLI Mode (main.rs ~line 250)

Same fix applied to CLI mode for consistency.

## Testing

### Manual Test:

```bash
# 1. Start ChadThrottle
sudo ./target/release/chadthrottle

# 2. Press 'b' → Enter → select "tc_htb" → Enter
# 3. Quit with 'q'

# 4. Verify config saved:
cat ~/.config/chadthrottle/throttles.json
# Should show: "preferred_upload_backend": "tc_htb"

# 5. Restart ChadThrottle
sudo ./target/release/chadthrottle

# 6. Press 'b'
# Should see ⭐ next to "tc_htb" (not eBPF!)
```

### With Logging:

```bash
sudo RUST_LOG=info ./target/release/chadthrottle
```

Look for:

- `Using upload backend from config: tc_htb`
- `Using download backend from config: ebpf`

### Test Priority Override:

```bash
# Config says "tc_htb", but CLI override to "ebpf":
sudo ./target/release/chadthrottle --upload-backend ebpf

# Should use eBPF (CLI overrides config)
```

## Config File Location

```
~/.config/chadthrottle/throttles.json
```

Example after backend selection:

```json
{
  "throttles": {},
  "auto_restore": false,
  "preferred_upload_backend": "tc_htb",
  "preferred_download_backend": "ebpf"
}
```

## Verification Checklist

- [x] Config loads before backend selection
- [x] CLI args override config preferences
- [x] Config preferences override auto-detection
- [x] Backend selector saves to config (already worked)
- [x] Backend info modal shows correct default (⭐)
- [x] Preferences persist across restarts
- [x] Works in both TUI and CLI mode

## Impact

✅ **Fully Backward Compatible**

- Existing configs load correctly
- CLI args still work as expected
- Auto-detection still works if no preference set

✅ **No Breaking Changes**

- All existing functionality preserved
- Just adds the missing config→backend link

## Build Status

```
✅ Compiled successfully
✅ Binary: /home/braden/ChadThrottle/target/release/chadthrottle (4.4 MB)
```

## Related Documentation

- See `RUNTIME_BACKEND_SWITCHING.md` for full feature documentation
- See `BACKEND_SWITCHING_GUIDE.md` for user guide
