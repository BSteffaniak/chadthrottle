# Backend Compatibility Modal - Test Plan

## Summary

This feature prevents silent failures when trying to throttle with traffic types (Internet Only/Local Only) that the current backend doesn't support.

## Prerequisites

- Build completed: `cargo build --release`
- Binary ready: `target/release/chadthrottle`
- Root privileges required to run

## Test Scenarios

### Scenario 1: eBPF Backend + Internet Only Traffic (Should Show Modal)

**Setup:**

1. Start ChadThrottle: `sudo target/release/chadthrottle`
2. eBPF should auto-select as default (highest priority)
3. Verify with `b` key - should show eBPF as active backend

**Test Steps:**

1. Select any process with `↑/↓` keys
2. Press `t` to open throttle dialog
3. Press `t` again to cycle traffic type to "Internet Only"
4. Set a download limit (e.g., type `1000` in Download field)
5. Press `Enter` to apply

**Expected Result:**

- Backend compatibility modal appears with red border
- Shows message about eBPF not supporting Internet Only traffic
- Displays 4+ options:
  - `○ Cancel`
  - `● Switch to nftables backend temporarily` (selected by default)
  - `○ Switch to nftables backend and make it default`
  - `○ Convert to "All Traffic" throttling`

**Test Modal Navigation:**

1. Press `↓` - selection moves to "Switch and make default"
2. Press `↑` - selection moves back to "Switch temporarily"
3. Press `j/k` - same navigation behavior
4. Press `Esc` - modal closes, throttle dialog remains open

### Scenario 2: Select "Switch Temporarily" (Default Option)

**Test Steps:**

1. Follow Scenario 1 to trigger modal
2. Ensure "Switch temporarily" is selected (should be default)
3. Press `Enter`

**Expected Result:**

- Modal closes
- Throttle dialog closes
- Status bar shows: "Throttle applied to [process] (PID [pid])"
- Press `b` to verify backend - should still show eBPF as default
- Throttle should be active with nftables backend for this one process

**Verification:**

```bash
# Check if nftables rules exist
sudo nft list ruleset | grep -i chad
```

### Scenario 3: Select "Switch and Make Default"

**Test Steps:**

1. Follow Scenario 1 to trigger modal
2. Press `↓` to select "Switch to nftables backend and make it default"
3. Press `Enter`

**Expected Result:**

- Modal closes
- Throttle dialog closes
- Status bar shows: "Throttle applied to [process] (PID [pid])"
- Press `b` to verify backend - should now show nftables as default (marked with `*`)
- Config file updated

**Verification:**

```bash
# Check config was saved
cat ~/.config/chadthrottle/config.toml | grep -A2 default_backends
```

### Scenario 4: Select "Convert to All Traffic"

**Test Steps:**

1. Follow Scenario 1 to trigger modal
2. Press `↓` multiple times to select "Convert to 'All Traffic' throttling"
3. Press `Enter`

**Expected Result:**

- Modal closes
- Throttle dialog closes
- Status bar shows: "Throttle applied to [process] (PID [pid])"
- Press `b` - should still show eBPF as default
- Throttle applied with TrafficType::All instead of Internet Only

### Scenario 5: Cancel Action

**Test Steps:**

1. Follow Scenario 1 to trigger modal
2. Press `↑` multiple times to select "Cancel"
3. Press `Enter` (or press `Esc` or `q`)

**Expected Result:**

- Modal closes
- Throttle dialog remains open
- No throttle applied
- Can still edit values or press `Esc` to close dialog

### Scenario 6: nftables Backend + Internet Only (No Modal)

**Setup:**

1. Start ChadThrottle: `sudo target/release/chadthrottle`
2. Press `b` to open backend selector
3. Press `Tab` if needed to switch to Upload backends
4. Select nftables backend and press `Enter`
5. Repeat for Download backends if needed

**Test Steps:**

1. Select a process
2. Press `t` to open throttle dialog
3. Press `t` to cycle to "Internet Only"
4. Set download limit
5. Press `Enter`

**Expected Result:**

- NO modal appears
- Throttle applies immediately
- Status bar shows success message
- nftables supports all traffic types, so no compatibility issue

### Scenario 7: Local Only Traffic

**Test Steps:**

1. Repeat Scenario 1 but cycle to "Local Only" instead of "Internet Only"
2. Press `Enter`

**Expected Result:**

- Same modal behavior as Scenario 1
- Options reference "Local Only" traffic instead of "Internet Only"

### Scenario 8: All Traffic Type (Never Shows Modal)

**Test Steps:**

1. Select process
2. Press `t` to open throttle dialog
3. Leave traffic type as "All Traffic" (default)
4. Set limits and press `Enter`

**Expected Result:**

- NO modal appears
- Throttle applies immediately
- All backends support "All Traffic" type

## Edge Cases

### Edge Case 1: No Compatible Backends Available

**Setup:**

1. This would require disabling nftables backend (unlikely scenario)
2. Try with system that doesn't have nftables support

**Expected Result:**

- Modal does NOT appear (no compatible backends to offer)
- Falls through to existing error handling
- Status bar shows error message about backend incompatibility

### Edge Case 2: Multiple Backend Switches

**Test Steps:**

1. Switch from eBPF to nftables temporarily (Scenario 2)
2. Apply another throttle with "Internet Only"
3. Should use nftables again without asking
4. Remove throttle
5. Apply throttle with "All Traffic"
6. Should use eBPF again (back to default)

## Visual Verification Checklist

When modal is displayed, verify:

- [ ] Red border around dialog
- [ ] Title: "Backend Compatibility Issue"
- [ ] Current backend name shown
- [ ] Traffic type shown (Internet Only / Local Only)
- [ ] Radio buttons with ○ (unselected) and ● (selected)
- [ ] Compatible backend names listed
- [ ] Clear option descriptions
- [ ] Navigation instructions at bottom
- [ ] Throttle dialog still visible behind modal

## Success Criteria

✅ All 8 scenarios work as expected
✅ Modal only appears when needed (incompatible backend + compatible alternatives exist)
✅ All 4 actions work correctly
✅ Config persistence works for "make default" option
✅ No crashes or panics
✅ Clear, intuitive user experience

## Cleanup After Testing

```bash
# Remove any test throttles
# (Use 'r' key in ChadThrottle to remove throttles)

# Check for leftover nftables rules
sudo nft list ruleset | grep -i chad

# Reset config if needed
rm ~/.config/chadthrottle/config.toml
```

## Known Limitations

1. **eBPF IP filtering not implemented** - This is why the modal exists
   - Future work: Implement BPF map-based IP filtering
   - Estimated effort: 8-12 hours

2. **Modal only shows for Upload OR Download** - Not both simultaneously
   - If both are incompatible, shows upload modal first
   - After handling upload, download modal would appear on next attempt

3. **Temporary switch persists until default backend is used again**
   - Not truly "one-time" - just doesn't change config
   - Uses switched backend until process is unthrottled
