# IFB Module Setup Guide

## Overview

ChadThrottle requires the **IFB (Intermediate Functional Block)** kernel module for **download throttling**. Upload throttling works without IFB, but bidirectional throttling needs this module.

## What is IFB?

IFB is a virtual network device that allows Linux to apply egress (outgoing) traffic control rules to ingress (incoming) traffic. This is necessary because traditional TC (traffic control) can only shape outgoing traffic directly.

## Checking IFB Availability

```bash
# Check if IFB module is loaded
lsmod | grep ifb

# Try to load IFB module
sudo modprobe ifb numifbs=1

# Verify it worked
ip link show type ifb
```

If you see output from `ip link show type ifb`, IFB is available!

## Platform-Specific Setup

### NixOS

IFB module needs to be enabled in your system configuration.

**Option 1: Load module at boot (Recommended)**

Edit `/etc/nixos/configuration.nix`:

```nix
{ config, pkgs, ... }:

{
  # ... other config ...

  # Load IFB kernel module at boot
  boot.kernelModules = [ "ifb" ];
  
  # Optional: Set number of IFB devices
  boot.extraModprobeConfig = ''
    options ifb numifbs=1
  '';
}
```

Then rebuild:
```bash
sudo nixos-rebuild switch
```

**Option 2: Temporary load (until reboot)**

```bash
sudo modprobe ifb numifbs=1
```

**Option 3: Enable in kernel config (for custom kernel)**

If building a custom kernel:
```nix
boot.kernelPatches = [
  {
    name = "enable-ifb";
    patch = null;
    extraConfig = ''
      IFB y
    '';
  }
];
```

### Ubuntu/Debian

IFB is usually available by default. If not:

```bash
# Install linux-modules-extra (if not installed)
sudo apt install linux-modules-extra-$(uname -r)

# Load the module
sudo modprobe ifb numifbs=1

# Make it load at boot
echo "ifb" | sudo tee -a /etc/modules
echo "options ifb numifbs=1" | sudo tee /etc/modprobe.d/ifb.conf
```

### Fedora/RHEL/CentOS

```bash
# Load the module
sudo modprobe ifb numifbs=1

# Make it persistent
echo "ifb" | sudo tee /etc/modules-load.d/ifb.conf
echo "options ifb numifbs=1" | sudo tee /etc/modprobe.d/ifb.conf
```

### Arch Linux

```bash
# Load the module
sudo modprobe ifb numifbs=1

# Make it persistent
echo "ifb" | sudo tee /etc/modules-load.d/ifb.conf
echo "options ifb numifbs=1" | sudo tee /etc/modprobe.d/ifb.conf
```

### Alpine Linux

```bash
# Load the module
sudo modprobe ifb numifbs=1

# Make it persistent
echo "ifb" >> /etc/modules
echo "options ifb numifbs=1" > /etc/modprobe.d/ifb.conf
```

## Verification

After setup, verify IFB is working:

```bash
# 1. Check module is loaded
lsmod | grep ifb
# Should show: ifb    16384  0

# 2. Check IFB device can be created
sudo ip link add name test_ifb type ifb
sudo ip link show type ifb
# Should show the test_ifb device

# 3. Clean up
sudo ip link del test_ifb

# 4. Run ChadThrottle - it will auto-detect IFB
sudo /home/braden/ChadThrottle/target/release/chadthrottle
```

## Troubleshooting

### "IFB module not available"

**Symptom:** ChadThrottle shows warning about IFB unavailable

**Solutions:**

1. **Check if kernel has IFB compiled:**
   ```bash
   zgrep IFB /proc/config.gz
   # or
   grep IFB /boot/config-$(uname -r)
   ```
   
   Should show: `CONFIG_IFB=m` (module) or `CONFIG_IFB=y` (built-in)

2. **If `CONFIG_IFB=m`:** Module exists but needs loading (see platform instructions above)

3. **If `CONFIG_IFB=y`:** Built into kernel, should work automatically

4. **If not present:** Kernel needs recompilation with IFB support

### NixOS: "modprobe: FATAL: Module ifb not found"

The kernel you're running doesn't have IFB compiled. You need to:

1. **Use a kernel that includes IFB:**
   ```nix
   boot.kernelPackages = pkgs.linuxPackages_latest;
   ```

2. **Or build a custom kernel with IFB:**
   ```nix
   boot.kernelPatches = [
     {
       name = "ifb-support";
       patch = null;
       extraConfig = ''
         IFB m
       '';
     }
   ];
   ```

### "Operation not permitted" when creating IFB

You need root privileges:
```bash
sudo modprobe ifb
sudo chadthrottle  # Run ChadThrottle with sudo
```

## What Happens Without IFB?

ChadThrottle will still work, but with limitations:

- ✅ **Upload throttling works** - Uses standard TC on main interface
- ✅ **Monitoring works** - All bandwidth tracking functions normally
- ❌ **Download throttling unavailable** - Only upload can be limited

When you try to set a download limit without IFB:
```
Warning: Download throttling requested but IFB module not available.
Only upload throttling will be applied.
To enable download throttling, ensure the 'ifb' kernel module is available.
```

## Alternative: eBPF-based Download Throttling (Future)

ChadThrottle may add eBPF-based download throttling in the future, which wouldn't require IFB. This would use:
- XDP (eXpress Data Path) for ingress rate limiting
- More complex but available on all recent kernels

## Further Reading

- [Linux IFB Documentation](https://www.kernel.org/doc/Documentation/networking/ifb.txt)
- [TC (Traffic Control) Documentation](https://tldp.org/HOWTO/Traffic-Control-HOWTO/)
- [NixOS Kernel Modules](https://nixos.wiki/wiki/Linux_Kernel)
- ChadThrottle [THROTTLING.md](THROTTLING.md) for technical details
