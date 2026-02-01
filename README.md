# Cardano LSM Tree - Pure Rust Port

A pure Rust port of Cardano's `lsm-tree` library from Haskell, designed specifically for blockchain indexing with a focus on UTxO workloads.

## Overview

This library implements a Log-Structured Merge (LSM) tree optimized for Cardano blockchain indexing. It was created to avoid the corruption and performance issues that plagued RocksDB-based Byron wallets, while providing advanced features needed for comprehensive blockchain indexing.

## Key Features

- **Pure Rust** - No FFI, no Haskell runtime dependencies
- **Incremental Merkle Trees** - Built-in cryptographic verification for governance actions
- **Monoidal Values** - Efficient range aggregations for balance queries
- **Cheap Snapshots** - Reference-counted snapshots for instant rollback (critical for blockchain reorgs)
- **Optimized Compaction** - Hybrid tiered/leveled strategy for blockchain write patterns
- **Crash Recovery** - Write-ahead log with checksums
- **No RocksDB** - Avoids historical corruption issues

## Why This LSM Tree?

The Cardano team developed their own LSM tree implementation in Haskell after experiencing significant issues with RocksDB in Byron-era wallets. This Rust port brings those improvements to the Rust ecosystem:

| Feature | RocksDB | Cardano LSM (This Port) |
|---------|---------|-------------------------|
| Corruption Issues | Known problems in Byron | Designed to prevent |
| Snapshot Cost | Expensive COW | Cheap (reference counting) |
| Merkle Trees | Not supported | Incremental, built-in |
| UTxO Optimization | Generic | Purpose-built |
| Monoidal Values | Not supported | Native support |

## Project Structure

```
cardano-lsm-rust/
├── src/
│   └── lib.rs              # Core types and trait definitions
├── tests/
│   ├── test_basic_operations.rs   # Insert, get, delete, iteration
│   ├── test_range_queries.rs      # Range scans and prefix queries
│   ├── test_compaction.rs         # Compaction correctness
│   ├── test_wal_recovery.rs       # WAL and crash recovery
│   ├── test_snapshots.rs          # Snapshots and rollback
│   ├── test_merkle_tree.rs        # Incremental Merkle trees
│   └── test_monoidal.rs           # Monoidal value aggregation
├── benches/
│   └── lsm_benchmarks.rs          # Performance benchmarks
├── Cargo.toml
└── README.md
```

## Development Approach: Test-Driven

This project follows a **test-driven development** approach:

1. ✅ **Test suite ported** from Haskell lsm-tree
2. ⏳ **Implementation** - Build library to pass tests
3. ⏳ **Optimization** - Tune performance
4. ⏳ **Integration** - Use in Cardano indexer

### Current Status

- [x] Test suite created (7 test files, ~80 tests)
- [ ] Core LSM implementation
- [ ] Incremental Merkle trees
- [ ] Monoidal value support
- [ ] Performance optimization

## Test Suite

### Basic Operations (test_basic_operations.rs)
- Empty tree lookup
- Single insert and lookup
- Multiple inserts
- Overwrite existing keys
- Delete operations
- Large batch inserts
- Persistence across restarts
- Concurrent reads
- Sorted iteration

### Range Queries (test_range_queries.rs)
- Range scans (inclusive bounds)
- Prefix scans
- Empty ranges
- Range queries with deletes
- Large dataset scans
- Address-like key patterns

### Compaction (test_compaction.rs)
- Data preservation during compaction
- Tombstone removal
- Overwrite handling
- Tiered compaction strategy
- Leveled compaction strategy
- Hybrid compaction strategy
- Compaction during reads
- Space reduction
- Multiple compaction rounds

### WAL & Recovery (test_wal_recovery.rs)
- Clean shutdown recovery
- Crash recovery from WAL
- Recovery with deletes
- Recovery with overwrites
- WAL sync modes
- WAL truncation after flush
- Partial entry recovery
- Checksum validation
- Multiple crash cycles

### Snapshots (test_snapshots.rs)
- Snapshot captures current state
- Rollback to snapshot
- Rollback with deletes
- Rollback with overwrites
- Multiple snapshots
- **Snapshot is cheap** (< 10ms)
- **Rollback is fast** (< 100ms)
- Snapshot isolation
- Blockchain-style rollback (simulates reorgs)

### Merkle Trees (test_merkle_tree.rs)
- Empty tree root
- Insert and proof generation
- Proof verification
- Invalid proof detection
- Root changes on insert
- **Incremental insertion is efficient**
- Proof size is logarithmic O(log n)
- Merkle diff between trees
- Snapshot and rollback
- Governance action verification
- Sparse tree efficiency
- Deterministic hashing

