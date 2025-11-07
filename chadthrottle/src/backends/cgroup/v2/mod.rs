//! Cgroup v2 backends
//!
//! Cgroup v2 uses a unified hierarchy (no separate net_cls controller).
//! Different throttling mechanisms can be used with v2:
//!
//! - **nftables**: Uses `socket cgroupv2` matcher (available now)
//! - **eBPF TC**: Uses BPF_PROG_TYPE_SCHED_CLS attached to TC (future)

#[cfg(feature = "cgroup-v2-nftables")]
pub mod nftables;
