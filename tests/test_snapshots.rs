/// Tests for snapshots and rollback
/// Critical feature for blockchain rollback handling
use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
use tempfile::TempDir;

fn create_test_tree() -> (LsmTree, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = LsmConfig::default();
    let tree = LsmTree::open(temp_dir.path(), config).unwrap();
    (tree, temp_dir)
}

#[test]
fn test_snapshot_captures_current_state() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert initial data
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Create snapshot
    let snapshot = tree.snapshot();
    
    // Insert more data
    for i in 50..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Current tree should have 100 keys
    let count = tree.iter().count();
    assert_eq!(count, 100);
    
    // Snapshot should only have 50 keys
    let snapshot_count = snapshot.iter().count();
    assert_eq!(snapshot_count, 50);
}

#[test]
fn test_rollback_to_snapshot() {
    let (mut tree, _temp) = create_test_tree();
    
    // State 1: Insert 0-49
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let snapshot1 = tree.snapshot();
    
    // State 2: Insert 50-99
    for i in 50..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    assert_eq!(tree.iter().count(), 100);
    
    // Rollback to snapshot1
    tree.rollback(snapshot1).unwrap();
    
    // Should only have keys 0-49
    assert_eq!(tree.iter().count(), 50);
    
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        assert!(tree.get(&key).unwrap().is_some());
    }
    
    for i in 50..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        assert!(tree.get(&key).unwrap().is_none());
    }
}

#[test]
fn test_rollback_with_deletes() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert keys
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let snapshot = tree.snapshot();
    
    // Delete some keys
    for i in (0..50).step_by(2) {
        let key = Key::from(format!("key_{}", i).as_bytes());
        tree.delete(&key).unwrap();
    }
    
    // Verify deletes worked
    for i in (0..50).step_by(2) {
        let key = Key::from(format!("key_{}", i).as_bytes());
        assert!(tree.get(&key).unwrap().is_none());
    }
    
    // Rollback - deleted keys should reappear
    tree.rollback(snapshot).unwrap();
    
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_rollback_with_overwrites() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert initial values
    for i in 0..30 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}_v1", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let snapshot = tree.snapshot();
    
    // Overwrite values
    for i in 0..30 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}_v2", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Verify overwrites
    for i in 0..30 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let expected = Value::from(format!("value_{}_v2", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
    
    // Rollback - should see v1 values
    tree.rollback(snapshot).unwrap();
    
    for i in 0..30 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let expected = Value::from(format!("value_{}_v1", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_multiple_snapshots() {
    let (mut tree, _temp) = create_test_tree();
    
    // Snapshot 1: Empty
    let snap1 = tree.snapshot();
    
    // Insert 0-19
    for i in 0..20 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Snapshot 2: 20 keys
    let snap2 = tree.snapshot();
    
    // Insert 20-39
    for i in 20..40 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Snapshot 3: 40 keys
    let snap3 = tree.snapshot();
    
    // Insert 40-59
    for i in 40..60 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Current: 60 keys
    assert_eq!(tree.iter().count(), 60);
    
    // Rollback to snap3: should have 40 keys
    tree.rollback(snap3).unwrap();
    assert_eq!(tree.iter().count(), 40);
    
    // Rollback to snap2: should have 20 keys
    tree.rollback(snap2).unwrap();
    assert_eq!(tree.iter().count(), 20);
    
    // Rollback to snap1: should have 0 keys
    tree.rollback(snap1).unwrap();
    assert_eq!(tree.iter().count(), 0);
}

#[test]
fn test_snapshot_is_cheap() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert large dataset
    for i in 0..10000 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    use std::time::Instant;
    
    // Taking snapshot should be very fast (< 10ms)
    let start = Instant::now();
    let _snapshot = tree.snapshot();
    let duration = start.elapsed();
    
    assert!(duration.as_millis() < 10, "Snapshot should be cheap: {:?}", duration);
}

#[test]
fn test_rollback_is_fast() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert dataset
    for i in 0..10000 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let snapshot = tree.snapshot();
    
    // Insert more data
    for i in 10000..20000 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    use std::time::Instant;
    
    // Rollback should be very fast (< 100ms)
    let start = Instant::now();
    tree.rollback(snapshot).unwrap();
    let duration = start.elapsed();
    
    assert!(duration.as_millis() < 100, "Rollback should be fast: {:?}", duration);
}

#[test]
fn test_snapshot_isolation() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert initial data
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let snapshot = tree.snapshot();
    
    // Modify tree
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}_modified", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Snapshot should still see original values
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        let result = snapshot.get(&key).unwrap();
        assert_eq!(result.unwrap(), expected, "Snapshot should be isolated from modifications");
    }
    
    // Current tree should see modified values
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let expected = Value::from(format!("value_{}_modified", i).as_bytes());
        let result = tree.get(&key).unwrap();
        assert_eq!(result.unwrap(), expected);
    }
}