### Monoidal Values (test_monoidal.rs)
- Monoidal identity law
- Range fold aggregation
- Prefix fold
- Asset map aggregation
- Fold with deletes/updates
- **Fold performance** (< 100ms for 10K entries)
- Associativity property
- Saturation behavior
- Wallet total balance queries
- Multi-asset UTxO aggregation

## Running Tests

```bash
# Run all tests
cargo test

# Run specific test file
cargo test --test test_basic_operations

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_snapshot_is_cheap
```

## Building

```bash
# Build library
cargo build

# Build with optimizations
cargo build --release

# Check without building
cargo check
```

## Performance Targets

Based on the test suite, we're targeting:

- **Genesis sync**: < 8 hours for Cardano mainnet from slot 0
- **Live block processing**: < 50ms per block
- **Snapshot creation**: < 10ms
- **Rollback**: < 1 second
- **Range fold**: < 100ms for 10,000 entries
- **Merkle insertion**: < 100μs per action

## Implementation Phases

### Phase 1: Core LSM (4-6 weeks)
- [ ] MemTable (in-memory sorted write buffer)
- [ ] SSTable format and I/O
- [ ] Basic tiered compaction
- [ ] Write-ahead log (WAL)
- [ ] Bloom filters
- [ ] Range scans

**Acceptance**: Pass all basic_operations and range_queries tests

### Phase 2: Advanced Features (3-4 weeks)
- [ ] Incremental Merkle trees
- [ ] Monoidal value support
- [ ] Cheap snapshots via reference counting
- [ ] Hybrid compaction strategy

**Acceptance**: Pass all tests

### Phase 3: Optimization (2-3 weeks)
- [ ] Performance tuning
- [ ] Memory optimization
- [ ] Concurrent operations
- [ ] Benchmarking

**Acceptance**: Meet performance targets

### Phase 4: Integration (2 weeks)
- [ ] Integration with Cardano indexer
- [ ] Real-world testing
- [ ] Documentation
- [ ] Production readiness

## API Preview

```rust
use cardano_lsm::{LsmTree, LsmConfig, Key, Value};

// Open LSM tree
let config = LsmConfig::default();
let mut tree = LsmTree::open("./data", config)?;

// Insert
tree.insert(&Key::from(b"hello"), &Value::from(b"world"))?;

// Get
let value = tree.get(&Key::from(b"hello"))?;
assert_eq!(value, Some(Value::from(b"world")));

// Range scan
for (key, value) in tree.range(&Key::from(b"a"), &Key::from(b"z")) {
    println!("{:?} -> {:?}", key, value);
}

// Snapshot (cheap!)
let snapshot = tree.snapshot();

// Modify tree
tree.insert(&Key::from(b"new_key"), &Value::from(b"new_value"))?;

// Rollback (fast!)
tree.rollback(snapshot)?;
```

### Monoidal Values

```rust
use cardano_lsm::{MonoidalLsmTree, Monoidal};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Balance(u64);

impl Monoidal for Balance {
    fn mempty() -> Self { Balance(0) }
    fn mappend(&self, other: &Self) -> Self {
        Balance(self.0 + other.0)
    }
}

let mut tree = MonoidalLsmTree::<Balance>::open("./balances", config)?;

// Insert balances for different addresses
tree.insert(&Key::from(b"addr1"), &Balance(1000))?;
tree.insert(&Key::from(b"addr2"), &Balance(2000))?;
tree.insert(&Key::from(b"addr3"), &Balance(3000))?;

// Efficiently aggregate range
let total = tree.range_fold(&Key::from(b"addr1"), &Key::from(b"addr3"));
assert_eq!(total, Balance(6000));
```

### Incremental Merkle Trees

```rust
use cardano_lsm::IncrementalMerkleTree;

let mut tree = IncrementalMerkleTree::new(16); // height 16

// Insert governance action
let proof = tree.insert(b"action_id", b"proposal_data");

// Verify proof
let root = tree.root();
let is_valid = IncrementalMerkleTree::verify_proof(
    root,
    b"action_id",
    b"proposal_data",
    &proof
);
assert!(is_valid);
```

## Contributing

We're building this in the open! The test suite is complete, and we're now implementing the library to pass all tests. Contributions welcome:

1. Pick a test file that's failing
2. Implement the features needed to pass those tests
3. Submit a PR

## References

- [Cardano lsm-tree (Haskell)](https://github.com/input-output-hk/lsm-tree)
- [LSM Tree Paper](https://www.cs.umb.edu/~poneil/lsmtree.pdf) - O'Neil et al.
- [Cardano Indexer Architecture](../cardano-indexer-architecture.md)

## License

Apache-2.0

## Acknowledgments

This is a port of the Haskell `lsm-tree` library developed by Input Output Global (IOG) for Cardano. The original design and algorithms are theirs; we're bringing it to Rust.
