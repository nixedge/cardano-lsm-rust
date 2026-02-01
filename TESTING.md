# Testing Guide

## Quick Test Commands

```bash
# Run all tests
cargo test

# Run specific test file
cargo test --test test_basic_operations
cargo test --test test_range_queries
cargo test --test test_compaction
cargo test --test test_wal_recovery
cargo test --test test_snapshots

# Run a single test
cargo test test_single_insert_and_lookup

# Show output from tests
cargo test -- --nocapture

# Run tests in release mode (faster)
cargo test --release
```

## Expected Results

### ✅ Should Pass (Core LSM Complete!)

#### test_basic_operations.rs
All tests should pass! We have:
- MemTable working
- SSTables working
- WAL working
- Persistence working

#### test_range_queries.rs
All tests should pass! We have:
- Range scans across memtable + SSTables
- Prefix scanning
- Proper key ordering

#### test_wal_recovery.rs
All tests should pass! We have:
- WAL replay on startup
- Checksum validation
- Partial entry handling
- Multiple crash recovery

#### test_snapshots.rs
All tests should pass! We have:
- Cheap snapshots (Arc references)
- Fast rollback
- Snapshot isolation
- Multi-level snapshots (memtable + SSTables)

#### test_compaction.rs
Most tests should pass! We have:
- Basic compaction working
- Tiered strategy
- Leveled strategy
- Hybrid strategy
- Data preservation during compaction
- Tombstone removal
- Overwrite handling

**May fail**:
- test_compaction_during_reads (no background compaction yet)
- Performance tests might be slow

### ❌ Will Fail (Not Implemented)

#### test_merkle_tree.rs
Incremental Merkle trees not implemented yet.

#### test_monoidal.rs
Monoidal values not implemented yet.

## Debugging Failed Tests

### If test_basic_operations fails:

1. **Check MemTable**:
   ```rust
   // Add debug prints to MemTable::insert
   println!("Inserting {:?} -> {:?}", key, value);
   ```

2. **Check WAL**:
   ```rust
   // Check WAL is being written
   println!("WAL size: {}", self.wal_size()?);
   ```

3. **Check SSTable**:
   ```rust
   // Check SSTables are being created
   println!("SSTable count: {}", self.sstables.read().unwrap().len());
   ```

### If test_persistence fails:

1. **Verify SSTable files exist**:
   ```bash
   ls -lh /tmp/test-*/sstables/
   ```

2. **Check SSTable can be read**:
   ```rust
   // Add to test
   let sstables = tree.sstables.read().unwrap();
   println!("Loaded {} SSTables", sstables.len());
   ```

3. **Verify WAL replay**:
   ```rust
   // Check sequence numbers
   println!("Sequence number after recovery: {}", tree.sequence_number.read().unwrap());
   ```

### If test_compaction fails:

1. **Verify compaction is triggered**:
   ```rust
   // Add before compact()
   println!("SSTable count before: {}", self.sstables.read().unwrap().len());
   // After
   println!("SSTable count after: {}", self.sstables.read().unwrap().len());
   ```

2. **Check compaction result**:
   ```rust
   // In compact()
   println!("Compaction: {} inputs -> {} output", 
            result.inputs_to_remove.len(),
            result.output.is_some());
   ```

3. **Verify data isn't lost**:
   ```rust
   // After compaction
   for key in test_keys {
       assert!(tree.get(&key)?.is_some());
   }
   ```

## Performance Testing

### Run benchmarks (once implemented):
```bash
cargo bench
```

### Profile with flamegraph:
```bash
cargo install flamegraph
cargo flamegraph --test test_large_batch_insert
```

### Memory usage:
```bash
/usr/bin/time -v cargo test test_large_batch_insert
```

## Common Issues

### Issue: "SSTable not found"
- Check that sstables/ directory exists
- Verify files are being written
- Check file permissions

### Issue: "Checksum mismatch"
- WAL or SSTable corruption
- Check disk space
- Verify bincode serialization

### Issue: "Sequence number mismatch"
- Rollback issue
- Check snapshot sequence numbers
- Verify WAL truncation

### Issue: Test hangs
- Deadlock in RwLock
- Check lock acquisition order
- Use `cargo test -- --test-threads=1` to isolate

### Issue: Tests pass individually but fail together
- Shared temp directories
- State leaking between tests
- Use proper cleanup in tests

## Test Development Tips

### Add a new test:
```rust
#[test]
fn test_my_new_feature() {
    let (mut tree, _temp) = create_test_tree();
    
    // Your test code
    tree.insert(&Key::from(b"test"), &Value::from(b"data")).unwrap();
    
    assert_eq!(tree.get(&Key::from(b"test")).unwrap(), Some(Value::from(b"data")));
}
```

### Use proptest for property testing:
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_insert_get_roundtrip(key_bytes: Vec<u8>, value_bytes: Vec<u8>) {
        let (mut tree, _temp) = create_test_tree();
        let key = Key::from(&key_bytes);
        let value = Value::from(&value_bytes);
        
        tree.insert(&key, &value)?;
        assert_eq!(tree.get(&key)?, Some(value));
    }
}
```

### Measure test performance:
```rust
use std::time::Instant;

#[test]
fn test_performance() {
    let start = Instant::now();
    
    // Your test
    
    let duration = start.elapsed();
    println!("Test took: {:?}", duration);
    assert!(duration.as_millis() < 1000, "Test too slow");
}
```

## Continuous Integration

### Run tests on every commit:
```bash
#!/bin/bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

### Pre-commit hook:
```bash
#!/bin/sh
cargo test --quiet
if [ $? -ne 0 ]; then
    echo "Tests failed, commit aborted"
    exit 1
fi
```

## Next Steps After Tests Pass

1. **Benchmarking** - Measure actual performance
2. **Optimization** - Profile and optimize hot paths
3. **Stress Testing** - Large datasets, long-running tests
4. **Integration** - Use in the Cardano indexer
5. **Documentation** - API docs and examples

## Success Metrics

- [ ] All basic_operations tests pass
- [ ] All range_queries tests pass
- [ ] All wal_recovery tests pass
- [ ] All snapshots tests pass
- [ ] All compaction tests pass (should pass now!)
- [ ] Genesis sync performance: < 8 hours
- [ ] Live block processing: < 50ms
- [ ] Rollback: < 1 second

Good luck with testing! 🧪
