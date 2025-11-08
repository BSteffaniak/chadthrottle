#!/usr/bin/env bash
set -e

echo "========================================="
echo "Testing eBPF Backend in CLI Mode"
echo "========================================="
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "❌ Error: This script must be run as root (use sudo)"
    exit 1
fi

# Get current shell PID
TEST_PID=$$

echo "This will test the eBPF cgroup backend by throttling PID $TEST_PID"
echo ""
echo "Environment variables you can set:"
echo "  CHADTHROTTLE_BPF_ATTACH_METHOD=auto|link|legacy - BPF attach method"
echo "  CHADTHROTTLE_USE_LEGACY_ATTACH=1                - Use legacy attach (deprecated, use above)"
echo "  CHADTHROTTLE_TEST_ROOT_CGROUP=1                 - Attach to root cgroup instead of process cgroup"
echo "  RUST_LOG=debug                                   - Enable detailed logging"
echo ""
echo "CLI options:"
echo "  --bpf-attach-method auto|link|legacy - Override BPF attach method"
echo ""

# Default to using eBPF backend
export RUST_LOG="${RUST_LOG:-info}"

echo "Running with:"
echo "  PID: $TEST_PID"
echo "  Download limit: 1M"
echo "  Upload limit: 500K"
echo "  Backend: ebpf (auto-detect attach method)"
echo "  Duration: 10 seconds"
echo ""

# Run CLI mode with eBPF backend (auto mode - will fallback automatically)
./target/release/chadthrottle \
    --pid "$TEST_PID" \
    --download-limit 1M \
    --upload-limit 500K \
    --download-backend ebpf \
    --upload-backend ebpf \
    --duration 10

echo ""
echo "✅ Test completed!"
echo ""
echo "Check the logs above for:"
echo "  - 'GPL' license in BPF program"
echo "  - Successful attachment (look for '✅ Throttle applied successfully!')"
echo "  - No EINVAL errors"
echo ""
echo "To test different attach methods:"
echo "  Auto (default, tries link then falls back to legacy):"
echo "    sudo RUST_LOG=debug ./test_ebpf_cli.sh"
echo ""
echo "  Force legacy attach:"
echo "    sudo RUST_LOG=debug CHADTHROTTLE_BPF_ATTACH_METHOD=legacy ./test_ebpf_cli.sh"
echo "    OR: sudo RUST_LOG=debug ./target/release/chadthrottle --pid \$\$ --download-limit 1M --download-backend ebpf --bpf-attach-method legacy"
echo ""
echo "  Force modern attach (will fail on systems without bpf_link_create support):"
echo "    sudo RUST_LOG=debug ./target/release/chadthrottle --pid \$\$ --download-limit 1M --download-backend ebpf --bpf-attach-method link"
echo ""
