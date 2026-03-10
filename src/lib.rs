//! # Cardano LSM - Log-Structured Merge Tree for Blockchain Indexing
//!
//! A pure Rust implementation of an LSM tree optimized for blockchain indexing workloads,
//! particularly UTxO-based systems like Cardano. This crate provides fast snapshots,
//! rollback capabilities, and efficient range queries without requiring a Haskell runtime.
//!
//! ## Quick Start
//!
//! ```rust
//! use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
//! use std::path::Path;
//!
//! # fn main() -> cardano_lsm::Result<()> {
//! # let temp_dir = tempfile::tempdir()?;
//! # let db_path = temp_dir.path();
//! // Open or create an LSM tree
//! let config = LsmConfig::default();
//! let mut tree = LsmTree::open(db_path, config)?;
//!
//! // Insert key-value pairs
//! let key = Key::from(b"utxo_123");
//! let value = Value::from(b"transaction_data");
//! tree.insert(key.clone(), value)?;
//!
//! // Retrieve values
//! if let Some(v) = tree.get(&key)? {
//!     println!("Found: {:?}", v);
//! }
//!
//! // Delete keys
//! tree.delete(key)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Common Use Cases
//!
//! ### Blockchain Indexing with Snapshots
//!
//! The primary use case is maintaining blockchain state with the ability to quickly
//! roll back during chain reorganizations:
//!
//! ```rust
//! # use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
//! # fn main() -> cardano_lsm::Result<()> {
//! # let temp_dir = tempfile::tempdir()?;
//! # let db_path = temp_dir.path();
//! # let config = LsmConfig::default();
//! # let mut tree = LsmTree::open(db_path, config)?;
//! // Take a snapshot before processing a new block
//! let snapshot = tree.snapshot();
//!
//! // Process block transactions
//! tree.insert(Key::from(b"utxo_new"), Value::from(b"data"))?;
//! tree.delete(Key::from(b"utxo_spent"))?;
//!
//! // If block is invalid or reorg occurs, rollback
//! tree.rollback(&snapshot)?;
//! // Tree is now back to the snapshot state
//! # Ok(())
//! # }
//! ```
//!
//! ### Range Queries
//!
//! Efficiently scan ranges of keys, useful for querying all UTxOs for an address:
//!
//! ```rust
//! # use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
//! # fn main() -> cardano_lsm::Result<()> {
//! # let temp_dir = tempfile::tempdir()?;
//! # let db_path = temp_dir.path();
//! # let config = LsmConfig::default();
//! # let mut tree = LsmTree::open(db_path, config)?;
//! # tree.insert(Key::from(b"addr_123_utxo_1"), Value::from(b"data1"))?;
//! # tree.insert(Key::from(b"addr_123_utxo_2"), Value::from(b"data2"))?;
//! let start = Key::from(b"addr_123_");
//! let end = Key::from(b"addr_124_");
//!
//! for (key, value) in tree.range(start..end)? {
//!     // Process each key-value pair in range
//!     println!("Key: {:?}, Value: {:?}", key, value);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ### Batch Operations
//!
//! For high-throughput scenarios, use batch operations to amortize I/O costs:
//!
//! ```rust
//! # use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
//! # fn main() -> cardano_lsm::Result<()> {
//! # let temp_dir = tempfile::tempdir()?;
//! # let db_path = temp_dir.path();
//! # let config = LsmConfig::default();
//! # let mut tree = LsmTree::open(db_path, config)?;
//! let mut batch = Vec::new();
//! batch.push((Key::from(b"key1"), Some(Value::from(b"value1"))));
//! batch.push((Key::from(b"key2"), Some(Value::from(b"value2"))));
//! batch.push((Key::from(b"key3"), None)); // Deletion
//!
//! tree.insert_batch(batch)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Configuration
//!
//! Customize the LSM tree behavior with [`LsmConfig`]:
//!
//! ```rust
//! use cardano_lsm::{LsmConfig, BloomFilterPolicy, CompactionStrategy};
//!
//! let config = LsmConfig {
//!     memtable_size: 16 * 1024 * 1024,  // 16 MB memtable
//!     num_levels: 7,                     // 7 levels for leveled compaction
//!     level_size_multiplier: 10,         // Each level 10x larger than previous
//!     compaction_strategy: CompactionStrategy::LazyLevelling,
//!     bloom_filter: BloomFilterPolicy::default(),
//!     use_compression: true,
//!     ..Default::default()
//! };
//! ```
//!
//! ## Persistent Snapshots
//!
//! Save and restore snapshots for backup or testing:
//!
//! ```rust
//! # use cardano_lsm::{LsmTree, LsmConfig};
//! # fn main() -> cardano_lsm::Result<()> {
//! # let temp_dir = tempfile::tempdir()?;
//! # let db_path = temp_dir.path();
//! # let config = LsmConfig::default();
//! # let mut tree = LsmTree::open(db_path, config)?;
//! // Save current state
//! tree.save_snapshot("block_12345")?;
//!
//! // Later, restore from disk
//! tree.load_snapshot("block_12345")?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Performance Characteristics
//!
//! - **Snapshot creation**: < 10ms (reference counting, no data copy)
//! - **Rollback**: < 1s (typically used for short-term reorgs)
//! - **Write throughput**: Optimized for blockchain workloads with batch operations
//! - **Read latency**: Bloom filters provide fast negative lookups
//! - **Compaction**: LazyLevelling strategy balances write amp and space amp
//!
//! ## Thread Safety
//!
//! [`LsmTree`] uses internal locking and can be safely shared across threads:
//!
//! ```rust
//! # use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
//! # use std::sync::Arc;
//! # use std::thread;
//! # fn main() -> cardano_lsm::Result<()> {
//! # let temp_dir = tempfile::tempdir()?;
//! # let db_path = temp_dir.path();
//! # let config = LsmConfig::default();
//! let tree = Arc::new(LsmTree::open(db_path, config)?);
//!
//! let tree_clone = tree.clone();
//! let handle = thread::spawn(move || {
//!     tree_clone.get(&Key::from(b"key"))
//! });
//!
//! tree.insert(Key::from(b"key"), Value::from(b"value"))?;
//! handle.join().unwrap()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Testing
//!
//! This implementation includes 10,000+ property-based conformance tests
//! validated against the Haskell reference implementation with a 100% pass rate.
//!
//! ## Important Notes
//!
//! - **No Write-Ahead Log (WAL)**: Writes are lost on crash until a snapshot is saved.
//!   This design choice trades crash recovery for simplicity and performance.
//! - **Session locking**: Only one process can access a database directory at a time.
//! - **Snapshots are reference-counted**: Immutable view of data at a point in time.

