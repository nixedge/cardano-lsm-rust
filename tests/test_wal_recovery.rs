/// Tests for Write-Ahead Log (WAL) and crash recovery
/// Critical for ensuring durability and data integrity
use cardano_lsm::{LsmTree, LsmConfig, Key, Value, WalSyncMode};
use tempfile::TempDir;
use std::collections::HashMap;

#[allow(dead_code)]
fn create_test_tree() -> (LsmTree, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = LsmConfig::default();
    let tree = LsmTree::open(temp_dir.path(), config).unwrap();
    (tree, temp_dir)
}

#[test]
fn test_recovery_after_clean_shutdown() {
    let temp_dir = TempDir::new().unwrap();
    let config = LsmConfig::default();
    
    let mut expected = HashMap::new();
    
    // Insert data and close cleanly
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        
        for i in 0..100 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}", i).as_bytes());
            tree.insert(&key, &value).unwrap();
            expected.insert(key, value);
        }
        
        tree.flush().unwrap();
        // Clean shutdown
    }
    
    // Reopen and verify
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        
        for (key, expected_value) in expected {
            let result = tree.get(&key).unwrap();
            assert_eq!(result.unwrap(), expected_value);
        }
    }
}

#[test]
fn test_recovery_from_wal_after_crash() {
    let temp_dir = TempDir::new().unwrap();
    let mut config = LsmConfig::default();
    config.wal_sync_mode = WalSyncMode::Always; // Ensure WAL is synced
    
    let mut expected = HashMap::new();
    
    // Insert data but DON'T flush memtable
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        
        for i in 0..100 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}", i).as_bytes());
            tree.insert(&key, &value).unwrap();
            expected.insert(key, value);
        }
        
        // Simulate crash - don't flush, don't close cleanly
        std::mem::forget(tree);
    }
    
    // Reopen - should recover from WAL
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        
        for (key, expected_value) in expected {
            let result = tree.get(&key).unwrap();
            assert_eq!(result.unwrap(), expected_value, "Should recover from WAL");
        }
    }
}

#[test]
fn test_wal_recovery_with_deletes() {
    let temp_dir = TempDir::new().unwrap();
    let mut config = LsmConfig::default();
    config.wal_sync_mode = WalSyncMode::Always;
    
    // Insert and delete, then crash
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        
        // Insert
        for i in 0..50 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}", i).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
        
        // Delete half
        for i in (0..50).step_by(2) {
            let key = Key::from(format!("key_{}", i).as_bytes());
            tree.delete(&key).unwrap();
        }
        
        // Crash
        std::mem::forget(tree);
    }
    
    // Recover
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        
        for i in 0..50 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let result = tree.get(&key).unwrap();
            
            if i % 2 == 0 {
                assert!(result.is_none(), "Even keys should be deleted after recovery");
            } else {
                let expected = Value::from(format!("value_{}", i).as_bytes());
                assert_eq!(result.unwrap(), expected);
            }
        }
    }
}

#[test]
fn test_wal_recovery_with_overwrites() {
    let temp_dir = TempDir::new().unwrap();
    let mut config = LsmConfig::default();
    config.wal_sync_mode = WalSyncMode::Always;
    
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        
        // Insert initial values
        for i in 0..30 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}_v1", i).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
        
        // Overwrite
        for i in 0..30 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}_v2", i).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
        
        // Crash
        std::mem::forget(tree);
    }
    
    // Recover - should see v2 values
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        
        for i in 0..30 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let expected = Value::from(format!("value_{}_v2", i).as_bytes());
            assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
        }
    }
}

#[test]
fn test_wal_sync_modes() {
    let temp_dir = TempDir::new().unwrap();
    
    // Test each sync mode
    let sync_modes = vec![
        WalSyncMode::Always,
        WalSyncMode::Periodic(100),
        WalSyncMode::None,
    ];
    
    for sync_mode in sync_modes {
        let mut config = LsmConfig::default();
        config.wal_sync_mode = sync_mode.clone();
        
        {
            let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
            
            for i in 0..10 {
                let key = Key::from(format!("key_{}_{:?}", i, sync_mode).as_bytes());
                let value = Value::from(format!("value_{}", i).as_bytes());
                tree.insert(&key, &value).unwrap();
            }
            
            // Clean shutdown
            tree.flush().unwrap();
        }
        
        // Reopen and verify
        {
            let tree = LsmTree::open(temp_dir.path(), config).unwrap();
            
            for i in 0..10 {
                let key = Key::from(format!("key_{}_{:?}", i, sync_mode).as_bytes());
                let expected = Value::from(format!("value_{}", i).as_bytes());
                assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
            }
        }
    }
}

#[test]
fn test_wal_truncation_after_flush() {
    let temp_dir = TempDir::new().unwrap();
    let mut config = LsmConfig::default();
    config.memtable_size = 1024; // Small memtable to trigger flush
    
    let mut tree = LsmTree::open(temp_dir.path(), config).unwrap();
    
    // Initial WAL size should be small
    let initial_wal_size = tree.wal_size().unwrap();
    
    // Insert data (goes to WAL and memtable)
    // With small memtable, this should trigger auto-flush
    for i in 0..1000 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        let value = Value::from(format!("value_{}", i).as_bytes());
        tree.insert(&key, &value).unwrap();
    }
    
    // WAL should have been cleared during auto-flush
    let wal_size_after = tree.wal_size().unwrap();
    assert!(wal_size_after < initial_wal_size + 10000, "WAL should not grow unbounded with auto-flush");
}

