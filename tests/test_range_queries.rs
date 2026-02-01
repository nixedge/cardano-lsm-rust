/// Tests for range queries and prefix scans
/// Critical for address-based queries in wallet indexer
use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
use tempfile::TempDir;

fn create_test_tree() -> (LsmTree, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = LsmConfig::default();
    let tree = LsmTree::open(temp_dir.path(), config).unwrap();
    (tree, temp_dir)
}

#[test]
fn test_range_scan_empty_tree() {
    let (tree, _temp) = create_test_tree();
    
    let from = Key::from(b"a");
    let to = Key::from(b"z");
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    assert!(results.is_empty(), "Range scan on empty tree should return nothing");
}

#[test]
fn test_range_scan_inclusive() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert keys: a, b, c, d, e, f
    for c in b'a'..=b'f' {
        let key = Key::from(&[c]);
        let value = Value::from(&[c]);
        tree.insert(&key, &value).unwrap();
    }
    
    // Range scan from 'b' to 'e' (inclusive)
    let from = Key::from(b"b");
    let to = Key::from(b"e");
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    
    assert_eq!(results.len(), 4); // b, c, d, e
    assert_eq!(results[0].0, Key::from(b"b"));
    assert_eq!(results[1].0, Key::from(b"c"));
    assert_eq!(results[2].0, Key::from(b"d"));
    assert_eq!(results[3].0, Key::from(b"e"));
}

#[test]
fn test_range_scan_at_boundaries() {
    let (mut tree, _temp) = create_test_tree();
    
    for c in b'a'..=b'z' {
        let key = Key::from(&[c]);
        let value = Value::from(&[c]);
        tree.insert(&key, &value).unwrap();
    }
    
    // Scan exactly at boundaries
    let from = Key::from(b"a");
    let to = Key::from(b"a");
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, Key::from(b"a"));
}

#[test]
fn test_range_scan_no_matching_keys() {
    let (mut tree, _temp) = create_test_tree();
    
    tree.insert(&Key::from(b"aaa"), &Value::from(b"1")).unwrap();
    tree.insert(&Key::from(b"zzz"), &Value::from(b"2")).unwrap();
    
    // Range that contains no keys
    let from = Key::from(b"bbb");
    let to = Key::from(b"yyy");
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    assert!(results.is_empty(), "Range with no matching keys should return empty");
}

#[test]
fn test_range_scan_full_table() {
    let (mut tree, _temp) = create_test_tree();
    
    let n = 100;
    for i in 0..n {
        let key = Key::from(format!("key_{:04}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Scan entire range
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 10]);
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    assert_eq!(results.len(), n);
}

#[test]
fn test_prefix_scan() {
    let (mut tree, _temp) = create_test_tree();
    
    let entries = vec![
        b"prefix_1" as &[u8],
        b"prefix_2" as &[u8],
        b"prefix_3" as &[u8],
        b"other_1" as &[u8],
        b"other_2" as &[u8],
    ];
    
    for entry in &entries {
        tree.insert(&Key::from(*entry), &Value::from(*entry)).unwrap();
    }
    
    // Scan with prefix "prefix_"
    let prefix = b"prefix_";
    let results: Vec<_> = tree.scan_prefix(prefix).collect();
    
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|(k, _)| k.as_ref().starts_with(prefix)));
}

#[test]
fn test_prefix_scan_empty_prefix() {
    let (mut tree, _temp) = create_test_tree();
    
    for i in 0..10 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Empty prefix should match all keys
    let results: Vec<_> = tree.scan_prefix(b"").collect();
    assert_eq!(results.len(), 10);
}

#[test]
fn test_prefix_scan_no_matches() {
    let (mut tree, _temp) = create_test_tree();
    
    tree.insert(&Key::from(b"foo"), &Value::from(b"1")).unwrap();
    tree.insert(&Key::from(b"bar"), &Value::from(b"2")).unwrap();
    
    let results: Vec<_> = tree.scan_prefix(b"baz").collect();
    assert!(results.is_empty());
}

#[test]
fn test_range_scan_with_deletes() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert a-z
    for c in b'a'..=b'z' {
        let key = Key::from(&[c]);
        let value = Value::from(&[c]);
        tree.insert(&key, &value).unwrap();
    }
    
    // Delete some keys in the middle
    for c in b'j'..=b'p' {
        let key = Key::from(&[c]);
        tree.delete(&key).unwrap();
    }
    
    // Range scan should skip deleted keys
    let from = Key::from(b"h");
    let to = Key::from(b"s");
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    
    // Should get: h, i, q, r, s (j-p are deleted)
    assert_eq!(results.len(), 5);
    assert_eq!(results[0].0, Key::from(b"h"));
    assert_eq!(results[1].0, Key::from(b"i"));
    assert_eq!(results[2].0, Key::from(b"q"));
    assert_eq!(results[3].0, Key::from(b"r"));
    assert_eq!(results[4].0, Key::from(b"s"));
}