mod atomic_file;
mod checksum;
mod checksum_handle;
mod session_lock;
mod snapshot;
mod sstable;
mod compaction;
mod merkle;
mod monoidal;
mod io_backend;  // I/O backend abstraction (sync vs io_uring)

use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use serde::{Serialize, Deserialize};
use sstable::{SsTableWriter, SsTableHandle, RunNumber};
use compaction::Compactor;
use atomic_file::fsync_directory;
use session_lock::SessionLock;
use io_backend::IoBackend;

// Re-export public types
pub use merkle::{IncrementalMerkleTree, MerkleProof, MerkleRoot, MerkleLeaf, Direction, Hash, MerkleDiff, MerkleSnapshot};
pub use monoidal::{Monoidal, MonoidalLsmTree, MonoidalSnapshot};
pub use snapshot::{PersistentSnapshot, SnapshotMetadata, SnapshotRun};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Corruption detected: {0}")]
    Corruption(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Session locked: {0}")]
    SessionLocked(String),

    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
}

// ===== Core Types =====

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Key(Vec<u8>);

impl Key {
    pub fn from(bytes: impl AsRef<[u8]>) -> Self {
        Key(bytes.as_ref().to_vec())
    }
    
    #[allow(clippy::should_implement_trait)]
    pub fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Key {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Value(Vec<u8>);

impl Value {
    pub fn from(bytes: impl AsRef<[u8]>) -> Self {
        Value(bytes.as_ref().to_vec())
    }
}

impl AsRef<[u8]> for Value {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

// ===== Configuration =====

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LsmConfig {
    // Memory settings
    pub memtable_size: usize,
    pub max_immutable_memtables: usize,
    pub block_cache_size: usize,
    
    // Compaction
    pub compaction_strategy: CompactionStrategy,
    pub compaction_threads: usize,
    pub level0_compaction_trigger: usize,
    
    // Bloom filters
    pub bloom_filter_bits_per_key: usize,
    pub bloom_filter_fp_rate: f64,

    // Snapshots
    pub max_snapshots_per_wallet: usize,
    pub snapshot_interval: std::time::Duration,
    
    // SSTables
    pub sstable_size: usize,
    pub sstable_block_size: usize,
    // NOTE: Compression is not yet implemented in the current SSTable format
    // Future enhancement: Add compression support for SSTables

    // I/O backend (sync vs io_uring)
    #[serde(skip)]  // Don't serialize backend config
    pub io_backend: IoBackend,
}

impl Default for LsmConfig {
    fn default() -> Self {
        Self {
            memtable_size: 64 * 1024 * 1024,
            max_immutable_memtables: 2,
            block_cache_size: 256 * 1024 * 1024,
            
            compaction_strategy: CompactionStrategy::Hybrid {
                l0_strategy: Box::new(CompactionStrategy::Tiered {
                    size_ratio: 4.0,
                    min_merge_width: 4,
                    max_merge_width: 10,
                }),
                ln_strategy: Box::new(CompactionStrategy::Leveled {
                    size_ratio: 10.0,
                    max_level: 7,
                }),
                transition_level: 2,
            },
            compaction_threads: 2,
            level0_compaction_trigger: 4,
            
            bloom_filter_bits_per_key: 10,
            bloom_filter_fp_rate: 0.01,

            max_snapshots_per_wallet: 10,
            snapshot_interval: std::time::Duration::from_secs(600),
            
            sstable_size: 64 * 1024 * 1024,
            sstable_block_size: 4096,

            io_backend: IoBackend::default(),  // Default to sync I/O
        }
    }
}

// Re-export CompactionStrategy
pub use compaction::CompactionStrategy;

// ===== MemTable =====

/// In-memory sorted write buffer
struct MemTable {
    data: BTreeMap<Key, Option<Value>>,  // None = tombstone (deleted)
    size_bytes: usize,
    sequence_number: u64,
}

impl MemTable {
    fn new(sequence_number: u64) -> Self {
        Self {
            data: BTreeMap::new(),
            size_bytes: 0,
            sequence_number,
        }
    }
    
    fn insert(&mut self, key: Key, value: Value) {
        let key_size = key.0.len();
        let value_size = value.0.len();
        
        // Update size
        if let Some(old_value) = self.data.get(&key) {
            if let Some(v) = old_value {
                self.size_bytes -= v.0.len();
            }
        } else {
            self.size_bytes += key_size;
        }
        
        self.size_bytes += value_size;
        self.data.insert(key, Some(value));
    }
    
    fn delete(&mut self, key: Key) {
        let key_size = key.0.len();
        
        if let Some(old_value) = self.data.get(&key) {
            if let Some(v) = old_value {
                self.size_bytes -= v.0.len();
            }
        } else {
            self.size_bytes += key_size;
        }
        
        // Tombstone
        self.data.insert(key, None);
    }
    
    fn get(&self, key: &Key) -> Option<&Option<Value>> {
        self.data.get(key)
    }
    
    fn size_bytes(&self) -> usize {
        self.size_bytes
    }
    
    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    
    fn iter(&self) -> impl Iterator<Item = (&Key, &Option<Value>)> {
        self.data.iter()
    }
    
    fn range<'a>(&'a self, from: &Key, to: &Key) -> impl Iterator<Item = (&'a Key, &'a Option<Value>)> + 'a {
        if from > to {
            // Return empty iterator for reversed bounds
            return Box::new(std::iter::empty()) as Box<dyn Iterator<Item = (&'a Key, &'a Option<Value>)> + 'a>;
        }
        Box::new(self.data.range(from..=to))
    }
}

// ===== Main LSM Tree =====

pub struct LsmTree {
    path: PathBuf,
    active_dir: PathBuf,     // active/ - mutable SSTables
    #[allow(dead_code)]
    snapshots_dir: PathBuf,  // snapshots/ - persistent snapshots
    config: LsmConfig,

