use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // Only build eBPF if the feature is enabled
    if env::var("CARGO_FEATURE_THROTTLE_EBPF").is_ok() {
        println!("cargo:rerun-if-changed=../chadthrottle-ebpf/src");

        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let workspace_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
            .parent()
            .unwrap()
            .to_path_buf();

        // Try to build eBPF programs if bpf-linker is available
        if check_bpf_linker_installed() {
            println!("cargo:warning=bpf-linker found, attempting to build eBPF programs");

            match build_ebpf_programs(&workspace_root, &out_dir) {
                Ok(_) => {
                    println!("cargo:warning=eBPF programs built successfully!");
                    // Tell the main crate where to find the eBPF bytecode
                    println!(
                        "cargo:rustc-env=EBPF_EGRESS_PATH={}/chadthrottle-egress",
                        out_dir.display()
                    );
                    println!(
                        "cargo:rustc-env=EBPF_INGRESS_PATH={}/chadthrottle-ingress",
                        out_dir.display()
                    );
                }
                Err(e) => {
                    println!("cargo:warning=Failed to build eBPF programs:");
                    for line in e.lines() {
                        println!("cargo:warning=  {}", line);
                    }
                    println!("cargo:warning=");
                    println!("cargo:warning=eBPF backends will not be functional");
                    print_ebpf_build_instructions();
                }
            }
        } else {
            println!("cargo:warning=bpf-linker not found - eBPF programs will not be built");
            print_ebpf_build_instructions();
        }
    }
}

fn check_bpf_linker_installed() -> bool {
    Command::new("bpf-linker").arg("--version").output().is_ok()
}

fn build_ebpf_programs(workspace_root: &Path, out_dir: &Path) -> Result<(), String> {
    let ebpf_dir = workspace_root.join("chadthrottle-ebpf");

    // Build egress program
    let output = Command::new("cargo")
        .current_dir(&ebpf_dir)
        .args(&[
            "build",
            "--release",
            "--target=bpfel-unknown-none",
            "-Z",
            "build-std=core",
            "--bin",
            "chadthrottle-egress",
        ])
        .env("RUSTFLAGS", "-C link-arg=--disable-memory-sanitizer")
        .output()
        .map_err(|e| format!("Failed to execute cargo: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "Failed to build egress program:\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
            stdout, stderr
        ));
    }

    // Build ingress program
    let output = Command::new("cargo")
        .current_dir(&ebpf_dir)
        .args(&[
            "build",
            "--release",
            "--target=bpfel-unknown-none",
            "-Z",
            "build-std=core",
            "--bin",
            "chadthrottle-ingress",
        ])
        .env("RUSTFLAGS", "-C link-arg=--disable-memory-sanitizer")
        .output()
        .map_err(|e| format!("Failed to execute cargo: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "Failed to build ingress program:\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
            stdout, stderr
        ));
    }

    // Copy built programs to out_dir
    let target_dir = workspace_root.join("target/bpfel-unknown-none/release");
    std::fs::copy(
        target_dir.join("chadthrottle-egress"),
        out_dir.join("chadthrottle-egress"),
    )
    .map_err(|e| format!("Failed to copy egress program: {}", e))?;

    std::fs::copy(
        target_dir.join("chadthrottle-ingress"),
        out_dir.join("chadthrottle-ingress"),
    )
    .map_err(|e| format!("Failed to copy ingress program: {}", e))?;

    Ok(())
}

fn print_ebpf_build_instructions() {
    println!("cargo:warning=");
    println!("cargo:warning=To enable eBPF backends:");
    println!("cargo:warning=1. Install bpf-linker: cargo install bpf-linker");
    println!("cargo:warning=2. Install rust-src: rustup component add rust-src");
    println!("cargo:warning=3. Rebuild: cargo build --release");
    println!("cargo:warning=");
    println!("cargo:warning=Or build eBPF programs manually:");
    println!("cargo:warning=  cd chadthrottle-ebpf");
    println!("cargo:warning=  cargo build --release --target bpfel-unknown-none -Z build-std=core");
    println!("cargo:warning=");
}
