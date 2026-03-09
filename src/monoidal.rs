//! Monoidal values for efficient range aggregation
//!
//! This module provides support for values that can be combined using an associative operation,
//! enabling efficient aggregation queries like "total balance across all UTxOs" without
//! materializing every value.
//!
//! # Example
//!
//! ```rust
//! use cardano_lsm::{MonoidalLsmTree, LsmConfig, Key};
//! # use tempfile::TempDir;
//!
//! # fn main() -> cardano_lsm::Result<()> {
//! # let temp = TempDir::new()?;
//! // Create a monoidal tree for u64 balances
//! let mut tree = MonoidalLsmTree::<u64>::open(temp.path(), LsmConfig::default())?;
//!
//! // Insert individual balances
//! tree.insert(&Key::from(b"alice"), &100)?;
//! tree.insert(&Key::from(b"bob"), &200)?;
//! tree.insert(&Key::from(b"charlie"), &50)?;
//!
//! // Efficiently compute total balance
//! let total = tree.range_fold(&Key::from(b"a"), &Key::from(b"z"));
//! assert_eq!(total, 350);
//! # Ok(())
//! # }
//! ```

use crate::{LsmTree, LsmConfig, LsmSnapshot, Key, Value, Result};
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::marker::PhantomData;

/// Values that can be combined with an associative operation (monoid)
///
/// A monoid consists of:
/// - An identity element (`mempty`)
/// - An associative binary operation (`mappend`)
///
/// Monoids enable efficient range aggregation in the LSM tree, such as
/// summing balances or concatenating logs.
///
/// # Laws
///
/// Implementations must satisfy these laws:
/// - **Identity**: `x.mappend(&mempty()) == x` and `mempty().mappend(&x) == x`
/// - **Associativity**: `a.mappend(&b).mappend(&c) == a.mappend(&b.mappend(&c))`
///
/// # Example
///
/// ```rust
/// use cardano_lsm::Monoidal;
///
/// // u64 forms a monoid under addition
/// let x = 10u64;
/// let y = 20u64;
/// let z = x.mappend(&y);
/// assert_eq!(z, 30);
/// ```
pub trait Monoidal: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de> {
    /// Identity element (e.g., 0 for addition, empty set for union)
    ///
    /// The identity element must satisfy: `x.mappend(&mempty()) == x`
    fn mempty() -> Self;

    /// Associative binary operation (e.g., addition, union)
    ///
    /// Must satisfy associativity: `a.mappend(b).mappend(c) == a.mappend(b.mappend(c))`
    fn mappend(&self, other: &Self) -> Self;
}

/// LSM tree with monoidal value support for efficient aggregation
///
/// Wraps an [`LsmTree`] to provide range fold operations that efficiently
/// aggregate values without materializing every entry. Useful for queries like:
/// - Total UTxO balance across addresses
/// - Concatenating event logs
/// - Merging multi-asset balances
///
/// Values are serialized using bincode and must implement the [`Monoidal`] trait.
pub struct MonoidalLsmTree<V: Monoidal> {
    tree: LsmTree,
    _phantom: PhantomData<V>,
}

impl<V: Monoidal> MonoidalLsmTree<V> {
    /// Opens or creates a monoidal LSM tree at the specified path
    ///
    /// # Arguments
    ///
    /// * `path` - Directory path where the database will be stored
    /// * `config` - Configuration for the underlying LSM tree
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be accessed or created.
    pub fn open(path: impl AsRef<Path>, config: LsmConfig) -> Result<Self> {
        let tree = LsmTree::open(path, config)?;
        Ok(Self {
            tree,
            _phantom: PhantomData,
        })
    }

    /// Inserts a key-value pair into the tree
    ///
    /// The value is serialized using bincode and stored in the underlying LSM tree.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to insert
    /// * `value` - The monoidal value to store
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or I/O fails.
    pub fn insert(&mut self, key: &Key, value: &V) -> Result<()> {
        let value_bytes = bincode::serialize(value)?;
        self.tree.insert(key, &Value::from(&value_bytes))
    }

