use std::env;
use std::path::PathBuf;

fn main() {
    // Only check for eBPF if the feature is enabled
    if env::var("CARGO_FEATURE_THROTTLE_EBPF").is_ok() {
        println!("cargo:rerun-if-changed=../chadthrottle-ebpf/src");

        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let workspace_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
            .parent()
            .unwrap()
            .to_path_buf();

        // Check if eBPF programs were already built by xtask
        let target_dir = workspace_root.join("target/bpfel-unknown-none/release");
        let egress_exists = target_dir.join("chadthrottle-egress").exists();
        let ingress_exists = target_dir.join("chadthrottle-ingress").exists();

        if egress_exists && ingress_exists {
            // Copy pre-built programs to out_dir
            if let Err(e) = std::fs::copy(
                target_dir.join("chadthrottle-egress"),
                out_dir.join("chadthrottle-egress"),
            ) {
                println!("cargo:warning=Failed to copy egress program: {}", e);
            } else if let Err(e) = std::fs::copy(
                target_dir.join("chadthrottle-ingress"),
                out_dir.join("chadthrottle-ingress"),
            ) {
                println!("cargo:warning=Failed to copy ingress program: {}", e);
            } else {
                // Successfully copied pre-built programs
                println!("cargo:rustc-cfg=ebpf_programs_built");
                return;
            }
        }

        // eBPF programs not found - print instructions
        println!("cargo:warning=");
        println!("cargo:warning=eBPF programs not found!");
        println!("cargo:warning=");
        println!("cargo:warning=eBPF programs must be built using xtask:");
        println!("cargo:warning=  cargo xtask build-ebpf");
        println!("cargo:warning=");
        println!("cargo:warning=Or build everything at once:");
        println!("cargo:warning=  cargo xtask build          # Debug build");
        println!("cargo:warning=  cargo xtask build-release  # Release build");
        println!("cargo:warning=");
        println!("cargo:warning=eBPF backends will not be functional until you run xtask.");
        println!("cargo:warning=");
    }
}