#[test]
fn test_range_scan_reverse_bounds() {
    let (mut tree, _temp) = create_test_tree();
    
    for c in b'a'..=b'f' {
        let key = Key::from(&[c]);
        let value = Value::from(&[c]);
        tree.insert(&key, &value).unwrap();
    }
    
    // Reversed bounds (from > to) should return empty
    let from = Key::from(b"f");
    let to = Key::from(b"a");
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    assert!(results.is_empty(), "Reversed bounds should return empty range");
}

#[test]
fn test_range_scan_sorted_order() {
    let (mut tree, _temp) = create_test_tree();
    
    // Insert in random order
    let keys = vec![b"zebra" as &[u8], b"apple" as &[u8], b"mango" as &[u8], b"banana" as &[u8], b"orange" as &[u8]];
    
    for key in &keys {
        tree.insert(&Key::from(*key), &Value::from(*key)).unwrap();
    }
    
    // Range scan should return in sorted order
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 10]);
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    
    let mut prev: Option<Key> = None;
    for (key, _) in results {
        if let Some(p) = prev {
            assert!(p < key, "Keys should be in sorted order");
        }
        prev = Some(key);
    }
}

#[test]
fn test_range_scan_large_dataset() {
    let (mut tree, _temp) = create_test_tree();
    
    let n = 10000;
    for i in 0..n {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Scan middle range
    let from = Key::from(b"key_00002000");
    let to = Key::from(b"key_00002999");
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    assert_eq!(results.len(), 1000);
    
    // Verify order
    let mut prev: Option<Key> = None;
    for (key, _) in results {
        if let Some(p) = prev {
            assert!(p < key);
        }
        prev = Some(key);
    }
}

#[test]
fn test_prefix_scan_with_common_prefixes() {
    let (mut tree, _temp) = create_test_tree();
    
    // Wallet address-like keys with common prefixes
    let addresses = vec![
        b"addr1qx2kd3euw8jhzl7",
        b"addr1qx2kd3euw8jhzl8",
        b"addr1qx2kd3euw8jhzl9",
        b"addr1qx2kd3euw8aaaaa",
        b"addr1vy5h2k8l3euw8jh",
        b"addr1vy5h2k8l3euw8ji",
    ];
    
    for addr in &addresses {
        tree.insert(&Key::from(*addr), &Value::from(*addr)).unwrap();
    }
    
    // Scan for specific prefix
    let prefix = b"addr1qx2kd3euw8jhzl";
    let results: Vec<_> = tree.scan_prefix(prefix).collect();
    
    assert_eq!(results.len(), 3);
    for (key, _) in results {
        assert!(key.as_ref().starts_with(prefix));
    }
}

#[test]
fn test_range_iterator_can_be_cloned() {
    let (mut tree, _temp) = create_test_tree();
    
    for i in 0..10 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    let from = Key::from(b"key_2");
    let to = Key::from(b"key_7");
    
    let iter1 = tree.range(&from, &to);
    let iter2 = iter1.clone();
    
    // Both iterators should produce same results
    let results1: Vec<_> = iter1.collect();
    let results2: Vec<_> = iter2.collect();
    
    assert_eq!(results1, results2);
}

#[test]
fn test_prefix_scan_empty_tree() {
    let (tree, _temp) = create_test_tree();
    
    let results: Vec<_> = tree.scan_prefix(b"any_prefix").collect();
    assert!(results.is_empty());
}

#[test]
fn test_range_scan_after_updates() {
    let (mut tree, _temp) = create_test_tree();
    
    // Initial insert
    for i in 0..5 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}_v1", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Update some values
    for i in 1..4 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}_v2", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // Range scan should see updated values
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 10]);
    
    let results: Vec<_> = tree.range(&from, &to).collect();
    
    assert_eq!(results.len(), 5);
    
    // Check that updated values are returned
    assert_eq!(results[1].1, Value::from(b"value_1_v2"));
    assert_eq!(results[2].1, Value::from(b"value_2_v2"));
    assert_eq!(results[3].1, Value::from(b"value_3_v2"));
}
