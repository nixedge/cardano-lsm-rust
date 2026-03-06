// Tests for batch operation APIs

use cardano_lsm::{Key, LsmConfig, LsmTree, Result, Value};
use tempfile::TempDir;

#[test]
fn test_insert_batch() -> Result<()> {
    let dir = TempDir::new().map_err(|e| cardano_lsm::Error::Io(e))?;
    let config = LsmConfig::default();
    let mut tree = LsmTree::open(dir.path(), config)?;

    // Insert multiple entries in batch
    let entries: Vec<(Key, Value)> = (1..=100)
        .map(|i| {
            (
                Key::from(format!("key{:03}", i).as_bytes()),
                Value::from(format!("value{}", i).as_bytes()),
            )
        })
        .collect();

    tree.insert_batch(entries.clone())?;

    // Verify all entries were inserted
    for (key, value) in entries {
        assert_eq!(tree.get(&key)?, Some(value));
    }

    Ok(())
}

#[test]
fn test_get_batch() -> Result<()> {
    let dir = TempDir::new().map_err(|e| cardano_lsm::Error::Io(e))?;
    let config = LsmConfig::default();
    let mut tree = LsmTree::open(dir.path(), config)?;

    // Insert test data
    let test_data: Vec<(Key, Value)> = (1..=50)
        .map(|i| {
            (
                Key::from(format!("key{:03}", i).as_bytes()),
                Value::from(format!("value{}", i).as_bytes()),
            )
        })
        .collect();

    tree.insert_batch(test_data.clone())?;

    // Batch lookup of all keys
    let keys: Vec<Key> = test_data.iter().map(|(k, _)| k.clone()).collect();
    let results = tree.get_batch(keys.clone())?;

    // Verify results match
    assert_eq!(results.len(), test_data.len());
    for (i, (_key, expected_value)) in test_data.iter().enumerate() {
        assert_eq!(results[i].as_ref(), Some(expected_value));
    }

    // Test with some non-existent keys
    let mixed_keys = vec![
        Key::from(b"key001"),
        Key::from(b"missing1"),
        Key::from(b"key025"),
        Key::from(b"missing2"),
    ];

    let mixed_results = tree.get_batch(mixed_keys)?;
    assert!(mixed_results[0].is_some());
    assert!(mixed_results[1].is_none());
    assert!(mixed_results[2].is_some());
    assert!(mixed_results[3].is_none());

    Ok(())
}

#[test]
fn test_delete_batch() -> Result<()> {
    let dir = TempDir::new().map_err(|e| cardano_lsm::Error::Io(e))?;
    let config = LsmConfig::default();
    let mut tree = LsmTree::open(dir.path(), config)?;

    // Insert test data
    let entries: Vec<(Key, Value)> = (1..=100)
        .map(|i| {
            (
                Key::from(format!("key{:03}", i).as_bytes()),
                Value::from(format!("value{}", i).as_bytes()),
            )
        })
        .collect();

    tree.insert_batch(entries.clone())?;

    // Delete half of them in batch
    let keys_to_delete: Vec<Key> = entries
        .iter()
        .step_by(2)
        .map(|(k, _)| k.clone())
        .collect();

    tree.delete_batch(keys_to_delete.clone())?;

    // Verify deleted keys are gone
    for key in keys_to_delete {
        assert_eq!(tree.get(&key)?, None);
    }

    // Verify remaining keys still exist
    for (i, (key, value)) in entries.iter().enumerate() {
        if i % 2 == 1 {
            assert_eq!(tree.get(key)?, Some(value.clone()));
        }
    }

    Ok(())
}

#[test]
fn test_batch_operations_empty() -> Result<()> {
    let dir = TempDir::new().map_err(|e| cardano_lsm::Error::Io(e))?;
    let config = LsmConfig::default();
    let mut tree = LsmTree::open(dir.path(), config)?;

    // Test empty batch operations
    tree.insert_batch(vec![])?;
    let results = tree.get_batch(vec![])?;
    assert!(results.is_empty());
    tree.delete_batch(vec![])?;

    Ok(())
}

#[test]
fn test_batch_operations_performance() -> Result<()> {
    let dir = TempDir::new().map_err(|e| cardano_lsm::Error::Io(e))?;
    let config = LsmConfig::default();
    let mut tree = LsmTree::open(dir.path(), config)?;

    let num_entries = 1000;

    // Prepare test data
    let entries: Vec<(Key, Value)> = (1..=num_entries)
        .map(|i| {
            (
                Key::from(format!("key{:06}", i).as_bytes()),
                Value::from(format!("value{}", i).as_bytes()),
            )
        })
        .collect();

    // Time batch insert
    let start = std::time::Instant::now();
    tree.insert_batch(entries.clone())?;
    let batch_duration = start.elapsed();

    println!(
        "Batch insert of {} entries: {:?}",
        num_entries, batch_duration
    );

    // Verify all were inserted
    let keys: Vec<Key> = entries.iter().map(|(k, _)| k.clone()).collect();
    let results = tree.get_batch(keys)?;
    assert_eq!(results.len(), num_entries);
    assert_eq!(results.iter().filter(|r| r.is_some()).count(), num_entries);

    Ok(())
}

#[test]
fn test_batch_with_duplicates() -> Result<()> {
    let dir = TempDir::new().map_err(|e| cardano_lsm::Error::Io(e))?;
    let config = LsmConfig::default();
    let mut tree = LsmTree::open(dir.path(), config)?;

    // Insert batch with duplicate keys (last one should win)
    let entries = vec![
        (Key::from(b"key1"), Value::from(b"value1")),
        (Key::from(b"key2"), Value::from(b"value2")),
        (Key::from(b"key1"), Value::from(b"value1_updated")),
        (Key::from(b"key3"), Value::from(b"value3")),
        (Key::from(b"key2"), Value::from(b"value2_updated")),
    ];

    tree.insert_batch(entries)?;

    // Verify last values win
    assert_eq!(
        tree.get(&Key::from(b"key1"))?,
        Some(Value::from(b"value1_updated"))
    );
    assert_eq!(
        tree.get(&Key::from(b"key2"))?,
        Some(Value::from(b"value2_updated"))
    );
    assert_eq!(tree.get(&Key::from(b"key3"))?, Some(Value::from(b"value3")));

    Ok(())
}
