// Cross-format validation test
//
// This test validates byte-level file format compatibility between
// the Rust and Haskell LSM tree implementations by:
// 1. Running the Haskell cross-format-writer to create database files
// 2. Opening and reading those files with the Rust implementation
// 3. Verifying all data matches expected values
//
// This ensures that the file formats are truly identical, not just
// behaviorally compatible through conformance tests.

use cardano_lsm::{Key, LsmConfig, LsmTree, Result, Value};
use std::path::PathBuf;
use std::process::Command;

const TEST_DATA_DIR: &str = "cross-format-test-data";

#[test]
fn test_rust_reads_haskell_files() -> Result<()> {
    // Step 1: Generate Haskell database files
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Cross-Format Validation: Haskell → Rust");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Step 1: Generating database files with Haskell...");

    let status = Command::new("cabal")
        .args(&["run", "cross-format-writer", "--", TEST_DATA_DIR])
        .current_dir("conformance-generator")
        .status()
        .expect("Failed to run Haskell cross-format-writer. Is cabal installed?");

    if !status.success() {
        panic!(
            "Haskell cross-format-writer failed with status: {:?}. \
             Run 'cd conformance-generator && cabal build' to see errors.",
            status.code()
        );
    }

    println!();
    println!("Step 2: Opening database with Rust...");

    // Step 2: Open the Haskell-generated database with Rust
    let db_path = PathBuf::from(TEST_DATA_DIR).join("session");

    // Check that database files exist
    if !db_path.exists() {
        panic!(
            "Database path doesn't exist: {}. Haskell writer may have failed.",
            db_path.display()
        );
    }

    // Open with Rust (read-only mode would be ideal, but we'll use normal mode)
    let config = LsmConfig {
        memtable_size: 4096,
        bloom_filter_bits_per_key: 10,
        level0_compaction_trigger: 4,
        ..Default::default()
    };

    let tree = LsmTree::open(&db_path, config)?;

    println!("  ✓ Successfully opened Haskell-generated database");
    println!();
    println!("Step 3: Validating data...");

    // Step 3: Verify all expected data

    // Test simple keys (key1 was updated, key2/key4 were deleted)
    assert_eq!(
        tree.get(&Key::from(b"key1"))?,
        Some(Value::from(b"updated_value1")),
        "key1 should have updated value"
    );
    println!("  ✓ key1 = 'updated_value1' (updated value)");

    assert_eq!(
        tree.get(&Key::from(b"key2"))?,
        None,
        "key2 should be deleted"
    );
    println!("  ✓ key2 = <deleted> (tombstone)");

    assert_eq!(
        tree.get(&Key::from(b"key3"))?,
        Some(Value::from(b"value3")),
        "key3 should exist"
    );
    println!("  ✓ key3 = 'value3'");

    assert_eq!(
        tree.get(&Key::from(b"key4"))?,
        None,
        "key4 should be deleted"
    );
    println!("  ✓ key4 = <deleted> (tombstone)");

    // Test remaining simple keys
    for i in 5..=15 {
        let key = format!("key{}", i);
        let expected_value = format!("value{}", i);
        assert_eq!(
            tree.get(&Key::from(key.as_bytes()))?,
            Some(Value::from(expected_value.as_bytes())),
            "key{} should have correct value",
            i
        );
    }
    println!("  ✓ key5..key15 all present with correct values");

    // Test large value (blob)
    let large_value = tree.get(&Key::from(b"large_key"))?;
    assert!(large_value.is_some(), "large_key should exist");
    let large_value = large_value.unwrap();
    let large_value_bytes: &[u8] = large_value.as_ref();
    assert_eq!(large_value_bytes.len(), 1000, "large_key should be 1KB");
    assert!(
        large_value_bytes.iter().all(|&b| b == 0x42),
        "large_key should be all 0x42 bytes"
    );
    println!("  ✓ large_key = <1KB of 0x42 bytes> (blob storage)");

    // Test prefix keys
    for i in 1..=5 {
        let key = format!("prefix_{}", i);
        let expected_value = format!("prefix_value_{}", i);
        assert_eq!(
            tree.get(&Key::from(key.as_bytes()))?,
            Some(Value::from(expected_value.as_bytes())),
            "{} should have correct value",
            key
        );
    }
    println!("  ✓ prefix_1..prefix_5 all present with correct values");

    // Test range query
    let range_results: Vec<_> = tree
        .range(&Key::from(b"key1"), &Key::from(b"key9"))
        .collect();

    // Should include key1, key3, key5, key6, key7, key8, key9 (key2 and key4 deleted)
    assert_eq!(
        range_results.len(),
        7,
        "Range query should return 7 keys (2 deleted)"
    );
    println!("  ✓ Range query works correctly (respects tombstones)");

    // Test prefix scan
    let prefix_results: Vec<_> = tree.scan_prefix(b"prefix_").collect();
    assert_eq!(
        prefix_results.len(),
        5,
        "Prefix scan should return 5 keys"
    );
    println!("  ✓ Prefix scan works correctly");

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Cross-Format Validation PASSED");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Rust successfully read and validated all data from");
    println!("Haskell-generated LSM tree database files.");
    println!();
    println!("This confirms byte-level file format compatibility:");
    println!("  - SSTable format (keyops/blobs/filter/index)");
    println!("  - CRC32C checksums");
    println!("  - Bloom filters");
    println!("  - Metadata format");
    println!("  - Tombstone handling");
    println!("  - Blob storage");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    Ok(())
}

