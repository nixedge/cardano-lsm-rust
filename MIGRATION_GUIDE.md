# Migration Guide: WAL-based → Snapshot-based API

**Audience**: Applications using cardano-lsm (chain indexer/wallet)
**Data Migration**: None required - Just replay blockchain from genesis
**API Changes**: How you interact with the library

---

## TL;DR

**Before (WAL-based)**: Writes were durable immediately
**After (Snapshot-based)**: You must call `save_snapshot()` periodically for durability

---

## Key Concept Change

### Old Durability Model (WAL)
```rust
// Every write was durable immediately
tree.insert(&key, &value)?;  // Written to WAL, survives crash
tree.delete(&key)?;          // Written to WAL, survives crash
// If crash happens here → all writes recovered on restart ✅
```

### New Durability Model (Snapshots)
```rust
// Writes are EPHEMERAL until snapshot
tree.insert(&key, &value)?;  // In-memory only, LOST on crash
tree.delete(&key)?;          // In-memory only, LOST on crash
// If crash happens here → all writes LOST ❌

// Make durable with snapshot
tree.save_snapshot("block-12345", "Processed up to block 12345")?;
// If crash happens here → writes recovered from snapshot ✅
```

---

## Required Code Changes

### 1. Opening the Database

#### Before (WAL)
```rust
use cardano_lsm::{LsmTree, LsmConfig};

// Open at path - WAL auto-recovered on open
let mut tree = LsmTree::open("./wallet-db", LsmConfig::default())?;

// Start processing from last WAL position
process_blocks(&mut tree)?;
```

#### After (Snapshots)
```rust
use cardano_lsm::{LsmTree, LsmConfig};

// Try to restore from latest snapshot
let mut tree = match get_latest_snapshot_name("./wallet-db")? {
    Some(snapshot_name) => {
        println!("Restoring from snapshot: {}", snapshot_name);
        LsmTree::open_snapshot("./wallet-db", &snapshot_name, LsmConfig::default())?
    }
    None => {
        println!("No snapshot found, starting fresh");
        LsmTree::open("./wallet-db", LsmConfig::default())?
    }
};

// Start processing from snapshot position (or genesis if no snapshot)
let start_block = get_snapshot_block_height(&snapshot_name)?;
process_blocks(&mut tree, start_block)?;
```

---

### 2. Processing Blocks

#### Before (WAL)
```rust
fn process_blocks(tree: &mut LsmTree) -> Result<()> {
    for block in blockchain.iter() {
        // Apply all UTxO changes
        for tx in block.transactions {
            for input in tx.inputs {
                tree.delete(&input.to_bytes())?;  // Durable immediately
            }
            for output in tx.outputs {
                tree.insert(&output.to_bytes(), &output.value_bytes())?;  // Durable immediately
            }
        }
        // Block fully processed, all changes durable ✅
    }
    Ok(())
}
```

#### After (Snapshots)
```rust
fn process_blocks(tree: &mut LsmTree, start_block: u64) -> Result<()> {
    let mut last_snapshot_block = start_block;

    for (block_height, block) in blockchain.iter_from(start_block) {
        // Apply all UTxO changes (ephemeral!)
        for tx in block.transactions {
            for input in tx.inputs {
                tree.delete(&input.to_bytes())?;  // Ephemeral
            }
            for output in tx.outputs {
                tree.insert(&output.to_bytes(), &output.value_bytes())?;  // Ephemeral
            }
        }

        // Snapshot every 100 blocks (or every 60 seconds, or both)
        if block_height % 100 == 0 || should_snapshot_by_time()? {
            let snapshot_name = format!("block-{}", block_height);
            let label = format!("Processed up to block {}", block_height);

            tree.save_snapshot(&snapshot_name, &label)?;
            last_snapshot_block = block_height;

            println!("Saved snapshot at block {}", block_height);
        }
    }

    Ok(())
}
```

---

### 3. Snapshot Strategy

Choose a strategy based on your needs:

#### Strategy A: Block-Based (Recommended for Chain Indexer)
```rust
// Snapshot every N blocks
const SNAPSHOT_INTERVAL_BLOCKS: u64 = 100;

if block_height % SNAPSHOT_INTERVAL_BLOCKS == 0 {
    tree.save_snapshot(
        &format!("block-{}", block_height),
        &format!("Block {}", block_height)
    )?;
}
```

**Pros**: Predictable, deterministic
**Cons**: Large gaps if blocks are slow

---

#### Strategy B: Time-Based
```rust
// Snapshot every 60 seconds
use std::time::{Duration, Instant};

let mut last_snapshot_time = Instant::now();
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(60);

if last_snapshot_time.elapsed() >= SNAPSHOT_INTERVAL {
    tree.save_snapshot(
        &format!("block-{}", block_height),
        &format!("Block {}", block_height)
    )?;
    last_snapshot_time = Instant::now();
}
```

**Pros**: Guarantees max data loss in wall-clock time
**Cons**: Non-deterministic snapshot points

---

#### Strategy C: Hybrid (Recommended for Production)
```rust
// Snapshot every 100 blocks OR every 60 seconds (whichever comes first)
const SNAPSHOT_INTERVAL_BLOCKS: u64 = 100;
const SNAPSHOT_INTERVAL_TIME: Duration = Duration::from_secs(60);

let mut last_snapshot_time = Instant::now();
let mut last_snapshot_block = start_block;

if (block_height - last_snapshot_block) >= SNAPSHOT_INTERVAL_BLOCKS
    || last_snapshot_time.elapsed() >= SNAPSHOT_INTERVAL_TIME {

    tree.save_snapshot(
        &format!("block-{}", block_height),
        &format!("Block {}", block_height)
    )?;

    last_snapshot_block = block_height;
    last_snapshot_time = Instant::now();
}
```

**Pros**: Best of both worlds
**Cons**: Slightly more complex

---

### 4. Snapshot Naming Convention

Use a consistent naming scheme that encodes the block height:

```rust
// Good: Easy to parse, sortable
format!("block-{:010}", block_height)  // "block-0000012345"

// Good: Human-readable with timestamp
format!("block-{}-{}", block_height, timestamp)  // "block-12345-1234567890"

// Bad: Not sortable lexicographically
format!("snapshot-{}", block_height)  // "snapshot-9" > "snapshot-10" ❌
```

Helper functions:
```rust
fn get_latest_snapshot_name(db_path: &str) -> Result<Option<String>> {
    let tree = LsmTree::open(db_path, LsmConfig::default())?;
    let snapshots = tree.list_snapshots()?;

    if snapshots.is_empty() {
        return Ok(None);
    }

    // Sort to get latest
    let mut snapshots = snapshots;
    snapshots.sort();
    Ok(Some(snapshots.last().unwrap().clone()))
}

fn get_snapshot_block_height(snapshot_name: &Option<String>) -> Result<u64> {
    match snapshot_name {
        Some(name) => {
            // Parse "block-0000012345" → 12345
            let height_str = name.strip_prefix("block-")
                .ok_or_else(|| anyhow!("Invalid snapshot name"))?;
            Ok(height_str.parse()?)
        }
        None => Ok(0)  // Start from genesis
    }
}
```

---

### 5. Handling Crashes

#### Old Behavior (WAL)
```
Process starts → WAL auto-recovered → Continue from last WAL position
```

#### New Behavior (Snapshots)
```
Process starts → Load latest snapshot → Replay from snapshot block height
```

**Example**:
```rust
// Last snapshot: block-12300
// Crash at block: 12387
// On restart: Restore from block-12300, replay 12301-12387
// Data loss: 87 blocks (if snapshot interval = 100)
```

**To minimize data loss**: Use time-based or hybrid snapshot strategy

---

## Configuration Changes

### Remove WAL Configuration

#### Before (WAL)
```rust
let config = LsmConfig {
    wal_sync_mode: WalSyncMode::Periodic(100),  // ❌ Remove
    wal_buffer_size: 1024 * 1024,               // ❌ Remove
    // ... other config
};
```

#### After (Snapshots)
```rust
let config = LsmConfig {
    max_snapshots_per_wallet: 10,  // ✅ Keep last 10 snapshots
    // ... other config (unchanged)
};
```

