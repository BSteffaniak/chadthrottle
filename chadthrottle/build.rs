use std::env;
use std::path::PathBuf;

fn main() {
    // Only build eBPF if the feature is enabled
    if env::var("CARGO_FEATURE_THROTTLE_EBPF").is_ok() {
        println!("cargo:rerun-if-changed=../chadthrottle-ebpf/src");

        // Tell cargo to pass the eBPF object files to rustc
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

        // The eBPF programs will be built separately and their bytecode
        // will be embedded using include_bytes! in the source code
        println!(
            "cargo:warning=eBPF backend enabled - make sure to build eBPF programs separately"
        );
        println!("cargo:warning=Run: cargo xtask build-ebpf");
    }
}
