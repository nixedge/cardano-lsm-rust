# Conformance Testing Plan - Rust LSM vs Haskell lsm-tree

## Overview

Implement property-based conformance tests that verify our Rust implementation matches the Haskell `lsm-tree` behavior exactly, similar to how Cardano uses Agda specs to validate the Haskell ledger.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                  Test Case Generator                        │
│                  (Shared Seed)                              │
└────────────┬───────────────────────────────┬────────────────┘
             │                               │
             ↓                               ↓
    ┌────────────────┐              ┌────────────────┐
    │  Rust Proptest │              │ Haskell QCheck │
    │  cardano-lsm   │              │  lsm-tree      │
    └────────┬───────┘              └────────┬───────┘
             │                               │
             ↓                               ↓
    ┌────────────────┐              ┌────────────────┐
    │  Rust Results  │              │ Haskell Results│
    └────────┬───────┘              └────────┬───────┘
             │                               │
             └───────────────┬───────────────┘
                             ↓
                    ┌────────────────┐
                    │   Comparator   │
                    │  (JSON-based)  │
                    └────────────────┘
```

## Step 1: Define Operation Schema

### operations.json
```json
{
  "version": "1.0",
  "seed": 42,
  "config": {
    "memtable_size": 1024,
    "bloom_bits_per_key": 10
  },
  "operations": [
    {
      "type": "Insert",
      "key": "AQIDBA==",
      "value": "ZGF0YQ=="
    },
    {
      "type": "Get",
      "key": "AQIDBA=="
    },
    {
      "type": "Delete",
      "key": "AQIDBA=="
    },
    {
      "type": "Range",
      "from": "AQIDBA==",
      "to": "BQYHCAk="
    },
    {
      "type": "Snapshot",
      "id": "snap_1"
    },
    {
      "type": "Rollback",
      "snapshot_id": "snap_1"
    },
    {
      "type": "Compact"
    }
  ],
  "expected_results": [
    {"Ok": null},
    {"Ok": "ZGF0YQ=="},
    {"Ok": null},
    {"OkRange": [["AQIDBA==", "ZGF0YQ=="]]},
    {"OkSnapshot": null},
    {"OkRollback": null},
    {"OkCompact": null}
  ]
}
```

## Step 2: Rust Test Harness

```rust
// tests/conformance.rs

use serde::{Serialize, Deserialize};
use cardano_lsm::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum Operation {
    Insert { key: String, value: String },  // base64
    Get { key: String },
    Delete { key: String },
    Range { from: String, to: String },
    Snapshot { id: String },
    Rollback { snapshot_id: String },
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
enum OperationResult {
    Ok(Option<String>),  // base64 value or None
    OkRange(Vec<(String, String)>),
    OkSnapshot,
    OkCompact,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct TestCase {
    seed: u64,
    config: TestConfig,
    operations: Vec<Operation>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TestConfig {
    memtable_size: usize,
    bloom_bits_per_key: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct TestResults {
    results: Vec<OperationResult>,
}

fn run_conformance_test(test_case: &TestCase) -> TestResults {
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
    
    let mut config = LsmConfig::default();
    config.memtable_size = test_case.config.memtable_size;
    config.bloom_filter_bits_per_key = test_case.config.bloom_bits_per_key;
    
    let temp_dir = tempfile::TempDir::new().unwrap();
    let mut tree = LsmTree::open(temp_dir.path(), config).unwrap();
    let mut snapshots: HashMap<String, LsmSnapshot> = HashMap::new();
    let mut results = Vec::new();
    
    for op in &test_case.operations {
        let result = match op {
            Operation::Insert { key, value } => {
                let k = BASE64.decode(key).unwrap();
                let v = BASE64.decode(value).unwrap();
                match tree.insert(&Key::from(&k), &Value::from(&v)) {
                    Ok(_) => OperationResult::Ok(None),
                    Err(e) => OperationResult::Err(e.to_string()),
                }
            }
            Operation::Get { key } => {
                let k = BASE64.decode(key).unwrap();
                match tree.get(&Key::from(&k)) {
                    Ok(Some(v)) => OperationResult::Ok(Some(BASE64.encode(v.as_ref()))),
                    Ok(None) => OperationResult::Ok(None),
                    Err(e) => OperationResult::Err(e.to_string()),
                }
            }
            Operation::Delete { key } => {
                let k = BASE64.decode(key).unwrap();
                match tree.delete(&Key::from(&k)) {
                    Ok(_) => OperationResult::Ok(None),
                    Err(e) => OperationResult::Err(e.to_string()),
                }
            }
            Operation::Range { from, to } => {
                let f = BASE64.decode(from).unwrap();
                let t = BASE64.decode(to).unwrap();
                let entries: Vec<_> = tree.range(&Key::from(&f), &Key::from(&t))
                    .map(|(k, v)| {
                        (BASE64.encode(k.as_ref()), BASE64.encode(v.as_ref()))
                    })
                    .collect();
                OperationResult::OkRange(entries)
            }
            Operation::Snapshot { id } => {
                let snap = tree.snapshot();
                snapshots.insert(id.clone(), snap);
                OperationResult::OkSnapshot
            }
            Operation::Rollback { snapshot_id } => {
                if let Some(snap) = snapshots.get(snapshot_id) {
                    match tree.rollback(snap.clone()) {
                        Ok(_) => OperationResult::OkSnapshot,
                        Err(e) => OperationResult::Err(e.to_string()),
                    }
                } else {
                    OperationResult::Err("Snapshot not found".to_string())
                }
            }
            Operation::Compact => {
                match tree.compact() {
                    Ok(_) => OperationResult::OkCompact,
                    Err(e) => OperationResult::Err(e.to_string()),
                }
            }
        };
        results.push(result);
    }
    
    TestResults { results }
}

#[test]
fn test_conformance_suite() {
    let conformance_dir = std::path::Path::new("./conformance-tests");
    
    if !conformance_dir.exists() {
        eprintln!("Skipping conformance tests - directory not found");
        eprintln!("Run Haskell test generator first to create conformance-tests/");
        return;
    }
    
    let test_files = std::fs::read_dir(conformance_dir)
        .expect("Should be able to read conformance-tests directory");
    
    let mut passed = 0;
    let mut failed = 0;
    
    for entry in test_files {
        let entry = entry.unwrap();
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let filename = path.file_name().unwrap().to_str().unwrap();
            
            // Skip expected files
            if filename.contains("expected") {
                continue;
            }
            
            let test_case: TestCase = serde_json::from_str(
                &std::fs::read_to_string(&path).unwrap()
            ).unwrap();
            
            let rust_results = run_conformance_test(&test_case);
            
            // Load expected results
            let expected_path = path.with_extension("expected.json");
            if !expected_path.exists() {
                eprintln!("Missing expected results for: {}", filename);
                failed += 1;
                continue;
            }
            
            let expected: TestResults = serde_json::from_str(
                &std::fs::read_to_string(&expected_path).unwrap()
            ).unwrap();
            
            if rust_results.results == expected.results {
                passed += 1;
            } else {
                failed += 1;
                eprintln!("FAILED: {}", filename);
                eprintln!("  Expected: {:?}", expected.results);
                eprintln!("  Got:      {:?}", rust_results.results);
            }
        }
    }
    
    println!("Conformance Tests: {} passed, {} failed", passed, failed);
    assert_eq!(failed, 0, "Some conformance tests failed");
}
```

## Step 3: Haskell Test Generator

```haskell
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE OverloadedStrings #-}

module ConformanceGen