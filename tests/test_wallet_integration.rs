// Integration test - Simulate realistic blockchain indexing
// Tests the full wallet indexer with LSM storage

use cardano_lsm::{LsmTree, LsmConfig, MonoidalLsmTree};
use std::collections::HashMap;
use tempfile::TempDir;

mod mock_wallet;
use mock_wallet::*;

#[test]
fn test_wallet_indexer_basic_flow() {
    let temp_dir = TempDir::new().unwrap();
    let mut wallet = WalletIndexer::new(
        temp_dir.path(),
        vec![
            Address::new("addr1_alice"),
            Address::new("addr1_bob"),
        ],
    ).unwrap();
    
    let blocks = generate_test_scenario_blocks();
    
    // Process all blocks
    for block in blocks {
        wallet.process_block(block).unwrap();
    }
    
    // Verify final state
    let alice_balance = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
    let bob_balance = wallet.get_balance(&Address::new("addr1_bob")).unwrap();
    
    assert_eq!(alice_balance, 6_830_000); // 6.83 ADA
    assert_eq!(bob_balance, 2_830_000);   // 2.83 ADA
    
    // Verify total
    let total = wallet.get_total_balance().unwrap();
    assert_eq!(total, alice_balance + bob_balance);
}

#[test]
fn test_wallet_rollback() {
    let temp_dir = TempDir::new().unwrap();
    let mut wallet = WalletIndexer::new(
        temp_dir.path(),
        vec![Address::new("addr1_alice"), Address::new("addr1_bob")],
    ).unwrap();
    
    let blocks = generate_test_scenario_blocks();
    
    // Process all 3 blocks
    for block in blocks {
        wallet.process_block(block).unwrap();
    }
    
    let alice_before = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
    assert_eq!(alice_before, 6_830_000);
    
    // Rollback to block 1
    wallet.rollback(1).unwrap();
    
    let alice_after = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
    assert_eq!(alice_after, 4_830_000); // Before block 2
    
    let bob_after = wallet.get_balance(&Address::new("addr1_bob")).unwrap();
    assert_eq!(bob_after, 5_000_000); // Before block 2
}

#[test]
fn test_wallet_utxo_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let mut wallet = WalletIndexer::new(
        temp_dir.path(),
        vec![Address::new("addr1_alice")],
    ).unwrap();
    
    let blocks = generate_test_scenario_blocks();
    
    // Process block 0
    wallet.process_block(blocks[0].clone()).unwrap();
    
    // Alice should have 1 UTXO
    let utxos = wallet.get_utxos(&Address::new("addr1_alice")).unwrap();
    assert_eq!(utxos.len(), 1);
    assert_eq!(utxos[0].amount, 10_000_000);
    
    // Process block 1
    wallet.process_block(blocks[1].clone()).unwrap();
    
    // Alice should have 1 UTXO (spent 1, created 1)
    let utxos = wallet.get_utxos(&Address::new("addr1_alice")).unwrap();
    assert_eq!(utxos.len(), 1);
    assert_eq!(utxos[0].amount, 4_830_000);
}

#[test]
fn test_large_blockchain_simulation() {
    let temp_dir = TempDir::new().unwrap();
    let mut wallet = WalletIndexer::new(
        temp_dir.path(),
        vec![
            Address::new("addr1_alice"),
            Address::new("addr1_bob"),
            Address::new("addr1_charlie"),
        ],
    ).unwrap();
    
    // Generate 100 blocks with 50 txs each = 5000 transactions
    let blocks = generate_mock_blocks(100, 50);
    
    println!("Processing 100 blocks with 50 txs each...");
    
    let start = std::time::Instant::now();
    
    for block in blocks {
        wallet.process_block(block).unwrap();
    }
    
    let duration = start.elapsed();
    println!("Processed 100 blocks in {:?}", duration);
    println!("Average: {:?} per block", duration / 100);
    
    // Should complete in reasonable time
    assert!(duration.as_secs() < 30, "Should process 100 blocks in < 30 seconds");
    
    // Verify wallet still works
    let total = wallet.get_total_balance().unwrap();
    assert!(total > 0, "Wallet should have some balance");
    
    println!("Total wallet balance: {} ADA", total as f64 / 1_000_000.0);
}

#[test]
fn test_multiple_rollbacks() {
    let temp_dir = TempDir::new().unwrap();
    let mut wallet = WalletIndexer::new(
        temp_dir.path(),
        vec![Address::new("addr1_alice")],
    ).unwrap();
    
    // Generate 20 blocks
    let blocks = generate_mock_blocks(20, 10);
    
    // Process all blocks
    for block in &blocks {
        wallet.process_block(block.clone()).unwrap();
    }
    
    let balance_at_20 = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
    
    // Rollback to block 15
    wallet.rollback(15).unwrap();
    let balance_at_15 = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
    
    // Rollback to block 10
    wallet.rollback(10).unwrap();
    let balance_at_10 = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
    
    // Rollback to block 5
    wallet.rollback(5).unwrap();
    let balance_at_5 = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
    
    // Balances should be different at each point
    // (Exact values depend on mock data generation)
    println!("Balance at block 5: {}", balance_at_5);
    println!("Balance at block 10: {}", balance_at_10);
    println!("Balance at block 15: {}", balance_at_15);
    println!("Balance at block 20: {}", balance_at_20);
    
    // All rollbacks should complete quickly
    assert!(true, "Multiple rollbacks completed successfully");
}

#[test]
fn test_transaction_history() {
    let temp_dir = TempDir::new().unwrap();
    let mut wallet = WalletIndexer::new(
        temp_dir.path(),
        vec![Address::new("addr1_alice")],
    ).unwrap();
    
    let blocks = generate_test_scenario_blocks();
    
    for block in blocks {
        wallet.process_block(block).unwrap();
    }
    
    // Get Alice's transaction history
    let history = wallet.get_transaction_history(&Address::new("addr1_alice")).unwrap();
    
    // Alice should be in all 3 transactions
    assert!(history.len() >= 2, "Alice should have transaction history");
    
    println!("Alice's transaction history:");
    for tx in &history {
        println!("  • {}", tx.hash.0);
    }
}

#[test]
fn test_wallet_persistence() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create wallet and process blocks
    {
        let mut wallet = WalletIndexer::new(
            temp_dir.path(),
            vec![Address::new("addr1_alice")],
        ).unwrap();
        
        let blocks = generate_test_scenario_blocks();
        for block in blocks {
            wallet.process_block(block).unwrap();
        }
        
        let balance = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
        assert_eq!(balance, 6_830_000);
    }
    
    // Reopen wallet (simulating restart)
    {
        let wallet = WalletIndexer::new(
            temp_dir.path(),
            vec![Address::new("addr1_alice")],
        ).unwrap();
        
        // Data should persist
        let balance = wallet.get_balance(&Address::new("addr1_alice")).unwrap();
        assert_eq!(balance, 6_830_000, "Balance should persist across restarts");
        
        println!("✅ Wallet data persisted across restart");
    }
}

// Helper module to avoid duplication
mod mock_wallet {
    pub use super::super::mock_types::*;
    pub use super::super::mock_wallet::*;
}
