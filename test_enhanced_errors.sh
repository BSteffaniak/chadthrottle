#!/bin/bash
echo "=== Starting ChadThrottle with enhanced error logging ==="
echo "Kernel version: $(uname -r)"
echo "Running as: $(whoami)"
echo ""
sudo RUST_LOG=debug ./target/release/chadthrottle 2>&1 | tee chadthrottle-debug.log

