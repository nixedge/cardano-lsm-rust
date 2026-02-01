/// Integration tests for basic LSM tree operations
/// Ported from Haskell lsm-tree test suite
use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
use tempfile::TempDir;

/// Helper to create a test LSM tree with default config
fn create_test_tree() -> (LsmTree, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = LsmConfig::default();
    let tree = LsmTree::open(temp_dir.path(), config).unwrap();
    (tree, temp_dir)
}

#[test]
fn test_empty_tree_lookup() {
    let (tree, _temp) = create_test_tree();
    
    let key = Key::from(b"nonexistent");
    let result = tree.get(&key).unwrap();
    
    assert!(result.is_none(), "Lookup in empty tree should return None");
}

#[test]
fn test_single_insert_and_lookup() {
    let (mut tree, _temp) = create_test_tree();
    
    let key = Key::from(b"hello");
    let value = Value::from(b"world");
    
    tree.insert(&key, &value).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert!(result.is_some(), "Inserted key should be found");
    assert_eq!(result.unwrap(), value, "Retrieved value should match inserted value");
}

#[test]
fn test_multiple_inserts() {
    let (mut tree, _temp) = create_test_tree();
    
    let entries = vec![
        (b"key1".as_slice(), b"value1".as_slice()),
        (b"key2".as_slice(), b"value2".as_slice()),
        (b"key3".as_slice(), b"value3".as_slice()),
        (b"key4".as_slice(), b"value4".as_slice()),
        (b"key5".as_slice(), b"value5".as_slice()),
    ];
    
    // Insert all entries
    for (k, v) in &entries {
        tree.insert(&Key::from(*k), &Value::from(*v)).unwrap();
    }
    
    // Verify all entries
    for (k, v) in &entries {
        let result = tree.get(&Key::from(*k)).unwrap();
        assert!(result.is_some(), "Key {:?} should be found", k);
        assert_eq!(result.unwrap(), Value::from(*v), "Value for {:?} should match", k);
    }
}

#[test]
fn test_overwrite_existing_key() {
    let (mut tree, _temp) = create_test_tree();
    
    let key = Key::from(b"key");
    let value1 = Value::from(b"value1");
    let value2 = Value::from(b"value2");
    
    tree.insert(&key, &value1).unwrap();
    tree.insert(&key, &value2).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert_eq!(result.unwrap(), value2, "Should retrieve most recent value");
}

#[test]
fn test_delete_existing_key() {
    let (mut tree, _temp) = create_test_tree();
    
    let key = Key::from(b"delete_me");
    let value = Value::from(b"some_value");
    
    tree.insert(&key, &value).unwrap();
    assert!(tree.get(&key).unwrap().is_some(), "Key should exist before delete");
    
    tree.delete(&key).unwrap();
    assert!(tree.get(&key).unwrap().is_none(), "Key should not exist after delete");
}

#[test]
fn test_delete_nonexistent_key() {
    let (mut tree, _temp) = create_test_tree();
    
    let key = Key::from(b"never_inserted");
    
    // Should not panic or error
    tree.delete(&key).unwrap();
    assert!(tree.get(&key).unwrap().is_none(), "Key should still not exist");
}

#[test]
fn test_insert_after_delete() {
    let (mut tree, _temp) = create_test_tree();
    
    let key = Key::from(b"key");
    let value1 = Value::from(b"value1");
    let value2 = Value::from(b"value2");
    
    tree.insert(&key, &value1).unwrap();
    tree.delete(&key).unwrap();
    tree.insert(&key, &value2).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert_eq!(result.unwrap(), value2, "Should retrieve new value after delete+insert");
}

