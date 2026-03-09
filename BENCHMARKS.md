# LSM Tree Benchmarks

## Overview

Comprehensive performance benchmarks for the Cardano LSM tree implementation, measuring all critical operations with realistic workloads.

## Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark group
cargo bench basic_ops
cargo bench merkle_ops
cargo bench blockchain_ops

# Run specific benchmark
cargo bench snapshot_10k

# Generate HTML report
cargo bench -- --save-baseline main

# Compare against baseline
cargo bench -- --baseline main
```

## Benchmark Groups

### 1. Basic Operations (`basic_ops`)

**Benchmarks:**
- `insert` - Insert throughput at 100, 1K, 10K entries
- `get_existing` - Lookup existing key
- `get_nonexistent` - Lookup non-existent key (bloom filter test)
- `delete` - Delete throughput

**Performance Targets:**
- Insert: > 100K ops/sec
- Get (existing): < 10μs (p50), < 50μs (p99)
- Get (non-existent): < 1μs (bloom filter)
- Delete: > 100K ops/sec

### 2. Range Operations (`range_ops`)

**Benchmarks:**
- `range_scan` - Scan 10, 100, 1000 entries
- `prefix_scan_1000_entries` - Prefix scan with 1000 matches

**Performance Targets:**
- Range scan (100 entries): < 1ms
- Range scan (1000 entries): < 10ms
- Prefix scan (1000 entries): < 10ms

### 3. Compaction (`compaction_ops`)

**Benchmarks:**
- `compaction` - Compact 1K, 5K, 10K entries

**Performance Targets:**
- Compaction (1K entries): < 100ms
- Compaction (10K entries): < 1s
- Throughput: > 10K entries/sec

### 4. Snapshot & Rollback (`snapshot_ops`)

**Benchmarks:**
- `snapshot_10k_entries` - Create snapshot of 10K entry tree
- `rollback` - Rollback 100, 1K, 5K new entries

**Performance Targets:**
- Snapshot: < 10ms (regardless of size) ⭐ CRITICAL
- Rollback (any size): < 1s ⭐ CRITICAL

### 5. Persistence (`persistence_ops`)

**Benchmarks:**
- `close_and_reopen_10k_entries` - Full persistence cycle

**Performance Targets:**
- Reopen: < 100ms (WAL replay + SSTable loading)

### 6. Blockchain-Specific (`blockchain_ops`)

**Benchmarks:**
- `utxo_lookup` - Lookup UTXO by tx_hash#index
- `process_block_100_txs` - Process block with 100 txs
- `chain_reorg` - Rollback 1, 10, 100 blocks

**Performance Targets:**
- UTXO lookup: < 10μs ⭐ CRITICAL PATH
- Block processing (100 txs): < 50ms ⭐ LIVE SYNC
- Chain reorg (10 blocks): < 1s ⭐ REORG HANDLING

### 7. Throughput (`throughput_ops`)

**Benchmarks:**
- `sequential_writes_100k` - Bulk insert throughput
- `random_reads_10k` - Random read throughput

**Performance Targets:**
- Write throughput: > 50K entries/sec
- Read throughput: > 100K ops/sec (with bloom filters)

## Performance Targets Summary

| Operation | Target | Critical For |
|-----------|--------|--------------|
| Insert | < 10μs | Live sync |
| Get (hit) | < 10μs | UTXO lookup |
| Get (miss) | < 1μs | Bloom filter |
| Range (100) | < 1ms | Address queries |
| Snapshot | < 10ms | Every block |
| Rollback | < 1s | Chain reorg |
| Block processing | < 50ms | Live sync |

## Interpreting Results

### Criterion Output

```
insert/1000            time:   [8.2ms 8.5ms 8.8ms]
                       thrpt:  [113K elem/s 117K elem/s 121K elem/s]
