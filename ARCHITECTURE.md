# ChadThrottle Architecture (v0.6.0+)

## Overview

ChadThrottle uses a **trait-based, pluggable backend architecture** that separates network monitoring and throttling concerns into independent, swappable components. This design allows for:

- Multiple implementation methods (eBPF, TC, WFP, etc.)
- Platform-specific optimizations
- Feature-gated compilation (only compile what's needed)
- Runtime backend selection
- Easy addition of new backends

## Core Architecture

### Directory Structure

```
src/
├── main.rs                          # Entry point, backend selection
├── backends/                        # Backend system (NEW in v0.6.0)
│   ├── mod.rs                      # Core traits and types
│   ├── capability.rs               # Capability detection utilities
│   ├── monitor/                    # Network monitoring backends
│   │   ├── mod.rs                 # MonitorBackend trait
│   │   └── pnet.rs                # pnet packet capture (Linux/BSD)
│   └── throttle/                   # Throttling backends
│       ├── mod.rs                 # Upload/Download traits
│       ├── manager.rs             # ThrottleManager coordinator
│       ├── linux_tc_utils.rs      # Shared TC/cgroup utilities
│       ├── upload/                # Upload (egress) backends
│       │   └── linux/
│       │       └── tc_htb.rs      # TC HTB upload throttling
│       └── download/              # Download (ingress) backends
│           └── linux/
│               └── ifb_tc.rs      # IFB+TC download throttling
├── monitor.rs                      # Legacy monitor (wrapped by backends)
├── throttle.rs                     # Legacy throttle (will be removed)
├── process.rs                      # Process data structures
└── ui.rs                          # TUI interface
```

### Core Traits

#### MonitorBackend

Network monitoring backend for tracking per-process bandwidth usage.

```rust
pub trait MonitorBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> BackendPriority;
    fn is_available() -> bool where Self: Sized;
    fn capabilities(&self) -> BackendCapabilities;
    fn init(&mut self) -> Result<()>;
    fn update(&mut self) -> Result<ProcessMap>;
    fn cleanup(&mut self) -> Result<()>;
}
```

**Implementations:**

- `PnetMonitor` - Packet capture using pnet library (Linux/BSD)

**Future:**

- `EbpfMonitor` - eBPF socket filter (Linux, best)
- `WfpMonitor` - Windows Filtering Platform (Windows)
- `PfMonitor` - PacketFilter (macOS/BSD)

#### UploadThrottleBackend

Upload (egress) traffic throttling backend.

```rust
pub trait UploadThrottleBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> BackendPriority;
    fn is_available() -> bool where Self: Sized;
    fn capabilities(&self) -> BackendCapabilities;
    fn init(&mut self) -> Result<()>;
    fn throttle_upload(&mut self, pid: i32, name: String, limit: u64) -> Result<()>;
    fn remove_upload_throttle(&mut self, pid: i32) -> Result<()>;
    fn get_upload_throttle(&self, pid: i32) -> Option<u64>;
    fn cleanup(&mut self) -> Result<()>;
}
```

**Implementations:**

- `TcHtbUpload` - TC HTB on main interface (Linux, always available)

**Future:**

- `EbpfCgroupUpload` - eBPF BPF_CGROUP_INET_EGRESS (Linux, best)
- `NftablesUpload` - nftables output chain (Linux)
- `WfpUpload` - WFP outbound layer (Windows)
- `PfUpload` - PacketFilter output (macOS/BSD)

#### DownloadThrottleBackend

Download (ingress) traffic throttling backend.

```rust
pub trait DownloadThrottleBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> BackendPriority;
    fn is_available() -> bool where Self: Sized;
    fn capabilities(&self) -> BackendCapabilities;
    fn init(&mut self) -> Result<()>;
    fn throttle_download(&mut self, pid: i32, name: String, limit: u64) -> Result<()>;
    fn remove_download_throttle(&mut self, pid: i32) -> Result<()>;
    fn get_download_throttle(&self, pid: i32) -> Option<u64>;
    fn cleanup(&mut self) -> Result<()>;
}
```

**Implementations:**

- `IfbTcDownload` - IFB redirect + TC HTB (Linux, needs IFB module)

**Future:**

- `EbpfCgroupDownload` - eBPF BPF_CGROUP_INET_INGRESS (Linux, best)
- `EbpfXdpDownload` - eBPF XDP early drop (Linux, good)
- `TcPoliceDownload` - TC ingress police, no IFB (Linux, fallback)
- `WfpDownload` - WFP inbound layer (Windows)
- `PfDownload` - PacketFilter input (macOS/BSD)

### Backend Priority System

Backends are ranked by priority for auto-selection:

```rust
pub enum BackendPriority {
    Fallback = 1,  // Works but limited (iptables, /proc parsing)
    Good = 2,      // Solid implementation (IFB+TC, pnet)
    Better = 3,    // Modern, efficient (eBPF XDP, nftables)
    Best = 4,      // Optimal (eBPF cgroup, native platform APIs)
}
```

Auto-selection chooses the highest priority backend that is available on the system.

### ThrottleManager

Coordinates separate upload and download backends:

```rust
pub struct ThrottleManager {
    upload_backend: Box<dyn UploadThrottleBackend>,
    download_backend: Option<Box<dyn DownloadThrottleBackend>>,
}
```

**Key features:**

- Upload backend is always required
- Download backend is optional (graceful degradation)
- Backends can be different (e.g., TC upload + eBPF download)
- Unified API for applying/removing throttles

## Backend Selection

### Automatic Selection

```rust
// Select best available backends
let upload = select_upload_backend(None)?;
let download = select_download_backend(None); // Returns None if unavailable

let manager = ThrottleManager::new(upload, download);
```

**Selection algorithm:**

1. Query all compiled backends via feature flags
2. Check `is_available()` for each
3. Sort by priority
4. Select highest priority available backend

### Manual Selection

```rust
// User specifies backends
let upload = select_upload_backend(Some("tc_htb"))?;
let download = select_download_backend(Some("ifb_tc"));
```

### Feature Flags

Control which backends are compiled:

```toml
[features]
default = ["monitor-pnet", "throttle-tc-htb", "throttle-ifb-tc"]

# Monitor backends
monitor-pnet = ["pnet", "pnet_datalink", "pnet_packet"]

# Upload throttle backends
throttle-tc-htb = []           # TC HTB (always works on Linux)
throttle-ebpf-cgroup = ["aya"] # eBPF cgroup (best, future)

# Download throttle backends
throttle-ifb-tc = []           # IFB+TC (needs IFB module)
throttle-ebpf-xdp = ["aya"]    # eBPF XDP (best, future)
```

## Current Backend Implementations

### Monitor: PnetMonitor

**File:** `src/backends/monitor/pnet.rs`

**Method:** Raw packet capture using pnet library

**How it works:**

1. Opens raw socket on network interface
2. Captures Ethernet frames
3. Parses IP/TCP/UDP headers
4. Maps packets to PIDs via socket inode matching
5. Tracks bandwidth per process

**Capabilities:**

- ✅ IPv4 + IPv6 support
- ✅ Per-process tracking
- ✅ 100% accurate byte counting
- ❌ No per-connection tracking

**Availability:** Linux, BSD (requires raw socket access)

### Upload: TcHtbUpload

**File:** `src/backends/throttle/upload/linux/tc_htb.rs`

**Method:** Linux TC (traffic control) + cgroups

**How it works:**

1. Create cgroup for process (`/sys/fs/cgroup/net_cls/chadthrottle/pid_X`)
2. Set cgroup classid (packet tagging)
3. Move process to cgroup
4. Setup TC HTB qdisc on main interface (egress)
5. Create TC class with rate limit
6. TC cgroup filter matches packets by classid

**Capabilities:**

- ✅ IPv4 + IPv6 support
- ✅ Per-process throttling
- ✅ Guaranteed rate limits
- ✅ Kernel-enforced (cannot bypass)

**Availability:** Always (Linux with TC + cgroups)

**Priority:** Good

### Download: IfbTcDownload

**File:** `src/backends/throttle/download/linux/ifb_tc.rs`

**Method:** IFB (Intermediate Functional Block) redirect + TC

**How it works:**

1. Load IFB kernel module
2. Create IFB virtual device (ifb0)
3. Setup ingress qdisc on main interface
4. Redirect ingress traffic to IFB device
5. Apply TC HTB on IFB (treats downloads as uploads)
6. Use same cgroup classid as upload

**Capabilities:**

- ✅ IPv4 + IPv6 support
- ✅ Per-process throttling
- ✅ Guaranteed rate limits
- ✅ Works alongside upload throttling

**Availability:** Linux with IFB module

**Priority:** Good

**Limitation:** Requires `ifb` kernel module (not always available, especially on NixOS)

## Adding New Backends

### Step 1: Implement the Trait

```rust
// src/backends/throttle/upload/linux/ebpf_cgroup.rs

use crate::backends::throttle::UploadThrottleBackend;

pub struct EbpfCgroupUpload {
    // ... state ...
}

impl UploadThrottleBackend for EbpfCgroupUpload {
    fn name(&self) -> &'static str { "ebpf_cgroup_upload" }
    fn priority(&self) -> BackendPriority { BackendPriority::Best }
    fn is_available() -> bool {
        // Check kernel version, eBPF support, etc.
        check_kernel_version("4.10") && check_bpf_support()
    }
    // ... implement other methods ...
}
```

### Step 2: Add Feature Flag

```toml
# Cargo.toml
[features]
throttle-ebpf-cgroup = ["aya", "aya-log"]

[dependencies]
aya = { version = "0.12", optional = true }
```

### Step 3: Register in Selection Logic

```rust
// src/backends/throttle/mod.rs

pub fn detect_upload_backends() -> Vec<UploadBackendInfo> {
    let mut backends = Vec::new();

    #[cfg(feature = "throttle-ebpf-cgroup")]
    {
        backends.push(UploadBackendInfo {
            name: "ebpf_cgroup",
            priority: BackendPriority::Best,
            available: EbpfCgroupUpload::is_available(),
        });
    }

    // ... other backends ...
    backends
}
```

### Step 4: Add to create_backend

```rust
fn create_upload_backend(name: &str) -> Result<Box<dyn UploadThrottleBackend>> {
    match name {
        #[cfg(feature = "throttle-ebpf-cgroup")]
        "ebpf_cgroup" => Ok(Box::new(EbpfCgroupUpload::new()?)),

        // ... other backends ...
        _ => Err(anyhow!("Unknown upload backend: {}", name)),
    }
}
```

## Benefits of This Architecture

### 1. Platform Independence

Same codebase supports Linux, macOS, Windows, BSD with platform-specific implementations.

### 2. Optimal Performance

Auto-selects best available backend for each platform (eBPF on Linux, WFP on Windows, etc.).

### 3. Minimal Dependencies

Feature flags ensure only needed code is compiled. NixOS users don't compile Windows WFP code!

### 4. Graceful Degradation

```
Best available:    eBPF cgroup (upload) + eBPF XDP (download)
Fallback:          TC HTB (upload) + IFB+TC (download)
Minimal:           TC HTB (upload) only (if IFB unavailable)
```

### 5. Easy Testing

Mock backends for testing without kernel dependencies:

```rust
struct MockUpload;
impl UploadThrottleBackend for MockUpload { /* ... */ }

let manager = ThrottleManager::new(Box::new(MockUpload), None);
```

### 6. Future-Proof

Easy to add new backends as technologies evolve (io_uring, etc.).

## Migration from v0.5.0

**v0.5.0 (monolithic):**

```rust
let mut throttle = ThrottleManager::new()?;
throttle.throttle_process(pid, name, &limit)?;
```

**v0.6.0 (trait-based):**

```rust
let upload = select_upload_backend(None)?;
let download = select_download_backend(None);
let mut throttle = ThrottleManager::new(upload, download);
throttle.throttle_process(pid, name, &limit)?;
```

**API is similar, but now with:**

- Backend selection
- Better error messages
- Capability reporting
- Platform flexibility

## Roadmap

### v0.7.0: eBPF Backends

- EbpfCgroupUpload (BPF_CGROUP_INET_EGRESS)
- EbpfCgroupDownload (BPF_CGROUP_INET_INGRESS)
- EbpfXdpDownload (XDP rate limiting)

### v0.8.0: Additional Linux Backends

- TcPoliceDownload (no IFB needed)
- NftablesUpload/Download
- IptablesUpload/Download (fallback)

### v0.9.0: macOS Support

- PfMonitor (PacketFilter monitoring)
- PfUpload/PfDownload (dummynet)
- NetworkLinkThrottle (Network Link Conditioner API)

### v1.0.0: Windows Support

- WfpMonitor (Windows Filtering Platform)
- WfpUpload/WfpDownload
- Feature parity with NetLimiter!

## See Also

- [THROTTLING.md](THROTTLING.md) - Technical throttling details
- [IFB_SETUP.md](IFB_SETUP.md) - IFB kernel module setup
- [REFACTORING_PLAN.md](REFACTORING_PLAN.md) - Implementation progress
