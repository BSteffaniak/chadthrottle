#!/bin/bash
# Quick test to see which backends are detected

echo "=== ChadThrottle Backend Detection Test ==="
echo ""

# Check TC availability
if command -v tc &> /dev/null; then
    echo "✓ TC (traffic control) is available"
else
    echo "✗ TC (traffic control) NOT available"
fi

# Check cgroups
if [ -d /sys/fs/cgroup/net_cls ]; then
    echo "✓ cgroups net_cls is available"
else
    echo "✗ cgroups net_cls NOT available"
fi

# Check IFB module
if lsmod | grep -q ifb || modprobe -n ifb &> /dev/null; then
    echo "✓ IFB module is available"
else
    echo "✗ IFB module NOT available"
fi

echo ""
echo "Expected backend selection:"
echo "  Upload:   tc_htb (if TC + cgroups available)"
echo "  Download: ifb_tc (if IFB available) OR tc_police (fallback)"
