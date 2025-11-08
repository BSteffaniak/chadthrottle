# eBPF Cleanup Fix - Comprehensive Plan

## Problem Analysis

### Current Bug

When a process terminates and we try to remove its throttle, the cleanup fails because:

1. `remove_download_throttle(pid)` tries to call `get_cgroup_path(pid)`
2. If the process has terminated, `/proc/{pid}/cgroup` doesn't exist
3. `get_cgroup_path()` returns `Err`, so `cgroup_path = None`
4. The detach code is skipped: `if let Some(ref path) = cgroup_path { ... }`
5. The program remains attached to the cgroup
6. The entry is removed from `attached_programs` vector (line 507)
7. Later cleanup can't find it because it's already gone from `attached_programs`

### Current Data Structures

```rust
struct AttachedProgram {
    cgroup_path: PathBuf,      // Already stored!
    attach_type: CgroupSkbAttachType,
    program_fd: i32,
}

struct EbpfDownload {
    ebpf: Option<Ebpf>,
    pid_to_cgroup: HashMap<i32, u64>,        // PID -> cgroup_id
    cgroup_refcount: HashMap<u64, usize>,    // cgroup_id -> count of PIDs
    attached_cgroups: HashSet<PathBuf>,      // Set of cgroup paths we've attached to
    attached_programs: Vec<AttachedProgram>, // Programs we've attached
    active_throttles: HashMap<i32, u64>,     // PID -> limit
}
```

### Current Flow

**When throttling a process:**

1. Get `cgroup_id` for PID
2. Get `cgroup_path` for PID
3. Store in `pid_to_cgroup[pid] = cgroup_id`
4. If not already attached to this cgroup:
   - Attach program to `cgroup_path`
   - Get program FD
   - Store `AttachedProgram { cgroup_path, attach_type, program_fd }`
5. Increment `cgroup_refcount[cgroup_id]`

**When removing throttle:**

1. Get `cgroup_id` from `pid_to_cgroup[pid]`
2. Try to get `cgroup_path` from PID (FAILS if process terminated!)
3. Decrement `cgroup_refcount[cgroup_id]`
4. If refcount reaches 0:
   - Remove from BPF maps
   - Find entry in `attached_programs` by matching `cgroup_path`
   - Remove from `attached_programs` and detach
   - Remove from `attached_cgroups`
5. Remove from `pid_to_cgroup`

**Problem:** Step 2 fails for terminated processes, so step 4 never runs!

## Root Cause

The fundamental issue is that we're trying to look up the cgroup path from the PID at removal time, but:

- The PID might be gone (process terminated)
- We ALREADY have the cgroup path stored in `attached_programs`!

But there's a mismatch: we know the `cgroup_id`, but `attached_programs` only stores `cgroup_path`. We need a way to find the right `AttachedProgram` entry.

## Solution Design

### Option 1: Store cgroup_id in AttachedProgram (RECOMMENDED)

Add `cgroup_id` to `AttachedProgram` so we can find it by ID:

```rust
struct AttachedProgram {
    cgroup_path: PathBuf,
    attach_type: CgroupSkbAttachType,
    program_fd: i32,
    cgroup_id: u64,  // NEW: So we can find this entry by cgroup_id
}
```

Then in `remove_download_throttle()`:

- We have `cgroup_id` from `pid_to_cgroup[pid]`
- Find the `AttachedProgram` by matching `cgroup_id`
- Use its stored `cgroup_path` and `program_fd` to detach
- No need to call `get_cgroup_path(pid)` at all!

**Pros:**

- Simple and direct
- No need to query /proc for terminated processes
- All needed info is stored upfront

**Cons:**

- Adds one field to struct

### Option 2: Store cgroup_path in pid_to_cgroup

Change `pid_to_cgroup` to store both ID and path:

```rust
pid_to_cgroup: HashMap<i32, (u64, PathBuf)>,  // PID -> (cgroup_id, cgroup_path)
```

**Pros:**

- Don't need to modify AttachedProgram

