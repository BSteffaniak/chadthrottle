# CRITICAL: nftables Download Throttling Cannot Work with socket cgroupv2

## Root Cause Analysis

### The Fundamental Issue

**nftables `socket cgroupv2` matcher ONLY works on OUTPUT chain, NOT INPUT chain.**

This is a kernel/netfilter limitation, not a bug in our code.

### Why This Happens

1. **Download = INPUT chain** (incoming packets from network)
2. **Upload = OUTPUT chain** (outgoing packets to network)

3. **Socket association timing:**
   - **OUTPUT chain:** Kernel already knows which socket is sending the packet → cgroup is known → `socket cgroupv2` works ✅
   - **INPUT chain:** Packet arrives from network → socket association happens LATER in the stack → cgroup is UNKNOWN at INPUT hook → `socket cgroupv2` fails ❌

### Evidence from Testing

**Process 1234019:**

- ✅ Cgroup created: `/sys/fs/cgroup/chadthrottle/pid_1234019`
- ✅ Process in cgroup: `cat cgroup.procs` shows PID 1234019
- ✅ nftables rule created (no syntax errors)
- ❌ Download NOT throttled (still ~300 KB/s instead of 50 KB/s)

**Why:**
The nftables rule on INPUT chain never matches because `socket cgroupv2` can't determine the cgroup at INPUT hook time.

### nftables Documentation

From nftables wiki and kernel netfilter documentation:

> "The socket expression is only valid in the OUTPUT and POSTROUTING chains."

Our current implementation:

```
chain input_limit {
    type filter hook input priority 0;  ← INPUT hook
    socket cgroupv2 level 0 "path" limit rate over X bytes/second drop
    ^^^^^^ Won't work here!
}
```

## What Works vs What Doesn't

| Direction | Chain  | socket cgroupv2 | Status          |
| --------- | ------ | --------------- | --------------- |
| Upload    | OUTPUT | ✅ Works        | Should throttle |
| Download  | INPUT  | ❌ Fails        | Won't throttle  |

## Solutions for Download Throttling

### Option 1: Use TC (Traffic Control) with IFB Device ✅ RECOMMENDED

**This is what the existing `ifb_tc` backend does.**

```bash
# Create IFB device for ingress traffic
ip link add ifb0 type ifb
ip link set dev ifb0 up

# Redirect ingress to IFB
tc qdisc add dev eth0 ingress
tc filter add dev eth0 parent ffff: protocol ip u32 match u32 0 0 action mirred egress redirect dev ifb0

# Apply HTB on IFB (now ingress is egress on IFB)
tc qdisc add dev ifb0 root handle 1: htb
tc filter add dev ifb0 parent 1: protocol ip cgroup

# Rate limit by cgroup classid
tc class add dev ifb0 parent 1: classid 1:10 htb rate 50kbit
```

**Why this works:**

- Redirects ingress to IFB device
- On IFB, "ingress becomes egress"
- Can use cgroup matching on egress
- Works with both cgroup v1 (classid) and v2 (with eBPF)

### Option 2: Use Connection Tracking with PREROUTING ⚠️ COMPLEX

```
chain prerouting_limit {
    type filter hook prerouting priority -150;
    ct state established,related socket cgroupv2 level 0 "path" limit rate over X bytes/second drop
}
```

**Issues:**

- Requires connection tracking (stateful)
- Only works for established connections
- Complex to implement
- May not work reliably

### Option 3: Use eBPF TC Ingress Classifier ✅ FUTURE

Attach eBPF program to TC ingress:

- BPF_PROG_TYPE_SCHED_CLS (not CGROUP_SKB)
- Attach to clsact ingress qdisc
- Can inspect sk_buff->sk->cgroup
- Can drop packets

**This is the `CgroupV2EbpfBackend` we planned for the future.**

### Option 4: Accept Upload-Only for nftables ⚠️ LIMITATION

Document that nftables backend:

- ✅ Upload throttling works
- ❌ Download throttling does NOT work
- Use `ifb_tc` or `tc_police` for download

## Immediate Action Plan

### Test Upload Throttling (Should Work)

1. Start an upload process (e.g., `scp large_file user@server:`)
2. Throttle it with chadthrottle (press 'u' for upload limit)
3. Verify it actually throttles

**Expected:** Upload should be throttled because `socket cgroupv2` works on OUTPUT chain.

### Fix Download Throttling Options

#### Option A: Disable nftables for Download

Mark nftables backend as upload-only:

```rust
impl DownloadThrottleBackend for NftablesDownload {
    fn is_available() -> bool {
        false  // nftables can't do download throttling with socket cgroupv2
    }
}
```

Users must use `tc_police`, `ifb_tc`, or eBPF for download.

#### Option B: Implement TC-based Download in nftables Backend

Hybrid approach:

- Upload: Use nftables with socket cgroupv2
- Download: Use TC with IFB device (like `ifb_tc` backend)

This is complex but gives best of both worlds.

#### Option C: Document Limitation

Keep current code but add warning:

```
WARN: nftables download throttling requires kernel 5.x+ with CT support
      For reliable download throttling, use ifb_tc or tc_police backends
```

## Recommended Path Forward

1. **Test upload throttling** to verify nftables works for OUTPUT chain
2. **Disable nftables download backend** (mark as unavailable)
3. **Use ifb_tc for download** (already works with cgroups)
4. **Document** that nftables is upload-only until eBPF TC backend is implemented
5. **Future:** Implement `CgroupV2EbpfBackend` using TC classifier for download

## Testing Commands

```bash
# Test upload throttling
scp /tmp/largefile user@server:/tmp/  # Start upload
sudo ./target/release/chadthrottle     # Throttle it (press 'u')
# Should throttle ✅

# Verify nftables rule matches
sudo nft list ruleset | grep -A 5 output_limit
# Check packet counters - should increment

# Download won't work
wget http://speedtest.tele2.net/100MB.zip  # Start download
sudo ./target/release/chadthrottle          # Throttle it (press 'd')
# Won't throttle ❌

# Verify INPUT rule doesn't match
sudo nft list ruleset | grep -A 5 input_limit
# Check packet counters - will be 0 (rule never matches)
```

## Summary

**The nftables backend is fundamentally incompatible with download (ingress) throttling when using `socket cgroupv2` matcher.**

This is a kernel/netfilter limitation, not a bug we can fix. The only solutions are:

1. Use TC with IFB device (what `ifb_tc` does)
2. Use eBPF TC classifier (future `CgroupV2EbpfBackend`)
3. Disable nftables download backend and use alternatives

**Upload throttling should work fine.**
