// Incremental Merkle Tree implementation
// Optimized for blockchain governance verification
// O(log n) insertions by only updating path from leaf to root

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodePath {
    pub level: u8,
    pub index: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hash(Vec<u8>);

impl Hash {
    pub fn new(bytes: Vec<u8>) -> Self {
        Hash(bytes)
    }
    
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    
    pub fn empty() -> Self {
        Hash(vec![0u8; 32])
    }
    
    pub fn hash_leaf(key: &[u8], value: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"leaf:");
        hasher.update(key);
        hasher.update(b":");
        hasher.update(value);
        Hash(hasher.finalize().as_bytes().to_vec())
    }
    
    pub fn hash_node(left: &Hash, right: &Hash) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"node:");
        hasher.update(left.as_bytes());
        hasher.update(b":");
        hasher.update(right.as_bytes());
        Hash(hasher.finalize().as_bytes().to_vec())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleRoot(pub Hash);

impl MerkleRoot {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerkleLeaf {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Direction {
    Left,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerkleProof {
    leaf: MerkleLeaf,
    siblings: Vec<(Direction, Hash)>,
}

impl MerkleProof {
    pub fn leaf(&self) -> &MerkleLeaf {
        &self.leaf
    }
    
    pub fn siblings(&self) -> &[(Direction, Hash)] {
        &self.siblings
    }
    
    pub fn directions(&self) -> Vec<Direction> {
        self.siblings.iter().map(|(d, _)| d.clone()).collect()
    }
}

#[derive(Clone, Debug)]
struct MerkleNode {
    hash: Hash,
}

pub struct IncrementalMerkleTree {
    /// Sparse representation - only store non-empty nodes
    nodes: HashMap<NodePath, MerkleNode>,
    
    /// Current root hash
    root: MerkleRoot,
    
    /// Tree height (max depth)
    height: u8,
    
    /// Number of leaves inserted
    leaf_count: usize,
    
    /// Mapping from key to leaf index (for proof generation)
    key_to_index: HashMap<Vec<u8>, u64>,
    
    /// Store actual leaf data for proof generation
    leaf_data: HashMap<Vec<u8>, Vec<u8>>,
}

impl IncrementalMerkleTree {
    pub fn new(height: u8) -> Self {
        Self {
            nodes: HashMap::new(),
            root: MerkleRoot(Hash::empty()),
            height,
            leaf_count: 0,
            key_to_index: HashMap::new(),
            leaf_data: HashMap::new(),
        }
    }
    
    /// Insert a new leaf and update only the path to root
    /// This is O(log n) instead of O(n)!
    pub fn insert(&mut self, key: &[u8], value: &[u8]) -> MerkleProof {
        let leaf_hash = Hash::hash_leaf(key, value);
        let leaf_index = self.leaf_count as u64;
        self.leaf_count += 1;
        
        // Store key -> index mapping for later proof generation
        self.key_to_index.insert(key.to_vec(), leaf_index);
        
        // Store leaf data for proof generation
        self.leaf_data.insert(key.to_vec(), value.to_vec());
        
        // Store the leaf node itself (at level 0)
        let leaf_path = NodePath { level: 0, index: leaf_index };
        self.nodes.insert(leaf_path, MerkleNode {
            hash: leaf_hash.clone(),
        });
        
        let mut current_hash = leaf_hash.clone();
        let mut current_index = leaf_index;
        let mut siblings = Vec::new();
        
        // Update path from leaf to root
        for level in 0..self.height {
            let sibling_index = current_index ^ 1; // Flip last bit
            
            // Get or compute sibling hash
            let sibling_path = NodePath { level, index: sibling_index };
            let sibling_hash = self.nodes
                .get(&sibling_path)
                .map(|n| n.hash.clone())
                .unwrap_or_else(Hash::empty);
            
            // Determine direction
            let direction = if current_index.is_multiple_of(2) {
                Direction::Right
            } else {
                Direction::Left
            };
            
            siblings.push((direction.clone(), sibling_hash.clone()));
            
            // Compute parent hash
            current_hash = if current_index.is_multiple_of(2) {
                Hash::hash_node(&current_hash, &sibling_hash)
            } else {
                Hash::hash_node(&sibling_hash, &current_hash)
            };
            
            // Store the parent node
            let parent_path = NodePath { 
                level: level + 1, 
                index: current_index / 2 
            };
            self.nodes.insert(parent_path, MerkleNode {
                hash: current_hash.clone(),
            });
            
            current_index /= 2;
        }
        
        self.root = MerkleRoot(current_hash);
        
        MerkleProof {
            leaf: MerkleLeaf { 
                key: key.to_vec(), 
                value: value.to_vec() 
            },
            siblings,
        }
    }
    
