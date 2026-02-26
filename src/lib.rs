// Cardano LSM Tree - Pure Rust port from Haskell lsm-tree
// Core implementation

mod atomic_file;
mod checksum;
mod checksum_handle;
mod session_lock;
mod snapshot;
mod sstable;
mod sstable_new;  // New Haskell-format SSTable (will replace sstable.rs)
mod compaction;
mod merkle;
mod monoidal;

use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use serde::{Serialize, Deserialize};
use sstable_new::{SsTableWriter, SsTableHandle, RunNumber};
use compaction::Compactor;
use atomic_file::fsync_directory;
use session_lock::SessionLock;

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
    pub enable_compression: bool,
    pub compression_algorithm: CompressionAlgorithm,
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
            enable_compression: true,
            compression_algorithm: CompressionAlgorithm::Lz4,
        }
    }
}

// Re-export CompactionStrategy
pub use compaction::CompactionStrategy;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    None,
    Lz4,
    Snappy,
}

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

    // SSTables
    sstables: Arc<RwLock<Vec<SsTableHandle>>>,

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
        let mut sstables = Vec::new();
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
                                Ok(handle) => sstables.push(handle),
                                Err(e) => eprintln!("Failed to load SSTable run {}: {}", run_num, e),
                            }
                        }
                    }
                }
            }
        }

        // Next run number starts after the highest existing one
        let next_run_number = max_run_number + 1;
        
        // Sort SSTables by min_key (oldest first)
        sstables.sort_by(|a, b| a.min_key.cmp(&b.min_key));
        
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
            sstables: Arc::new(RwLock::new(sstables)),
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
        
        // Check SSTables (newest to oldest)
        {
            let sstables = self.sstables.read().unwrap();
            for sstable in sstables.iter().rev() {
                // Check if key could be in this SSTable
                if key >= &sstable.min_key && key <= &sstable.max_key {
                    if let Some(value) = sstable.get(key)? {
                        return Ok(Some(value));
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
    
    pub fn range(&self, from: &Key, to: &Key) -> RangeIter {
        // Collect all entries from all levels
        let mut entries: BTreeMap<Key, Option<Value>> = BTreeMap::new();
        
        // From SSTables (oldest, lowest priority)
        {
            let sstables = self.sstables.read().unwrap();
            for sstable in sstables.iter() {
                match sstable.range(from, to) {
                    Ok(sstable_entries) => {
                        for (k, v) in sstable_entries {
                            entries.entry(k).or_insert(v);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading from SSTable: {}", e);
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
        self.range(&Key::from(b""), &Key::from(&[0xFF; 256]))
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

        // Add to SSTable list
        {
            let mut sstables = self.sstables.write().unwrap();
            sstables.push(handle);

            // Check if we should trigger compaction
            if sstables.len() >= self.config.level0_compaction_trigger {
                drop(sstables); // Release lock before compaction
                // Trigger compaction
                self.compact()?;
            }
        }

        Ok(())
    }
    
    pub fn compact(&mut self) -> Result<()> {
        let sstables = self.sstables.read().unwrap();

        // Select tables to compact
        let job = match self.compactor.select_compaction(&*sstables) {
            Some(job) => job,
            None => {
                // Nothing to compact
                return Ok(());
            }
        };
        
        drop(sstables); // Release read lock

        // Get next run number for compacted SSTable
        let run_number = {
            let mut run_num = self.next_run_number.write().unwrap();
            let current = *run_num;
            *run_num += 1;
            current
        };

        // Execute compaction
        let sstables_for_compact = self.sstables.read().unwrap().clone();
        let result = self.compactor.compact(job, &sstables_for_compact, &self.active_dir, run_number)?;
        
        // Update SSTable list
        {
            let mut sstables = self.sstables.write().unwrap();
            
            // Remove input SSTables (in reverse order to maintain indices)
            let mut to_remove = result.inputs_to_remove.clone();
            to_remove.sort_by(|a, b| b.cmp(a)); // Sort descending
            
            for idx in to_remove {
                if idx < sstables.len() {
                    let removed = sstables.remove(idx);
                    // Delete all files for this SSTable run
                    if let Err(e) = removed.delete_files() {
                        eprintln!("Failed to delete old SSTable: {}", e);
                    }
                }
            }
            
            // Add output SSTable
            if let Some(output) = result.output {
                sstables.push(output);
            }
        }
        
        Ok(())
    }
    
    /// Compact ALL SSTables into one (removes all tombstones)
    pub fn compact_all(&mut self) -> Result<()> {
        let sstables = self.sstables.read().unwrap();
        
        if sstables.is_empty() {
            return Ok(());
        }
        
        // Create job with all SSTables
        let all_indices: Vec<usize> = (0..sstables.len()).collect();
        let job = compaction::CompactionJob {
            inputs: all_indices,
            strategy: self.config.compaction_strategy.clone(),
        };
        
        drop(sstables);

        // Get next run number for compacted SSTable
        let run_number = {
            let mut run_num = self.next_run_number.write().unwrap();
            let current = *run_num;
            *run_num += 1;
            current
        };

        let sstables_for_compact = self.sstables.read().unwrap().clone();
        let result = self.compactor.compact(job, &sstables_for_compact, &self.active_dir, run_number)?;
        
        // Update SSTable list
        {
            let mut sstables = self.sstables.write().unwrap();
            
            // Clear all old SSTables
            for sstable in sstables.drain(..) {
                if let Err(e) = sstable.delete_files() {
                    eprintln!("Failed to delete old SSTable: {}", e);
                }
            }
            
            // Add the single compacted SSTable
            if let Some(output) = result.output {
                sstables.push(output);
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
        let sstables = self.sstables.read().unwrap();
        let seq = *self.sequence_number.read().unwrap();
        
        LsmSnapshot {
            memtable: Arc::new((*memtable).clone()),
            immutable_memtables: immutables.clone(),
            sstables: sstables.clone(),
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
        *self.sstables.write().unwrap() = snapshot.sstables;
        *self.sequence_number.write().unwrap() = snapshot.sequence_number;

        Ok(())
    }
    
    pub fn disk_usage(&self) -> Result<u64> {
        let mut total = 0u64;

        // Count SSTable sizes
        let sstables = self.sstables.read().unwrap();
        for sstable in sstables.iter() {
            if let Ok(metadata) = std::fs::metadata(sstable.path()) {
                total += metadata.len();
            }
        }

        Ok(total)
    }
    
    pub fn get_stats(&self) -> Result<LsmStats> {
        let memtable = self.memtable.read().unwrap();
        let immutables = self.immutable_memtables.read().unwrap();
        let sstables = self.sstables.read().unwrap();

        Ok(LsmStats {
            memtable_size_bytes: memtable.size_bytes() as u64,
            immutable_memtables_count: immutables.len(),
            l0_sstables_count: sstables.len(),
            total_sstables_count: sstables.len(),
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

        // Get current sequence number and SSTables
        let sequence_number = *self.sequence_number.read().unwrap();
        let sstables = self.sstables.read().unwrap();

        // Create snapshot using hard-links
        PersistentSnapshot::create(
            &self.path,
            name,
            label,
            &sstables,
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
            .map_err(|e| Error::Io(e))
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
    sstables: Vec<SsTableHandle>,
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
        
        // Check SSTables
        for sstable in self.sstables.iter().rev() {
            if key >= &sstable.min_key && key <= &sstable.max_key {
                if let Some(value) = sstable.get(key)? {
                    return Ok(Some(value));
                }
            }
        }
        
        Ok(None)
    }
    
    pub fn iter(&self) -> RangeIter {
        let mut entries: BTreeMap<Key, Option<Value>> = BTreeMap::new();
        
        // Collect from SSTables
        for sstable in &self.sstables {
            if let Ok(sstable_entries) = sstable.range(&Key::from(b""), &Key::from(&[0xFF; 256])) {
                for (k, v) in sstable_entries {
                    entries.entry(k).or_insert(v);
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