    /// Retrieves a value by key
    ///
    /// If the key is not found, returns the identity element (`mempty`).
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// The monoidal value associated with the key, or `V::mempty()` if not found.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization or I/O fails.
    pub fn get(&self, key: &Key) -> Result<V> {
        match self.tree.get(key)? {
            Some(value_bytes) => {
                let value: V = bincode::deserialize(value_bytes.as_ref())?;
                Ok(value)
            }
            None => Ok(V::mempty()),
        }
    }

    /// Deletes a key from the tree
    ///
    /// Inserts a tombstone marker that will be removed during compaction.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to delete
    ///
    /// # Errors
    ///
    /// Returns an error if I/O fails.
    pub fn delete(&mut self, key: &Key) -> Result<()> {
        self.tree.delete(key)
    }

    /// Folds over a range of keys, combining values with `mappend`
    ///
    /// This is the key feature of monoidal trees - efficient aggregation
    /// without materializing all values. The values are combined incrementally
    /// as they are read.
    ///
    /// # Arguments
    ///
    /// * `from` - Start of the key range (inclusive)
    /// * `to` - End of the key range (inclusive)
    ///
    /// # Returns
    ///
    /// The aggregated result of combining all values in the range using `mappend`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use cardano_lsm::{MonoidalLsmTree, LsmConfig, Key};
    /// # use tempfile::TempDir;
    /// # fn main() -> cardano_lsm::Result<()> {
    /// # let temp = TempDir::new()?;
    /// let mut tree = MonoidalLsmTree::<u64>::open(temp.path(), LsmConfig::default())?;
    /// tree.insert(&Key::from(b"balance_001"), &100)?;
    /// tree.insert(&Key::from(b"balance_002"), &200)?;
    ///
    /// let total = tree.range_fold(&Key::from(b"balance_000"), &Key::from(b"balance_999"));
    /// assert_eq!(total, 300);
    /// # Ok(())
    /// # }
    /// ```
    pub fn range_fold(&self, from: &Key, to: &Key) -> V {
        let mut accumulator = V::mempty();

        for (_key, value_bytes) in self.tree.range(from, to) {
            if let Ok(value) = bincode::deserialize::<V>(value_bytes.as_ref()) {
                accumulator = accumulator.mappend(&value);
            }
        }

        accumulator
    }

    /// Folds over all keys with a given prefix
    ///
    /// Similar to [`range_fold`](Self::range_fold), but filters keys by prefix.
    /// Useful for aggregating all values under a common key prefix.
    ///
    /// # Arguments
    ///
    /// * `prefix` - The key prefix to match
    ///
    /// # Returns
    ///
    /// The aggregated result of combining all matching values using `mappend`.
    pub fn prefix_fold(&self, prefix: &[u8]) -> V {
        let mut accumulator = V::mempty();

        for (_key, value_bytes) in self.tree.scan_prefix(prefix) {
            if let Ok(value) = bincode::deserialize::<V>(value_bytes.as_ref()) {
                accumulator = accumulator.mappend(&value);
            }
        }

        accumulator
    }

    /// Creates an immutable snapshot of the current tree state
    ///
    /// Snapshots capture the state of the tree at a point in time and can be
    /// used for rollback or historical queries.
    ///
    /// # Returns
    ///
    /// A [`MonoidalSnapshot`] that can query the tree's state at this moment.
    pub fn snapshot(&self) -> MonoidalSnapshot<V> {
        MonoidalSnapshot {
            inner: self.tree.snapshot(),
            _phantom: PhantomData,
        }
    }

    /// Rolls back the tree to a previous snapshot
    ///
    /// Discards all changes made after the snapshot was created.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - The snapshot to roll back to
    ///
    /// # Errors
    ///
    /// Returns an error if the rollback operation fails.
    pub fn rollback(&mut self, snapshot: MonoidalSnapshot<V>) -> Result<()> {
        self.tree.rollback(snapshot.inner)
    }

    /// Triggers compaction of the underlying LSM tree
    ///
    /// Merges SSTables to reduce space amplification and improve read performance.
    ///
    /// # Errors
    ///
    /// Returns an error if compaction fails.
    pub fn compact(&mut self) -> Result<()> {
        self.tree.compact()
    }
}