---

## Snapshot Cleanup

Old snapshots accumulate over time. Clean them up periodically:

```rust
fn cleanup_old_snapshots(tree: &LsmTree, keep_latest: usize) -> Result<()> {
    let mut snapshots = tree.list_snapshots()?;

    if snapshots.len() <= keep_latest {
        return Ok(());  // Nothing to clean
    }

    snapshots.sort();
    let to_delete = snapshots.len() - keep_latest;

    for snapshot_name in snapshots.iter().take(to_delete) {
        std::fs::remove_dir_all(format!("{}/snapshots/{}", tree.path(), snapshot_name))?;
        println!("Deleted old snapshot: {}", snapshot_name);
    }

    Ok(())
}

// Call periodically (e.g., after each snapshot)
if block_height % SNAPSHOT_INTERVAL_BLOCKS == 0 {
    tree.save_snapshot(&format!("block-{}", block_height), &label)?;
    cleanup_old_snapshots(&tree, 10)?;  // Keep last 10
}
```

---

## Full Example: Chain Indexer

```rust
use cardano_lsm::{LsmTree, LsmConfig, Key, Value, Result};
use std::time::{Duration, Instant};

const SNAPSHOT_INTERVAL_BLOCKS: u64 = 100;
const SNAPSHOT_INTERVAL_TIME: Duration = Duration::from_secs(60);

fn main() -> Result<()> {
    // 1. Try to restore from latest snapshot
    let snapshot_name = get_latest_snapshot_name("./wallet-db")?;
    let start_block = get_snapshot_block_height(&snapshot_name)?;

    let mut tree = match snapshot_name {
        Some(ref name) => {
            println!("Restoring from snapshot: {}", name);
            LsmTree::open_snapshot("./wallet-db", name, LsmConfig::default())?
        }
        None => {
            println!("No snapshot found, starting from genesis");
            LsmTree::open("./wallet-db", LsmConfig::default())?
        }
    };

    // 2. Process blocks with hybrid snapshot strategy
    let mut last_snapshot_time = Instant::now();
    let mut last_snapshot_block = start_block;

    for (block_height, block) in fetch_blocks_from(start_block) {
        // Process block (apply UTxO changes)
        for tx in block.transactions {
            for input in tx.inputs {
                tree.delete(&Key::from(input.to_bytes()))?;
            }
            for output in tx.outputs {
                tree.insert(
                    &Key::from(output.to_bytes()),
                    &Value::from(output.value_bytes())
                )?;
            }
        }

        // Snapshot every 100 blocks OR every 60 seconds
        let blocks_since_snapshot = block_height - last_snapshot_block;
        let time_since_snapshot = last_snapshot_time.elapsed();

        if blocks_since_snapshot >= SNAPSHOT_INTERVAL_BLOCKS
            || time_since_snapshot >= SNAPSHOT_INTERVAL_TIME {

            let snapshot_name = format!("block-{:010}", block_height);
            let label = format!("Processed up to block {}", block_height);

            tree.save_snapshot(&snapshot_name, &label)?;

            last_snapshot_block = block_height;
            last_snapshot_time = Instant::now();

            println!("Snapshot saved at block {} ({:.1}s since last)",
                     block_height, time_since_snapshot.as_secs_f64());

            // Cleanup old snapshots
            cleanup_old_snapshots(&tree, 10)?;
        }

        if block_height % 1000 == 0 {
            println!("Processed block {}", block_height);
        }
    }

    // 3. Final snapshot on graceful shutdown
    let final_block = get_latest_block_height()?;
    tree.save_snapshot(
        &format!("block-{:010}", final_block),
        "Graceful shutdown"
    )?;

    Ok(())
}

// Helper functions
fn get_latest_snapshot_name(db_path: &str) -> Result<Option<String>> {
    let tree = LsmTree::open(db_path, LsmConfig::default())?;
    let snapshots = tree.list_snapshots()?;
    Ok(snapshots.into_iter().max())  // Lexicographic sort works with "block-{:010}"
}

fn get_snapshot_block_height(snapshot_name: &Option<String>) -> Result<u64> {
    match snapshot_name {
        Some(name) => {
            let height_str = name.strip_prefix("block-")
                .ok_or_else(|| anyhow!("Invalid snapshot name"))?;
            Ok(height_str.parse()?)
        }
        None => Ok(0)  // Genesis
    }
}

fn cleanup_old_snapshots(tree: &LsmTree, keep_latest: usize) -> Result<()> {
    let mut snapshots = tree.list_snapshots()?;
    if snapshots.len() <= keep_latest {
        return Ok(());
    }

    snapshots.sort();
    for snapshot_name in snapshots.iter().take(snapshots.len() - keep_latest) {
        let snapshot_path = format!("{}/snapshots/{}", tree.path(), snapshot_name);
        std::fs::remove_dir_all(&snapshot_path)?;
    }
    Ok(())
}
```

