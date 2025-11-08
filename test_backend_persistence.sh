#!/bin/bash

echo "=== Testing Backend Persistence ==="
echo ""

# Clean up any existing config
CONFIG_FILE="$HOME/.config/chadthrottle/throttles.json"
echo "1. Checking config file: $CONFIG_FILE"

if [ -f "$CONFIG_FILE" ]; then
    echo "   Config exists. Current contents:"
    cat "$CONFIG_FILE" | jq '.' 2>/dev/null || cat "$CONFIG_FILE"
else
    echo "   No config file found (will be created)"
fi

echo ""
echo "2. Test Plan:"
echo "   - The fix loads config BEFORE selecting backends"
echo "   - Priority: CLI args > Config file > Auto-detect"
echo "   - Config preferences now properly passed to select_*_backend()"
echo ""

echo "3. How to verify it works:"
echo "   a) Run: sudo ./target/release/chadthrottle"
echo "   b) Press 'b' to view backends"
echo "   c) Press Enter to open backend selector"
echo "   d) Select a different backend (e.g., tc_htb)"
echo "   e) Press Enter to confirm"
echo "   f) Quit with 'q'"
echo "   g) Check config file:"
echo "      cat $CONFIG_FILE | jq '.preferred_upload_backend'"
echo "   h) Restart chadthrottle"
echo "   i) Press 'b' to view backends"
echo "   j) Verify the ⭐ star is next to the backend you selected!"
echo ""

echo "4. Logs to watch for:"
echo "   - 'Using upload backend from config: tc_htb'"
echo "   - 'Using download backend from config: ebpf'"
echo ""

echo "Run with RUST_LOG=info to see these messages:"
echo "   sudo RUST_LOG=info ./target/release/chadthrottle"

