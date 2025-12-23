{
  description = "ChadThrottle - TUI network monitor and throttler for Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [ "rust-src" "llvm-tools-preview" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            # Rust toolchain
            rustToolchain

            # eBPF tooling (bpf-linker works on macOS too)
            pkgs.bpf-linker
            pkgs.llvmPackages.clang
            pkgs.llvmPackages.llvm

            # Build dependencies
            pkgs.pkg-config
            pkgs.openssl
            pkgs.libpcap # for pnet

            # Shell
            pkgs.fish
          ]
          # Linux-only packages
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.bpftools
            pkgs.nftables
          ];

          shellHook = ''
            # Only exec fish if we're in an interactive shell (not running a command)
            if [ -z "$IN_NIX_SHELL_FISH" ] && [ -z "$BASH_EXECUTION_STRING" ]; then
              case "$-" in
                *i*) export IN_NIX_SHELL_FISH=1; exec fish ;;
              esac
            fi
          '';
        };
      });
}
