#!/usr/bin/env bash
# Test script to verify BPF program cleanup works properly

set -e

CGROUP_PATH="/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/tmux-spawn-9be16c84-7ec6-49af-9c51-538052a1e645.scope"

echo "=== Cleanup Test for eBPF Programs ==="
echo ""

# First, clean up any existing programs
echo "Step 1: Cleaning up any existing attached programs..."
EXISTING_PROGS=$(sudo bpftool cgroup show "$CGROUP_PATH" 2>/dev/null | grep chadthrottle_in | awk '{print $1}' || true)
if [ -n "$EXISTING_PROGS" ]; then
    echo "Found existing programs to clean up:"
    echo "$EXISTING_PROGS"
    for prog_id in $EXISTING_PROGS; do
        echo "  Detaching program ID: $prog_id"
        sudo bpftool cgroup detach "$CGROUP_PATH" ingress id "$prog_id" 2>/dev/null || true
    done
else
    echo "  No existing programs found"
fi
echo ""

# Verify cleanup
echo "Step 2: Verifying cgroup is clean..."
REMAINING=$(sudo bpftool cgroup show "$CGROUP_PATH" 2>/dev/null | grep chadthrottle || true)
if [ -n "$REMAINING" ]; then
    echo "  ⚠️  WARNING: Programs still attached after cleanup:"
    echo "$REMAINING"
else
    echo "  ✅ Cgroup is clean"
fi
echo ""

echo "Step 3: Running chadthrottle with wget (will exit after 10 seconds)..."
echo "  Starting wget in background..."
wget -O /dev/null https://github.com/sst/opencode/releases/download/v1.0.40/opencode-windows-x64.zip &
WGET_PID=$!
sleep 1

echo "  Starting chadthrottle..."
sudo timeout 10 ./target/release/chadthrottle --pid $WGET_PID --download-limit 30K --duration 10 2>&1 | grep -E "(Loaded|Attaching|attached|Detaching|detach|cleanup)" || true

# Kill wget if still running
kill $WGET_PID 2>/dev/null || true
wait $WGET_PID 2>/dev/null || true

echo ""
echo "Step 4: Checking if programs were properly detached..."
ATTACHED_AFTER=$(sudo bpftool cgroup show "$CGROUP_PATH" 2>/dev/null | grep chadthrottle || true)
if [ -n "$ATTACHED_AFTER" ]; then
    echo "  ❌ FAILED: Programs still attached after cleanup:"
    echo "$ATTACHED_AFTER"
    exit 1
else
    echo "  ✅ SUCCESS: All programs properly detached!"
fi

echo ""
echo "=== Test Complete ==="
