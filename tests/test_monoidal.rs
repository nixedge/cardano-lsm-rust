/// Tests for monoidal values and aggregations
/// Critical for efficient balance queries in wallet indexer
use cardano_lsm::{MonoidalLsmTree, Monoidal, Key, LsmConfig};
use tempfile::TempDir;
use std::collections::HashMap;

// Test monoidal types

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Balance(u64);

impl Monoidal for Balance {
    fn mempty() -> Self {
        Balance(0)
    }
    
    fn mappend(&self, other: &Self) -> Self {
        Balance(self.0.saturating_add(other.0))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct AssetMap(HashMap<String, u64>);

impl Monoidal for AssetMap {
    fn mempty() -> Self {
        AssetMap(HashMap::new())
    }
    
    fn mappend(&self, other: &Self) -> Self {
        let mut result = self.0.clone();
        for (asset, amount) in &other.0 {
            *result.entry(asset.clone()).or_insert(0) += amount;
        }
        AssetMap(result)
    }
}

fn create_test_tree<V: Monoidal>() -> (MonoidalLsmTree<V>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = LsmConfig::default();
    let tree = MonoidalLsmTree::open(temp_dir.path(), config).unwrap();
    (tree, temp_dir)
}

#[test]
fn test_monoidal_identity() {
    let (tree, _temp) = create_test_tree::<Balance>();
    
    let empty_key = Key::from(b"nonexistent");
    let result = tree.get(&empty_key).unwrap();
    
    // Non-existent key should return identity
    assert_eq!(result, Balance::mempty());
}

#[test]
fn test_monoidal_insert_and_get() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    let key = Key::from(b"addr1");
    let balance = Balance(1000);
    
    tree.insert(&key, &balance).unwrap();
    
    let result = tree.get(&key).unwrap();
    assert_eq!(result, balance);
}

#[test]
fn test_monoidal_range_fold() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Insert balances for different addresses
    let balances = vec![
        (b"addr_a", 100),
        (b"addr_b", 200),
        (b"addr_c", 300),
        (b"addr_d", 400),
        (b"addr_e", 500),
    ];
    
    for (addr, amount) in &balances {
        tree.insert(&Key::from(*addr), &Balance(*amount)).unwrap();
    }
    
    // Fold over range addr_b to addr_d
    let from = Key::from(b"addr_b");
    let to = Key::from(b"addr_d");
    
    let total = tree.range_fold(&from, &to);
    
    // Should sum 200 + 300 + 400 = 900
    assert_eq!(total, Balance(900));
}

#[test]
fn test_monoidal_full_range_fold() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    let mut expected_total = 0u64;
    
    for i in 0..100 {
        let key = Key::from(format!("addr_{:03}", i).as_bytes());
        let amount = i * 10;
        tree.insert(&key, &Balance(amount)).unwrap();
        expected_total += amount;
    }
    
    // Fold entire tree
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 20]);
    
    let total = tree.range_fold(&from, &to);
    
    assert_eq!(total, Balance(expected_total));
}

#[test]
fn test_monoidal_prefix_fold() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Insert with common prefixes
    tree.insert(&Key::from(b"wallet1_addr_a"), &Balance(100)).unwrap();
    tree.insert(&Key::from(b"wallet1_addr_b"), &Balance(200)).unwrap();
    tree.insert(&Key::from(b"wallet1_addr_c"), &Balance(300)).unwrap();
    tree.insert(&Key::from(b"wallet2_addr_a"), &Balance(400)).unwrap();
    tree.insert(&Key::from(b"wallet2_addr_b"), &Balance(500)).unwrap();
    
    // Fold with prefix "wallet1_"
    let total = tree.prefix_fold(b"wallet1_");
    
    // Should sum 100 + 200 + 300 = 600
    assert_eq!(total, Balance(600));
}

#[test]
fn test_monoidal_asset_map_aggregation() {
    let (mut tree, _temp) = create_test_tree::<AssetMap>();
    
    // Address 1 has some assets
    let mut assets1 = HashMap::new();
    assets1.insert("ADA".to_string(), 1000);
    assets1.insert("TOKEN_A".to_string(), 50);
    tree.insert(&Key::from(b"addr1"), &AssetMap(assets1)).unwrap();
    
    // Address 2 has different assets
    let mut assets2 = HashMap::new();
    assets2.insert("ADA".to_string(), 2000);
    assets2.insert("TOKEN_B".to_string(), 100);
    tree.insert(&Key::from(b"addr2"), &AssetMap(assets2)).unwrap();
    
    // Address 3 has overlapping assets
    let mut assets3 = HashMap::new();
    assets3.insert("ADA".to_string(), 500);
    assets3.insert("TOKEN_A".to_string(), 25);
    tree.insert(&Key::from(b"addr3"), &AssetMap(assets3)).unwrap();
    
    // Fold all addresses
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 20]);
    
    let total = tree.range_fold(&from, &to);
    
    // Verify aggregated balances
    assert_eq!(total.0.get("ADA"), Some(&3500)); // 1000 + 2000 + 500
    assert_eq!(total.0.get("TOKEN_A"), Some(&75)); // 50 + 25
    assert_eq!(total.0.get("TOKEN_B"), Some(&100)); // 100
}

