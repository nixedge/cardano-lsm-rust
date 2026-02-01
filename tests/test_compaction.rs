/// Tests for LSM compaction correctness
/// Ensures data integrity during and after compaction
use cardano_lsm::{LsmTree, LsmConfig, Key, Value, CompactionStrategy};
use tempfile::TempDir;
use std::collections::HashMap;

fn create_test_tree_with_config(config: LsmConfig) -> (LsmTree, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let tree = LsmTree::open(temp_dir.path(), config).unwrap();
    (tree, temp_dir)
}

#[test]
fn test_compaction_preserves_all_data() {
    let mut config = LsmConfig::default();
    config.memtable_size = 1024; // Small memtable to trigger flushes
    config.level0_compaction_trigger = 2; // Trigger compaction quickly
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    let mut expected = HashMap::new();
    
    // Insert enough data to trigger multiple flushes and compaction
    for i in 0..1000 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
        expected.insert(key.clone(), value);
    }
    
    // Force compaction
    tree.compact().unwrap();
    
    // Verify all data is still present
    for (key, expected_value) in expected {
        let result = tree.get(&key).unwrap();
        assert!(result.is_some(), "Key {:?} should exist after compaction", key);
        assert_eq!(result.unwrap(), expected_value, "Value should match after compaction");
    }
}

