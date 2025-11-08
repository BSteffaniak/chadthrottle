# Graph Modal Fix - Testing Instructions

## What was fixed:

- ESC/q now close the graph modal instead of quitting the entire app
- Graph modal is now handled before the general quit handler

## How to test:

1. Run ChadThrottle:

   ```bash
   sudo ./target/release/chadthrottle
   ```

2. Select a process with network activity (use arrow keys or j/k)

3. Press 'g' to open the bandwidth graph

4. Test closing the graph with each key:
   - Press 'g' → graph should close (app stays open) ✓
   - Open graph again with 'g'
   - Press 'Esc' → graph should close (app stays open) ✓
   - Open graph again with 'g'
   - Press 'q' → graph should close (app stays open) ✓

5. Verify the app only quits when:
   - Pressing 'q' or 'Esc' when NO modal is open
   - Pressing 'Ctrl+C' (force quit, always works)

## Expected behavior:

✅ Graph closes on 'g', 'q', or 'Esc'
✅ App remains running after closing graph
✅ Instructions at bottom show: "Press 'g', 'q', or 'Esc' to close graph"
