# TC Police Download Backend Implementation

## Summary

Implemented a new download throttling backend using TC (Traffic Control) Police action that works without requiring the IFB (Intermediate Functional Block) kernel module.

## What Was Added

### 1. New Backend: `TcPoliceDownload`
**File:** `src/backends/throttle/download/linux/tc_police.rs`

A fallback download throttling implementation that:
- Uses TC's `police` action directly on the ingress qdisc
- Does NOT require the IFB kernel module
- Works on systems where IFB is unavailable or cannot be loaded

**Key Characteristics:**
- Priority: `Fallback` (lower than IFB+TC which is `Good`)
- IPv4 Support: ✅ Yes
- IPv6 Support: ❌ Limited (police action has limited IPv6 support)
- Per-Process Filtering: ❌ No (limitation of this approach)
- Per-Connection Filtering: ❌ No

**Important Limitation:**
The TC Police backend cannot filter traffic by process/PID because it doesn't use cgroups. It applies a global rate limit on all ingress traffic on the interface. This is documented in the code with a warning log message.

### 2. Updated Files

**`Cargo.toml`:**
- Added `throttle-tc-police` feature flag
- Enabled by default alongside other throttle backends

**`src/backends/throttle/download/linux/mod.rs`:**
- Added conditional compilation for `tc_police` module

**`src/backends/throttle/mod.rs`:**
- Updated `detect_download_backends()` to include TC Police
- Updated `create_download_backend()` to instantiate TC Police backend
- TC Police is automatically selected when IFB is unavailable

## Backend Selection Priority

Download backends are now selected in this order:

1. **IFB + TC HTB** (`ifb_tc`) - Priority: Good
   - Requires: TC + cgroups + IFB module
   - Per-process filtering: ✅
   - Flexible and feature-rich

2. **TC Police** (`tc_police`) - Priority: Fallback
   - Requires: TC only
   - Per-process filtering: ❌
   - Global rate limiting
   - Works when IFB unavailable

## How It Works

### Setup Process
1. Creates an ingress qdisc on the network interface
2. No IFB device creation needed

### Throttling Process
1. When throttling is requested for a process:
   - Adds a TC filter with police action on the ingress qdisc
   - Sets rate limit in bits/sec
   - Matches ALL traffic (cannot filter by process)
   - Drops packets exceeding the rate limit

### Cleanup Process
1. Removes TC filters from ingress qdisc
2. Removes ingress qdisc from interface

## Usage

The backend is automatically selected based on availability:

```rust
// Automatic selection (prefers IFB+TC, falls back to TC Police)
let download_backend = select_download_backend(None);

// Manual selection
let download_backend = select_download_backend(Some("tc_police"));
```

## Testing

### Build Verification
```bash
cargo build
# Should compile successfully with no errors
```

### Runtime Detection
When you run ChadThrottle, it will show which backend is selected:
```
🔥 ChadThrottle v0.6.0 - Backend Status:

  ✅ Download throttling: tc_police_download (available)
```

Or if IFB is available:
```
  ✅ Download throttling: ifb_tc_download (available)
```

## Future Improvements

Potential enhancements for TC Police backend:

1. **IP-based Filtering:** Use `/proc/net/tcp` to find which IP:port pairs belong to a process, then create filters matching those specific connections

2. **IPv6 Support:** Improve IPv6 support in police action filters

3. **Better Rate Control:** Experiment with different burst sizes and more sophisticated rate limiting

4. **Per-Process via eBPF:** Eventually replace with eBPF-based solution that can do per-process filtering without IFB

## References

- [TC Police Documentation](https://man7.org/linux/man-pages/man8/tc-police.8.html)
- [Traffic Control HOWTO](https://tldp.org/HOWTO/Traffic-Control-HOWTO/)

## Status

✅ **COMPLETE** - TC Police backend is fully implemented and integrated
- Compiles successfully
- Feature flag added
- Backend selection logic updated
- Documentation complete

Next step: Implement CLI arguments for backend selection (`--backend`, `--list-backends`)
