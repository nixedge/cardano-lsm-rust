// Test snapshot save and restore functionality
//
// Verifies that:
// 1. Snapshots can be saved
// 2. LSM trees can be opened from snapshots
// 3. Data is correctly restored

use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
use tempfile::TempDir;

#[test]
fn test_snapshot_save_and_restore() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    // Create tree and insert data
    {
        let mut tree = LsmTree::open(db_path, LsmConfig::default()).unwrap();

        tree.insert(&Key::from(b"key1"), &Value::from(b"value1")).unwrap();
        tree.insert(&Key::from(b"key2"), &Value::from(b"value2")).unwrap();
        tree.insert(&Key::from(b"key3"), &Value::from(b"value3")).unwrap();

        // Save snapshot
        tree.save_snapshot("test_snap", "Test snapshot").unwrap();
    }
    // Tree is closed here (dropped)

    // Open from snapshot
    let tree = LsmTree::open_snapshot(db_path, "test_snap").unwrap();

    // Verify data is restored
    assert_eq!(
        tree.get(&Key::from(b"key1")).unwrap(),
        Some(Value::from(b"value1"))
    );
    assert_eq!(
        tree.get(&Key::from(b"key2")).unwrap(),
        Some(Value::from(b"value2"))
    );
    assert_eq!(
        tree.get(&Key::from(b"key3")).unwrap(),
        Some(Value::from(b"value3"))
    );
}

#[test]
fn test_snapshot_restore_with_more_writes() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    // Create tree, insert data, save snapshot
    {
        let mut tree = LsmTree::open(db_path, LsmConfig::default()).unwrap();

        tree.insert(&Key::from(b"key1"), &Value::from(b"value1")).unwrap();
        tree.insert(&Key::from(b"key2"), &Value::from(b"value2")).unwrap();

        tree.save_snapshot("snap1", "First snapshot").unwrap();
    }

    // Open from snapshot and add more data
    {
        let mut tree = LsmTree::open_snapshot(db_path, "snap1").unwrap();

        // Verify original data
        assert_eq!(
            tree.get(&Key::from(b"key1")).unwrap(),
            Some(Value::from(b"value1"))
        );

        // Add new data
        tree.insert(&Key::from(b"key3"), &Value::from(b"value3")).unwrap();
        tree.insert(&Key::from(b"key4"), &Value::from(b"value4")).unwrap();

        // Save another snapshot
        tree.save_snapshot("snap2", "Second snapshot").unwrap();
    }

    // Open from second snapshot
    let tree = LsmTree::open_snapshot(db_path, "snap2").unwrap();

    // Verify all data is there
    assert_eq!(
        tree.get(&Key::from(b"key1")).unwrap(),
        Some(Value::from(b"value1"))
    );
    assert_eq!(
        tree.get(&Key::from(b"key2")).unwrap(),
        Some(Value::from(b"value2"))
    );
    assert_eq!(
        tree.get(&Key::from(b"key3")).unwrap(),
        Some(Value::from(b"value3"))
    );
    assert_eq!(
        tree.get(&Key::from(b"key4")).unwrap(),
        Some(Value::from(b"value4"))
    );
}

#[test]
fn test_snapshot_restore_nonexistent() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    // Try to open from non-existent snapshot
    let result = LsmTree::open_snapshot(db_path, "nonexistent");

    assert!(result.is_err());
    match result {
        Err(e) => assert!(e.to_string().contains("does not exist")),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

#[test]
fn test_snapshot_list_after_restore() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    // Create and save snapshots
    {
        let mut tree = LsmTree::open(db_path, LsmConfig::default()).unwrap();
        tree.insert(&Key::from(b"key1"), &Value::from(b"value1")).unwrap();
        tree.save_snapshot("snap1", "First").unwrap();
        tree.save_snapshot("snap2", "Second").unwrap();
    }

    // Open from snapshot and list snapshots
    let tree = LsmTree::open_snapshot(db_path, "snap1").unwrap();
    let snapshots = tree.list_snapshots().unwrap();

    // Should see both snapshots
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots.contains(&"snap1".to_string()));
    assert!(snapshots.contains(&"snap2".to_string()));
}