#[test]
fn test_monoidal_fold_with_deletes() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Insert balances
    for i in 0..10 {
        let key = Key::from(format!("addr_{}", i).as_bytes());
        tree.insert(&key, &Balance(100)).unwrap();
    }
    
    // Initial total should be 1000
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 20]);
    assert_eq!(tree.range_fold(&from, &to), Balance(1000));
    
    // Delete some keys
    for i in (0..10).step_by(2) {
        let key = Key::from(format!("addr_{}", i).as_bytes());
        tree.delete(&key).unwrap();
    }
    
    // New total should be 500 (5 remaining addresses × 100)
    assert_eq!(tree.range_fold(&from, &to), Balance(500));
}

#[test]
fn test_monoidal_fold_with_updates() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Insert initial balances
    for i in 0..5 {
        let key = Key::from(format!("addr_{}", i).as_bytes());
        tree.insert(&key, &Balance(100)).unwrap();
    }
    
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 20]);
    
    // Initial total: 500
    assert_eq!(tree.range_fold(&from, &to), Balance(500));
    
    // Update balances
    for i in 0..5 {
        let key = Key::from(format!("addr_{}", i).as_bytes());
        tree.insert(&key, &Balance(200)).unwrap();
    }
    
    // New total: 1000
    assert_eq!(tree.range_fold(&from, &to), Balance(1000));
}

#[test]
fn test_monoidal_fold_empty_range() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    tree.insert(&Key::from(b"addr_a"), &Balance(100)).unwrap();
    tree.insert(&Key::from(b"addr_z"), &Balance(200)).unwrap();
    
    // Range with no matching keys
    let from = Key::from(b"addr_m");
    let to = Key::from(b"addr_n");
    
    let total = tree.range_fold(&from, &to);
    
    // Should return identity
    assert_eq!(total, Balance::mempty());
}

#[test]
fn test_monoidal_fold_performance() {
    use std::time::Instant;
    
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Insert many balances
    for i in 0..10000 {
        let key = Key::from(format!("addr_{:08}", i).as_bytes());
        tree.insert(&key, &Balance(i)).unwrap();
    }
    
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 20]);
    
    // Folding should be efficient
    let start = Instant::now();
    let _ = tree.range_fold(&from, &to);
    let duration = start.elapsed();
    
    // Should complete in reasonable time (< 100ms)
    assert!(duration.as_millis() < 100, "Fold took too long: {:?}", duration);
}

#[test]
fn test_monoidal_associativity() {
    // Verify associativity property: (a + b) + c = a + (b + c)
    let a = Balance(100);
    let b = Balance(200);
    let c = Balance(300);
    
    let left = a.mappend(&b).mappend(&c);
    let right = a.mappend(&b.mappend(&c));
    
    assert_eq!(left, right, "Monoidal mappend should be associative");
}

#[test]
fn test_monoidal_identity_law() {
    // Verify identity property: mempty + a = a + mempty = a
    let a = Balance(42);
    let empty = Balance::mempty();
    
    assert_eq!(a.mappend(&empty), a);
    assert_eq!(empty.mappend(&a), a);
}

#[test]
fn test_monoidal_saturation() {
    // Test that saturating_add prevents overflow
    let max = Balance(u64::MAX);
    let one = Balance(1);
    
    let result = max.mappend(&one);
    
    // Should saturate at MAX, not overflow
    assert_eq!(result, Balance(u64::MAX));
}