```

- `time`: p50, mean, p99 latency
- `thrpt`: Throughput (operations/sec)

### Good vs Bad Performance

**Good:**
```
snapshot/10k_entries   time:   [2.1ms 2.3ms 2.5ms]  ✅ < 10ms target
```

**Concerning:**
```
snapshot/10k_entries   time:   [45ms 48ms 51ms]  ❌ Way over 10ms
```

### What to Optimize

If benchmarks show poor performance:

1. **Snapshot > 10ms**: Check Arc cloning overhead
2. **Rollback > 1s**: Check lock contention
3. **Get > 10μs**: Check bloom filter effectiveness
4. **Compaction slow**: Check merge algorithm
5. **Merkle > 100μs**: Check hash algorithm or tree updates

## Running Benchmarks in CI

```bash
# In Nix environment
nix develop

# Run benchmarks and save baseline
cargo bench -- --save-baseline ci-baseline

# Later, compare
cargo bench -- --baseline ci-baseline

# Check for regressions
```

## Profiling

### CPU Profiling

```bash
# Install flamegraph
cargo install flamegraph

# Profile a benchmark
cargo flamegraph --bench lsm_benchmarks -- --bench insert

# Open flamegraph.svg
```

### Memory Profiling

```bash
# Use heaptrack (Linux)
heaptrack cargo bench

# Or valgrind
valgrind --tool=massif cargo bench
```

## Benchmark-Driven Optimization

### Example Workflow

1. **Baseline**
   ```bash
   cargo bench -- --save-baseline before-opt
   ```

2. **Optimize**
   ```rust
   // Make changes to improve performance
   ```

3. **Compare**
   ```bash
   cargo bench -- --baseline before-opt
   ```

4. **Verify**
   - Check improvement percentage
   - Ensure no regressions in other benchmarks
   - Run tests to ensure correctness

## Realistic Workloads

### Genesis Sync Simulation

```bash
# Benchmark: Insert 1M entries (simulating genesis sync)
cargo bench write_throughput

# Target: Complete in < 20 seconds
# 1M entries / 20s = 50K entries/sec
```

### Live Sync Simulation

```bash
# Benchmark: Process block (100 txs)
cargo bench process_block

# Target: < 50ms per block
# Cardano: ~20 sec blocks, so plenty of time
```

### Wallet Balance Query

```bash
# Benchmark: Aggregate all addresses
cargo bench monoidal_prefix_fold

# Target: < 100ms for 10K addresses
```

### Governance Verification

```bash
# Benchmark: Verify Merkle proof
cargo bench merkle_verify

# Target: < 1ms
# Can verify 1000 actions/sec
```

## Expected Results

Based on the implementation:

### Fast Operations (μs range)
- ✅ Insert (memtable): ~1-10μs
- ✅ Get with bloom: ~5-15μs
- ✅ Delete: ~1-10μs
- ✅ Snapshot: ~2-8ms (Arc clones)

### Medium Operations (ms range)
- ✅ Range scan (100): ~0.5-2ms
- ✅ Rollback: ~10-100ms

### Slow Operations (100ms+ range)
- ✅ Compaction: 100ms-1s (acceptable, background)
- ✅ Large range scans: 10-100ms
- ✅ Block processing: 20-50ms

## Cardano-Specific Metrics

### UTXO Indexing
- Lookup: < 10μs ✅
- Insert: < 10μs ✅
- Scan address: < 10ms for 1000 UTXOs ✅

### Chain Reorg
- Snapshot per block: < 10ms ✅
- Rollback 10 blocks: < 1s ✅
- Critical for consensus! ⛓️

## Optimization Ideas

If benchmarks show issues:

### For Snapshots
- Check Arc<RwLock> overhead
- Consider finer-grained locking
- Profile clone operations

### For Compaction
- Implement background threads
- Parallel SSTable reads
- Optimize merge algorithm

### For Bloom Filters
- Tune bits per key
- Check hash function performance
- Measure false positive rate

### For Range Scans
- Add block cache
- Optimize SSTable reading
- Consider mmap for large files

## Success Criteria

Core LSM operations meet target performance requirements:
- Insert/delete: < 10μs
- Get operations: < 10μs (hit), < 1μs (miss with bloom filter)
- Range scans: < 1ms for 100 entries
- Snapshots: < 10ms
- Rollback: < 1s
- Block processing: < 50ms

Run benchmarks with `cargo bench` to verify performance on your hardware.
