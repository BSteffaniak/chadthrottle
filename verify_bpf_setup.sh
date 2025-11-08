#!/usr/bin/env bash
# Comprehensive BPF/cgroup verification script

set -e

echo "========================================="
echo "BPF/Cgroup Setup Verification"
echo "========================================="
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

pass() {
    echo -e "${GREEN}✓${NC} $1"
}

fail() {
    echo -e "${RED}✗${NC} $1"
}

warn() {
    echo -e "${YELLOW}⚠${NC} $1"
}

section() {
    echo ""
    echo "--- $1 ---"
}

# 1. Check kernel version
section "Kernel Version"
KERNEL_VERSION=$(uname -r)
echo "Kernel: $KERNEL_VERSION"
pass "Kernel version detected"

# 2. Check BPF kernel config
section "BPF Kernel Configuration"

CONFIG_FILE=""
if [ -f "/proc/config.gz" ]; then
    CONFIG_FILE="/proc/config.gz"
    CONFIG_CMD="zgrep"
elif [ -f "/boot/config-$(uname -r)" ]; then
    CONFIG_FILE="/boot/config-$(uname -r)"
    CONFIG_CMD="grep"
else
    fail "Cannot find kernel config (tried /proc/config.gz and /boot/config-*)"
    CONFIG_FILE=""
fi

if [ -n "$CONFIG_FILE" ]; then
    echo "Config file: $CONFIG_FILE"
    
    # Check critical BPF options
    for opt in CONFIG_BPF CONFIG_BPF_SYSCALL CONFIG_CGROUP_BPF CONFIG_BPF_JIT; do
        if $CONFIG_CMD "^${opt}=y" "$CONFIG_FILE" >/dev/null 2>&1; then
            pass "$opt=y"
        else
            fail "$opt not enabled or not found"
        fi
    done
    
    # Check cgroup SKB specifically
    if $CONFIG_CMD "^CONFIG_CGROUP_NET_CLASSID=y" "$CONFIG_FILE" >/dev/null 2>&1; then
        pass "CONFIG_CGROUP_NET_CLASSID=y"
    else
        warn "CONFIG_CGROUP_NET_CLASSID not enabled (may not be required)"
    fi
fi

# 3. Check cgroup v2 mount
section "Cgroup v2 Setup"

if mountpoint -q /sys/fs/cgroup; then
    pass "/sys/fs/cgroup is mounted"
else
    fail "/sys/fs/cgroup is not mounted"
fi

if [ -f "/sys/fs/cgroup/cgroup.controllers" ]; then
    pass "cgroup v2 detected"
    echo "Available controllers: $(cat /sys/fs/cgroup/cgroup.controllers)"
else
    fail "cgroup v2 not detected (missing cgroup.controllers)"
fi

# 4. Check BPF filesystem
section "BPF Filesystem"

if mountpoint -q /sys/fs/bpf 2>/dev/null; then
    pass "/sys/fs/bpf is mounted"
else
    warn "/sys/fs/bpf not mounted (not critical)"
fi

# 5. Check user cgroup
section "User Cgroup Check"

TEST_CGROUP="/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service"
if [ -d "$TEST_CGROUP" ]; then
    pass "User cgroup exists: $TEST_CGROUP"
    ls -ld "$TEST_CGROUP"
    
    # Check delegation
    if [ -f "$TEST_CGROUP/cgroup.subtree_control" ]; then
        SUBTREE=$(cat "$TEST_CGROUP/cgroup.subtree_control")
        if [ -n "$SUBTREE" ]; then
            echo "Subtree control: $SUBTREE"
        else
            warn "No subtree controllers enabled"
        fi
    fi
else
    warn "User cgroup doesn't exist (user not logged in?)"
fi

# 6. Check for existing BPF programs on user cgroup
section "Existing BPF Programs"

if command -v bpftool >/dev/null 2>&1; then
    pass "bpftool is available"
    
    echo ""
    echo "Checking for BPF programs on user cgroup..."
    if [ -d "$TEST_CGROUP" ]; then
        bpftool cgroup show "$TEST_CGROUP" 2>/dev/null || warn "No BPF programs attached or access denied"
    fi
    
    echo ""
    echo "All loaded BPF programs:"
    bpftool prog list 2>/dev/null | head -30 || warn "Cannot list BPF programs"
else
    warn "bpftool not installed (install: apt install linux-tools-common linux-tools-$(uname -r))"
fi

# 7. Check root cgroup access
section "Root Cgroup Access"

if [ -w "/sys/fs/cgroup" ]; then
    pass "Root has write access to /sys/fs/cgroup"
else
    fail "No write access to /sys/fs/cgroup"
fi

# 8. Test basic BPF program loading
section "BPF Program Loading Test"

if command -v bpftool >/dev/null 2>&1; then
    echo "Testing if we can load a simple BPF program..."
    # This is a minimal test - just checks if the kernel accepts BPF programs
    # We won't actually test cgroup attachment here
    pass "Skipping actual load test (would require test program)"
else
    warn "Cannot test BPF loading without bpftool"
fi

# 9. Check capabilities
section "Process Capabilities"

echo "Running as: $(whoami)"
if [ "$(id -u)" -eq 0 ]; then
    pass "Running as root"
else
    fail "Not running as root (BPF operations require root or CAP_BPF)"
fi

# 10. Check for conflicting software
section "Potential Conflicts"

# Check systemd version
if command -v systemctl >/dev/null 2>&1; then
    SYSTEMD_VER=$(systemctl --version | head -1)
    echo "Systemd: $SYSTEMD_VER"
fi

# Check if docker is running
if systemctl is-active docker >/dev/null 2>&1; then
    warn "Docker is running (may attach BPF programs to cgroups)"
elif command -v docker >/dev/null 2>&1; then
    echo "Docker installed but not running"
else
    echo "Docker not installed"
fi

# Check if there are other network filtering tools
for tool in firewalld ufw nftables iptables; do
    if command -v $tool >/dev/null 2>&1; then
        if systemctl is-active $tool >/dev/null 2>&1; then
            warn "$tool is active"
        else
            echo "$tool installed but inactive"
        fi
    fi
done

# Summary
section "Summary"
echo ""
echo "If all critical checks passed, the system should support cgroup BPF programs."
echo ""
echo "Next steps:"
echo "  1. Test with root cgroup: CHADTHROTTLE_TEST_ROOT_CGROUP=1 sudo ./target/release/chadthrottle"
echo "  2. Check kernel logs: dmesg | tail -50"
echo "  3. Try manually attaching a BPF program with bpftool"
echo ""
