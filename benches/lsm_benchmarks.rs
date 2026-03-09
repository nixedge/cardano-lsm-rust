// Benchmarks for Cardano LSM Tree
// Measures performance of core operations and Cardano-specific patterns

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use cardano_lsm::{LsmTree, LsmConfig, Key, Value, IncrementalMerkleTree, MonoidalLsmTree};
use tempfile::TempDir;

fn create_tree() -> (LsmTree, TempDir) {
    let temp = TempDir::new().unwrap();
    let tree = LsmTree::open(temp.path(), LsmConfig::default()).unwrap();
    (tree, temp)
}

#[allow(dead_code)]
fn create_tree_with_config(config: LsmConfig) -> (LsmTree, TempDir) {
    let temp = TempDir::new().unwrap();
    let tree = LsmTree::open(temp.path(), config).unwrap();
    (tree, temp)
}

// ===== Core Operations =====

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert");
    
    for size in [100, 1000, 10_000] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &n| {
            b.iter_batched(
                create_tree,
                |(mut tree, _temp)| {
                    for i in 0..n {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        let value = Value::from(b"value_data");
                        tree.insert(&key, &value).unwrap();
                    }
                },
                criterion::BatchSize::SmallInput
            );
        });
    }
    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("get");
    
    for size in [100, 1000, 10_000] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &n| {
            b.iter_batched(
                || {
                    let (mut tree, temp) = create_tree();
                    for i in 0..n {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        tree.insert(&key, &Value::from(b"value")).unwrap();
                    }
                    (tree, temp)
                },
                |(tree, _temp)| {
                    for i in 0..n {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        tree.get(&key).unwrap();
                    }
                },
                criterion::BatchSize::SmallInput
            );
        });
    }
    group.finish();
}

fn bench_range(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_scan");
    
    for range_size in [10, 100, 1000] {
        group.throughput(Throughput::Elements(range_size));
        group.bench_with_input(BenchmarkId::from_parameter(range_size), &range_size, |b, &n| {
            b.iter_batched(
                || {
                    let (mut tree, temp) = create_tree();
                    for i in 0..10_000 {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        tree.insert(&key, &Value::from(b"value")).unwrap();
                    }
                    (tree, temp)
                },
                |(tree, _temp)| {
                    let from = Key::from(b"key_00000000");
                    let to = Key::from(format!("key_{:08}", n).as_bytes());
                    let count = tree.range(&from, &to).count();
                    black_box(count);
                },
                criterion::BatchSize::SmallInput
            );
        });
    }
    group.finish();
}

// ===== Snapshot Performance =====

fn bench_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot");
    
    for size in [1_000, 10_000, 100_000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &n| {
            b.iter_batched(
                || {
                    let (mut tree, temp) = create_tree();
                    for i in 0..n {
                        let key = Key::from(format!("key_{:08}", i).as_bytes());
                        tree.insert(&key, &Value::from(b"value")).unwrap();
                    }
                    (tree, temp)
                },
                |(tree, _temp)| {
                    let snapshot = tree.snapshot();
                    black_box(snapshot);
                },
                criterion::BatchSize::SmallInput
            );
        });
    }
    group.finish();
}

fn bench_rollback(c: &mut Criterion) {
    let mut group = c.benchmark_group("rollback");
    
    for size in [1_000, 10_000, 100_000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &n| {
            b.iter_batched(
                || {
                    let (mut tree, temp) = create_tree();
                    for i in 0..n {
                        tree.insert(&Key::from(format!("k{}", i).as_bytes()), &Value::from(b"v")).unwrap();
                    }
                    let snap = tree.snapshot();
                    for i in 0..100 {
                        tree.delete(&Key::from(format!("k{}", i).as_bytes())).unwrap();
                    }
                    (tree, snap, temp)
                },
                |(mut tree, snap, _temp)| {
                    tree.rollback(snap).unwrap();
                },
                criterion::BatchSize::SmallInput
            );
        });
    }
    group.finish();
}

// ===== Cardano Workloads =====

fn bench_cardano_utxo(c: &mut Criterion) {
    let mut group = c.benchmark_group("cardano_utxo");
    
    group.bench_function("address_balance_query", |b| {
        b.iter_batched(
            || {
                let (mut tree, temp) = create_tree();
                // Simulate 1000 addresses with 10 UTxOs each
                for addr in 0..1000 {
                    for utxo in 0..10 {
                        let key = Key::from(format!("addr_{:04}#utxo_{}", addr, utxo).as_bytes());
                        tree.insert(&key, &Value::from(b"100000000")).unwrap(); // 100 ADA
                    }
                }
                (tree, temp)
            },
            |(tree, _temp)| {
                // Query all UTxOs for one address
                let count = tree.scan_prefix(b"addr_0500#").count();
                black_box(count);
            },
            criterion::BatchSize::SmallInput
        );
    });
    
    group.finish();
}

fn bench_cardano_governance(c: &mut Criterion) {
    let mut group = c.benchmark_group("cardano_governance");
    
    group.bench_function("merkle_insert_1000_actions", |b| {
        b.iter(|| {
            let mut merkle = IncrementalMerkleTree::new(16);
            for i in 0..1000 {
                let action_id = format!("action_{}", i);
                merkle.insert(action_id.as_bytes(), b"proposal_data");
            }
            black_box(merkle);
        });
    });
    
    group.finish();
}

fn bench_monoidal_balance(c: &mut Criterion) {
    let mut group = c.benchmark_group("monoidal_balance");
    
    group.bench_function("aggregate_1000_balances", |b| {
        b.iter_batched(
            || {
                let temp = TempDir::new().unwrap();
                let mut tree = MonoidalLsmTree::<u64>::open(temp.path(), LsmConfig::default()).unwrap();
                for i in 0..1000 {
                    tree.insert(&Key::from(format!("bal{}", i).as_bytes()), &(i as u64 * 1_000_000)).unwrap();
                }
                (tree, temp)
            },
            |(tree, _temp)| {
                let total = tree.prefix_fold(b"bal");
                black_box(total);
            },
            criterion::BatchSize::SmallInput
        );
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_insert,
    bench_get,
    bench_range,
    bench_snapshot,
    bench_rollback,
    bench_cardano_utxo,
    bench_cardano_governance,
    bench_monoidal_balance,
);

criterion_main!(benches);