#[test]
fn test_monoidal_wallet_total_balance() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Simulate wallet with multiple addresses
    let addresses = vec![
        "addr1qx2kd3euw8jhzl7_account0_index0",
        "addr1qx2kd3euw8jhzl7_account0_index1",
        "addr1qx2kd3euw8jhzl7_account0_index2",
        "addr1qx2kd3euw8jhzl7_account1_index0",
        "addr1qx2kd3euw8jhzl7_account1_index1",
    ];
    
    let mut expected_total = 0u64;
    
    for (i, addr) in addresses.iter().enumerate() {
        let balance = (i as u64 + 1) * 1000; // 1000, 2000, 3000, 4000, 5000
        tree.insert(&Key::from(addr.as_bytes()), &Balance(balance)).unwrap();
        expected_total += balance;
    }
    
    // Get total balance for wallet (all addresses with prefix)
    let total = tree.prefix_fold(b"addr1qx2kd3euw8jhzl7");
    
    assert_eq!(total, Balance(expected_total));
}

#[test]
fn test_monoidal_asset_balance_per_policy() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Store asset balances with keys: address/policy_id/asset_name
    tree.insert(&Key::from(b"addr1/policy_abc/token1"), &Balance(100)).unwrap();
    tree.insert(&Key::from(b"addr1/policy_abc/token2"), &Balance(200)).unwrap();
    tree.insert(&Key::from(b"addr1/policy_xyz/token1"), &Balance(300)).unwrap();
    tree.insert(&Key::from(b"addr2/policy_abc/token1"), &Balance(400)).unwrap();
    
    // Get total for policy_abc across all addresses
    let total = tree.prefix_fold(b"addr1/policy_abc/");
    
    assert_eq!(total, Balance(300)); // 100 + 200
}

#[test]
fn test_monoidal_fold_after_compaction() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Insert data
    for i in 0..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        tree.insert(&key, &Balance(i * 10)).unwrap();
    }
    
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 20]);
    
    let total_before = tree.range_fold(&from, &to);
    
    // Compact
    tree.compact().unwrap();
    
    let total_after = tree.range_fold(&from, &to);
    
    // Total should be unchanged after compaction
    assert_eq!(total_before, total_after);
}

#[test]
fn test_monoidal_fold_with_snapshots() {
    let (mut tree, _temp) = create_test_tree::<Balance>();
    
    // Insert initial data
    for i in 0..50 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        tree.insert(&key, &Balance(100)).unwrap();
    }
    
    let snapshot = tree.snapshot();
    
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 20]);
    
    let total_snapshot = snapshot.range_fold(&from, &to);
    assert_eq!(total_snapshot, Balance(5000));
    
    // Add more data
    for i in 50..100 {
        let key = Key::from(format!("key_{}", i).as_bytes());
        tree.insert(&key, &Balance(100)).unwrap();
    }
    
    let total_current = tree.range_fold(&from, &to);
    assert_eq!(total_current, Balance(10000));
    
    // Snapshot should still show old total
    let total_snapshot_again = snapshot.range_fold(&from, &to);
    assert_eq!(total_snapshot_again, Balance(5000));
}

#[test]
fn test_monoidal_complex_asset_map() {
    let (mut tree, _temp) = create_test_tree::<AssetMap>();
    
    // Simulate realistic Cardano multi-asset UTxOs
    let mut utxo1_assets = HashMap::new();
    utxo1_assets.insert("lovelace".to_string(), 2_000_000); // 2 ADA
    utxo1_assets.insert("token_abc123".to_string(), 100);
    utxo1_assets.insert("nft_xyz789".to_string(), 1);
    
    let mut utxo2_assets = HashMap::new();
    utxo2_assets.insert("lovelace".to_string(), 3_000_000); // 3 ADA
    utxo2_assets.insert("token_abc123".to_string(), 50);
    
    let mut utxo3_assets = HashMap::new();
    utxo3_assets.insert("lovelace".to_string(), 1_500_000); // 1.5 ADA
    utxo3_assets.insert("token_def456".to_string(), 200);
    
    tree.insert(&Key::from(b"utxo_1"), &AssetMap(utxo1_assets)).unwrap();
    tree.insert(&Key::from(b"utxo_2"), &AssetMap(utxo2_assets)).unwrap();
    tree.insert(&Key::from(b"utxo_3"), &AssetMap(utxo3_assets)).unwrap();
    
    // Aggregate all UTxOs
    let from = Key::from(b"");
    let to = Key::from(&[0xFF; 20]);
    
    let total_assets = tree.range_fold(&from, &to);
    
    // Verify aggregated balances
    assert_eq!(total_assets.0.get("lovelace"), Some(&6_500_000)); // 6.5 ADA total
    assert_eq!(total_assets.0.get("token_abc123"), Some(&150)); // 100 + 50
    assert_eq!(total_assets.0.get("token_def456"), Some(&200));
    assert_eq!(total_assets.0.get("nft_xyz789"), Some(&1));
}
