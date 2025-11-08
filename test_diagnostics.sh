#!/bin/bash
set -e

echo "=== Test 1: Normal mode (user cgroup) with enhanced logging ==="
echo "This will show the program's expected_attach_type and file opening details"
echo ""
timeout 10 sudo RUST_LOG=debug ./target/release/chadthrottle 2>&1 | rg "expected_attach_type|Opened cgroup|Failed to attach|Failed to open" || true

echo ""
echo "=== Test 2: Root cgroup test mode ==="
echo "This will attempt to attach to /sys/fs/cgroup instead of the process cgroup"
echo ""
timeout 10 sudo CHADTHROTTLE_TEST_ROOT_CGROUP=1 RUST_LOG=debug ./target/release/chadthrottle 2>&1 | rg "TEST MODE|expected_attach_type|Opened cgroup|Successfully attached|Failed to attach|Failed to open" || true
