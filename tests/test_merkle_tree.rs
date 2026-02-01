/// Tests for incremental Merkle trees
/// Critical for governance action verification in Cardano
use cardano_lsm::{IncrementalMerkleTree, MerkleProof};

#[test]
fn test_empty_tree_root() {
    let tree = IncrementalMerkleTree::new(8); // height 8 = 256 leaves max
    
    let root = tree.root();
    
    // Empty tree should have deterministic root
    assert!(!root.as_bytes().is_empty());
}

#[test]
fn test_insert_single_leaf() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    let key = b"governance_action_1";
    let value = b"proposal_data";
    
    let proof = tree.insert(key, value);
    
    // Proof should be valid
    assert!(tree.verify(&proof).is_ok());
    
    // Proof should contain correct leaf
    assert_eq!(proof.leaf().key, key);
    assert_eq!(proof.leaf().value, value);
}

#[test]
fn test_insert_multiple_leaves() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    let mut proofs = Vec::new();
    let mut roots = Vec::new();
    
    for i in 0..10 {
        let key = format!("action_{}", i);
        let value = format!("data_{}", i);
        
        let proof = tree.insert(key.as_bytes(), value.as_bytes());
        proofs.push((key, value, proof));
        roots.push(tree.root().clone());
    }
    
    // Each proof should be valid against its respective root
    for (i, (key, value, proof)) in proofs.iter().enumerate() {
        let root = &roots[i];
        let is_valid = IncrementalMerkleTree::verify_proof(
            root,
            key.as_bytes(),
            value.as_bytes(),
            proof
        );
        assert!(is_valid, "Proof {} should be valid against its root", i);
    }
}

#[test]
fn test_proof_verification() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    let key = b"test_key";
    let value = b"test_value";
    
    let proof = tree.insert(key, value);
    let root = tree.root();
    
    // Static verification without tree
    let is_valid = IncrementalMerkleTree::verify_proof(&root, key, value, &proof);
    assert!(is_valid, "Proof should be valid");
}

#[test]
fn test_proof_invalid_for_wrong_data() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    let key = b"test_key";
    let value = b"test_value";
    let wrong_value = b"wrong_value";
    
    let proof = tree.insert(key, value);
    let root = tree.root();
    
    // Proof should fail with wrong value
    let is_valid = IncrementalMerkleTree::verify_proof(&root, key, wrong_value, &proof);
    assert!(!is_valid, "Proof should be invalid for wrong data");
}

#[test]
fn test_root_changes_on_insert() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    let root_0 = tree.root().clone();
    
    tree.insert(b"key1", b"value1");
    let root_1 = tree.root().clone();
    
    tree.insert(b"key2", b"value2");
    let root_2 = tree.root().clone();
    
    // Each insert should change the root
    assert_ne!(root_0, root_1);
    assert_ne!(root_1, root_2);
    assert_ne!(root_0, root_2);
}

#[test]
fn test_incremental_insertion_is_efficient() {
    use std::time::Instant;
    
    let mut tree = IncrementalMerkleTree::new(16); // 65536 leaves max
    
    // Insert many leaves and measure time
    let start = Instant::now();
    
    for i in 0..1000 {
        let key = format!("key_{}", i);
        let value = format!("value_{}", i);
        tree.insert(key.as_bytes(), value.as_bytes());
    }
    
    let duration = start.elapsed();
    let avg_per_insert = duration.as_micros() / 1000;
    
    // Each insert should be fast (< 100 microseconds)
    assert!(avg_per_insert < 100, "Avg insert time: {} μs", avg_per_insert);
}

#[test]
fn test_proof_size_is_logarithmic() {
    let mut tree = IncrementalMerkleTree::new(16);
    
    // Insert many leaves
    for i in 0..1000 {
        tree.insert(format!("key_{}", i).as_bytes(), b"value");
    }
    
    let proof = tree.prove(b"key_500").unwrap();
    
    // Proof size should be O(log n), roughly 16 siblings for height 16
    let sibling_count = proof.siblings().len();
    assert!(sibling_count <= 16, "Proof should have ≤ height siblings: {}", sibling_count);
}

