# chadthrottle

The main binary crate for ChadThrottle - a TUI network monitor and throttler.

For full documentation, see the [project README](../README.md).

## Features

This crate supports the following cargo features:

### Default

- `monitor-pnet` - Network monitoring via pnet packet capture (cross-platform)

### Throttle Backends (Linux-only)

- `throttle-tc-htb` - TC HTB upload throttling
- `throttle-ifb-tc` - IFB+TC download throttling
- `throttle-tc-police` - TC Police download throttling (fallback)
- `throttle-nftables` - nftables throttling (modern)
- `throttle-ebpf` - eBPF cgroup throttling (best performance)

### Cgroup Backends (Linux-only)

- `cgroup-v1` - Cgroup v1 net_cls controller (legacy)
- `cgroup-v2-nftables` - Cgroup v2 with nftables socket matching
- `cgroup-v2-ebpf` - Cgroup v2 with eBPF TC classifier (future)

### Convenience Bundles

- `linux-full` - All Linux monitoring and throttling backends
- `macos-full` - All macOS monitoring backends

## License

MIT
