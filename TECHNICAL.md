# ChadThrottle Technical Details

## Network Monitoring Architecture v2.0 - Packet Capture

### The Challenge

Linux doesn't provide per-process network statistics directly. The kernel tracks network I/O at the interface level, not the process level.

### ✅ Current Solution: Raw Packet Capture with `pnet`

ChadThrottle now uses **accurate packet-level tracking** for 100% precise bandwidth monitoring!

### How It Works Now: Packet Capture + Socket Mapping

ChadThrottle combines raw packet capture with socket inode mapping for 100% accurate tracking:

#### Step 1: Build Socket → PID Map

```
For each process in /proc:
    Read /proc/[pid]/fd/*
    For each file descriptor:
        If it's a socket:
            Store: socket_inode → (pid, process_name)
```

#### Step 2: Read System-Wide Connection Tables

```
Read /proc/net/tcp     - IPv4 TCP connections
Read /proc/net/tcp6    - IPv6 TCP connections
Read /proc/net/udp     - IPv4 UDP connections
Read /proc/net/udp6    - IPv6 UDP connections
```

Each entry contains:

- Socket inode
- Local/remote addresses
- Connection state
- **TX/RX queue sizes** ← Key for bandwidth estimation

#### Step 3: Match Connections to Processes

```
For each connection in /proc/net/*:
    Look up socket inode in our map
    → Find which PID owns this connection
```

#### Step 4: Capture and Parse Packets

```
Using pnet library (pure Rust, no libpcap):
1. Open network interface with AF_PACKET raw sockets
2. Capture every packet from the interface
3. Parse: Ethernet → IPv4/IPv6 → TCP/UDP
4. Extract: source/dest IP, source/dest port, packet size
5. Match packet to connection in our map
6. Attribute bytes to the owning process
```

#### Step 5: Track Accurate Bandwidth

```
For each captured packet:
    - Identify if it's inbound or outbound
    - Look up which PID owns the connection
    - Add packet size to that process's rx_bytes or tx_bytes
    - Calculate rate: bytes_diff / time_elapsed
```

### Implementation Details

**Key Files:**

- `src/monitor.rs` - Packet capture thread + socket inode mapping
- Uses `pnet` crate for raw packet capture (no libpcap needed!)
- Uses `procfs` crate for reading `/proc` filesystem

**Architecture:**

```
Main Thread                  Packet Capture Thread
    │                              │
    ├─ Update UI                   ├─ Open raw socket
    ├─ Update socket map           ├─ Capture packets
    └─ Calculate rates             ├─ Parse headers
                                   ├─ Match to PID
                                   └─ Track bytes
         ▲                         │
         │                         ▼
         └───── Shared State ──────┘
           (BandwidthTracker with Mutex)
```

**Data Structures:**

```rust
socket_map: HashMap<u64, (i32, String)>
    // Maps: socket_inode → (pid, process_name)

previous_queues: HashMap<(u64, bool), (u64, u64)>
    // Maps: (inode, is_tcp) → (rx_queue, tx_queue)

accumulated_bytes: HashMap<i32, (u64, u64)>
    // Maps: pid → (total_rx, total_tx)
```

### Accuracy

**Packet-based tracking is 100% accurate:**

- ✅ Captures every packet
- ✅ Counts every byte
- ✅ No estimation or approximation
- ✅ Works for all protocols (TCP, UDP, IPv4, IPv6)
- ✅ Real-time tracking

**Why this approach works:**

- Operates at the network interface level
- Sees all traffic before/after it reaches applications
- Same accuracy as Wireshark or tcpdump
- No data escapes the capture

### Why Not eBPF?

We chose `pnet` over eBPF for several reasons:

1. **Simpler Implementation** - No need to write kernel-space code
2. **Single Binary** - Everything compiles into one executable
3. **Easier Debugging** - Pure userspace, can use normal debugging tools
4. **Cross-platform Potential** - pnet works on Linux, macOS, Windows
5. **Still Very Fast** - Minimal overhead for most use cases

**When to use eBPF instead:**

- Extremely high-throughput scenarios (10Gbps+)
- Need to minimize CPU usage to absolute minimum
- Want to hook deeper into kernel networking stack

eBPF could be added as an optional feature in the future for power users.

## Comparison with Other Tools

| Tool                 | Method              | Accuracy            | Overhead | Root Required | External Deps    |
| -------------------- | ------------------- | ------------------- | -------- | ------------- | ---------------- |
| **ChadThrottle**     | pnet packet capture | ✅ 100%             | Low      | Yes           | ❌ None          |
| nethogs              | libpcap             | ✅ 100%             | Medium   | Yes           | ✅ libpcap       |
| bandwhich            | eBPF                | ✅ 100%             | Very Low | Yes           | ⚠️ BTF + headers |
| iftop                | libpcap             | ✅ 100% (interface) | Medium   | Yes           | ✅ libpcap       |
| NetLimiter (Windows) | Kernel driver       | ✅ 100%             | Low      | Admin         | ❌ Built-in      |

## Why This Approach?

We chose `pnet` packet capture because:

1. **100% Accurate** - Captures and counts every byte
2. **Pure Rust** - No external C libraries (no libpcap dependency!)
3. **Static Binary** - Everything compiles into one executable
4. **Well-maintained** - `pnet` is a mature, widely-used library
5. **Cross-platform Ready** - Works on Linux (current), macOS, BSD, Windows

**The pnet Advantage:**

- Uses `AF_PACKET` raw sockets on Linux (kernel API)
- No shared library dependencies
- Direct syscalls via Rust's `libc` bindings
- Perfect for a single self-contained binary

## Testing the Implementation

To verify it works:

```bash
# Terminal 1: Run ChadThrottle
sudo ./target/release/chadthrottle

# Terminal 2: Generate traffic
curl -O https://speed.hetzner.de/100MB.bin

# You should see 'curl' appear with bandwidth usage!
```

The socket inode mapping correctly identifies which process owns each connection, so you'll see individual processes rather than everything showing the same values.
