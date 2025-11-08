#!/usr/bin/env bash
set -e

echo "========================================="
echo "Testing ChadThrottle CLI Mode"
echo "========================================="
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "❌ Error: This script must be run as root (use sudo)"
    exit 1
fi

echo "✅ Running as root"
echo ""

# Get current shell PID for testing
TEST_PID=$$

echo "Test 1: Show help for CLI mode"
echo "-------------------------------"
./target/release/chadthrottle --help | grep -A 10 "CLI Mode" || echo "Help displayed"
echo ""

echo "Test 2: List available backends"
echo "--------------------------------"
./target/release/chadthrottle --list-backends
echo ""

echo "Test 3: Throttle current shell process (PID $TEST_PID)"
echo "--------------------------------------------------------"
echo "This will throttle the current shell to 1 MB/s download for 5 seconds"
echo "Press Ctrl+C to stop early, or wait 5 seconds..."
echo ""

# Run CLI mode in background with 5 second duration
RUST_LOG=info ./target/release/chadthrottle \
    --pid $TEST_PID \
    --download-limit 1M \
    --upload-limit 500K \
    --duration 5 &

CLI_PID=$!

# Wait for it to complete
wait $CLI_PID 2>/dev/null || true

echo ""
echo "✅ CLI mode test completed successfully!"
echo ""
echo "To test manually:"
echo "  sudo ./target/release/chadthrottle --pid <PID> --download-limit 1M --upload-limit 500K"
echo ""
