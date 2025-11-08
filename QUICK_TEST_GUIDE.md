# Quick Test Guide - eBPF Traffic Type Filtering

## 🚀 Quick Start (30 seconds)

```bash
# 1. Run ChadThrottle
sudo ./target/release/chadthrottle

# 2. Press 't' on any process to create throttle

# 3. Set limit: 100 KB/s (or any value)

# 4. Traffic type: Select "Internet" (not "All")

# 5. ✅ VERIFY: No modal appears saying "eBPF doesn't support..."
```

## ✅ Expected: SUCCESS

- Throttle created without any modal or error
- eBPF backend accepted Internet/Local traffic type

## ❌ Expected: FAILURE

- Modal appears saying backend incompatible
- Check `/tmp/chadthrottle_debug.log` for errors
- Possible causes:
  - Old binary still running (killall chadthrottle first)
  - eBPF programs not loaded

## 🧪 Functional Test (2 minutes)

```bash
# Test 1: Create "Internet Only" throttle (100 KB/s)

# Local traffic should NOT be throttled:
ping -c 5 192.168.1.1
# Expect: Normal latency (< 10ms typically)

# Internet traffic SHOULD be throttled:
ping -c 5 8.8.8.8
# Expect: Rate limited (may see packet drops or higher latency)

# More obvious test with curl:
curl -O http://192.168.1.1/somefile  # Fast
curl -O http://example.com/somefile  # Slow (100 KB/s limit)
```

```bash
# Test 2: Create "Local Only" throttle

ping 192.168.1.1  # Should be throttled
ping 8.8.8.8      # Should NOT be throttled
```

## 📊 What Counts as "Local"?

IPv4: 10.x.x.x, 172.16-31.x.x, 192.168.x.x, 127.x.x.x, 169.254.x.x
IPv6: ::1, fe80::/10, fc00::/7

Everything else = "Internet"

## 🐛 Troubleshooting

```bash
# Check if old process running:
killall chadthrottle

# Verify binary was rebuilt:
ls -lh target/release/chadthrottle
# Should show recent timestamp

# Check debug log:
tail -f /tmp/chadthrottle_debug.log

# Verify eBPF programs exist:
ls -lh target/bpfel-unknown-none/debug/deps/chadthrottle_*
```

## 📝 Report Results

When testing, please report:

1. Did modal appear? (Yes/No)
2. Did traffic filtering work correctly? (Yes/No)
3. Any errors in debug log? (Check /tmp/chadthrottle_debug.log)
4. Which kernel version? (uname -r)

---

**That's it!** The feature should just work. If the modal doesn't appear and traffic is filtered correctly, it's working! 🎉