#[test]
fn test_merkle_diff_between_trees() {
    let mut tree1 = IncrementalMerkleTree::new(8);
    let mut tree2 = IncrementalMerkleTree::new(8);
    
    // Both insert common keys
    for i in 0..10 {
        let key = format!("common_{}", i);
        tree1.insert(key.as_bytes(), b"value");
        tree2.insert(key.as_bytes(), b"value");
    }
    
    // tree1 has additional keys
    for i in 0..5 {
        let key = format!("tree1_only_{}", i);
        tree1.insert(key.as_bytes(), b"value");
    }
    
    // tree2 has different additional keys
    for i in 0..5 {
        let key = format!("tree2_only_{}", i);
        tree2.insert(key.as_bytes(), b"value");
    }
    
    let diff = tree1.diff(&tree2);
    
    // Diff should identify differences
    assert!(!diff.is_empty());
}

#[test]
fn test_snapshot_merkle_tree() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    // Insert data
    for i in 0..20 {
        tree.insert(format!("key_{}", i).as_bytes(), b"value");
    }
    
    let snapshot = tree.snapshot();
    let root_before = tree.root().clone();
    
    // Insert more data
    for i in 20..40 {
        tree.insert(format!("key_{}", i).as_bytes(), b"value");
    }
    
    let root_after = tree.root().clone();
    
    // Roots should be different
    assert_ne!(root_before, root_after);
    
    // Snapshot should have original root
    assert_eq!(snapshot.root(), &root_before);
}

#[test]
fn test_rollback_merkle_tree() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    // Insert data
    for i in 0..20 {
        tree.insert(format!("key_{}", i).as_bytes(), b"value");
    }
    
    let snapshot = tree.snapshot();
    let root_snapshot = snapshot.root().clone();
    
    // Insert more data
    for i in 20..40 {
        tree.insert(format!("key_{}", i).as_bytes(), b"value");
    }
    
    // Rollback
    tree.rollback(snapshot).unwrap();
    
    // Root should match snapshot
    assert_eq!(tree.root(), &root_snapshot);
    
    // Should have 20 leaves
    assert_eq!(tree.leaf_count(), 20);
}

#[test]
fn test_merkle_proof_for_governance_action() {
    let mut tree = IncrementalMerkleTree::new(16);
    
    // Simulate governance actions
    let actions: Vec<(&[u8], &[u8])> = vec![
        (b"action_param_change_1", b"increase_k_parameter"),
        (b"action_hard_fork_2", b"protocol_version_9"),
        (b"action_treasury_3", b"withdraw_100k_ada"),
        (b"action_no_confidence_4", b"committee_vote"),
        (b"action_committee_5", b"add_member_xyz"),
    ];
    
    // Insert all actions
    for (key, value) in &actions {
        tree.insert(*key, *value);
    }
    
    let root = tree.root();
    
    // Generate and verify proofs using prove()
    for (i, (key, value)) in actions.iter().enumerate() {
        let proof = tree.prove(key).expect(&format!("Should generate proof for action {}", i));
        
        // Debug: check the proof
        eprintln!("Action {}: key={:?}, value={:?}", i, 
                  std::str::from_utf8(key).unwrap_or("binary"),
                  std::str::from_utf8(value).unwrap_or("binary"));
        eprintln!("  Proof leaf key: {:?}", std::str::from_utf8(&proof.leaf().key).unwrap_or("binary"));
        eprintln!("  Proof leaf value: {:?}", std::str::from_utf8(&proof.leaf().value).unwrap_or("binary"));
        eprintln!("  Siblings count: {}", proof.siblings().len());
        
        let is_valid = IncrementalMerkleTree::verify_proof(root, key, value, &proof);
        eprintln!("  Valid: {}", is_valid);
        
        assert!(is_valid, "Governance action {} should be verifiable", i);
    }
}

#[test]
fn test_concurrent_proof_generation() {
    use std::sync::Arc;
    use std::thread;
    
    let mut tree = IncrementalMerkleTree::new(12);
    
    // Insert data
    for i in 0..100 {
        tree.insert(format!("key_{}", i).as_bytes(), b"value");
    }
    
    let tree = Arc::new(tree);
    let mut handles = vec![];
    
    // Generate proofs concurrently
    for i in 0..10 {
        let tree_clone = Arc::clone(&tree);
        let handle = thread::spawn(move || {
            let key = format!("key_{}", i * 10);
            tree_clone.prove(key.as_bytes())
        });
        handles.push(handle);
    }
    
    // All should succeed
    for handle in handles {
        let proof = handle.join().unwrap();
        assert!(proof.is_some());
    }
}

