# CLI Mode Implementation Complete

## Overview

ChadThrottle now supports **CLI mode** - a non-interactive mode for scripting and automation. You can throttle processes directly from the command line without using the TUI.

## New Features

### 1. CLI Arguments Added

- `--pid <PID>` - Specify process ID to throttle (enables CLI mode)
- `--download-limit <LIMIT>` - Download bandwidth limit (e.g., "1M", "500K", "1.5M")
- `--upload-limit <LIMIT>` - Upload bandwidth limit (e.g., "1M", "500K", "1.5M")
- `--duration <SECONDS>` - Optional duration in seconds (default: run until Ctrl+C)

### 2. Bandwidth Limit Format

The parser supports flexible formats:

- `500K` or `500KB` = 500 KB/s
- `1M` or `1MB` = 1 MB/s
- `1.5M` = 1.5 MB/s (decimal values supported)
- `1G` or `1GB` = 1 GB/s

### 3. CLI Mode Behavior

When `--pid` is specified:

- Skips TUI initialization completely
- Applies throttle immediately
- Prints status to stdout
- Runs until:
  - Duration expires (if `--duration` specified), or
  - Ctrl+C is pressed
- Automatically removes throttle on exit
- Returns proper exit codes for scripting

## Usage Examples

### Basic Usage

```bash
# Throttle download only
sudo ./target/release/chadthrottle --pid 1234 --download-limit 1M

# Throttle both download and upload
sudo ./target/release/chadthrottle --pid 1234 --download-limit 1M --upload-limit 500K

# Throttle for 30 seconds then auto-exit
sudo ./target/release/chadthrottle --pid 1234 --download-limit 1M --duration 30
```

### With Backend Selection

```bash
# Use specific backends
sudo ./target/release/chadthrottle \
    --pid 1234 \
    --download-limit 1M \
    --download-backend ebpf-cgroup \
    --upload-backend tc-htb
```

### Testing eBPF Backend

```bash
# Test eBPF with detailed logging
sudo RUST_LOG=debug ./target/release/chadthrottle \
    --pid $$ \
    --download-limit 1M \
    --download-backend ebpf-cgroup

# Test with legacy attach method
sudo RUST_LOG=debug CHADTHROTTLE_USE_LEGACY_ATTACH=1 ./target/release/chadthrottle \
    --pid $$ \
    --download-limit 1M \
    --download-backend ebpf-cgroup
```

### Scripting

```bash
#!/bin/bash
# Throttle Firefox while downloading large file

# Find Firefox PID
FIREFOX_PID=$(pgrep -o firefox)

if [ -z "$FIREFOX_PID" ]; then
    echo "Firefox not running"
    exit 1
fi

# Throttle to 2 MB/s for 5 minutes
echo "Throttling Firefox (PID $FIREFOX_PID) to 2 MB/s for 5 minutes..."
sudo ./target/release/chadthrottle \
    --pid "$FIREFOX_PID" \
    --download-limit 2M \
    --duration 300

echo "Throttle removed"
```

## Implementation Details

### Files Modified

1. **`chadthrottle/src/main.rs`**
   - Added CLI arguments to `Args` struct
   - Implemented `parse_bandwidth_limit()` function
   - Implemented `run_cli_mode()` async function
   - Added mode detection (CLI vs TUI) based on `--pid` presence

### Key Functions

#### `parse_bandwidth_limit(limit_str: &str) -> Result<u64>`

Parses human-readable bandwidth limits into bytes per second:

- Handles units: K, KB, M, MB, G, GB
- Supports decimal values (e.g., "1.5M")
- Returns error for invalid formats

#### `run_cli_mode(args: &Args) -> Result<()>`

Main CLI mode logic:

- Validates arguments (at least one limit required)
- Gets process name from `/proc/<pid>/comm`
- Selects appropriate backends
- Applies throttle using `ThrottleManager`
- Waits for duration or Ctrl+C
- Removes throttle on exit

### Signal Handling

Uses `tokio::signal::ctrl_c()` for graceful shutdown:

- Catches Ctrl+C
- Removes throttle before exiting
- Ensures cleanup even on interrupt

### Duration Handling

Uses `tokio::select!` for timeout:

```rust
if let Some(duration) = args.duration {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(duration)) => {
            // Duration elapsed
        }
        _ = signal::ctrl_c() => {
            // User interrupted
        }
    }
}
```

## Testing

### Test Scripts

1. **`test_cli_mode.sh`** - General CLI mode testing
2. **`test_ebpf_cli.sh`** - eBPF backend specific testing

Run with:

```bash
sudo ./test_cli_mode.sh
sudo ./test_ebpf_cli.sh
```

### Manual Testing

```bash
# 1. Build
cargo build --release

# 2. Test with current shell
sudo ./target/release/chadthrottle --pid $$ --download-limit 1M --duration 5

# 3. Test with a real process
# Terminal 1:
curl -O https://speed.hetzner.de/100MB.bin

# Terminal 2 (while curl is running):
CURL_PID=$(pgrep curl)
sudo ./target/release/chadthrottle --pid $CURL_PID --download-limit 500K
```

## Documentation Updates

### Updated Files

1. **`README.md`** - Added complete CLI mode section with examples
2. **`QUICKSTART.md`** - Added CLI mode quick start examples
3. **`test_ebpf_cli.sh`** - Script for testing eBPF backend in CLI mode
4. **`test_cli_mode.sh`** - General CLI mode test script

## Benefits

### For Users

- **Automation**: Integrate throttling into scripts and cron jobs
- **Remote use**: Works over SSH without terminal complexity
- **Quick throttling**: Throttle a process in one command
- **No learning curve**: Simple command-line interface

### For Testing

- **eBPF debugging**: Easy to test eBPF backend with different configurations
- **Backend testing**: Quickly test different backends
- **Reproducibility**: Same command always produces same result
- **CI/CD ready**: Can be used in automated testing pipelines

## Backward Compatibility

- TUI mode still works exactly as before (default when no `--pid`)
- All existing arguments preserved
- No breaking changes to existing functionality

## Future Enhancements

Possible additions:

- `--cgroup <PATH>` - Throttle by cgroup path instead of PID
- `--process-name <NAME>` - Throttle all processes matching name
- `--output json` - JSON output for scripting
- `--dry-run` - Show what would be done without applying
- Configuration file support for CLI mode

## Related to eBPF Fix

This CLI mode makes it much easier to test the eBPF backend fixes:

- Added GPL license to eBPF programs (`ingress.rs`, `egress.rs`)
- Fixed `BPF_F_ALLOW_MULTI` flag value (1 → 2)
- Implemented legacy `bpf_prog_attach` method as fallback
- All testable via `test_ebpf_cli.sh` script

## Conclusion

CLI mode is now fully implemented and documented. The feature is production-ready and makes ChadThrottle suitable for both interactive monitoring and automated throttling tasks.