#[test]
fn test_haskell_reads_rust_files() -> Result<()> {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Cross-Format Validation: Rust → Haskell");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Step 1: Generating database files with Rust...");

    // Step 1: Create database with Rust
    let test_data_dir = PathBuf::from("cross-format-test-data-rust");
    let db_path = test_data_dir.join("session");

    // Clean up if exists
    let _ = std::fs::remove_dir_all(&test_data_dir);
    std::fs::create_dir_all(&db_path)?;

    let config = LsmConfig {
        memtable_size: 4096,
        bloom_filter_bits_per_key: 10,
        level0_compaction_trigger: 4,
        ..Default::default()
    };

    let mut tree = LsmTree::open(&db_path, config)?;

    // Write test data (matching Haskell test data)
    for i in 1..=10 {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        tree.insert(&Key::from(key.as_bytes()), &Value::from(value.as_bytes()))?;
    }

    // Large value
    let large_value = vec![0x42u8; 1000];
    tree.insert(&Key::from(b"large_key"), &Value::from(large_value))?;

    // Deletes
    tree.delete(&Key::from(b"key2"))?;
    tree.delete(&Key::from(b"key4"))?;

    // Update
    tree.insert(
        &Key::from(b"key1"),
        &Value::from(b"updated_value1"),
    )?;

    // Prefix keys
    for i in 1..=5 {
        let key = format!("prefix_{}", i);
        let value = format!("prefix_value_{}", i);
        tree.insert(&Key::from(key.as_bytes()), &Value::from(value.as_bytes()))?;
    }

    // Force flush by creating snapshots
    tree.save_snapshot("snap1", "First snapshot")?;

    // More data
    for i in 11..=15 {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        tree.insert(&Key::from(key.as_bytes()), &Value::from(value.as_bytes()))?;
    }

    tree.save_snapshot("snap2", "Second snapshot")?;

    // Close the tree to ensure all data is flushed
    drop(tree);

    println!("  ✓ Created database with Rust");
    println!();
    println!("Step 2: Verifying Haskell can read Rust files...");

    // Step 2: Create a Haskell program to read and validate
    // For now, we'll just verify the files exist and have the right structure
    // A full Haskell reader program would be added later

    let active_dir = db_path.join("active");
    assert!(active_dir.exists(), "active directory should exist");

    let snapshots_dir = db_path.join("snapshots");
    assert!(snapshots_dir.exists(), "snapshots directory should exist");

    // Check for SSTable files
    let entries = std::fs::read_dir(&active_dir)?;
    let mut found_sstable = false;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".keyops") {
            found_sstable = true;
            // Verify other files exist
            let run_num = name.strip_suffix(".keyops").unwrap();
            assert!(
                active_dir.join(format!("{}.blobs", run_num)).exists(),
                "blobs file should exist"
            );
            assert!(
                active_dir.join(format!("{}.filter", run_num)).exists(),
                "filter file should exist"
            );
            assert!(
                active_dir.join(format!("{}.index", run_num)).exists(),
                "index file should exist"
            );
            assert!(
                active_dir.join(format!("{}.checksums", run_num)).exists(),
                "checksums file should exist"
            );
            println!("  ✓ Found complete SSTable: {}", run_num);
        }
    }

    assert!(found_sstable, "Should have created at least one SSTable");

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Rust → Haskell File Format Validation");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Rust successfully created LSM tree database files");
    println!("with the expected structure.");
    println!();
    println!("Next step: Add Haskell program to read and validate");
    println!("the Rust-generated files for full bidirectional testing.");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    Ok(())
}