#[test]
fn test_snapshot_after_compaction() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert data
    for i in 0..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let snapshot_before = tree.snapshot();
    
    // Force compaction
    tree.compact().unwrap();
    
    let snapshot_after = tree.snapshot();
    
    // Both snapshots should have same data
    for i in 0..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        
        assert_eq!(snapshot_before.get(&key).unwrap().unwrap(), expected);
        assert_eq!(snapshot_after.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_rollback_after_compaction() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert data and take snapshot
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let snapshot = tree.snapshot();
    
    // Insert more data
    for i in 50..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Compact
    tree.compact().unwrap();
    
    // Rollback should still work after compaction
    tree.rollback(snapshot).unwrap();
    
    assert_eq!(tree.iter().count(), 50);
    
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        assert!(tree.get(&key).unwrap().is_some());
    }
    
    for i in 50..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        assert!(tree.get(&key).unwrap().is_none());
    }
}

#[test]
fn test_snapshot_sequence_numbers() {
    let (mut tree, _temp) = create_test_tree();
    
    let snap1 = tree.snapshot();
    
    tree.insert(&Key::from(b"key1"), &Value::from(b"value1")).unwrap();
    let snap2 = tree.snapshot();
    
    tree.insert(&Key::from(b"key2"), &Value::from(b"value2")).unwrap();
    let snap3 = tree.snapshot();
    
    // Snapshot sequence numbers should be increasing
    assert!(snap1.sequence_number() < snap2.sequence_number());
    assert!(snap2.sequence_number() < snap3.sequence_number());
}

#[test]
fn test_cannot_rollback_to_future_snapshot() {
    let (mut tree, _temp) = create_test_tree();
    
    let snap1 = tree.snapshot();
    
    tree.insert(&Key::from(b"key1"), &Value::from(b"value1")).unwrap();
    
    let snap2 = tree.snapshot();
    
    // Rollback to snap1
    tree.rollback(snap1.clone()).unwrap();
    
    // Should not be able to rollback to snap2 (from the "future")
    let result = tree.rollback(snap2);
    assert!(result.is_err(), "Should not rollback to future snapshot");
}

#[test]
fn test_blockchain_style_rollback() {
    let (mut tree, _temp) = create_test_tree();
    
    // Simulate blockchain blocks
    let mut snapshots = Vec::new();
    
    // "Block" 0
    snapshots.push(tree.snapshot());
    
    // Blocks 1-10
    for block in 1..=10 {
        // Each block adds some transactions
        for tx in 0..10 {
            let key = Key::from(format!("block_{}_tx_{}", block, tx).as_bytes());
            let value = Value::from(format!("data", ).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
        
        snapshots.push(tree.snapshot());
    }
    
    // Total: 100 keys
    assert_eq!(tree.iter().count(), 100);
    
    // Rollback to block 7 (common chain reorganization scenario)
    tree.rollback(snapshots[7].clone()).unwrap();
    
    // Should have 70 keys
    assert_eq!(tree.iter().count(), 70);
    
    // Verify we can still access blocks 1-7
    for block in 1..=7 {
        for tx in 0..10 {
            let key = Key::from(format!("block_{}_tx_{}", block, tx).as_bytes());
            assert!(tree.get(&key).unwrap().is_some());
        }
    }
    
    // Verify blocks 8-10 are gone
    for block in 8..=10 {
        for tx in 0..10 {
            let key = Key::from(format!("block_{}_tx_{}", block, tx).as_bytes());
            assert!(tree.get(&key).unwrap().is_none());
        }
    }
}

#[test]
fn test_snapshot_memory_overhead() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert data
    for i in 0..1000 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Create many snapshots
    let mut snapshots = Vec::new();
    for _ in 0..100 {
        snapshots.push(tree.snapshot());
    }
    
    // Memory overhead should be minimal (snapshots are just references)
    // This test passes if it doesn't OOM
    assert_eq!(snapshots.len(), 100);
}

#[test]
fn test_rollback_preserves_range_query_order() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert in random order
    let mut keys: Vec<_> = (0..100).collect();
    
    // Simple shuffle without rand
    keys.reverse();
    
    for i in keys {
        let key = Key::from(format!("key_{:04}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let snapshot = tree.snapshot();
    
    // Add more keys
    for i in 100..200 {
        let key = Key::from(format!("key_{:04}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Rollback
    tree.rollback(snapshot).unwrap();
    
    // Verify sorted order is maintained
    let mut prev: Option<Key> = None;
    for (key, _) in tree.iter() {
        if let Some(p) = prev {
            assert!(p < key, "Keys should remain sorted after rollback");
        }
        prev = Some(key);
    }
}
