#!/usr/bin/env bash
echo "========================================="
echo "Testing with Enhanced BPF Diagnostics"
echo "========================================="
echo ""
echo "This will show:"
echo "  - Program FD validation"
echo "  - Cgroup FD details"  
echo "  - Exact attach parameters"
echo "  - Detailed error information"
echo ""
echo "Starting chadthrottle..."
echo ""

sudo RUST_LOG=debug ./target/release/chadthrottle 2>&1 | tee /tmp/chadthrottle-detailed-diag.log

echo ""
echo "Log saved to: /tmp/chadthrottle-detailed-diag.log"