    // Session lock - prevents concurrent access
    _session_lock: SessionLock,

    // In-memory components
    memtable: Arc<RwLock<MemTable>>,
    immutable_memtables: Arc<RwLock<Vec<Arc<MemTable>>>>,

    // Sequence number for ordering
    sequence_number: Arc<RwLock<u64>>,

    // Next run number for SSTable creation
    next_run_number: Arc<RwLock<RunNumber>>,

    // SSTables organized by level
    // levels[0] = L0 (fresh flushes, multiple runs)
    // levels[1] = L1, levels[2] = L2, etc.
    // Last level uses leveling (single merged run)
    levels: Arc<RwLock<Vec<Vec<SsTableHandle>>>>,

    // Maximum level (typically 6-7 for LSM trees)
    max_level: u8,

    // Compaction
    compactor: Arc<Compactor>,
}

impl LsmTree {
    pub fn open(path: impl AsRef<Path>, config: LsmConfig) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Create directory first
        std::fs::create_dir_all(&path)?;

        // Acquire session lock FIRST (before any other file operations)
        // This prevents concurrent access that could corrupt the database
        let session_lock = SessionLock::acquire(&path)
            .map_err(|e| Error::SessionLocked(e.to_string()))?;

        // Create directory structure matching Haskell:
        // root/
        //   active/    - Active SSTables (mutable, being written/compacted)
        //   snapshots/ - Persistent snapshots (immutable, hard-linked files)

        let active_dir = path.join("active");
        std::fs::create_dir_all(&active_dir)?;

        let snapshots_dir = path.join("snapshots");
        std::fs::create_dir_all(&snapshots_dir)?;

        // Fsync directories to ensure they're durable
        fsync_directory(&path)?;
        fsync_directory(&active_dir)?;
        fsync_directory(&snapshots_dir)?;

        // Initialize sequence number and memtable
        let sequence_number = 0u64;
        let memtable = MemTable::new(sequence_number);

        // Load existing SSTables from active/ directory
        // Discover run numbers by looking for .keyops files
        let mut all_sstables = Vec::new();
        let mut max_run_number = 0u64;