    /// Generate proof for a previously inserted key
    pub fn prove(&self, key: &[u8]) -> Option<MerkleProof> {
        let leaf_index = self.key_to_index.get(key)?;
        let value = self.leaf_data.get(key)?;
        
        let mut current_index = *leaf_index;
        let mut siblings = Vec::new();
        
        for level in 0..self.height {
            let sibling_index = current_index ^ 1;
            let sibling_path = NodePath { level, index: sibling_index };
            
            let sibling_hash = self.nodes
                .get(&sibling_path)
                .map(|n| n.hash.clone())
                .unwrap_or_else(Hash::empty);
            
            let direction = if current_index % 2 == 0 {
                Direction::Right
            } else {
                Direction::Left
            };
            
            siblings.push((direction, sibling_hash));
            current_index /= 2;
        }
        
        Some(MerkleProof {
            leaf: MerkleLeaf {
                key: key.to_vec(),
                value: value.clone(),
            },
            siblings,
        })
    }
    
    /// Get current root hash
    pub fn root(&self) -> &MerkleRoot {
        &self.root
    }
    
    /// Verify a proof is valid (static method - no tree needed)
    pub fn verify_proof(root: &MerkleRoot, key: &[u8], value: &[u8], proof: &MerkleProof) -> bool {
        // Recompute root from proof
        let mut current_hash = Hash::hash_leaf(key, value);
        
        for (direction, sibling) in &proof.siblings {
            current_hash = match direction {
                Direction::Left => Hash::hash_node(sibling, &current_hash),
                Direction::Right => Hash::hash_node(&current_hash, sibling),
            };
        }
        
        current_hash == root.0
    }
    
    /// Verify a proof against this tree's current root
    pub fn verify(&self, proof: &MerkleProof) -> crate::Result<()> {
        let is_valid = Self::verify_proof(
            &self.root,
            &proof.leaf.key,
            &proof.leaf.value,
            proof
        );
        
        if is_valid {
            Ok(())
        } else {
            Err(crate::Error::InvalidOperation("Merkle proof verification failed".to_string()))
        }
    }
    
    /// Compute difference between two trees
    pub fn diff(&self, other: &IncrementalMerkleTree) -> MerkleDiff {
        let mut different_nodes = Vec::new();
        
        // Compare roots first
        if self.root != other.root {
            // Trees are different, find which nodes differ
            for (path, node) in &self.nodes {
                if let Some(other_node) = other.nodes.get(path) {
                    if node.hash != other_node.hash {
                        different_nodes.push(path.clone());
                    }
                } else {
                    different_nodes.push(path.clone());
                }
            }
            
            // Check for nodes only in other tree
            for path in other.nodes.keys() {
                if !self.nodes.contains_key(path) {
                    different_nodes.push(path.clone());
                }
            }
        }
        
        MerkleDiff {
            different_nodes,
        }
    }
    
    /// Create a snapshot of the current tree state
    pub fn snapshot(&self) -> MerkleSnapshot {
        MerkleSnapshot {
            root: self.root.clone(),
            nodes: self.nodes.clone(),
            leaf_count: self.leaf_count,
            key_to_index: self.key_to_index.clone(),
            leaf_data: self.leaf_data.clone(),
            height: self.height,
        }
    }
    
    /// Rollback to a previous snapshot
    pub fn rollback(&mut self, snapshot: MerkleSnapshot) -> crate::Result<()> {
        self.root = snapshot.root;
        self.nodes = snapshot.nodes;
        self.leaf_count = snapshot.leaf_count;
        self.key_to_index = snapshot.key_to_index;
        self.leaf_data = snapshot.leaf_data;
        self.height = snapshot.height;
        Ok(())
    }
    
    /// Get number of leaves in the tree
    pub fn leaf_count(&self) -> usize {
        self.leaf_count
    }
    
    /// Get maximum number of leaves this tree can hold
    pub fn max_leaves(&self) -> u64 {
        1u64 << self.height
    }
    
