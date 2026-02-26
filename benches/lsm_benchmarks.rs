// LSM Tree Benchmarks
// Comprehensive performance testing for all operations

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use cardano_lsm::{LsmTree, LsmConfig, Key, Value, IncrementalMerkleTree, MonoidalLsmTree};
use tempfile::TempDir;
use std::collections::HashMap;

// Helper to create test tree
fn create_bench_tree() -> (LsmTree, TempDir) {
    let temp = TempDir::new().unwrap();
    let config = LsmConfig::default();
    let tree = LsmTree::open(temp.path(), config).unwrap();
    (tree, temp)
}

fn create_bench_tree_with_config(config: LsmConfig) -> (LsmTree, TempDir) {
    let temp = TempDir::new().unwrap();
    let tree = LsmTree::open(temp.path(), config).unwrap();
    (tree, temp)
}

// ============================================================================
// Basic Operation Benchmarks
// ============================================================================

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert");
    
    for size in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_batched(
                || create_bench_tree(),
                |(mut tree, _temp)| {
                    for i in 0..size {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        let value = Value::from(format!("value_{}", i).as_bytes());
                        tree.insert(&key, &value).unwrap();
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("get");
    
    // Pre-populate tree
    let (mut tree, _temp) = create_bench_tree();
    for i in 0..10000 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    group.bench_function("get_existing", |b| {
        b.iter(|| {
            let key = Key::from(format!("key_{:08}", black_box(5000)).as_bytes());
            tree.get(&key).unwrap()
        });
    });
    
    group.bench_function("get_nonexistent", |b| {
        b.iter(|| {
            let key = Key::from(format!("key_{:08}", black_box(99999)).as_bytes());
            tree.get(&key).unwrap()
        });
    });
    
    group.finish();
}

fn bench_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("delete");
    
    group.bench_function("delete", |b| {
        b.iter_batched(
            || {
                let (mut tree, temp) = create_bench_tree();
                // Pre-populate
                for i in 0..1000 {
                    let key = Key::from(format!("key_{:08}", i).as_bytes());
                    let value = Value::from(format!("value_{}", i).as_bytes());
                    tree.insert(&key, &value).unwrap();
                }
                (tree, temp)
            },
            |(mut tree, _temp)| {
                for i in 0..1000 {
                    let key = Key::from(format!("key_{:08}", black_box(i)).as_bytes());
                    tree.delete(&key).unwrap();
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    
    group.finish();
}

// ============================================================================
// Range Query Benchmarks
// ============================================================================

fn bench_range_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_scan");
    
    // Pre-populate tree
    let (mut tree, _temp) = create_bench_tree();
    for i in 0..10000 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    for range_size in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*range_size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(range_size), range_size, |b, &size| {
            b.iter(|| {
                let from = Key::from(format!("key_{:08}", black_box(1000)).as_bytes());
                let to = Key::from(format!("key_{:08}", black_box(1000 + size)).as_bytes());
                tree.range(&from, &to).count()
            });
        });
    }
    
    group.finish();
}

fn bench_prefix_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("prefix_scan");
    
    // Pre-populate with address-like keys
    let (mut tree, _temp) = create_bench_tree();
    for wallet in 0..10 {
        for addr in 0..1000 {
            let key = Key::from(format!("wallet_{}_addr_{:06}", wallet, addr).as_bytes());
            let value = Value::from(format!("utxo_data_{}", addr).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
    }
    
    group.bench_function("prefix_scan_1000_entries", |b| {
        b.iter(|| {
            tree.scan_prefix(black_box(b"wallet_5_")).count()
        });
    });
    
    group.finish();
}

// ============================================================================
// Compaction Benchmarks
// ============================================================================

fn bench_compaction(c: &mut Criterion) {
    let mut group = c.benchmark_group("compaction");
    group.sample_size(10); // Compaction is slow, fewer samples
    
    for num_entries in [1000, 5000, 10000].iter() {
        group.throughput(Throughput::Elements(*num_entries as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_entries), num_entries, |b, &size| {
            b.iter_batched(
                || {
                    let mut config = LsmConfig::default();
                    config.memtable_size = 1024; // Small to force many SSTables
                    config.level0_compaction_trigger = 100; // Don't auto-compact
                    let (mut tree, temp) = create_bench_tree_with_config(config);
                    
                    // Insert data to create multiple SSTables
                    for i in 0..size {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        let value = Value::from(format!("value_{}", i).as_bytes());
                        tree.insert(&key, &value).unwrap();
                    }
                    
                    (tree, temp)
                },
                |(mut tree, _temp)| {
                    tree.compact().unwrap();
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    
    group.finish();
}

// ============================================================================
// Snapshot and Rollback Benchmarks
// ============================================================================

fn bench_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot");
    
    // Pre-populate tree
    let (mut tree, _temp) = create_bench_tree();
    for i in 0..10000 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    group.bench_function("snapshot_10k_entries", |b| {
        b.iter(|| {
            black_box(tree.snapshot())
        });
    });
    
    group.finish();
}

fn bench_rollback(c: &mut Criterion) {
    let mut group = c.benchmark_group("rollback");
    
    for num_new_entries in [100, 1000, 5000].iter() {
        group.throughput(Throughput::Elements(*num_new_entries as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_new_entries), num_new_entries, |b, &new_entries| {
            b.iter_batched(
                || {
                    let (mut tree, temp) = create_bench_tree();
                    
                    // Insert initial data
                    for i in 0..10000 {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        let value = Value::from(format!("value_{}", i).as_bytes());
                        tree.insert(&key, &value).unwrap();
                    }
                    
                    // Take snapshot
                    let snapshot = tree.snapshot();
                    
                    // Insert more data
                    for i in 10000..(10000 + new_entries) {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        let value = Value::from(format!("value_{}", i).as_bytes());
                        tree.insert(&key, &value).unwrap();
                    }
                    
                    (tree, snapshot, temp)
                },
                |(mut tree, snapshot, _temp)| {
                    tree.rollback(snapshot).unwrap();
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    
    group.finish();
}

// ============================================================================
// Merkle Tree Benchmarks
// ============================================================================

fn bench_merkle_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_insert");
    
    for height in [8, 16, 20].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(height), height, |b, &h| {
            b.iter_batched(
                || IncrementalMerkleTree::new(h),
                |mut tree| {
                    for i in 0..1000 {
                        let key = format!("action_{}", i);
                        let value = format!("data_{}", i);
                        tree.insert(black_box(key.as_bytes()), black_box(value.as_bytes()));
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    
    group.finish();
}

fn bench_merkle_prove(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_prove");
    
    // Pre-populate tree
    let mut tree = IncrementalMerkleTree::new(16);
    for i in 0..10000 {
        tree.insert(format!("action_{}", i).as_bytes(), b"data");
    }
    
    group.bench_function("prove", |b| {
        b.iter(|| {
            tree.prove(black_box(b"action_5000")).unwrap()
        });
    });
    
    group.finish();
}

fn bench_merkle_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_verify");
    
    let mut tree = IncrementalMerkleTree::new(16);
    let proof = tree.insert(b"test_key", b"test_value");
    let root = tree.root().clone();
    
    group.bench_function("verify_proof", |b| {
        b.iter(|| {
            IncrementalMerkleTree::verify_proof(
                black_box(&root),
                black_box(b"test_key"),
                black_box(b"test_value"),
                black_box(&proof)
            )
        });
    });
    
    group.finish();
}

// ============================================================================
// Monoidal Operation Benchmarks
// ============================================================================

fn bench_monoidal_fold(c: &mut Criterion) {
    let mut group = c.benchmark_group("monoidal_fold");
    
    for num_entries in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*num_entries as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_entries), num_entries, |b, &size| {
            let temp = TempDir::new().unwrap();
            let mut tree = MonoidalLsmTree::<u64>::open(temp.path(), LsmConfig::default()).unwrap();
            
            // Pre-populate
            for i in 0..size {
                let key = Key::from(format!("addr_{:08}", i).as_bytes());
                tree.insert(&key, &(i as u64 * 1000)).unwrap();
            }
            
            b.iter(|| {
                let from = Key::from(b"");
                let to = Key::from(&[0xFF; 20]);
                black_box(tree.range_fold(&from, &to))
            });
        });
    }
    
    group.finish();
}

fn bench_monoidal_prefix_fold(c: &mut Criterion) {
    let mut group = c.benchmark_group("monoidal_prefix_fold");
    
    let temp = TempDir::new().unwrap();
    let mut tree = MonoidalLsmTree::<u64>::open(temp.path(), LsmConfig::default()).unwrap();
    
    // Insert with common prefixes (wallet addresses)
    for wallet in 0..10 {
        for addr in 0..1000 {
            let key = Key::from(format!("wallet_{}_addr_{:06}", wallet, addr).as_bytes());
            tree.insert(&key, &1_000_000).unwrap();
        }
    }
    
    group.bench_function("prefix_fold_1000_entries", |b| {
        b.iter(|| {
            black_box(tree.prefix_fold(b"wallet_5_"))
        });
    });
    
    group.finish();
}

fn bench_monoidal_asset_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("monoidal_asset_aggregation");
    
    let temp = TempDir::new().unwrap();
    let mut tree = MonoidalLsmTree::<HashMap<String, u64>>::open(
        temp.path(), 
        LsmConfig::default()
    ).unwrap();
    
    // Insert multi-asset UTXOs
    for i in 0..1000 {
        let mut assets = HashMap::new();
        assets.insert("ADA".to_string(), 2_000_000);
        assets.insert("TOKEN_A".to_string(), 100);
        if i % 2 == 0 {
            assets.insert("TOKEN_B".to_string(), 50);
        }
        
        let key = Key::from(format!("utxo_{:08}", i).as_bytes());
        tree.insert(&key, &assets).unwrap();
    }
    
    group.bench_function("aggregate_1000_utxos", |b| {
        b.iter(|| {
            let from = Key::from(b"");
            let to = Key::from(&[0xFF; 20]);
            black_box(tree.range_fold(&from, &to))
        });
    });
    
    group.finish();
}

// ============================================================================
// Persistence Benchmarks
// ============================================================================

fn bench_persistence(c: &mut Criterion) {
    let mut group = c.benchmark_group("persistence");
    
    group.bench_function("close_and_reopen_10k_entries", |b| {
        b.iter_batched(
            || {
                let temp = TempDir::new().unwrap();
                let mut tree = LsmTree::open(temp.path(), LsmConfig::default()).unwrap();
                
                // Insert data
                for i in 0..10000 {
                    let key = Key::from(format!("key_{:08}", i).as_bytes());
                    let value = Value::from(format!("value_{}", i).as_bytes());
                    tree.insert(&key, &value).unwrap();
                }
                
                drop(tree);
                temp
            },
            |temp| {
                // Reopen and verify
                let tree = LsmTree::open(temp.path(), LsmConfig::default()).unwrap();
                let key = Key::from(b"key_00005000");
                black_box(tree.get(&key).unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });
    
    group.finish();
}

// ============================================================================
// Blockchain-Specific Benchmarks
// ============================================================================

fn bench_utxo_lookup_pattern(c: &mut Criterion) {
    let mut group = c.benchmark_group("blockchain_utxo");
    
    // Simulate UTXO set
    let (mut tree, _temp) = create_bench_tree();
    for tx in 0..1000 {
        for output in 0..5 {
            let key = Key::from(format!("tx_{:08}#{}", tx, output).as_bytes());
            let value = Value::from(format!("{{\"amount\": 1000000, \"address\": \"addr{}\"}}", output).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
    }
    
    group.bench_function("utxo_lookup", |b| {
        b.iter(|| {
            let key = Key::from(format!("tx_{:08}#{}", black_box(500), black_box(2)).as_bytes());
            tree.get(&key).unwrap()
        });
    });
    
    group.finish();
}

fn bench_block_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("blockchain_block");
    group.sample_size(10);
    
    // Simulate processing a block with 100 transactions
    group.bench_function("process_block_100_txs", |b| {
        b.iter_batched(
            || create_bench_tree(),
            |(mut tree, _temp)| {
                // Simulate block processing
                let snapshot = tree.snapshot();
                
                // Process 100 transactions
                for tx in 0..100 {
                    // Spend some UTXOs
                    for input in 0..2 {
                        let key = Key::from(format!("utxo_{}#{}", black_box(tx), input).as_bytes());
                        tree.delete(&key).unwrap();
                    }
                    
                    // Create new UTXOs
                    for output in 0..3 {
                        let key = Key::from(format!("utxo_{}#{}", black_box(tx + 1000), output).as_bytes());
                        let value = Value::from(b"utxo_data");
                        tree.insert(&key, &value).unwrap();
                    }
                }
                
                snapshot
            },
            criterion::BatchSize::SmallInput,
        );
    });
    
    group.finish();
}

fn bench_chain_reorg(c: &mut Criterion) {
    let mut group = c.benchmark_group("blockchain_reorg");
    
    for reorg_depth in [1, 10, 100].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(reorg_depth), reorg_depth, |b, &depth| {
            b.iter_batched(
                || {
                    let (mut tree, temp) = create_bench_tree();
                    
                    // Process blocks
                    let mut snapshots = Vec::new();
                    for block in 0..200 {
                        snapshots.push(tree.snapshot());
                        
                        // Each block has 50 transactions
                        for tx in 0..50 {
                            let key = Key::from(format!("block_{}_tx_{}", block, tx).as_bytes());
                            let value = Value::from(b"tx_data");
                            tree.insert(&key, &value).unwrap();
                        }
                    }
                    
                    // Get snapshot from `depth` blocks ago
                    let rollback_point = snapshots[200 - depth].clone();
                    (tree, rollback_point, temp)
                },
                |(mut tree, snapshot, _temp)| {
                    tree.rollback(snapshot).unwrap();
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    
    group.finish();
}

// ============================================================================
// Throughput Benchmarks
// ============================================================================

fn bench_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_write");
    group.throughput(Throughput::Elements(100000));
    
    group.bench_function("sequential_writes_100k", |b| {
        b.iter_batched(
            || create_bench_tree(),
            |(mut tree, _temp)| {
                for i in 0..100000 {
                    let key = Key::from(format!("key_{:08}", i).as_bytes());
                    let value = Value::from(b"value");
                    tree.insert(&key, &value).unwrap();
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    
    group.finish();
}

fn bench_read_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_read");
    
    // Pre-populate
    let (mut tree, _temp) = create_bench_tree();
    for i in 0..100000 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(b"value");
        tree.insert(&key, &value).unwrap();
    }
    
    group.throughput(Throughput::Elements(10000));
    group.bench_function("random_reads_10k", |b| {
        b.iter(|| {
            for i in (0..100000).step_by(10) {
                let key = Key::from(format!("key_{:08}", black_box(i)).as_bytes());
                tree.get(&key).unwrap();
            }
        });
    });
    
    group.finish();
}

// ============================================================================
// Benchmark Groups
// ============================================================================

criterion_group!(
    basic_ops,
    bench_insert,
    bench_get,
    bench_delete
);

criterion_group!(
    range_ops,
    bench_range_scan,
    bench_prefix_scan
);

criterion_group!(
    compaction_ops,
    bench_compaction
);

criterion_group!(
    snapshot_ops,
    bench_snapshot,
    bench_rollback
);

criterion_group!(
    merkle_ops,
    bench_merkle_insert,
    bench_merkle_prove,
    bench_merkle_verify
);

criterion_group!(
    monoidal_ops,
    bench_monoidal_fold,
    bench_monoidal_prefix_fold,
    bench_monoidal_asset_aggregation
);

criterion_group!(
    persistence_ops,
    bench_persistence
);

criterion_group!(
    blockchain_ops,
    bench_utxo_lookup_pattern,
    bench_block_processing,
    bench_chain_reorg
);

criterion_group!(
    throughput_ops,
    bench_write_throughput,
    bench_read_throughput
);

criterion_main!(
    basic_ops,
    range_ops,
    compaction_ops,
    snapshot_ops,
    merkle_ops,
    monoidal_ops,
    persistence_ops,
    blockchain_ops,
    throughput_ops
);
