// Linux upload throttling backends

#[cfg(feature = "throttle-ebpf")]
pub mod ebpf;

#[cfg(feature = "throttle-tc-htb")]
pub mod tc_htb;

#[cfg(feature = "throttle-nftables")]
pub mod nftables;
