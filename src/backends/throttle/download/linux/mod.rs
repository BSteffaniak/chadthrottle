// Linux download throttling backends

#[cfg(feature = "throttle-ifb-tc")]
pub mod ifb_tc;

#[cfg(feature = "throttle-tc-police")]
pub mod tc_police;
