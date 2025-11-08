#!/usr/bin/env bash
# Minimal BPF cgroup attachment test
# This creates a simple BPF program and tries to attach it

set -e

echo "========================================="
echo "Minimal BPF Cgroup Attachment Test"
echo "========================================="
echo ""

# Check if bpftool is available
if ! command -v bpftool >/dev/null 2>&1; then
    echo "ERROR: bpftool not found"
    echo "Install with: sudo apt install linux-tools-common linux-tools-$(uname -r)"
    exit 1
fi

# Create a minimal BPF program that does nothing (just returns 1 = allow)
# This is the simplest possible cgroup_skb program
cat > /tmp/minimal_cgroup_skb.c << 'EOF'
#include <linux/bpf.h>
#include <bpf/bpf_helpers.h>

SEC("cgroup_skb/ingress")
int minimal_ingress(struct __sk_buff *skb) {
    return 1;  // Allow all packets
}

char _license[] SEC("license") = "GPL";
EOF

echo "Created minimal BPF program: /tmp/minimal_cgroup_skb.c"

# Try to compile it
if command -v clang >/dev/null 2>&1; then
    echo "Compiling with clang..."
    clang -O2 -target bpf -c /tmp/minimal_cgroup_skb.c -o /tmp/minimal_cgroup_skb.o 2>&1 || {
        echo "WARNING: Compilation failed (this is OK, just a test)"
        echo "Trying alternative method..."
    }
    
    if [ -f /tmp/minimal_cgroup_skb.o ]; then
        echo "✓ BPF program compiled"
        
        # Show the program
        echo ""
        echo "Program sections:"
        readelf -S /tmp/minimal_cgroup_skb.o 2>/dev/null | grep -E "Name|cgroup" || true
        
        # Try to load it
        echo ""
        echo "Attempting to load into kernel..."
        bpftool prog load /tmp/minimal_cgroup_skb.o /sys/fs/bpf/test_minimal 2>&1 || {
            echo "Failed to load (expected if section name or format is wrong)"
        }
        
        # Try to attach to root cgroup
        if [ -f /sys/fs/bpf/test_minimal ]; then
            echo ""
            echo "Attempting to attach to root cgroup..."
            bpftool cgroup attach /sys/fs/cgroup ingress pinned /sys/fs/bpf/test_minimal 2>&1 && {
                echo "✓ Successfully attached to root cgroup!"
                echo "Detaching..."
                bpftool cgroup detach /sys/fs/cgroup ingress pinned /sys/fs/bpf/test_minimal
                rm /sys/fs/bpf/test_minimal
            } || {
                echo "✗ Failed to attach to root cgroup"
                rm -f /sys/fs/bpf/test_minimal
            }
        fi
    fi
else
    echo "WARNING: clang not installed, cannot compile test program"
    echo "Install with: sudo apt install clang llvm"
fi

# Cleanup
rm -f /tmp/minimal_cgroup_skb.c /tmp/minimal_cgroup_skb.o

echo ""
echo "Test complete"
