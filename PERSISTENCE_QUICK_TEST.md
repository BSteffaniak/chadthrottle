# Quick Test: Backend Persistence

## 🧪 5-Minute Verification Test

### Step 1: Start Fresh

```bash
# Remove any existing config
rm -f ~/.config/chadthrottle/throttles.json

# Start ChadThrottle
sudo ./target/release/chadthrottle
```

### Step 2: Check Default Backend

```
Press 'b' to view backends
→ You should see ⭐ next to "ebpf" (auto-selected highest priority)
Press 'Esc' to close
```

### Step 3: Switch to Different Backend

```
Press 'b' → Press 'Enter' to open selector
Navigate to "tc_htb" with ↓
Press 'Enter' to select
→ Status message: "✅ New throttles will use 'tc_htb' backend"
```

### Step 4: Quit and Verify Config Saved

```bash
# Quit ChadThrottle
Press 'q'

# Check config file was created and saved
cat ~/.config/chadthrottle/throttles.json

# Expected output:
{
  "throttles": {},
  "auto_restore": false,
  "preferred_upload_backend": "tc_htb",
  "preferred_download_backend": "ebpf"
}
```

### Step 5: Restart and Verify Persistence ✨

```bash
# Restart ChadThrottle
sudo ./target/release/chadthrottle

# Press 'b' to view backends
# ⭐ SHOULD NOW BE NEXT TO "tc_htb" (not ebpf!)
```

**✅ SUCCESS:** If the ⭐ moved to tc_htb, persistence is working!

---

## 🐛 If It Still Doesn't Work

### Enable Debug Logging:

```bash
sudo RUST_LOG=info ./target/release/chadthrottle 2>&1 | grep -i "backend"
```

### Look for These Messages:

```
✅ GOOD: "Using upload backend from config: tc_htb"
❌ BAD:  "Auto-selected upload backend: ebpf"
```

If you see "Auto-selected" instead of "from config", the fix didn't work.

### Check Config File Permissions:

```bash
ls -la ~/.config/chadthrottle/
cat ~/.config/chadthrottle/throttles.json
```

### Verify Binary Version:

```bash
ls -lh ./target/release/chadthrottle
# Should show recent timestamp (after the fix)
```

---

## 🎯 Expected Behavior

| Action                | Before Fix              | After Fix ✅                   |
| --------------------- | ----------------------- | ------------------------------ |
| Select backend in TUI | Config saved            | Config saved                   |
| Restart app           | Uses auto-detect (eBPF) | Uses saved preference (tc_htb) |
| Backend info modal    | ⭐ on wrong backend     | ⭐ on saved backend            |

---

## 📊 Priority Test

Test the 3-tier priority system:

```bash
# 1. Set preference via TUI to "tc_htb"
sudo ./target/release/chadthrottle
# Press 'b' → Enter → select "tc_htb" → Enter → 'q'

# 2. Verify config saved
cat ~/.config/chadthrottle/throttles.json | grep preferred

# 3. Override with CLI flag
sudo ./target/release/chadthrottle --upload-backend ebpf
# Press 'b' → Should show ⭐ next to "ebpf" (CLI override)

# 4. Restart without CLI flag
sudo ./target/release/chadthrottle
# Press 'b' → Should show ⭐ next to "tc_htb" (from config)
```

**Priority:** CLI flags > Config file > Auto-detect ✅

---

## 🔍 Troubleshooting

### Problem: ⭐ Always on eBPF

**Cause:** Old binary, config not loaded  
**Fix:** Rebuild and verify timestamp

### Problem: Config file empty

**Cause:** Permission issue or save failed  
**Fix:** Check ~/.config/chadthrottle/ permissions

### Problem: Config shows preference but not used

**Cause:** This was the original bug!  
**Fix:** Make sure you're running the NEW binary (after fix)

---

## ✅ Success Criteria

- [ ] Config file created on first backend selection
- [ ] Config file shows correct `preferred_*_backend` fields
- [ ] Restarting app loads backend from config (not auto-detect)
- [ ] Backend info modal shows ⭐ on saved backend
- [ ] CLI flags override config preferences
- [ ] Logs show "Using upload backend from config: X"

All checkboxes ticked = **Persistence is working!** 🎉