---

## Testing the Migration

### 1. Verify Snapshot Creation
```bash
# Process some blocks
cargo run --release

# Check snapshots directory
ls -la ./wallet-db/snapshots/
# Should see: block-0000000100/, block-0000000200/, etc.
```

### 2. Test Crash Recovery
```bash
# Process to block 500, kill process at block 550
cargo run --release
# Ctrl+C at block 550

# Restart - should restore from block-0000000500
cargo run --release
# Should log: "Restoring from snapshot: block-0000000500"
# Should replay blocks 501-550
```

### 3. Verify No Data Loss After Snapshot
```bash
# Query some keys after snapshot
cargo run --release -- query <key>
# Should return expected values
```

---

## Performance Considerations

### Snapshot Cost
- **First snapshot**: ~50-100ms (depends on data size)
- **Subsequent snapshots**: ~10-20ms (hard-links are fast!)
- **Impact on sync**: Negligible if snapshot interval ≥ 100 blocks

### Tuning Snapshot Interval

**Too Frequent** (every 10 blocks):
- ❌ Slower sync due to snapshot overhead
- ✅ Minimal data loss on crash

**Too Infrequent** (every 10,000 blocks):
- ✅ Fastest sync
- ❌ Large data loss on crash (replay 10,000 blocks)

**Recommended** (every 100 blocks):
- ✅ Good balance
- ✅ ~1-2% overhead
- ✅ Acceptable replay on crash

---

## Summary

### Key Changes

1. **Add snapshot calls**: Every 100 blocks or 60 seconds
2. **Restore from snapshot**: On startup, use `open_snapshot()` if available
3. **Remove WAL config**: No longer needed
4. **Handle replay**: Expect to replay N blocks on crash (where N = snapshot interval)

### Code Diff

```diff
  fn main() -> Result<()> {
-     let mut tree = LsmTree::open("./wallet-db", config)?;
+     let snapshot = get_latest_snapshot_name("./wallet-db")?;
+     let start_block = get_snapshot_block_height(&snapshot)?;
+     let mut tree = match snapshot {
+         Some(ref name) => LsmTree::open_snapshot("./wallet-db", name, config)?,
+         None => LsmTree::open("./wallet-db", config)?,
+     };

-     for block in blockchain.iter() {
+     for block in blockchain.iter_from(start_block) {
          process_block(&mut tree, block)?;

+         if block.height % 100 == 0 {
+             tree.save_snapshot(&format!("block-{:010}", block.height), &label)?;
+         }
      }
  }
```

---

## Questions?

**Q: Do I need to migrate existing data?**
A: No! Just delete `./wallet-db/` and replay from genesis. Blockchain makes this easy.

**Q: What's the performance impact?**
A: ~1-2% slower sync due to snapshot overhead. Negligible in practice.

**Q: How much data can I lose on crash?**
A: Up to N blocks, where N is your snapshot interval (default: 100 blocks).

**Q: Can I reduce data loss risk?**
A: Yes, use time-based or hybrid strategy with smaller intervals.

**Q: Why remove WAL?**
A: Matches Haskell lsm-tree design. Simpler architecture, fewer edge cases, better tested.

---

**Happy indexing!** 🚀