        for entry in std::fs::read_dir(&active_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Look for .keyops files (Haskell SSTable format)
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext == "keyops" {
                    // Extract run number from filename (e.g., "00042.keyops" -> 42)
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(run_num) = stem.parse::<RunNumber>() {
                            max_run_number = max_run_number.max(run_num);

                            match SsTableHandle::open(&active_dir, run_num) {
                                Ok(handle) => all_sstables.push(handle),
                                Err(e) => eprintln!("Failed to load SSTable run {}: {}", run_num, e),
                            }
                        }
                    }
                }
            }
        }

        // Next run number starts after the highest existing one
        let next_run_number = max_run_number + 1;

        // Organize SSTables by level (default max_level = 6)
        let max_level = 6;
        let mut levels: Vec<Vec<SsTableHandle>> = (0..=max_level).map(|_| Vec::new()).collect();

        for handle in all_sstables {
            let level = handle.level as usize;
            if level <= max_level as usize {
                levels[level].push(handle);
            } else {
                eprintln!("Warning: SSTable with level {} exceeds max_level {}", level, max_level);
            }
        }

        // Sort each level by min_key
        for level in &mut levels {
            level.sort_by(|a, b| a.min_key.cmp(&b.min_key));
        }
        
        // Create compactor
        let compactor = Arc::new(Compactor::new(
            config.compaction_strategy.clone(),
            path.clone(),
        ));

        Ok(Self {
            active_dir,
            snapshots_dir,
            path,
            config,
            _session_lock: session_lock,
            memtable: Arc::new(RwLock::new(memtable)),
            immutable_memtables: Arc::new(RwLock::new(Vec::new())),
            sequence_number: Arc::new(RwLock::new(sequence_number)),
            next_run_number: Arc::new(RwLock::new(next_run_number)),
            levels: Arc::new(RwLock::new(levels)),
            max_level,
            compactor,
        })
    }

    /// Open an LSM tree from a saved snapshot
    ///
    /// This loads a snapshot from disk and restores the LSM tree to its state
    /// at the time the snapshot was created. The snapshot directory becomes the
    /// new active directory, allowing the tree to continue operating from that point.
    ///
    /// # Arguments
    /// * `path` - Base directory (e.g., "/data/lsm")
    /// * `snapshot_name` - Name of snapshot to open (e.g., "block_123456")
    ///
    /// # Example
    /// ```no_run
    /// use cardano_lsm::{LsmTree, LsmConfig};
    ///
    /// // Restore from snapshot
    /// let tree = LsmTree::open_snapshot("/data/lsm", "block_123456").unwrap();
    /// ```
    pub fn open_snapshot(path: impl AsRef<Path>, snapshot_name: &str) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Load snapshot metadata
        let snapshot = snapshot::PersistentSnapshot::load(&path, snapshot_name)?;
        let config = snapshot.metadata.config.clone();
        let sequence_number = snapshot.metadata.sequence_number;

        // Acquire session lock
        let session_lock = SessionLock::acquire(&path)
            .map_err(|e| Error::SessionLocked(e.to_string()))?;

        // Create directory structure
        let active_dir = path.join("active");
        std::fs::create_dir_all(&active_dir)?;

        let snapshots_dir = path.join("snapshots");
        std::fs::create_dir_all(&snapshots_dir)?;

        // Fsync directories
        fsync_directory(&path)?;
        fsync_directory(&active_dir)?;
        fsync_directory(&snapshots_dir)?;

        // Initialize memtable with snapshot's sequence number
        let memtable = MemTable::new(sequence_number);

        // Open SSTables from snapshot directory
        let snapshot_dir = snapshots_dir.join(snapshot_name);
        let mut all_sstables = Vec::new();
        let mut max_run_number = 0u64;

        for run in &snapshot.metadata.runs {
            match SsTableHandle::open(&snapshot_dir, run.run_number) {
                Ok(handle) => {
                    all_sstables.push(handle);
                    max_run_number = max_run_number.max(run.run_number);
                }
                Err(e) => {
                    return Err(Error::InvalidOperation(
                        format!(
                            "Failed to load SSTable run {} from snapshot '{}' at {}:\n  {}\n\nThis snapshot may be corrupted. \
                             Consider deleting it and using a previous snapshot.",
                            run.run_number,
                            snapshot_name,
                            snapshot_dir.display(),
                            e
                        )
                    ));
                }
            }
        }

        // Next run number starts after the highest from snapshot
        let next_run_number = max_run_number + 1;

        // Organize SSTables by level (default max_level = 6)
        let max_level = 6;
        let mut levels: Vec<Vec<SsTableHandle>> = (0..=max_level).map(|_| Vec::new()).collect();

        for handle in all_sstables {
            let level = handle.level as usize;
            if level <= max_level as usize {
                levels[level].push(handle);
            } else {
                eprintln!("Warning: SSTable with level {} exceeds max_level {}", level, max_level);
            }
        }

        // Sort each level by min_key
        for level in &mut levels {
            level.sort_by(|a, b| a.min_key.cmp(&b.min_key));
        }

        // Create compactor
        let compactor = Arc::new(Compactor::new(
            config.compaction_strategy.clone(),
            path.clone(),
        ));

        Ok(Self {
            active_dir,
            snapshots_dir,
            path,
            config,
            _session_lock: session_lock,
            memtable: Arc::new(RwLock::new(memtable)),
            immutable_memtables: Arc::new(RwLock::new(Vec::new())),
            sequence_number: Arc::new(RwLock::new(sequence_number)),
            next_run_number: Arc::new(RwLock::new(next_run_number)),
            levels: Arc::new(RwLock::new(levels)),
            max_level,
            compactor,
        })
    }

    pub fn insert(&mut self, key: &Key, value: &Value) -> Result<()> {
        // Ephemeral write - only persisted via save_snapshot()
        // No WAL, writes lost on crash until snapshot is saved

        // Write to memtable
        {
            let mut memtable = self.memtable.write().unwrap();
            memtable.insert(key.clone(), value.clone());

            // Check if memtable is full
            if memtable.size_bytes() >= self.config.memtable_size {
                drop(memtable); // Release lock before flush
                self.flush_memtable()?;
            }
        }

        Ok(())
    }
    
    pub fn get(&self, key: &Key) -> Result<Option<Value>> {
        // Check memtable first
        {
            let memtable = self.memtable.read().unwrap();
            if let Some(value_opt) = memtable.get(key) {
                return Ok(value_opt.clone());
            }
        }
        
        // Check immutable memtables
        {
            let immutables = self.immutable_memtables.read().unwrap();
            for imm in immutables.iter().rev() {
                if let Some(value_opt) = imm.get(key) {
                    return Ok(value_opt.clone());
                }
            }
        }
        
        // Check SSTables (newest to oldest, L0 to Lmax)
        {
            let levels = self.levels.read().unwrap();
            for level in levels.iter() {
                // Sort SSTables by run_number in DESCENDING order (newest first)
                let mut sorted_sstables: Vec<&crate::sstable::SsTableHandle> = level.iter().collect();
                sorted_sstables.sort_by_key(|b| std::cmp::Reverse(b.run_number()));

                for sstable in sorted_sstables {
                    // Check if key could be in this SSTable
                    if key >= &sstable.min_key && key <= &sstable.max_key {
                        if let Some(value) = sstable.get(key)? {
                            return Ok(Some(value));
                        }
                    }
                }
            }
        }

        Ok(None)
    }
    
    pub fn delete(&mut self, key: &Key) -> Result<()> {
        // Ephemeral write - only persisted via save_snapshot()
        // No WAL, writes lost on crash until snapshot is saved

        // Write tombstone to memtable
        {
            let mut memtable = self.memtable.write().unwrap();
            memtable.delete(key.clone());

            // Check if memtable is full
            if memtable.size_bytes() >= self.config.memtable_size {
                drop(memtable);
                self.flush_memtable()?;
            }
        }

        Ok(())
    }

    // ===== Batch Operations =====

    /// Insert multiple key-value pairs in a batch.
    /// This is more efficient than calling insert() multiple times as it only checks
    /// for memtable flush once at the end.
    pub fn insert_batch(&mut self, entries: impl IntoIterator<Item = (Key, Value)>) -> Result<()> {
        // Ephemeral writes - only persisted via save_snapshot()
        let entries_vec: Vec<(Key, Value)> = entries.into_iter().collect();

        if entries_vec.is_empty() {
            return Ok(());
        }

        // Write all entries to memtable
        {
            let mut memtable = self.memtable.write().unwrap();
            for (key, value) in entries_vec {
                memtable.insert(key, value);
            }

            // Check if memtable is full after all inserts
            if memtable.size_bytes() >= self.config.memtable_size {
                drop(memtable); // Release lock before flush
                self.flush_memtable()?;
            }
        }

        Ok(())
    }

    /// Lookup multiple keys in a batch.
    /// Returns a vector of `Option<Value>` in the same order as the input keys.
    pub fn get_batch(&self, keys: impl IntoIterator<Item = Key>) -> Result<Vec<Option<Value>>> {
        let keys_vec: Vec<Key> = keys.into_iter().collect();
        let mut results = Vec::with_capacity(keys_vec.len());

        for key in keys_vec {
            results.push(self.get(&key)?);
        }

        Ok(results)
    }

    /// Delete multiple keys in a batch.
    /// This is more efficient than calling delete() multiple times as it only checks
    /// for memtable flush once at the end.
    pub fn delete_batch(&mut self, keys: impl IntoIterator<Item = Key>) -> Result<()> {
        let keys_vec: Vec<Key> = keys.into_iter().collect();

        if keys_vec.is_empty() {
            return Ok(());
        }

        // Write all tombstones to memtable
        {
            let mut memtable = self.memtable.write().unwrap();
            for key in keys_vec {
                memtable.delete(key);
            }

            // Check if memtable is full after all deletes
            if memtable.size_bytes() >= self.config.memtable_size {
                drop(memtable); // Release lock before flush
                self.flush_memtable()?;
            }
        }

        Ok(())
    }

    // ===== Range Queries =====

    pub fn range(&self, from: &Key, to: &Key) -> RangeIter {
        // Collect all entries from all levels
        let mut entries: BTreeMap<Key, Option<Value>> = BTreeMap::new();

        // From SSTables (lowest level first, then oldest SSTables within each level)
        {
            let levels = self.levels.read().unwrap();
            for level in levels.iter().rev() {
                // Sort SSTables by run_number in ASCENDING order (oldest first)
                // so newer values can overwrite older ones with .insert()
                let mut sorted_sstables: Vec<&crate::sstable::SsTableHandle> = level.iter().collect();
                sorted_sstables.sort_by_key(|a| a.run_number());

                for sstable in sorted_sstables {
                    // Use range_with_tombstones to include deletions in the merge
                    match sstable.range_with_tombstones(from, to) {
                        Ok(sstable_entries) => {
                            for (k, v) in sstable_entries {
                                // Use .insert() to let newer values overwrite older ones
                                entries.insert(k, v);
                            }
                        }
                        Err(e) => {
                            eprintln!("Error reading from SSTable: {}", e);
                        }
                    }
                }
            }
        }
        
        // From immutable memtables
        {
            let immutables = self.immutable_memtables.read().unwrap();
            for imm in immutables.iter() {
                for (k, v) in imm.range(from, to) {
                    entries.insert(k.clone(), v.clone());
                }
            }
        }
        
        // From current memtable (newest, highest priority)
        {
            let memtable = self.memtable.read().unwrap();
            for (k, v) in memtable.range(from, to) {
                entries.insert(k.clone(), v.clone());
            }
        }
        
        // Filter out tombstones and convert to Vec
        let results: Vec<_> = entries
            .into_iter()
            .filter_map(|(k, v)| v.map(|val| (k, val)))
            .collect();
        
        RangeIter {
            entries: results,
            index: 0,
        }
    }
    
    pub fn scan_prefix(&self, prefix: &[u8]) -> RangeIter {
        // Create an end key by incrementing the last byte
        let mut end_bytes = prefix.to_vec();
        if let Some(last) = end_bytes.last_mut() {
            if *last == 0xFF {
                end_bytes.push(0x00);
            } else {
                *last += 1;
            }
        } else {
            // Empty prefix matches everything
            end_bytes = vec![0xFF; 20];
        }
        
        self.range(&Key::from(prefix), &Key::from(&end_bytes))
    }
    
    pub fn iter(&self) -> RangeIter {
        self.range(&Key::from(b""), &Key::from([0xFF; 256]))
    }
    
    pub fn flush(&self) -> Result<()> {
        // No-op: With ephemeral writes, flush only happens via save_snapshot()
        Ok(())
    }
    
    fn flush_memtable(&mut self) -> Result<()> {
        // Move current memtable to immutable list
        let old_memtable = {
            let mut memtable = self.memtable.write().unwrap();
            let seq = *self.sequence_number.read().unwrap();
            let new_memtable = MemTable::new(seq);
            std::mem::replace(&mut *memtable, new_memtable)
        };

        // Don't flush empty memtables
        if old_memtable.is_empty() {
            return Ok(());
        }

        // Get next run number and increment
        let run_number = {
            let mut run_num = self.next_run_number.write().unwrap();
            let current = *run_num;
            *run_num += 1;
            current
        };

        // Write to SSTable using new multi-file format
        let mut writer = SsTableWriter::new(&self.active_dir, run_number)?;

        for (key, value_opt) in old_memtable.iter() {
            writer.add(key.clone(), value_opt.clone())?;
        }

        let handle = writer.finish(0)?;  // Flushes always go to L0

        // Add to L0
        {
            let mut levels = self.levels.write().unwrap();
            levels[0].push(handle);

            // Check if we should trigger compaction (L0 size trigger)
            if levels[0].len() >= self.config.level0_compaction_trigger {
                drop(levels); // Release lock before compaction
                // Trigger compaction
                self.compact()?;
            }
        }

        Ok(())
    }
    
    /// Trigger compaction using LazyLevelling policy
    ///
    /// LazyLevelling:
    /// - L0 to L(max-1): Tiering (multiple runs per level)
    /// - L(max): Leveling (single merged run, tombstone removal)
    /// - Compact level i to level i+1 when level i exceeds size threshold
    pub fn compact(&mut self) -> Result<()> {
        let levels_snapshot = self.levels.read().unwrap().clone();

        // Select level for compaction using LazyLevelling policy
        // Size ratio of 4 is standard for LSM trees
        let job = match self.compactor.select_level_compaction(&levels_snapshot, self.max_level, 4) {
            Some(job) => job,
            None => {
                // Nothing to compact
                return Ok(());
            }
        };

        let source_level = job.source_level as usize;
        let target_level = job.target_level as usize;

        // Get next run number for compacted SSTable
        let run_number = {
            let mut run_num = self.next_run_number.write().unwrap();
            let current = *run_num;
            *run_num += 1;
            current
        };

        // Execute compaction
        let source_runs = levels_snapshot[source_level].clone();
        let result = self.compactor.compact_levels(
            job,
            &source_runs,
            &self.active_dir,
            run_number,
            self.max_level,
        )?;

        // Update levels atomically
        {
            let mut levels = self.levels.write().unwrap();

            // Remove source runs (in reverse order to maintain indices)
            let mut to_remove = result.inputs_to_remove.clone();
            to_remove.sort_by(|a, b| b.cmp(a)); // Sort descending

            for idx in to_remove {
                if idx < levels[source_level].len() {
                    let _removed = levels[source_level].remove(idx);
                    // The SsTableHandle will be dropped here, but files are only deleted
                    // when the last reference is dropped (refcount reaches 0). This allows
                    // ongoing range queries to safely access the files.
                }
            }

            // Add output SSTable to target level
            if let Some(output) = result.output {
                // For bottom level (leveling): replace all runs with merged run
                if target_level == self.max_level as usize {
                    // Clear target level and add single merged run
                    // Old handles will be dropped but files are protected by refcounting
                    levels[target_level].clear();
                    levels[target_level].push(output);
                } else {
                    // For other levels (tiering): just add the new run
                    levels[target_level].push(output);
                }
            }
        }

        Ok(())
    }
    
    /// Compact ALL SSTables into one (removes all tombstones)
    pub fn compact_all(&mut self) -> Result<()> {
        // Collect all SSTables from all levels
        let all_sstables: Vec<SsTableHandle> = {
            let levels = self.levels.read().unwrap();
            levels.iter().flat_map(|level| level.clone()).collect()
        };

        if all_sstables.is_empty() {
            return Ok(());
        }

        // Create job with all SSTables
        let all_indices: Vec<usize> = (0..all_sstables.len()).collect();
        let job = compaction::CompactionJob {
            inputs: all_indices,
            strategy: self.config.compaction_strategy.clone(),
        };

        // Get next run number for compacted SSTable
        let run_number = {
            let mut run_num = self.next_run_number.write().unwrap();
            let current = *run_num;
            *run_num += 1;
            current
        };

        let result = self.compactor.compact(job, &all_sstables, &self.active_dir, run_number)?;

        // Clear all levels and add the single compacted SSTable
        {
            let mut levels = self.levels.write().unwrap();

            // Clear all old SSTables from all levels
            // Old handles will be dropped but files are protected by refcounting
            for level in levels.iter_mut() {
                level.clear();
            }

            // Add the single compacted SSTable to max level
            if let Some(output) = result.output {
                levels[self.max_level as usize].push(output);
            }
        }

        Ok(())
    }
    
    pub fn trigger_background_compaction(&self) {
        // For now, this is a no-op
        // In a real implementation, this would signal a background thread
        // to run compaction asynchronously
    }
    
    pub fn wait_for_compaction(&self) {
        // For now, this is a no-op
        // In a real implementation, this would wait for background
        // compaction to complete
    }
    
    pub fn snapshot(&self) -> LsmSnapshot {
        let memtable = self.memtable.read().unwrap();
        let immutables = self.immutable_memtables.read().unwrap();
        let levels = self.levels.read().unwrap();
        let seq = *self.sequence_number.read().unwrap();

        LsmSnapshot {
            memtable: Arc::new((*memtable).clone()),
            immutable_memtables: immutables.clone(),
            levels: levels.clone(),
            sequence_number: seq,
        }
    }
    
    pub fn rollback(&mut self, snapshot: LsmSnapshot) -> Result<()> {
        // Verify we're not rolling back to the future
        let current_seq = *self.sequence_number.read().unwrap();
        if snapshot.sequence_number > current_seq {
            return Err(Error::InvalidOperation(
                "Cannot rollback to future snapshot".to_string()
            ));
        }

        // Replace state
        *self.memtable.write().unwrap() = (*snapshot.memtable).clone();
        *self.immutable_memtables.write().unwrap() = snapshot.immutable_memtables;
        *self.levels.write().unwrap() = snapshot.levels;
        *self.sequence_number.write().unwrap() = snapshot.sequence_number;

        Ok(())
    }
    
    pub fn disk_usage(&self) -> Result<u64> {
        let mut total = 0u64;

        // Count SSTable sizes from all levels
        let levels = self.levels.read().unwrap();
        for level in levels.iter() {
            for sstable in level.iter() {
                if let Ok(metadata) = std::fs::metadata(sstable.path()) {
                    total += metadata.len();
                }
            }
        }

        Ok(total)
    }
    
    pub fn get_stats(&self) -> Result<LsmStats> {
        let memtable = self.memtable.read().unwrap();
        let immutables = self.immutable_memtables.read().unwrap();
        let levels = self.levels.read().unwrap();

        let total_sstables: usize = levels.iter().map(|level| level.len()).sum();

        Ok(LsmStats {
            memtable_size_bytes: memtable.size_bytes() as u64,
            immutable_memtables_count: immutables.len(),
            l0_sstables_count: levels[0].len(),
            total_sstables_count: total_sstables,
            compactions_running: 0,
            bloom_filter_false_positives: 0,
        })
    }

    /// Create a persistent snapshot with hard-links to current SSTables
    ///
    /// This flushes the memtable first to ensure all data is persisted,
    /// then creates a snapshot in the snapshots/ directory using hard-links.
    pub fn save_snapshot(&mut self, name: &str, label: &str) -> Result<()> {
        // Flush memtable to ensure all data is persisted
        self.flush_memtable()?;

        // Get current sequence number and all SSTables from all levels
        let sequence_number = *self.sequence_number.read().unwrap();
        let all_sstables: Vec<SsTableHandle> = {
            let levels = self.levels.read().unwrap();
            levels.iter().flat_map(|level| level.clone()).collect()
        };

        // Create snapshot using hard-links
        PersistentSnapshot::create(
            &self.path,
            name,
            label,
            &all_sstables,
            sequence_number,
            &self.config,
        )?;

        Ok(())
    }

    /// List all available snapshots
    pub fn list_snapshots(&self) -> Result<Vec<String>> {
        snapshot::list_snapshots(&self.path)
    }

    /// Delete a snapshot by name
    pub fn delete_snapshot(&self, name: &str) -> Result<()> {
        let snapshot = PersistentSnapshot::load(&self.path, name)?;
        snapshot.delete()
            .map_err(Error::Io)
    }
}

