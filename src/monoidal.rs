// Monoidal values for efficient aggregation
// Enables range fold operations like "total balance across all addresses"

use crate::{LsmTree, LsmConfig, LsmSnapshot, Key, Value, Result};
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::marker::PhantomData;

/// Values that can be combined with an associative operation
pub trait Monoidal: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de> {
    /// Identity element (e.g., 0 for addition, empty set for union)
    fn mempty() -> Self;
    
    /// Associative binary operation (e.g., addition, union)
    /// Must satisfy: a.mappend(b).mappend(c) == a.mappend(b.mappend(c))
    fn mappend(&self, other: &Self) -> Self;
}

/// LSM tree with monoidal value support for efficient aggregation
pub struct MonoidalLsmTree<V: Monoidal> {
    tree: LsmTree,
    _phantom: PhantomData<V>,
}

impl<V: Monoidal> MonoidalLsmTree<V> {
    pub fn open(path: impl AsRef<Path>, config: LsmConfig) -> Result<Self> {
        let tree = LsmTree::open(path, config)?;
        Ok(Self {
            tree,
            _phantom: PhantomData,
        })
    }
    
    pub fn insert(&mut self, key: &Key, value: &V) -> Result<()> {
        let value_bytes = bincode::serialize(value)?;
        self.tree.insert(key, &Value::from(&value_bytes))
    }
    
    pub fn get(&self, key: &Key) -> Result<V> {
        match self.tree.get(key)? {
            Some(value_bytes) => {
                let value: V = bincode::deserialize(value_bytes.as_ref())?;
                Ok(value)
            }
            None => Ok(V::mempty()),
        }
    }
    
    pub fn delete(&mut self, key: &Key) -> Result<()> {
        self.tree.delete(key)
    }
    
    /// Fold over a range, combining values with mappend
    /// This is the key feature - efficient aggregation without materializing all values
    pub fn range_fold(&self, from: &Key, to: &Key) -> V {
        let mut accumulator = V::mempty();
        
        for (_key, value_bytes) in self.tree.range(from, to) {
            if let Ok(value) = bincode::deserialize::<V>(value_bytes.as_ref()) {
                accumulator = accumulator.mappend(&value);
            }
        }
        
        accumulator
    }
    
    /// Fold over all keys with a given prefix
    pub fn prefix_fold(&self, prefix: &[u8]) -> V {
        let mut accumulator = V::mempty();
        
        for (_key, value_bytes) in self.tree.scan_prefix(prefix) {
            if let Ok(value) = bincode::deserialize::<V>(value_bytes.as_ref()) {
                accumulator = accumulator.mappend(&value);
            }
        }
        
        accumulator
    }
    
    pub fn snapshot(&self) -> MonoidalSnapshot<V> {
        MonoidalSnapshot {
            inner: self.tree.snapshot(),
            _phantom: PhantomData,
        }
    }
    
    pub fn rollback(&mut self, snapshot: MonoidalSnapshot<V>) -> Result<()> {
        self.tree.rollback(snapshot.inner)
    }
    
    pub fn compact(&mut self) -> Result<()> {
        self.tree.compact()
    }
}

pub struct MonoidalSnapshot<V: Monoidal> {
    inner: LsmSnapshot,
    _phantom: PhantomData<V>,
}

impl<V: Monoidal> MonoidalSnapshot<V> {
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
            let entry = result.entry(k.clone()).or_insert_with(V::default);
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