/// Immutable snapshot of a [`MonoidalLsmTree`]
///
/// Captures the state of the tree at a specific point in time.
/// Can be used for historical queries or rollback operations.
pub struct MonoidalSnapshot<V: Monoidal> {
    inner: LsmSnapshot,
    _phantom: PhantomData<V>,
}

impl<V: Monoidal> MonoidalSnapshot<V> {
    /// Folds over a range of keys in the snapshot
    ///
    /// Combines values within the specified range using `mappend`.
    ///
    /// # Arguments
    ///
    /// * `from` - Start of the key range (inclusive)
    /// * `to` - End of the key range (inclusive)
    ///
    /// # Returns
    ///
    /// The aggregated result of combining all values in the range.
    pub fn range_fold(&self, from: &Key, to: &Key) -> V {
        let mut accumulator = V::mempty();
        
        // Use the snapshot's iter to get all entries
        for (key, value_bytes) in self.inner.iter() {
            if &key >= from && &key <= to {
                if let Ok(value) = bincode::deserialize::<V>(value_bytes.as_ref()) {
                    accumulator = accumulator.mappend(&value);
                }
            }
        }
        
        accumulator
    }
}

// Built-in monoidal instances

/// u64 with addition
impl Monoidal for u64 {
    fn mempty() -> Self {
        0
    }
    
    fn mappend(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

/// i64 with addition
impl Monoidal for i64 {
    fn mempty() -> Self {
        0
    }
    
    fn mappend(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

/// Vec with concatenation
impl<T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>> Monoidal for Vec<T> {
    fn mempty() -> Self {
        Vec::new()
    }
    
    fn mappend(&self, other: &Self) -> Self {
        let mut result = self.clone();
        result.extend_from_slice(other);
        result
    }
}

/// HashMap with value addition (for multi-asset balances)
impl<K, V> Monoidal for std::collections::HashMap<K, V>
where
    K: Clone + Eq + std::hash::Hash + Send + Sync + Serialize + for<'de> Deserialize<'de>,
    V: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de> + std::ops::AddAssign + Default,
{
    fn mempty() -> Self {
        std::collections::HashMap::new()
    }
    
    fn mappend(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for (k, v) in other {
            let entry = result.entry(k.clone()).or_default();
            *entry += v.clone();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_u64_monoidal_laws() {
        // Identity
        assert_eq!(0u64.mappend(&42), 42);
        assert_eq!(42u64.mappend(&0), 42);
        
        // Associativity
        let a = 10u64;
        let b = 20u64;
        let c = 30u64;
        assert_eq!(a.mappend(&b).mappend(&c), a.mappend(&b.mappend(&c)));
    }
    
    #[test]
    fn test_monoidal_lsm_basic() {
        let temp = TempDir::new().unwrap();
        let mut tree = MonoidalLsmTree::<u64>::open(temp.path(), LsmConfig::default()).unwrap();
        
        tree.insert(&Key::from(b"balance1"), &100).unwrap();
        tree.insert(&Key::from(b"balance2"), &200).unwrap();
        
        assert_eq!(tree.get(&Key::from(b"balance1")).unwrap(), 100);
        assert_eq!(tree.get(&Key::from(b"balance2")).unwrap(), 200);
    }
    
    #[test]
    fn test_range_fold() {
        let temp = TempDir::new().unwrap();
        let mut tree = MonoidalLsmTree::<u64>::open(temp.path(), LsmConfig::default()).unwrap();
        
        tree.insert(&Key::from(b"addr_a"), &100).unwrap();
        tree.insert(&Key::from(b"addr_b"), &200).unwrap();
        tree.insert(&Key::from(b"addr_c"), &300).unwrap();
        tree.insert(&Key::from(b"addr_d"), &400).unwrap();
        
        let total = tree.range_fold(&Key::from(b"addr_b"), &Key::from(b"addr_c"));
        assert_eq!(total, 500); // 200 + 300
    }
}
