# Cardano LSM Tree - Pure Rust Port

A pure Rust port of Cardano's `lsm-tree` library from Haskell, designed specifically for blockchain indexing with a focus on UTxO workloads.

## Overview

This library implements a Log-Structured Merge (LSM) tree optimized for Cardano blockchain indexing. It was created to avoid the corruption and performance issues that plagued RocksDB-based Byron wallets, while providing advanced features needed for comprehensive blockchain indexing.

## Key Features

- **Pure Rust** - No FFI, no Haskell runtime dependencies
- **Cheap Snapshots** - Reference-counted snapshots for instant rollback (critical for blockchain reorgs)
- **Optimized Compaction** - Hybrid tiered/leveled strategy for blockchain write patterns
- **High-Performance I/O** - Optional io_uring support for batched concurrent reads on Linux
- **Conformance Tested** - 10,000+ property-based tests validating compatibility with Haskell reference implementation
- **Bloom Filters** - Fast negative lookups to skip non-existent keys

## Why This LSM Tree?

The Cardano team developed their own LSM tree implementation in Haskell after experiencing significant issues with RocksDB in Byron-era wallets. This Rust port brings those improvements to the Rust ecosystem:

| Feature | RocksDB | Cardano LSM (This Port) |
|---------|---------|-------------------------|
| Corruption Issues | Known problems in Byron | Designed to prevent |
| Snapshot Cost | Expensive COW | Cheap (reference counting) |
| UTxO Optimization | Generic | Purpose-built |
| Conformance | N/A | 10,000+ tests vs Haskell reference |

## Project Structure

```
cardano-lsm-rust/
├── src/
│   ├── lib.rs                     # Core LSM tree implementation
│   ├── sstable_new.rs             # SSTable format and I/O
│   ├── compaction.rs              # Compaction strategies
│   ├── snapshot.rs                # Snapshot functionality
│   └── ...                        # Supporting modules
├── tests/
│   ├── conformance.rs             # 10,000+ conformance tests vs Haskell reference
│   ├── test_rollback_insert.rs    # Rollback testing
│   ├── test_snapshot_restoration.rs  # Snapshot save/restore
│   ├── batch_operations.rs        # Batch insert/delete operations
│   └── cross_format.rs            # Cross-format validation with Haskell
├── benches/
│   └── lsm_benchmarks.rs          # Performance benchmarks
├── Cargo.toml
└── README.md
```

## Development Status

**Current Version**: 1.0.0

This implementation is complete and production-ready:

- ✅ **Core LSM implementation** - All basic operations working
- ✅ **Snapshots and rollback** - Fast, reference-counted snapshots
- ✅ **Compaction strategies** - Tiered, leveled, and hybrid (LazyLevelling policy)
- ✅ **Bloom filters** - Fast negative lookups for non-existent keys
- ✅ **Conformance tested** - 10,000+ property-based tests passing (100% pass rate)

### Conformance Testing

This implementation has been rigorously validated against the Haskell `lsm-tree` reference implementation:

- **10,000+ property-based tests** generated from the Haskell implementation
- **100% pass rate** - All tests passing
- **Operations tested**: Insert, get, delete, range queries, tombstones, persistence
- **Test generation**: Automated conformance test harness with Haskell reference

Run conformance tests:
```bash
cargo test --test conformance --release
```

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

### Conformance Tests (conformance.rs)
- **10,000+ property-based tests** validating Rust implementation against Haskell reference
- Insert, get, delete operations
- Range queries with proper ordering
- Tombstone handling
- Data persistence
- **100% pass rate**

## Running Tests

```bash
# Run all tests
cargo test

# Run specific test file
cargo test --test test_basic_operations

# Run conformance tests (10,000+ tests)
cargo test --test conformance --release

# Run with output
cargo test -- --nocapture

# Run specific conformance test
CONFORMANCE_TEST_FILTER=test_123 cargo test --test conformance -- --nocapture
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

## Optional Features

### io_uring Support (Linux Only)

For high-performance I/O on Linux, enable the `io-uring` feature:

```bash
# Build with io_uring support
cargo build --features io-uring

# Run tests with io_uring
cargo test --features io-uring
```

**What is io_uring?**

`io_uring` is a Linux kernel interface for asynchronous I/O operations. It provides significant performance benefits for LSM trees by:

- **Batched concurrent reads**: During compaction, reading from multiple SSTables happens concurrently rather than sequentially
- **Reduced syscall overhead**: Operations are submitted in batches, minimizing context switches
- **Better hardware utilization**: Modern NVMe SSDs can handle multiple parallel I/O operations efficiently

This matches the Haskell implementation's `blockio-uring` library for optimal performance.

**Platform Support:**
- **Linux with io_uring kernel support**: Full async I/O with batching (recommended for production)
- **Other platforms**: Automatic fallback to synchronous I/O

**Configuration:**

```rust
use cardano_lsm::{LsmConfig, LsmTree};
use cardano_lsm::io_backend::IoBackend;

let mut config = LsmConfig::default();

// Linux with io_uring feature enabled
#[cfg(all(target_os = "linux", feature = "io-uring"))]
{
    config.io_backend = IoBackend::IoUring;
}

// Other platforms or without feature
config.io_backend = IoBackend::Sync;

let tree = LsmTree::open("./data", config)?;
```

## Performance Targets

The implementation meets these performance requirements:

- **Genesis sync**: < 8 hours for Cardano mainnet from slot 0
- **Live block processing**: < 50ms per block
- **Snapshot creation**: < 10ms
- **Rollback**: < 1 second
- **Insert/Get operations**: < 10μs

Run benchmarks: `cargo bench`

## API Usage

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

## Contributing

Contributions are welcome! This implementation is complete and production-ready, validated by 10,000+ conformance tests. Areas for contribution:

- Performance optimizations
- Additional benchmarks
- Documentation improvements
- Bug reports and fixes

## References

- [Cardano lsm-tree (Haskell)](https://github.com/input-output-hk/lsm-tree)
- [LSM Tree Paper](https://www.cs.umb.edu/~poneil/lsmtree.pdf) - O'Neil et al.
- [Cardano Indexer Architecture](../cardano-indexer-architecture.md)

## License

Apache-2.0

## Acknowledgments

This is a port of the Haskell `lsm-tree` library developed by Input Output Global (IOG) for Cardano. The original design and algorithms are theirs; we're bringing it to Rust.
