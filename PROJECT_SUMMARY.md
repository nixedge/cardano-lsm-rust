# Cardano LSM Rust Port - Project Summary

## What We've Created

A complete **test-driven development foundation** for porting Cardano's LSM tree from Haskell to Rust. The test suite acts as our specification, and we'll build the implementation to pass these tests.

## Project Structure

```
cardano-lsm-rust/
├── Cargo.toml                       # Project manifest with all dependencies
├── README.md                        # Comprehensive project documentation
├── src/
│   └── lib.rs                      # Type definitions and trait signatures (stubs)
└── tests/                          # Complete test suite (our specification!)
    ├── test_basic_operations.rs    # 24 tests - core CRUD operations
    ├── test_range_queries.rs       # 18 tests - range scans and prefix queries
    ├── test_compaction.rs          # 13 tests - compaction correctness
    ├── test_wal_recovery.rs        # 12 tests - crash recovery
    ├── test_snapshots.rs           # 17 tests - snapshots and rollback
    ├── test_merkle_tree.rs         # 24 tests - incremental Merkle trees
    └── test_monoidal.rs            # 19 tests - monoidal aggregations

Total: ~127 tests across 7 test suites
```

## Test Coverage

### 1. Basic Operations (test_basic_operations.rs)
**Purpose**: Verify core LSM tree functionality

Key tests:
- Insert, get, delete operations
- Overwrite handling
- Empty keys/values
- Large keys/values (1MB)
- Persistence across restarts
- Concurrent reads (10 threads)
- Binary data handling
- Sorted iteration order

### 2. Range Queries (test_range_queries.rs)
**Purpose**: Critical for address-based wallet queries

Key tests:
- Range scans with inclusive bounds
- Prefix scanning (crucial for "all addresses with prefix")
- Empty ranges
- Range queries after deletes/updates
- Large dataset scans (10,000 keys)
- Reverse bounds handling
- Sorted order verification

### 3. Compaction (test_compaction.rs)
**Purpose**: Ensure data integrity during compaction