#[test]
fn test_sparse_merkle_tree_efficiency() {
    let mut tree = IncrementalMerkleTree::new(20); // 1M leaves possible
    
    // Insert sparse data (only 100 out of 1M possible)
    for i in (0..100000).step_by(1000) {
        tree.insert(format!("key_{}", i).as_bytes(), b"value");
    }
    
    // Tree should not allocate 1M nodes
    let node_count = tree.node_count();
    assert!(node_count < 10000, "Sparse tree should have few nodes: {}", node_count);
}

#[test]
fn test_deterministic_hashing() {
    let mut tree1 = IncrementalMerkleTree::new(8);
    let mut tree2 = IncrementalMerkleTree::new(8);
    
    // Insert same data in same order
    for i in 0..50 {
        let key = format!("key_{}", i);
        let value = format!("value_{}", i);
        tree1.insert(key.as_bytes(), value.as_bytes());
        tree2.insert(key.as_bytes(), value.as_bytes());
    }
    
    // Roots should be identical
    assert_eq!(tree1.root(), tree2.root());
}

#[test]
fn test_insertion_order_matters() {
    let mut tree1 = IncrementalMerkleTree::new(8);
    let mut tree2 = IncrementalMerkleTree::new(8);
    
    // Insert in different orders
    tree1.insert(b"key_a", b"value_a");
    tree1.insert(b"key_b", b"value_b");
    
    tree2.insert(b"key_b", b"value_b");
    tree2.insert(b"key_a", b"value_a");
    
    // Roots should be different (order matters)
    assert_ne!(tree1.root(), tree2.root());
}

#[test]
fn test_proof_contains_full_path() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    // Insert enough to have multiple levels
    for i in 0..16 {
        tree.insert(format!("key_{}", i).as_bytes(), b"value");
    }
    
    let proof = tree.prove(b"key_5").unwrap();
    
    // Proof should have siblings at each level
    let siblings = proof.siblings();
    assert!(!siblings.is_empty(), "Proof should have sibling hashes");
    
    // Each sibling should have direction (left or right)
    assert_eq!(siblings.len(), proof.directions().len());
}

#[test]
fn test_governance_action_history_verification() {
    let mut tree = IncrementalMerkleTree::new(16);
    
    // Simulate sequential governance actions over time
    let mut action_history = Vec::new();
    
    for epoch in 0..10 {
        for action in 0..10 {
            let key = format!("epoch_{}_action_{}", epoch, action);
            let value = format!("governance_data");
            
            let proof = tree.insert(key.as_bytes(), value.as_bytes());
            action_history.push((key, value, proof, tree.root().clone()));
        }
    }
    
    // Verify each action against its epoch's root
    for (key, value, proof, root) in action_history {
        let is_valid = IncrementalMerkleTree::verify_proof(&root, key.as_bytes(), value.as_bytes(), &proof);
        assert!(is_valid, "Historical governance action should be verifiable");
    }
}

#[test]
fn test_merkle_tree_height_limit() {
    let tree = IncrementalMerkleTree::new(32); // height 32 = 4B leaves
    
    // Should allow up to 2^32 leaves (4 billion)
    assert_eq!(tree.max_leaves(), 1u64 << 32);
}

#[test]
fn test_empty_key_and_value() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    let proof = tree.insert(b"", b"");
    
    // Should handle empty key/value
    assert!(tree.verify(&proof).is_ok());
}

#[test]
fn test_large_values_in_merkle_tree() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    // Insert large governance proposal data
    let large_value = vec![b'x'; 100_000]; // 100KB proposal
    
    let proof = tree.insert(b"large_proposal", &large_value);
    
    // Should handle large values efficiently
    assert!(tree.verify(&proof).is_ok());
}

#[test]
fn test_proof_serialization() {
    let mut tree = IncrementalMerkleTree::new(8);
    
    let key = b"action_1";
    let value = b"data_1";
    
    let proof = tree.insert(key, value);
    
    // Serialize proof
    let serialized = bincode::serialize(&proof).unwrap();
    
    // Deserialize
    let deserialized: MerkleProof = bincode::deserialize(&serialized).unwrap();
    
    // Should still be valid
    assert!(tree.verify(&deserialized).is_ok());
}
