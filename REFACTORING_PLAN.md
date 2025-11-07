# ChadThrottle v0.6.0 - Trait-Based Architecture Refactoring

## Status: IN PROGRESS

This document tracks the ongoing refactoring to a trait-based backend system.

## Phase 1: Foundation (CURRENT)

- [x] Create directory structure
- [x] Define core traits (MonitorBackend, UploadThrottleBackend, DownloadThrottleBackend)
- [x] Create shared TC/cgroup utilities
- [ ] Create TcHtbUpload backend (wrapping existing logic)
- [ ] Create IfbTcDownload backend (wrapping existing logic)
- [ ] Create PnetMonitor backend (wrapping existing logic)
- [ ] Create ThrottleManager coordinator
- [ ] Update main.rs to use new architecture
- [ ] Add feature flags to Cargo.toml
- [ ] Test basic functionality

## Phase 2: Additional Backends (FUTURE)

- [ ] eBPF cgroup upload backend
- [ ] eBPF cgroup download backend
- [ ] eBPF XDP download backend
- [ ] TC police download backend (no IFB)
- [ ] nftables backends

## Phase 3: Cross-Platform (FUTURE)

- [ ] Windows WFP backends
- [ ] macOS PF backends
- [ ] BSD PF backends

## Current Architecture

```
src/
├── main.rs                 # Entry point
├── backends/               # NEW: Backend system
│   ├── mod.rs             # Core traits and types
│   ├── monitor/
│   │   ├── mod.rs         # MonitorBackend trait
│   │   └── pnet.rs        # pnet backend (wraps existing monitor.rs)
│   ├── throttle/
│   │   ├── mod.rs         # Upload/Download traits
│   │   ├── manager.rs     # ThrottleManager coordinator
│   │   ├── linux_tc_utils.rs  # Shared TC/cgroup utilities
│   │   ├── upload/
│   │   │   └── linux/
│   │   │       └── tc_htb.rs  # TC HTB upload (wraps existing)
│   │   └── download/
│   │       └── linux/
│   │           └── ifb_tc.rs  # IFB+TC download (wraps existing)
├── monitor.rs              # LEGACY: Keep for now, wrap in backend
├── throttle.rs             # LEGACY: Keep for now, extract logic
├── process.rs
└── ui.rs
```

## Migration Strategy

**Incremental approach to avoid breaking current functionality:**

1. **Keep existing files**: monitor.rs, throttle.rs stay functional
2. **Create wrappers**: New backends wrap existing implementations
3. **Test equivalence**: Ensure new system works identically
4. **Switch main.rs**: Update to use new backend system
5. **Remove legacy**: Once stable, remove old files

This allows us to test the new architecture without breaking the current working code.

## Feature Flags

```toml
[features]
default = ["monitor-pnet", "throttle-tc-htb", "throttle-ifb-tc"]

# Monitor backends
monitor-pnet = ["pnet", "pnet_datalink", "pnet_packet"]

# Throttle backends (Linux)
throttle-tc-htb = []     # TC HTB upload (always available)
throttle-ifb-tc = []     # IFB+TC download (needs IFB module)
```

## Testing Plan

1. Build with new features enabled
2. Run and verify monitoring still works
3. Test upload throttling
4. Test download throttling (if IFB available)
5. Test backend selection
6. Test CLI arguments for backend choice