Key tests:
- All data preserved after compaction
- Deleted keys actually removed
- Overwrites handled correctly
- Tiered compaction strategy
- Leveled compaction strategy
- **Hybrid compaction** (Cardano's approach)
- Compaction during concurrent reads
- Space reduction verification
- Multiple compaction rounds
- Bloom filter effectiveness

### 4. WAL & Recovery (test_wal_recovery.rs)
**Purpose**: Durability and crash recovery

Key tests:
- Clean shutdown recovery
- **Crash recovery from WAL** (critical!)
- Recovery with deletes
- Recovery with overwrites
- Different WAL sync modes (Always, Periodic, None)
- WAL truncation after flush
- Partial entry recovery (corrupted WAL)
- Checksum validation
- Multiple crash/recovery cycles
- Operation replay order

### 5. Snapshots & Rollback (test_snapshots.rs)
**Purpose**: Blockchain reorg handling (CRITICAL for indexer!)

Key tests:
- Snapshot captures state
- Rollback to snapshot
- Rollback with deletes/overwrites
- Multiple snapshots
- **Snapshot is cheap (<10ms)** - key requirement!
- **Rollback is fast (<100ms)** - key requirement!
- Snapshot isolation (MVCC-like)
- Snapshot after compaction
- **Blockchain-style rollback** - simulates 10-block reorg
- Memory overhead is minimal

### 6. Merkle Trees (test_merkle_tree.rs)
**Purpose**: Governance action verification

Key tests:
- Empty tree root
- Single/multiple leaf insertion
- Proof generation and verification
- Invalid proof detection
- Root changes on insert
- **Incremental insertion is efficient**
- Proof size is O(log n)
- Merkle diff between trees
- Snapshot and rollback of Merkle state
- **Governance action history verification**
- Sparse tree efficiency (don't allocate 1M nodes for 100 leaves)
- Deterministic hashing
- Insertion order matters
- Large value handling (100KB proposals)
- Proof serialization

### 7. Monoidal Values (test_monoidal.rs)
**Purpose**: Efficient balance aggregation

Key tests:
- Monoidal identity law (mempty + a = a)
- Associativity law ((a + b) + c = a + (b + c))
- Range fold aggregation
- Prefix fold (e.g., all addresses for wallet)
- Asset map aggregation (multi-asset UTxOs)
- Fold with deletes/updates
- **Fold performance (<100ms for 10K entries)**
- Saturation arithmetic (no overflow)
- Wallet total balance queries
- Asset balance per policy
- Fold after compaction
- Fold with snapshots
- **Complex multi-asset aggregation** (realistic Cardano UTxOs)

## Why This Approach Works

### 1. Specification First
The tests ARE the specification. We know exactly what the library needs to do because we've codified it in executable tests.

### 2. Incremental Development
We can implement features incrementally:
- Start with basic operations
- Add range queries
- Add compaction
- Add WAL recovery
- Add snapshots
- Add Merkle trees
- Add monoidal values

Each step has clear acceptance criteria (pass the tests).

### 3. Confidence in Correctness
When all tests pass, we know we've correctly ported the Haskell implementation's behavior.

### 4. Regression Prevention
As we optimize, tests prevent breaking changes.

### 5. Documentation
Tests serve as executable examples of how to use the API.

## Development Roadmap

### Phase 1: Core LSM (4-6 weeks)
**Goal**: Pass basic_operations and range_queries tests

Tasks:
- Implement MemTable (BTreeMap-based)
- Implement SSTable format
- Implement WAL
- Implement basic flush
- Implement get/insert/delete
- Implement range scans
- Implement bloom filters

**Acceptance**: `cargo test --test test_basic_operations` passes
                `cargo test --test test_range_queries` passes

### Phase 2: Compaction & Recovery (3-4 weeks)
**Goal**: Pass compaction and wal_recovery tests

Tasks:
- Implement tiered compaction
- Implement leveled compaction
- Implement hybrid compaction
- Implement WAL replay
- Implement crash recovery
- Implement checksum validation

**Acceptance**: `cargo test --test test_compaction` passes
                `cargo test --test test_wal_recovery` passes

### Phase 3: Advanced Features (3-4 weeks)
**Goal**: Pass snapshots, merkle_tree, and monoidal tests

Tasks:
- Implement cheap snapshots (reference counting)
- Implement rollback
- Implement incremental Merkle trees
- Implement monoidal value support
- Implement range fold

**Acceptance**: ALL tests pass! 🎉

### Phase 4: Optimization (2-3 weeks)
**Goal**: Meet performance targets

Tasks:
- Profile and optimize hot paths
- Tune memory usage
- Optimize compaction scheduling
- Add concurrent operations
- Run benchmarks

**Acceptance**: Performance targets met:
- Snapshot: < 10ms
- Rollback: < 1 second
- Range fold: < 100ms for 10K entries
- Merkle insert: < 100μs

## How to Use This

### For You (Now)
1. Review the test suite to understand requirements
2. Read the architecture document for design context
3. When ready to code, start with Phase 1

### When You're Coding
1. Pick a test file (start with `test_basic_operations.rs`)
2. Run tests: `cargo test --test test_basic_operations`
3. Implement features until tests pass
4. Move to next test file
5. Repeat until all tests pass

### Test-Driven Development Loop
```
1. Run test → Test fails
2. Write minimal code to make it pass
3. Test passes → Refactor if needed
4. Run test again → Still passes
5. Move to next test
```

## Key Design Insights from Tests

### 1. Snapshots Must Be Cheap
`test_snapshot_is_cheap` requires < 10ms for 10,000 keys. This rules out copy-on-write and requires reference counting.

### 2. Rollback Must Be Fast
`test_rollback_is_fast` requires < 100ms for 10,000 keys. This confirms snapshots are just references, not copies.

### 3. Merkle Trees Are Incremental
`test_incremental_insertion_is_efficient` requires < 100μs per insertion. This means we can't rebuild the entire tree - must update only O(log n) nodes.

### 4. Monoidal Folds Are Efficient
`test_monoidal_fold_performance` requires < 100ms for 10,000 entries. This means we can't deserialize every value - need to leverage monoidal properties for optimization.

### 5. Compaction Doesn't Block Reads
`test_compaction_during_reads` verifies that reads work while compaction runs. This requires MVCC-like isolation.

### 6. Blockchain Rollback Pattern
`test_blockchain_style_rollback` simulates a 10-block reorg. This is the REAL use case we're optimizing for!

## Dependencies Ready

All dependencies are specified in Cargo.toml:
- **Serialization**: serde, bincode
- **Async**: tokio, async-trait
- **Hashing**: blake2, blake3
- **Compression**: lz4, snap
- **I/O**: memmap2, byteorder
- **Testing**: proptest, quickcheck, criterion
- **Utilities**: parking_lot, crossbeam, bytes, crc32fast

## Next Steps

1. **Install Rust** (if not already): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

2. **Clone/Create the project**:
   ```bash
   cargo new cardano-lsm --lib
   # Copy Cargo.toml, src/lib.rs, and tests/ from this export
   ```

3. **Verify tests compile**:
   ```bash
   cargo test --no-run
   # They should compile but fail (since lib.rs has stubs)
   ```

4. **Start implementing**:
   ```bash
   # Run specific test file
   cargo test --test test_basic_operations
   
   # Implement features in src/lib.rs until tests pass
   ```

5. **Iterate through test files** until all tests pass

## Success Criteria

✅ All 127 tests pass  
✅ Performance targets met  
✅ No unsafe code (unless absolutely necessary and well-justified)  
✅ Comprehensive documentation  
✅ Ready for integration with Cardano indexer  

## Questions?

The tests are your specification. If you're unsure about a requirement, look at the relevant test - it shows exactly what's expected.

Good luck with the implementation! 🚀