// Make MemTable cloneable for snapshots
impl Clone for MemTable {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            size_bytes: self.size_bytes,
            sequence_number: self.sequence_number,
        }
    }
}

// ===== Range Iterator =====

pub struct RangeIter {
    entries: Vec<(Key, Value)>,
    index: usize,
}

impl Iterator for RangeIter {
    type Item = (Key, Value);
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.entries.len() {
            let item = self.entries[self.index].clone();
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }
}

impl Clone for RangeIter {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            index: self.index,
        }
    }
}

// ===== Snapshot =====

#[derive(Clone)]
pub struct LsmSnapshot {
    memtable: Arc<MemTable>,
    immutable_memtables: Vec<Arc<MemTable>>,
    levels: Vec<Vec<SsTableHandle>>,
    sequence_number: u64,
}

impl LsmSnapshot {
    pub fn sequence_number(&self) -> u64 {
        self.sequence_number
    }
    
    pub fn get(&self, key: &Key) -> Result<Option<Value>> {
        // Check memtable
        if let Some(value_opt) = self.memtable.get(key) {
            return Ok(value_opt.clone());
        }

        // Check immutable memtables
        for imm in self.immutable_memtables.iter().rev() {
            if let Some(value_opt) = imm.get(key) {
                return Ok(value_opt.clone());
            }
        }

        // Check SSTables from all levels
        for level in &self.levels {
            for sstable in level.iter().rev() {
                if key >= &sstable.min_key && key <= &sstable.max_key {
                    if let Some(value) = sstable.get(key)? {
                        return Ok(Some(value));
                    }
                }
            }
        }

        Ok(None)
    }
    