**Cons:**

- Duplicates cgroup_path storage
- Still need to find AttachedProgram entry by path

### Option 3: Use cgroup_path as the key

Instead of tracking by `cgroup_id`, use `cgroup_path` as the key everywhere.

**Pros:**

- Consistent key throughout

**Cons:**

- Large refactoring required
- PathBuf as HashMap key is less efficient

## Recommended Solution: Option 1

Add `cgroup_id` to `AttachedProgram` and fix the lookup logic.

## Implementation Plan

### Phase 1: Update AttachedProgram struct

**Files:** `download/linux/ebpf.rs`, `upload/linux/ebpf.rs`

```rust
#[derive(Debug, Clone)]
struct AttachedProgram {
    cgroup_path: PathBuf,
    attach_type: CgroupSkbAttachType,
    program_fd: i32,
    cgroup_id: u64,  // NEW
}
```

### Phase 2: Update where AttachedProgram is created

**Files:** `download/linux/ebpf.rs`, `upload/linux/ebpf.rs`

In `throttle_download()` / `throttle_upload()`, when pushing to `attached_programs`:

```rust
self.attached_programs.push(AttachedProgram {
    cgroup_path: cgroup_path.clone(),
    attach_type: CgroupSkbAttachType::Ingress,
    program_fd,
    cgroup_id,  // NEW: We already have this variable
});
```

### Phase 3: Fix remove_download_throttle / remove_upload_throttle

**Current problematic code:**

```rust
let cgroup_path = match get_cgroup_path(pid) {
    Ok(path) => Some(path),
    Err(e) => {
        log::warn!("Could not get cgroup path for PID {}: {}", pid, e);
        None  // PROBLEM: Returns None for terminated processes
    }
};
```

**New code:**

```rust
// Find the attached program by cgroup_id (works even if process terminated)
let attached_program_info = self.attached_programs
    .iter()
    .find(|p| p.cgroup_id == cgroup_id)
    .map(|p| (p.cgroup_path.clone(), p.program_fd));
```

Then update the detach logic:

```rust
if *refcount == 0 {
    // ... remove from maps ...

    // Detach BPF program using stored info
    if let Some((cgroup_path, program_fd)) = attached_program_info {
        if let Some(pos) = self.attached_programs
            .iter()
            .position(|p| p.cgroup_id == cgroup_id)
        {
            let attached = self.attached_programs.remove(pos);
            log::info!(
                "Detaching BPF program from cgroup: {:?} (id: {}, fd: {})",
                attached.cgroup_path,
                attached.cgroup_id,
                attached.program_fd
            );
            if let Err(e) = detach_cgroup_skb_legacy(
                &attached.cgroup_path,
                attached.attach_type,
                attached.program_fd,
            ) {
                log::error!(
                    "Failed to detach program from {:?}: {}",
                    attached.cgroup_path,
                    e
                );
                // Don't return error - continue cleanup
            } else {
                log::info!("✅ Successfully detached BPF program");
            }
            self.attached_cgroups.remove(&attached.cgroup_path);
        }
    } else {
        log::warn!(
            "Could not find attached program for cgroup_id {} - may have already been cleaned up",
            cgroup_id
        );
    }

    // Remove reference count entry
    self.cgroup_refcount.remove(&cgroup_id);
}
```

### Phase 4: Simplify cleanup() function

The `cleanup()` function currently calls `remove_download_throttle()` for each PID, which removes entries from `attached_programs`. Then it tries to iterate over `attached_programs` again (which is now empty).

**Current problematic code:**

```rust
// Remove all throttles (clears BPF maps)
let pids: Vec<i32> = self.active_throttles.keys().copied().collect();
for pid in pids {
    let _ = self.remove_download_throttle(pid);  // Empties attached_programs!
}

// Detach all attached programs (now empty!)
for attached in &self.attached_programs {
    // ...
}
```

**Options:**

**Option A: Skip remove_download_throttle in cleanup**