    /// Get number of nodes stored (sparse representation)
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

#[derive(Clone)]
pub struct MerkleDiff {
    different_nodes: Vec<NodePath>,
}

impl MerkleDiff {
    pub fn is_empty(&self) -> bool {
        self.different_nodes.is_empty()
    }
}

#[derive(Clone)]
pub struct MerkleSnapshot {
    root: MerkleRoot,
    nodes: HashMap<NodePath, MerkleNode>,
    leaf_count: usize,
    key_to_index: HashMap<Vec<u8>, u64>,
    leaf_data: HashMap<Vec<u8>, Vec<u8>>,
    height: u8,
}

impl MerkleSnapshot {
    pub fn root(&self) -> &MerkleRoot {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_empty_tree() {
        let tree = IncrementalMerkleTree::new(8);
        assert_eq!(tree.leaf_count(), 0);
        assert_eq!(tree.max_leaves(), 256);
    }
    
    #[test]
    fn test_single_insert() {
        let mut tree = IncrementalMerkleTree::new(8);
        let proof = tree.insert(b"key1", b"value1");
        
        assert_eq!(tree.leaf_count(), 1);
        assert!(tree.verify(&proof).is_ok());
    }
    
    #[test]
    fn test_multiple_inserts() {
        let mut tree = IncrementalMerkleTree::new(8);
        
        let proof1 = tree.insert(b"key1", b"value1");
        let root1 = tree.root().clone();
        
        let proof2 = tree.insert(b"key2", b"value2");
        let root2 = tree.root().clone();
        
        let proof3 = tree.insert(b"key3", b"value3");
        let root3 = tree.root().clone();
        
        // Each proof should be valid against its respective root
        assert!(IncrementalMerkleTree::verify_proof(&root1, b"key1", b"value1", &proof1));
        assert!(IncrementalMerkleTree::verify_proof(&root2, b"key2", b"value2", &proof2));
        assert!(IncrementalMerkleTree::verify_proof(&root3, b"key3", b"value3", &proof3));
        
        // Latest proof should verify against current tree
        assert!(tree.verify(&proof3).is_ok());
    }
    
    #[test]
    fn test_proof_verification() {
        let mut tree = IncrementalMerkleTree::new(8);
        
        let proof = tree.insert(b"test_key", b"test_value");
        let root = tree.root();
        
        // Should verify correctly
        assert!(IncrementalMerkleTree::verify_proof(
            root,
            b"test_key",
            b"test_value",
            &proof
        ));
        
        // Should fail with wrong value
        assert!(!IncrementalMerkleTree::verify_proof(
            root,
            b"test_key",
            b"wrong_value",
            &proof
        ));
    }
    
    #[test]
    fn test_root_changes() {
        let mut tree = IncrementalMerkleTree::new(8);
        
        let root0 = tree.root().clone();
        tree.insert(b"key1", b"value1");
        let root1 = tree.root().clone();
        tree.insert(b"key2", b"value2");
        let root2 = tree.root().clone();
        
        assert_ne!(root0, root1);
        assert_ne!(root1, root2);
    }
    
    #[test]
    fn test_snapshot_and_rollback() {
        let mut tree = IncrementalMerkleTree::new(8);
        
        tree.insert(b"key1", b"value1");
        tree.insert(b"key2", b"value2");
        
        let snapshot = tree.snapshot();
        let root_snapshot = snapshot.root().clone();
        
        tree.insert(b"key3", b"value3");
        
        assert_ne!(tree.root(), &root_snapshot);
        
        tree.rollback(snapshot).unwrap();
        
        assert_eq!(tree.root(), &root_snapshot);
        assert_eq!(tree.leaf_count(), 2);
    }
    
    #[test]
    fn test_sparse_tree() {
        let mut tree = IncrementalMerkleTree::new(20); // 1M leaves possible
        
        // Insert sparse data
        for i in (0..1000).step_by(100) {
            tree.insert(format!("key_{}", i).as_bytes(), b"value");
        }
        
        // Should only have O(log n) nodes per insertion
        let expected_max_nodes = 10 * 20; // 10 insertions * 20 levels
        assert!(tree.node_count() < expected_max_nodes * 2);
    }
    
    #[test]
    fn test_deterministic_hashing() {
        let mut tree1 = IncrementalMerkleTree::new(8);
        let mut tree2 = IncrementalMerkleTree::new(8);
        
        for i in 0..10 {
            let key = format!("key_{}", i);
            let value = format!("value_{}", i);
            tree1.insert(key.as_bytes(), value.as_bytes());
            tree2.insert(key.as_bytes(), value.as_bytes());
        }
        
        assert_eq!(tree1.root(), tree2.root());
    }
}