#[test]
fn test_wal_max_size_rotation() {
    let temp_dir = TempDir::new().unwrap();
    let mut config = LsmConfig::default();
    config.memtable_size = 1024; // Small memtable to trigger auto-flush
    config.wal_max_size = 4096; // Small WAL
    let wal_max_size = config.wal_max_size;
    
    let mut tree = LsmTree::open(temp_dir.path(), config).unwrap();
    
    // Insert enough data to trigger memtable flushes
    for i in 0..500 {
        let key = Key::from(format!("key_{:08}", i).as_bytes());
        let value = Value::from(vec![b'v'; 100]); // Large values
        tree.insert(&key, &value).unwrap();
    }
    
    // With auto-flush, WAL should stay small
    let wal_size = tree.wal_size().unwrap();
    assert!(wal_size < (wal_max_size as u64) * 3, "WAL should stay bounded with auto-flush");
}

#[test]
fn test_recovery_with_partial_wal_entry() {
    let temp_dir = TempDir::new().unwrap();
    let config = LsmConfig::default();
    
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        
        for i in 0..50 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}", i).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
        
        // Simulate crash that corrupts last WAL entry
        drop(tree);
        
        // Corrupt the WAL file (truncate last few bytes)
        let wal_path = temp_dir.path().join("wal.log");
        if wal_path.exists() {
            let mut data = std::fs::read(&wal_path).unwrap();
            if data.len() > 10 {
                // Remove last 10 bytes
                data.truncate(data.len() - 10);
                std::fs::write(&wal_path, data).unwrap();
            }
        }
    }
    
    // Should recover gracefully, skipping corrupted entry
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        
        // Most keys should be recovered (except possibly the last few)
        let mut recovered = 0;
        for i in 0..50 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            if tree.get(&key).unwrap().is_some() {
                recovered += 1;
            }
        }
        
        assert!(recovered >= 45, "Should recover most entries despite corruption");
    }
}

#[test]
fn test_wal_checksum_validation() {
    let temp_dir = TempDir::new().unwrap();
    let mut config = LsmConfig::default();
    config.enable_wal_checksums = true;
    
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        
        for i in 0..30 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}", i).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
        
        drop(tree);
        
        // Corrupt WAL by flipping bits
        let wal_path = temp_dir.path().join("wal.log");
        if wal_path.exists() {
            let mut data = std::fs::read(&wal_path).unwrap();
            if data.len() > 100 {
                // Flip some bits in the middle
                data[50] ^= 0xFF;
                data[51] ^= 0xFF;
                std::fs::write(&wal_path, data).unwrap();
            }
        }
    }
    
    // Should detect corruption via checksum
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        
        // Should recover entries before corruption
        // Entries after corruption should be discarded
        let mut recovered = 0;
        for i in 0..30 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            if tree.get(&key).unwrap().is_some() {
                recovered += 1;
            }
        }
        
        // Should have recovered partial data
        assert!(recovered > 0 && recovered < 30, "Should recover some but not all");
    }
}

#[test]
fn test_multiple_crashes_and_recoveries() {
    let temp_dir = TempDir::new().unwrap();
    let mut config = LsmConfig::default();
    config.wal_sync_mode = WalSyncMode::Always;
    
    // Cycle 1: Insert and crash
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        for i in 0..20 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}_v1", i).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
        std::mem::forget(tree);
    }
    
    // Cycle 2: Recover, insert more, crash
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        for i in 20..40 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let value = Value::from(format!("value_{}_v1", i).as_bytes());
            tree.insert(&key, &value).unwrap();
        }
        std::mem::forget(tree);
    }
    
    // Cycle 3: Recover, verify all data
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        
        for i in 0..40 {
            let key = Key::from(format!("key_{}", i).as_bytes());
            let expected = Value::from(format!("value_{}_v1", i).as_bytes());
            assert_eq!(tree.get(&key).unwrap().unwrap(), expected);
        }
    }
}

#[test]
fn test_wal_replay_order_correctness() {
    let temp_dir = TempDir::new().unwrap();
    let mut config = LsmConfig::default();
    config.wal_sync_mode = WalSyncMode::Always;
    
    {
        let mut tree = LsmTree::open(temp_dir.path(), config.clone()).unwrap();
        
        // Series of operations that must be replayed in order
        let key = Key::from(b"test_key");
        
        tree.insert(&key, &Value::from(b"v1")).unwrap();
        tree.insert(&key, &Value::from(b"v2")).unwrap();
        tree.delete(&key).unwrap();
        tree.insert(&key, &Value::from(b"v3")).unwrap();
        
        std::mem::forget(tree);
    }
    
    // Recover and verify final state
    {
        let tree = LsmTree::open(temp_dir.path(), config).unwrap();
        
        let key = Key::from(b"test_key");
        let result = tree.get(&key).unwrap();
        
        assert_eq!(result.unwrap(), Value::from(b"v3"), "Should replay WAL in correct order");
    }
}
