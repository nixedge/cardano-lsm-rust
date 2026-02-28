use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
use tempfile::tempdir;

#[test]
fn test_insert_after_rollback() {
    let dir = tempdir().unwrap();
    let config = LsmConfig::default();
    let mut tree = LsmTree::open(dir.path(), config).unwrap();

    // Insert key with value A
    let key = Key::from(b"test_key");
    let value_a = Value::from(b"short_value_A");
    tree.insert(&key, &value_a).unwrap();

    // Verify it's there
    assert_eq!(tree.get(&key).unwrap(), Some(value_a.clone()));

    // Create snapshot
    let snapshot = tree.snapshot();

    // Rollback to snapshot (should still have value A)
    tree.rollback(snapshot.clone()).unwrap();
    assert_eq!(tree.get(&key).unwrap(), Some(value_a.clone()));

    // Compact after rollback (like test_98)
    tree.compact().unwrap();

    // Insert key with value B (different, longer value)
    let value_b = Value::from(b"much_longer_value_B_that_should_replace_A");
    tree.insert(&key, &value_b).unwrap();

    // Verify immediately after insert
    assert_eq!(tree.get(&key).unwrap(), Some(value_b.clone()),
        "Immediately after insert, expected new value but got old value");

    // Compact multiple times (like test_98 has ops 174, 178)
    tree.compact().unwrap();
    assert_eq!(tree.get(&key).unwrap(), Some(value_b.clone()),
        "After first compact, expected new value but got old value");

    tree.compact().unwrap();
    assert_eq!(tree.get(&key).unwrap(), Some(value_b.clone()),
        "After second compact, expected new value but got old value");

    // Final check with range query (like test_98 op 192)
    let range_results: Vec<_> = tree.range(&key, &Key::from(&[0xFF; 32])).collect();
    assert!(!range_results.is_empty(), "Range query should find the key");

    let (found_key, found_value) = &range_results[0];
    assert_eq!(found_key, &key, "Range query found wrong key");
    assert_eq!(found_value, &value_b,
        "Range query: expected new value but got old value");
}
