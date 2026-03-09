use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
use tempfile::TempDir;

#[test]
fn test_range_query_with_deleted_key() {
    let temp_dir = TempDir::new().unwrap();
    let mut tree = LsmTree::open(temp_dir.path(), LsmConfig::default()).unwrap();

    // Insert a key
    let key = Key::from(b"test_key");
    let value = Value::from(b"test_value");
    tree.insert(&key, &value).unwrap();

    // Save snapshot to force data to SSTable
    tree.save_snapshot("snap1", "after insert").unwrap();

    // Delete the key
    tree.delete(&key).unwrap();

    // Save another snapshot to force deletion to SSTable
    tree.save_snapshot("snap2", "after delete").unwrap();

    // Do a range query that should include this key range
    // Bug: We now have 2 SSTables:
    //   - SSTable A (older, lower run_number): has the insert
    //   - SSTable B (newer, higher run_number): has the tombstone
    // Range query should NOT return the deleted key because the tombstone
    // should override the insert
    let from = Key::from(b"a");
    let to = Key::from(b"z");
    let results: Vec<_> = tree.range(&from, &to).collect();

    println!("Range query results: {:?}", results.len());
    for (k, v) in &results {
        println!("  Key: {:?}, Value: {:?}",
            String::from_utf8_lossy(k.as_ref()),
            String::from_utf8_lossy(v.as_ref()));
    }

    // BUG: The deleted key appears in results!
    assert_eq!(
        results.len(),
        0,
        "Range query should not return deleted keys, but found {} results",
        results.len()
    );
}