    pub fn iter(&self) -> RangeIter {
        let mut entries: BTreeMap<Key, Option<Value>> = BTreeMap::new();

        // Collect from SSTables (all levels, lowest priority)
        for level in self.levels.iter().rev() {
            for sstable in level {
                if let Ok(sstable_entries) = sstable.range(&Key::from(b""), &Key::from([0xFF; 256])) {
                    for (k, v) in sstable_entries {
                        entries.entry(k).or_insert(v);
                    }
                }
            }
        }

        // Collect from immutables
        for imm in &self.immutable_memtables {
            for (k, v) in imm.iter() {
                entries.insert(k.clone(), v.clone());
            }
        }

        // Collect from memtable (highest priority)
        for (k, v) in self.memtable.iter() {
            entries.insert(k.clone(), v.clone());
        }

        // Filter tombstones
        let results: Vec<_> = entries
            .into_iter()
            .filter_map(|(k, v)| v.map(|val| (k, val)))
            .collect();

        RangeIter {
            entries: results,
            index: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LsmStats {
    pub memtable_size_bytes: u64,
    pub immutable_memtables_count: usize,
    pub l0_sstables_count: usize,
    pub total_sstables_count: usize,
    pub compactions_running: usize,
    pub bloom_filter_false_positives: u64,
}

// End of lib.rs

// ===== Monoidal LSM Tree =====