```rust
// Clear BPF maps manually
if let Some(ref mut ebpf) = self.ebpf {
    // Remove all entries from maps
    // ... (similar code to what's in remove_download_throttle)
}

// Detach all attached programs
for attached in &self.attached_programs {
    // Detach using stored info
}

// Clear all tracking structures
self.attached_programs.clear();
self.active_throttles.clear();
self.pid_to_cgroup.clear();
self.cgroup_refcount.clear();
self.attached_cgroups.clear();
```

**Option B: Only call remove_download_throttle, remove duplicate detach loop**

```rust
// Remove all throttles (this will detach programs)
let pids: Vec<i32> = self.active_throttles.keys().copied().collect();
for pid in pids {
    let _ = self.remove_download_throttle(pid);
}

// No need for second loop - already detached above
// Just clear any remaining state
self.attached_programs.clear();
self.attached_cgroups.clear();
```

**Recommendation: Option B** - Less code duplication, clearer intent

### Phase 5: Add defensive logging

Add warnings if we detect orphaned programs:

```rust
fn cleanup(&mut self) -> Result<()> {
    log::info!("Cleaning up eBPF download backend");

    // Remove all throttles
    let pids: Vec<i32> = self.active_throttles.keys().copied().collect();
    for pid in pids {
        if let Err(e) = self.remove_download_throttle(pid) {
            log::warn!("Error removing throttle for PID {}: {}", pid, e);
        }
    }

    // Check for orphaned programs (shouldn't happen, but log if they exist)
    if !self.attached_programs.is_empty() {
        log::warn!(
            "Found {} orphaned attached programs during cleanup - detaching them now",
            self.attached_programs.len()
        );

        // Detach any remaining programs
        for attached in &self.attached_programs {
            log::warn!("Detaching orphaned program: {:?}", attached);
            if let Err(e) = detach_cgroup_skb_legacy(
                &attached.cgroup_path,
                attached.attach_type,
                attached.program_fd,
            ) {
                log::error!("Failed to detach orphaned program: {}", e);
            }
        }
    }

    // Final cleanup
    self.attached_programs.clear();
    self.ebpf = None;
    self.pid_to_cgroup.clear();
    self.cgroup_refcount.clear();
    self.attached_cgroups.clear();

    log::info!("eBPF download backend cleanup complete");
    Ok(())
}
```

## Testing Plan

### Test Case 1: Normal removal

1. Start ChadThrottle
2. Throttle a running process
3. Press 'r' to remove throttle
4. **Expected:** Program detaches immediately, no leftovers

### Test Case 2: Terminated process removal

1. Start ChadThrottle
2. Throttle a process
3. Kill the process externally
4. Press 'r' to remove throttle
5. **Expected:** Program detaches even though process is gone

### Test Case 3: Exit with active throttles

1. Start ChadThrottle
2. Throttle multiple processes
3. Kill some of them externally
4. Press 'q' to quit
5. **Expected:** All programs detached, no leftovers

### Test Case 4: Multiple throttles same cgroup

1. Throttle multiple processes in the same cgroup
2. Remove them one by one
3. **Expected:** Program only detaches when last one is removed

### Verification Commands

Check for leftover programs:

```bash
set CGROUP "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/tmux-spawn-*.scope"
sudo bpftool cgroup show "$CGROUP" | grep chadthrottle
```

Should return nothing after cleanup.

## Files to Modify

1. `chadthrottle/src/backends/throttle/download/linux/ebpf.rs`
   - Update `AttachedProgram` struct
   - Fix `throttle_download()` to store cgroup_id
   - Fix `remove_download_throttle()` to use stored info
   - Improve `cleanup()` with orphan detection

2. `chadthrottle/src/backends/throttle/upload/linux/ebpf.rs`
   - Same changes as download backend

## Success Criteria

✅ Programs detach when removing throttle (even for terminated processes)
✅ Programs detach on application exit
✅ No orphaned programs left attached
✅ Cleanup logs show successful detachment
✅ Warning logs if orphaned programs are detected