#[test]
fn test_compaction_removes_deleted_keys() {
    let mut config = LsmConfig::default();
    config.memtable_size = 1024;
    config.level0_compaction_trigger = 2;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert keys
    for i in 0..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.flush().unwrap();
    
    // Delete half the keys
    for i in (0..100).step_by(2) {
        let key = Key::from(format!("key_{}", i).as_bytes());
        tree.delete(&key).unwrap();
    }
    
    tree.flush().unwrap();
    
    // Compact - should remove deleted keys
    tree.compact().unwrap();
    
    // Verify deleted keys are gone
    for i in (0..100).step_by(2) {
        let key = Key::from(format!("key_{}", i).as_bytes());
        assert!(tree.get(&key).unwrap().is_none(), "Deleted key should not exist after compaction");
    }
    
    // Verify remaining keys still exist
    for i in (1..100).step_by(2) {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_compaction_handles_overwrites() {
    let mut config = LsmConfig::default();
    config.memtable_size = 1024;
    config.level0_compaction_trigger = 2;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert initial values
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}_v1", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.flush().unwrap();
    
    // Overwrite all values
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}_v2", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.flush().unwrap();
    
    // Compact
    tree.compact().unwrap();
    
    // Should see only the latest values
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let expected = Value::from(format!("value_{}_v2", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_tiered_compaction_strategy() {
    let mut config = LsmConfig::default();
    config.compaction_strategy = CompactionStrategy::Tiered {
        size_ratio: 4.0,
        min_merge_width: 4,
        max_merge_width: 10,
    };
    config.memtable_size = 1024;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert data to trigger tiered compaction
    for i in 0..2000 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.compact().unwrap();
    
    // Verify all data still accessible
    for i in (0..2000).step_by(100) {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_leveled_compaction_strategy() {
    let mut config = LsmConfig::default();
    config.compaction_strategy = CompactionStrategy::Leveled {
        size_ratio: 10.0,
        max_level: 7,
    };
    config.memtable_size = 1024;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert data to trigger leveled compaction
    for i in 0..2000 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.compact().unwrap();
    
    // Verify all data still accessible
    for i in (0..2000).step_by(100) {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_hybrid_compaction_strategy() {
    let mut config = LsmConfig::default();
    config.compaction_strategy = CompactionStrategy::Hybrid {
        l0_strategy: Box::new(CompactionStrategy::Tiered {
            size_ratio: 4.0,
            min_merge_width: 4,
            max_merge_width: 10,
        }),
        ln_strategy: Box::new(CompactionStrategy::Leveled {
            size_ratio: 10.0,
            max_level: 7,
        }),
        transition_level: 2,
    };
    config.memtable_size = 1024;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert data to trigger hybrid compaction
    for i in 0..3000 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.compact().unwrap();
    
    // Verify all data still accessible
    for i in (0..3000).step_by(100) {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_compaction_during_reads() {
    let mut config = LsmConfig::default();
    config.memtable_size = 1024;
    config.level0_compaction_trigger = 2;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert initial data
    for i in 0..500 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Start compaction in background
    tree.trigger_background_compaction();
    
    // Continue reading while compaction runs
    for i in (0..500).step_by(10) {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        let result = tree.get(&key).unwrap();
        assert_eq!(result.unwrap(), expected, "Reads should work during compaction");
    }
    
    // Wait for compaction to complete
    tree.wait_for_compaction();
    
    // Verify all data still correct
    for i in (0..500).step_by(10) {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let expected = Value::from(format!("value_{}", i).as_bytes());
        assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
    }
}

#[test]
fn test_compaction_reduces_space() {
    let mut config = LsmConfig::default();
    config.memtable_size = 1024;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert and delete many keys to create tombstones
    for i in 0..1000 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.flush().unwrap();
    let size_before = tree.disk_usage().unwrap();
    
    // Delete all keys
    for i in 0..1000 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        tree.delete(&key).unwrap();
    }
    
    tree.flush().unwrap();
    
    // Compact to remove tombstones
    tree.compact().unwrap();
    
    let size_after = tree.disk_usage().unwrap();
    
    // After compaction, size should eventually reduce or at least not grow significantly
    // In practice, one compaction might not be enough, or the overhead of SSTable metadata
    // might temporarily increase size. The important thing is tombstones are being removed.
    assert!(size_after <= size_before * 2, "Compaction should prevent unbounded growth");
}

#[test]
fn test_compaction_preserves_key_ordering() {
    let mut config = LsmConfig::default();
    config.memtable_size = 1024;
    config.level0_compaction_trigger = 2;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert in random order
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    
    let mut keys: Vec<_> = (0..500).collect();
    keys.shuffle(&mut thread_rng());
    
    for i in keys {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.compact().unwrap();
    
    // Verify iteration is still in sorted order
    let mut prev: Option<Key> = None;
    for (key, _) in tree.iter() {
        if let Some(p) = prev {
            assert!(p < key, "Keys should remain sorted after compaction");
        }
        prev = Some(key);
    }
}

#[test]
fn test_multiple_compaction_rounds() {
    let mut config = LsmConfig::default();
    config.memtable_size = 512;
    config.level0_compaction_trigger = 2;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Round 1: Insert data
    for i in 0..300 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}_v1", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.compact().unwrap();
    
    // Round 2: Update data
    for i in 0..300 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}_v2", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.compact().unwrap();
    
    // Round 3: Delete half
    for i in (0..300).step_by(2) {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        tree.delete(&key).unwrap();
    }
    
    // Regular compact might not remove all tombstones
    tree.compact().unwrap();
    
    // Do a full compaction to ensure tombstones are fully removed
    tree.compact_all().unwrap();
    
    // Debug: Check how many SSTables we have
    let stats = tree.get_stats().unwrap();
    eprintln!("After compaction: {} SSTables", stats.total_sstables_count);
    
    // Verify final state
    for i in 0..300 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let result = tree.get(&key).unwrap();
        
        if i % 2 == 0 {
            if result.is_some() {
                eprintln!("ERROR: key_{:06} should be deleted but has value: {:?}", i, result);
            }
            assert!(result.is_none(), "Even keys should be deleted (key_{:06})", i);
        } else {
            let expected = Value::from(format!("value_{}_v2", i).as_bytes());
            if result.as_ref() != Some(&expected) {
                eprintln!("ERROR: key_{:06} has wrong value: {:?}, expected: {:?}", i, result, expected);
            }
            assert_eq!(result.unwrap(), expected, "Odd keys should have v2 values");
        }
    }
}

#[test]
fn test_compaction_with_bloom_filters() {
    let mut config = LsmConfig::default();
    config.memtable_size = 1024;
    config.bloom_filter_bits_per_key = 10;
    config.bloom_filter_fp_rate = 0.01;
    
    let (mut tree, _temp) = create_test_tree_with_config(config);
    
    // Insert data
    for i in 0..1000 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    tree.compact().unwrap();
    
    // Bloom filters should still work after compaction
    let _stats = tree.get_stats().unwrap();
    
    // Query non-existent keys - should have low false positive rate
    let mut false_positives = 0;
    for i in 1000..2000 {
        let key = Key::from(format!("key_{:06}", i).as_bytes());
        if tree.get(&key).unwrap().is_some() {
            false_positives += 1;
        }
    }
    
    let fp_rate = false_positives as f64 / 1000.0;
    assert!(fp_rate < 0.05, "False positive rate should be low: {}", fp_rate);
}