#[test]
fn test_large_batch_insert() {
    let (mut tree, _temp) = create_test_tree();
    
    let n = 10000;
    
    // Insert many entries
    for i in 0..n {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{:08}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Verify random subset
    for i in (0..n).step_by(100) {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let expected = Value::from(format!("value_{:08}", i).as_bytes());
        let result = tree.get(&key).unwrap();
        assert_eq!(result.unwrap(), expected);
    }
}

#[test]
fn test_keys_with_common_prefixes() {
    let (mut tree, _temp) = create_test_tree();
    
    let entries = vec![
        (b"prefix".as_slice(), b"value0".as_slice()),
        (b"prefix1".as_slice(), b"value1".as_slice()),
        (b"prefix12".as_slice(), b"value2".as_slice()),
        (b"prefix123".as_slice(), b"value3".as_slice()),
        (b"prefix2".as_slice(), b"value4".as_slice()),
    ];
    
    for (k, v) in &entries {
        tree.insert(&Key::from(*k), &Value::from(*v)).unwrap();
    }
    
    for (k, v) in &entries {
        let result = tree.get(&Key::from(*k)).unwrap();
        assert_eq!(result.unwrap(), Value::from(*v));
    }
}

#[test]
fn test_binary_keys_and_values() {
    let (mut tree, _temp) = create_test_tree();
    
    // Test with binary data (not valid UTF-8)
    let key = Key::from(&[0x00, 0x01, 0x02, 0xFF, 0xFE]);
    let value = Value::from(&[0xDE, 0xAD, 0xBE, 0xEF]);
    
    tree.insert(&key, &value).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert_eq!(result.unwrap(), value);
}

#[test]
fn test_empty_key() {
    let (mut tree, _temp) = create_test_tree();
    
    let key = Key::from(b"");
    let value = Value::from(b"empty_key_value");
    
    tree.insert(&key, &value).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert_eq!(result.unwrap(), value);
}

#[test]
fn test_empty_value() {
    let (mut tree, _temp) = create_test_tree();
    
    let key = Key::from(b"key_with_empty_value");
    let value = Value::from(b"");
    
    tree.insert(&key, &value).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert_eq!(result.unwrap(), value);
}

#[test]
fn test_very_large_key() {
    let (mut tree, _temp) = create_test_tree();
    
    let large_key = vec![b'k'; 10000];
    let key = Key::from(&large_key);
    let value = Value::from(b"value_for_large_key");
    
    tree.insert(&key, &value).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert_eq!(result.unwrap(), value);
}

#[test]
fn test_very_large_value() {
    let (mut tree, _temp) = create_test_tree();
    
    let key = Key::from(b"key");
    let large_value = vec![b'v'; 1_000_000]; // 1MB value
    let value = Value::from(&large_value);
    
    tree.insert(&key, &value).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert_eq!(result.unwrap(), value);
}

#[test]
fn test_persistence_across_close_and_reopen() {
    let temp_dir = TempDir::new().unwrap();
    let config = LsmConfig::default();
    
    let entries = vec![
        (b"persist1".as_slice(), b"value1".as_slice()),
        (b"persist2".as_slice(), b"value2".as_slice()),
        (b"persist3".as_slice(), b"value3".as_slice()),
    ];
    
    // Insert and close
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        for (k, v) in &entries {
            tree.insert(&Key::from(*k), &Value::from(*v)).unwrap();
        }
        tree.flush().unwrap();
    }
    
    // Reopen and verify
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        for (k, v) in &entries {
            let result = tree.get(&Key::from(*k)).unwrap();
            assert_eq!(result.unwrap(), Value::from(*v));
        }
    }
}

#[test]
fn test_concurrent_reads() {
    use std::sync::Arc;
    use std::thread;
    
    let (mut tree, _temp) = create_test_tree();
    
    // Insert test data
    for i in 0..1000 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let tree = Arc::new(tree);
    let mut handles = vec![];
    
    // Spawn multiple reader threads
    for _ in 0..10 {
        let tree_clone = Arc::clone(&tree);
        let handle = thread::spawn(move || {
            for i in (0..1000).step_by(10) {
                let key = Key::from(format!("key_{}", i).as_bytes());
                let expected = Value::from(format!("value_{}", i).as_bytes());
                let result = tree_clone.get(&key).unwrap();
                assert_eq!(result.unwrap(), expected);
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_sorted_iteration_order() {
    let (mut tree, _temp) = create_test_tree();
    
    let mut entries = vec![
        (b"gamma" as &[u8], b"3" as &[u8]),
        (b"alpha" as &[u8], b"1" as &[u8]),
        (b"delta" as &[u8], b"4" as &[u8]),
        (b"beta" as &[u8], b"2" as &[u8]),
        (b"epsilon" as &[u8], b"5" as &[u8]),
    ];
    
    for (k, v) in &entries {
        tree.insert(&Key::from(*k), &Value::from(*v)).unwrap();
    }
    
    // Sort expected order
    entries.sort_by_key(|(k, _)| *k);
    
    let mut iter = tree.iter();
    for (expected_k, expected_v) in entries {
        let (k, v) = iter.next().unwrap();
        assert_eq!(k, Key::from(expected_k));
        assert_eq!(v, Value::from(expected_v));
    }
    
    assert!(iter.next().is_none(), "Iterator should be exhausted");
}
