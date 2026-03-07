// Conformance tests against Haskell lsm-tree reference implementation
//
// This test harness:
// 1. Loads test cases from conformance-tests/*.json
// 2. Executes operations against Rust cardano-lsm
// 3. Compares results with *.expected.json
// 4. Reports pass/fail for each test case

use cardano_lsm::{LsmTree, LsmConfig, Key, Value, Result as LsmResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ===== Test Case Format =====

#[derive(Debug, Deserialize)]
struct TestCase {
    seed: u64,
    version: String,
    config: TestConfig,
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
struct TestConfig {
    memtable_size: usize,
    bloom_bits_per_key: usize,
    level0_compaction_trigger: usize,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Operation {
    Insert {
        key: String,      // Base64-encoded
        value: String,    // Base64-encoded
    },
    Delete {
        key: String,      // Base64-encoded
    },
    Get {
        key: String,      // Base64-encoded
    },
    Range {
        from: String,     // Base64-encoded
        to: String,       // Base64-encoded
    },
    Snapshot {
        id: String,       // e.g., "snap_1"
    },
    Rollback {
        snapshot_id: String,
    },
    Compact,              // Trigger manual compaction
}

// ===== Expected Results Format =====

#[derive(Debug, Deserialize)]
struct ExpectedResults {
    results: Vec<OperationResult>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
enum OperationResult {
    OkUnit(Option<()>),
    Ok(Option<String>),  // Base64-encoded value or null (for Get)
    OkRange(Vec<(String, String)>),  // Array of key-value pairs (for Range)
    Err(String),
}

// ===== Test Execution =====

struct TestRunner {
    tree: LsmTree,
    snapshots: HashMap<String, cardano_lsm::LsmSnapshot>,
    temp_dir: PathBuf,
}

impl TestRunner {
    fn new(config: &TestConfig, test_name: &str) -> LsmResult<Self> {
        // Use timestamp and process ID to ensure unique temp directories
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        let temp_dir = std::env::temp_dir()
            .join(format!("conformance_{}_{}_{}",  test_name, pid, timestamp));

        // Aggressively clean up any old conformance test directories for this test name
        let base_temp = std::env::temp_dir();
        if let Ok(entries) = fs::read_dir(&base_temp) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(&format!("conformance_{}_", test_name)) {
                        let _ = fs::remove_dir_all(&path);
                    }
                }
            }
        }

        // Small delay to ensure file system operations complete
        std::thread::sleep(std::time::Duration::from_millis(10));

        fs::create_dir_all(&temp_dir)?;

        let lsm_config = LsmConfig {
            memtable_size: config.memtable_size,
            bloom_filter_bits_per_key: config.bloom_bits_per_key,
            level0_compaction_trigger: config.level0_compaction_trigger,
            ..Default::default()
        };

        let tree = LsmTree::open(&temp_dir, lsm_config)?;

        Ok(Self {
            tree,
            snapshots: HashMap::new(),
            temp_dir,
        })
    }

    fn execute_operation(&mut self, op: &Operation) -> OperationResult {
        match op {
            Operation::Insert { key, value } => {
                let key_bytes = base64::decode(key).expect("Invalid base64 key");
                let value_bytes = base64::decode(value).expect("Invalid base64 value");

                match self.tree.insert(&Key::from(&key_bytes), &Value::from(&value_bytes)) {
                    Ok(()) => OperationResult::OkUnit(None),
                    Err(e) => OperationResult::Err(format!("{}", e)),
                }
            }

            Operation::Delete { key } => {
                let key_bytes = base64::decode(key).expect("Invalid base64 key");

                match self.tree.delete(&Key::from(&key_bytes)) {
                    Ok(()) => OperationResult::OkUnit(None),
                    Err(e) => OperationResult::Err(format!("{}", e)),
                }
            }

            Operation::Get { key } => {
                let key_bytes = base64::decode(key).expect("Invalid base64 key");

                match self.tree.get(&Key::from(&key_bytes)) {
                    Ok(Some(value)) => {
                        // Encode value as base64
                        let encoded = base64::encode(value.as_ref());
                        OperationResult::Ok(Some(encoded))
                    }
                    Ok(None) => OperationResult::Ok(None),
                    Err(e) => OperationResult::Err(format!("{}", e)),
                }
            }

            Operation::Snapshot { id } => {
                let snapshot = self.tree.snapshot();
                self.snapshots.insert(id.clone(), snapshot);
                OperationResult::OkUnit(None)
            }

            Operation::Range { from, to } => {
                let from_bytes = base64::decode(from).expect("Invalid base64 from key");
                let to_bytes = base64::decode(to).expect("Invalid base64 to key");

                // range() returns an iterator directly, not a Result
                let entries: Vec<(String, String)> = self.tree
                    .range(&Key::from(&from_bytes), &Key::from(&to_bytes))
                    .map(|(k, v)| {
                        let key_b64 = base64::encode(k.as_ref());
                        let value_b64 = base64::encode(v.as_ref());
                        (key_b64, value_b64)
                    })
                    .collect();

                OperationResult::OkRange(entries)
            }

            Operation::Rollback { snapshot_id } => {
                match self.snapshots.get(snapshot_id) {
                    Some(snapshot) => {
                        match self.tree.rollback(snapshot.clone()) {
                            Ok(()) => OperationResult::OkUnit(None),
                            Err(e) => OperationResult::Err(format!("{}", e)),
                        }
                    }
                    None => OperationResult::Err("Snapshot not found".to_string()),
                }
            }

            Operation::Compact => {
                match self.tree.compact() {
                    Ok(()) => OperationResult::OkUnit(None),
                    Err(e) => OperationResult::Err(format!("{}", e)),
                }
            }
        }
    }

    fn run_test(&mut self, test_case: &TestCase) -> Vec<OperationResult> {
        test_case
            .operations
            .iter()
            .map(|op| self.execute_operation(op))
            .collect()
    }
}

impl Drop for TestRunner {
    fn drop(&mut self) {
        // Explicitly drop snapshots and tree before cleanup
        self.snapshots.clear();

        // Clean up temporary directory with retries for robustness
        for attempt in 0..3 {
            match fs::remove_dir_all(&self.temp_dir) {
                Ok(_) => break,
                Err(_) if attempt < 2 => {
                    // Small delay before retry
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => {
                    // Last attempt failed, but don't panic in Drop
                }
            }
        }
    }
}

// ===== Test Discovery and Execution =====

fn find_test_cases() -> Vec<PathBuf> {
    let conformance_dir = Path::new("conformance-tests");

    if !conformance_dir.exists() {
        return Vec::new();
    }

    let mut test_files = Vec::new();

    if let Ok(entries) = fs::read_dir(conformance_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json")
                && !path.file_name().unwrap().to_str().unwrap().contains("expected")
            {
                test_files.push(path);
            }
        }
    }

    test_files.sort();
    test_files
}

fn run_conformance_test(test_path: &Path) -> Result<(), String> {
    let test_name = test_path.file_stem().unwrap().to_str().unwrap();

    println!("\n=== Testing: {} ===", test_name);

    // Load test case
    let test_json = fs::read_to_string(test_path)
        .map_err(|e| format!("Failed to read test file: {}", e))?;

    let test_case: TestCase = serde_json::from_str(&test_json)
        .map_err(|e| format!("Failed to parse test case: {}", e))?;

    // Load expected results
    let expected_path = test_path.with_file_name(format!("{}.expected.json", test_name));
    let expected_json = fs::read_to_string(&expected_path)
        .map_err(|e| format!("Failed to read expected results: {}", e))?;

    let expected: ExpectedResults = serde_json::from_str(&expected_json)
        .map_err(|e| format!("Failed to parse expected results: {}", e))?;

    // Run test
    let mut runner = TestRunner::new(&test_case.config, test_name)
        .map_err(|e| format!("Failed to create test runner: {}", e))?;

    let actual_results = runner.run_test(&test_case);

    // Compare results
    if actual_results.len() != expected.results.len() {
        return Err(format!(
            "Result count mismatch: expected {}, got {}",
            expected.results.len(),
            actual_results.len()
        ));
    }

    let mut failures = Vec::new();

    for (i, (actual, expected)) in actual_results.iter().zip(expected.results.iter()).enumerate() {
        if !results_match(actual, expected) {
            failures.push((i, actual.clone(), expected.clone()));
        }
    }

    if failures.is_empty() {
        println!("✅ PASS: {} ({} operations)", test_name, actual_results.len());
        Ok(())
    } else {
        println!("❌ FAIL: {} ({}/{} operations failed)",
                 test_name, failures.len(), actual_results.len());

        for (i, actual, expected) in failures.iter().take(5) {
            println!("  Operation {}: ", i);

            // Print summary for range results to avoid extremely long output
            match (expected, actual) {
                (OperationResult::OkRange(exp_entries), OperationResult::OkRange(act_entries)) => {
                    println!("    Expected: OkRange with {} entries", exp_entries.len());
                    if !exp_entries.is_empty() {
                        println!("      First entry: key={}", exp_entries[0].0);
                        if exp_entries.len() > 1 {
                            println!("      Last entry:  key={}", exp_entries.last().unwrap().0);
                        }
                    }
                    println!("    Actual: OkRange with {} entries", act_entries.len());
                    if !act_entries.is_empty() {
                        println!("      First entry: key={}", act_entries[0].0);
                        if act_entries.len() > 1 {
                            println!("      Last entry:  key={}", act_entries.last().unwrap().0);
                        }
                    }

                    // Find differences
                    if exp_entries.len() != act_entries.len() {
                        println!("    Difference: Entry count mismatch");

                        // Find keys in actual but not in expected
                        let exp_keys: std::collections::HashSet<_> = exp_entries.iter().map(|(k, _)| k).collect();
                        let act_keys: std::collections::HashSet<_> = act_entries.iter().map(|(k, _)| k).collect();

                        for (idx, (k, _)) in act_entries.iter().enumerate() {
                            if !exp_keys.contains(k) {
                                println!("    EXTRA key in actual at index {}: {}", idx, k);
                            }
                        }

                        for (idx, (k, _)) in exp_entries.iter().enumerate() {
                            if !act_keys.contains(k) {
                                println!("    MISSING key from actual (expected at index {}): {}", idx, k);
                            }
                        }
                    } else {
                        // Check for key differences
                        for (idx, (exp, act)) in exp_entries.iter().zip(act_entries.iter()).enumerate() {
                            if exp.0 != act.0 {
                                println!("    Difference at index {}: expected key='{}', actual key='{}'", idx, exp.0, act.0);
                                if idx > 3 {
                                    println!("    ... (showing first few differences)");
                                    break;
                                }
                            }
                        }
                    }
                }
                _ => {
                    println!("    Expected: {:?}", expected);
                    println!("    Actual:   {:?}", actual);
                }
            }
        }

        if failures.len() > 5 {
            println!("  ... and {} more failures", failures.len() - 5);
        }

        Err(format!("{}/{} operations failed", failures.len(), actual_results.len()))
    }
}

fn results_match(actual: &OperationResult, expected: &OperationResult) -> bool {
    match (actual, expected) {
        // Both are OkUnit
        (OperationResult::OkUnit(_), OperationResult::OkUnit(_)) => true,

        // Both are Ok with same value
        (OperationResult::Ok(actual_val), OperationResult::Ok(expected_val)) => {
            actual_val == expected_val
        }

        // Both are OkRange with same entries
        (OperationResult::OkRange(actual_entries), OperationResult::OkRange(expected_entries)) => {
            actual_entries == expected_entries
        }

        // Both are Err - compare error messages (may need to be more lenient)
        (OperationResult::Err(actual_err), OperationResult::Err(expected_err)) => {
            // Exact match or contains key words
            actual_err == expected_err
                || (actual_err.contains("Snapshot") && expected_err.contains("Snapshot"))
                || (actual_err.contains("not found") && expected_err.contains("not found"))
        }

        // Different types - not a match
        _ => false,
    }
}

// ===== Main Test Entry Point =====

#[test]
fn conformance_tests() {
    let test_files = find_test_cases();

    // Auto-generate test files if they don't exist
    if test_files.is_empty() {
        println!("No conformance test files found. Generating 100 test cases...");
        println!("This may take a minute...");

        let status = std::process::Command::new("just")
            .args(&["gen-conformance", "100"])
            .status()
            .expect("Failed to run 'just gen-conformance'. Is just installed?");

        if !status.success() {
            panic!("Failed to generate conformance tests. Run 'just gen-conformance 100' manually.");
        }

        // Re-scan for test files
        let test_files_after = find_test_cases();
        if test_files_after.is_empty() {
            panic!("Test generation succeeded but no test files found. Check conformance-generator/conformance-tests/");
        }
    }

    let test_files = find_test_cases();
    println!("\nFound {} conformance test cases", test_files.len());

    let mut passed = 0;
    let mut failed = 0;
    let mut errors = Vec::new();

    for test_path in &test_files {
        match run_conformance_test(test_path) {
            Ok(()) => passed += 1,
            Err(e) => {
                failed += 1;
                errors.push((test_path.file_stem().unwrap().to_str().unwrap().to_string(), e));
            }
        }
    }

    println!("\n===========================================");
    println!("Conformance Test Results:");
    println!("  Passed: {}/{}", passed, test_files.len());
    println!("  Failed: {}/{}", failed, test_files.len());
    println!("  Pass Rate: {:.1}%", (passed as f64 / test_files.len() as f64) * 100.0);
    println!("===========================================\n");

    if failed > 0 {
        println!("Failed tests:");
        for (name, error) in &errors {
            println!("  - {}: {}", name, error);
        }
        panic!("\n{} conformance tests failed", failed);
    }
}
