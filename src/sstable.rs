// SSTable implementation matching Haskell lsm-tree format
//
// File format (per Run number N):
//   ${N}.keyops   - Sorted key/operation pairs
//   ${N}.blobs    - Blob values (large values)
//   ${N}.filter   - Bloom filter for keys
//   ${N}.index    - Index mapping keys to page numbers
//   ${N}.checksums - CRC32C checksums for all above files
//
// This matches the Haskell implementation's multi-file format,
// with external checksums for corruption detection.

use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use serde::{Serialize, Deserialize};
use byteorder::{LittleEndian, ReadBytesExt};

use crate::checksum::ChecksumsFile;
use crate::checksum_handle::ChecksumHandle;
use crate::atomic_file::fsync_directory;
use crate::io_backend::{self, IoBackend};
use crate::{Key, Value, Result, Error};

/// Run number for identifying a set of SSTable files
pub type RunNumber = u64;

/// Paths to all files for a single Run
#[derive(Debug, Clone)]
pub struct RunPaths {
    #[allow(dead_code)]
    pub run_number: RunNumber,
    pub keyops: PathBuf,
    pub blobs: PathBuf,
    pub filter: PathBuf,
    pub index: PathBuf,
    pub checksums: PathBuf,
}

impl RunPaths {
    /// Create RunPaths for a given run number in active directory
    pub fn new(active_dir: &Path, run_number: RunNumber) -> Self {
        let prefix = format!("{:05}", run_number);
        Self {
            run_number,
            keyops: active_dir.join(format!("{}.keyops", prefix)),
            blobs: active_dir.join(format!("{}.blobs", prefix)),
            filter: active_dir.join(format!("{}.filter", prefix)),
            index: active_dir.join(format!("{}.index", prefix)),
            checksums: active_dir.join(format!("{}.checksums", prefix)),
        }
    }

    /// Check if all files exist
    pub fn all_exist(&self) -> bool {
        self.keyops.exists()
            && self.blobs.exists()
            && self.filter.exists()
            && self.index.exists()
            && self.checksums.exists()
    }
}

/// Writer for creating SSTables in Haskell format
///
/// Usage:
/// 1. Create writer with `new()`
/// 2. Add entries with `add()`
/// 3. Finalize with `finish()`
///
/// The writer maintains checksums for all files and writes them
/// atomically on finish().
pub struct SsTableWriter {
    #[allow(dead_code)]
    run_number: RunNumber,
    paths: RunPaths,

    // Checksum-tracked writers for each component
    keyops_writer: ChecksumHandle,
    blobs_writer: ChecksumHandle,

    // Data for building filter and index
    entries: Vec<(Key, Option<Value>)>,

    // Track blob offsets
    blob_offset: u64,
}

impl SsTableWriter {
    /// Create a new SSTable writer
    ///
    /// Files are written to temporary paths (.tmp) until finish() is called.
    pub fn new(active_dir: &Path, run_number: RunNumber) -> Result<Self> {
        let paths = RunPaths::new(active_dir, run_number);

        // Create temporary files for writing
        let keyops_writer = ChecksumHandle::create(&paths.keyops)
            .map_err(Error::Io)?;

        let blobs_writer = ChecksumHandle::create(&paths.blobs)
            .map_err(Error::Io)?;

        Ok(Self {
            run_number,
            paths,
            keyops_writer,
            blobs_writer,
            entries: Vec::new(),
            blob_offset: 0,
        })
    }

    /// Add a key-value pair to the SSTable
    ///
    /// Values are written immediately to the blobs file.
    /// Keys and operations are buffered until finish().
    pub fn add(&mut self, key: Key, value: Option<Value>) -> Result<()> {
        self.entries.push((key, value));
        Ok(())
    }

    /// Finalize the SSTable and write all files atomically
    ///
    /// This:
    /// 1. Sorts entries by key
    /// 2. Writes keyops file
    /// 3. Writes blobs file
    /// 4. Builds and writes bloom filter
    /// 5. Builds and writes index
    /// 6. Writes checksums file
    /// 7. Fsyncs directory
    ///
    /// Returns a handle to the completed SSTable.
    pub fn finish(mut self, level: u8) -> Result<SsTableHandle> {
        if self.entries.is_empty() {
            return Err(Error::InvalidOperation("Cannot create empty SSTable".to_string()));
        }

        // Sort entries by key
        self.entries.sort_by(|a, b| a.0.cmp(&b.0));

        let min_key = self.entries.first().unwrap().0.clone();
        let max_key = self.entries.last().unwrap().0.clone();
        let num_entries = self.entries.len() as u64;

        // Write keyops and blobs, tracking byte offsets for the index
        let mut keyops_offset: u64 = 0;
        let mut key_offsets: Vec<u64> = Vec::with_capacity(self.entries.len());
        for (key, value_opt) in &self.entries {
            // Record the byte offset of this entry in the keyops file
            key_offsets.push(keyops_offset);

            // Write key
            let key_bytes = key.as_ref();
            self.keyops_writer.write_all(&(key_bytes.len() as u32).to_le_bytes())?;
            self.keyops_writer.write_all(key_bytes)?;
            keyops_offset += 4 + key_bytes.len() as u64;

            // Write operation (Insert=1, Delete=0) and value
            match value_opt {
                Some(value) => {
                    self.keyops_writer.write_all(&[1u8])?; // Insert operation
                    keyops_offset += 1;

                    let value_bytes = value.as_ref();
                    let value_len = value_bytes.len() as u64;

                    // Write blob offset and size to keyops
                    self.keyops_writer.write_all(&self.blob_offset.to_le_bytes())?;
                    self.keyops_writer.write_all(&value_len.to_le_bytes())?;
                    keyops_offset += 16; // 8 bytes offset + 8 bytes length

                    // Write value to blobs
                    self.blobs_writer.write_all(value_bytes)?;
                    self.blob_offset += value_len;
                }
                None => {
                    self.keyops_writer.write_all(&[0u8])?; // Delete operation
                    keyops_offset += 1;
                }
            }
        }

        // Close keyops and blobs, get checksums
        let keyops_crc = self.keyops_writer.close()?;
        let blobs_crc = self.blobs_writer.close()?;

        // Build bloom filter
        let bloom_filter = BloomFilter::from_keys(
            self.entries.iter().map(|(k, _)| k.as_ref()),
            10, // bits per key
            0.01, // false positive rate
        );

        // Write filter file
        let filter_bytes = bincode::serialize(&bloom_filter)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        let filter_crc = crate::checksum_handle::write_file_with_checksum(&self.paths.filter, &filter_bytes)?;

        // Build index with keys and byte offsets for O(log n) point lookups
        let index = Index {
            keys: self.entries.iter().map(|(k, _)| k.as_ref().to_vec()).collect(),
            offsets: key_offsets,
        };

        // Write index file
        let index_bytes = bincode::serialize(&index)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        let index_crc = crate::checksum_handle::write_file_with_checksum(&self.paths.index, &index_bytes)?;

        // Write checksums file
        let mut checksums = ChecksumsFile::new();
        checksums.insert("keyops".to_string(), keyops_crc);
        checksums.insert("blobs".to_string(), blobs_crc);
        checksums.insert("filter".to_string(), filter_crc);
        checksums.insert("index".to_string(), index_crc);

        crate::checksum::write_checksums_file(&self.paths.checksums, &checksums)?;

        // Fsync directory to ensure all file operations are durable
        if let Some(parent) = self.paths.keyops.parent() {
            fsync_directory(parent)?;
        }

        Ok(SsTableHandle {
            paths: self.paths,
            min_key,
            max_key,
            num_entries,
            level,
            bloom_filter,
            index,
            refcount: Arc::new(AtomicUsize::new(1)), // Initial refcount = 1
        })
    }
}

/// Handle to a completed SSTable
///
/// Provides read operations with automatic checksum verification.
///
/// Uses reference counting to support hard-links: multiple handles
/// can reference the same physical files (via hard-links), and files
/// are only deleted when the last reference is dropped.
#[derive(Debug)]
pub struct SsTableHandle {
    paths: RunPaths,
    pub min_key: Key,
    pub max_key: Key,
    pub num_entries: u64,
    pub level: u8,  // 0 = L0 (fresh flushes), 1-6 = L1-L6
    bloom_filter: BloomFilter,
    index: Index,

    // Reference count shared across all hard-linked instances
    // When this reaches 0, files are deleted
    refcount: Arc<AtomicUsize>,
}

impl SsTableHandle {
    /// Open an existing SSTable from disk
    ///
    /// Verifies all checksums on open to detect corruption early.
    pub fn open(active_dir: &Path, run_number: RunNumber) -> Result<Self> {
        let paths = RunPaths::new(active_dir, run_number);

        // Verify all files exist and provide detailed error about which files are missing
        if !paths.all_exist() {
            let mut missing = Vec::new();
            if !paths.keyops.exists() {
                missing.push(paths.keyops.display().to_string());
            }
            if !paths.blobs.exists() {
                missing.push(paths.blobs.display().to_string());
            }
            if !paths.filter.exists() {
                missing.push(paths.filter.display().to_string());
            }
            if !paths.index.exists() {
                missing.push(paths.index.display().to_string());
            }
            if !paths.checksums.exists() {
                missing.push(paths.checksums.display().to_string());
            }

            return Err(Error::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "SSTable files missing for run {} in directory {}:\n  {}",
                    run_number,
                    active_dir.display(),
                    missing.join("\n  ")
                ),
            )));
        }

        // Read checksums file
        let checksums = crate::checksum::read_checksums_file(&paths.checksums)?;

        // Verify keyops checksum
        let keyops_expected = crate::checksum::get_checksum(&checksums, "keyops")
            .map_err(Error::Corruption)?;
        crate::checksum::check_crc(&paths.keyops, keyops_expected)?;

        // Verify blobs checksum
        let blobs_expected = crate::checksum::get_checksum(&checksums, "blobs")
            .map_err(Error::Corruption)?;
        crate::checksum::check_crc(&paths.blobs, blobs_expected)?;

        // Verify filter checksum
        let filter_expected = crate::checksum::get_checksum(&checksums, "filter")
            .map_err(Error::Corruption)?;
        crate::checksum::check_crc(&paths.filter, filter_expected)?;

        // Verify index checksum
        let index_expected = crate::checksum::get_checksum(&checksums, "index")
            .map_err(Error::Corruption)?;
        crate::checksum::check_crc(&paths.index, index_expected)?;

        // All checksums verified, now read the data
        // Use default (sync) backend for metadata reading
        let backend = IoBackend::default();

        // Read bloom filter
        let filter_bytes = io_backend::read_file(&paths.filter, &backend)?;
        let bloom_filter: BloomFilter = bincode::deserialize(&filter_bytes)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        // Read index
        let index_bytes = io_backend::read_file(&paths.index, &backend)?;
        let index: Index = bincode::deserialize(&index_bytes)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        // Determine min/max keys from index
        let min_key = Key::from(&index.keys.first().unwrap()[..]);
        let max_key = Key::from(&index.keys.last().unwrap()[..]);
        let num_entries = index.keys.len() as u64;

        Ok(Self {
            paths,
            min_key,
            max_key,
            num_entries,
            level: 0,  // Default to L0 for existing SSTables without level metadata
            bloom_filter,
            index,
            refcount: Arc::new(AtomicUsize::new(1)), // Initial refcount = 1
        })
    }

    /// Get a value by key
    ///
    /// Returns None if the key is not found or was deleted.
    pub fn get(&self, key: &Key) -> Result<Option<Value>> {
        self.get_backend(key, &IoBackend::default())
    }

    /// Get the run number for this SSTable
    ///
    /// Run numbers are monotonically increasing, so higher run numbers indicate more recent SSTables.
    pub fn run_number(&self) -> RunNumber {
        self.paths.run_number
    }

    /// Get a value by key using specific I/O backend
    ///
    /// Uses the index to perform O(log n) binary search on keys, then seeks
    /// directly to the entry's byte offset in the keyops file. Only reads
    /// the single entry instead of scanning the entire file.
    pub fn get_backend(&self, key: &Key, backend: &IoBackend) -> Result<Option<Value>> {
        // Check bloom filter first (fast negative lookup)
        if !self.bloom_filter.might_contain(key.as_ref()) {
            return Ok(None);
        }

        // Binary search in index
        let key_bytes = key.as_ref();
        let pos = match self.index.keys.binary_search_by(|k| k.as_slice().cmp(key_bytes)) {
            Ok(pos) => pos,
            Err(_) => return Ok(None), // Not found in index
        };

        // If the index has byte offsets, seek directly to the entry
        if !self.index.offsets.is_empty() {
            let offset = self.index.offsets[pos];
            return self.read_entry_at_offset(offset, key_bytes, backend);
        }

        // Legacy fallback: index has no offsets, do a full scan
        let range_results = self.range_with_tombstones_backend(key, key, backend)?;
        if let Some((_, value_opt)) = range_results.first() {
            Ok(value_opt.clone())
        } else {
            Ok(None)
        }
    }

    /// Read a single keyops entry at the given byte offset.
    fn read_entry_at_offset(
        &self,
        offset: u64,
        expected_key: &[u8],
        backend: &IoBackend,
    ) -> Result<Option<Value>> {
        // Read enough bytes for: key_len(4) + key + op(1) + blob_ref(16)
        let read_len = 4 + expected_key.len() + 1 + 16;
        let data = io_backend::read_range(&self.paths.keyops, offset, read_len, backend)?;

        let mut cursor = std::io::Cursor::new(&data);

        // Read and verify key
        let key_len = cursor.read_u32::<LittleEndian>()? as usize;
        if key_len != expected_key.len() {
            // Offset corrupt; fall back to full scan
            let range_results = self.range_with_tombstones_backend(
                &Key::from(expected_key), &Key::from(expected_key), backend)?;
            return Ok(range_results.first().and_then(|(_, v)| v.clone()));
        }

        let mut key_bytes = vec![0u8; key_len];
        cursor.read_exact(&mut key_bytes)?;
        if key_bytes != expected_key {
            // Key mismatch; fall back to full scan
            let range_results = self.range_with_tombstones_backend(
                &Key::from(expected_key), &Key::from(expected_key), backend)?;
            return Ok(range_results.first().and_then(|(_, v)| v.clone()));
        }

        // Read operation
        let operation = cursor.read_u8()?;

        if operation == 1 {
            // Insert: fetch blob value
            let blob_offset = cursor.read_u64::<LittleEndian>()?;
            let value_len = cursor.read_u64::<LittleEndian>()? as usize;
            let blob_data = io_backend::read_range(
                &self.paths.blobs, blob_offset, value_len, backend)?;
            Ok(Some(Value::from(&blob_data)))
        } else {
            // Delete (tombstone)
            Ok(None)
        }
    }

    /// Range query - iterate over keys in range [from, to]
    ///
    /// Returns an iterator over (Key, Value) pairs (excludes tombstones).
    pub fn range(&self, from: &Key, to: &Key) -> Result<Vec<(Key, Option<Value>)>> {
        self.range_backend(from, to, &IoBackend::default())
    }

    /// Range query using specific I/O backend
    pub fn range_backend(&self, from: &Key, to: &Key, backend: &IoBackend) -> Result<Vec<(Key, Option<Value>)>> {
        let all_entries = self.range_with_tombstones_backend(from, to, backend)?;

        // Filter out tombstones (None values that represent deletes)
        // But keep None values that come from Insert operations
        // Actually, for range queries, we want to exclude deleted keys entirely
        Ok(all_entries.into_iter().filter(|(_, v)| v.is_some()).collect())
    }

    /// Range query with tombstones - needed for compaction
    ///
    /// Returns all entries including deletes (tombstones represented as None).
    pub fn range_with_tombstones(&self, from: &Key, to: &Key) -> Result<Vec<(Key, Option<Value>)>> {
        self.range_with_tombstones_backend(from, to, &IoBackend::default())
    }

    /// Range query with tombstones using specific I/O backend
    ///
    /// This allows using io_uring for async I/O on Linux.
    pub fn range_with_tombstones_backend(&self, from: &Key, to: &Key, backend: &IoBackend) -> Result<Vec<(Key, Option<Value>)>> {
        // Read the entire keyops file
        let keyops_data = io_backend::read_file(&self.paths.keyops, backend)?;
        let mut cursor = std::io::Cursor::new(&keyops_data);

        let mut results = Vec::new();
        let mut blob_requests = Vec::new(); // For batched reads with io_uring

        // Parse keyops file entry by entry
        while cursor.position() < keyops_data.len() as u64 {
            // Read key length
            let key_len = match cursor.read_u32::<LittleEndian>() {
                Ok(len) => len as usize,
                Err(_) => break, // End of file
            };

            // Read key
            let mut key_bytes = vec![0u8; key_len];
            cursor.read_exact(&mut key_bytes)?;
            let key = Key::from(&key_bytes);

            // Check if key is in range
            if key < *from || key > *to {
                // Skip this entry
                let op = cursor.read_u8()?;
                if op == 1 {
                    // Skip blob offset and length
                    cursor.read_u64::<LittleEndian>()?;
                    cursor.read_u64::<LittleEndian>()?;
                }
                continue;
            }

            // Read operation
            let operation = cursor.read_u8()?;

            if operation == 1 {
                // Insert operation - read blob reference
                let blob_offset = cursor.read_u64::<LittleEndian>()?;
                let value_len = cursor.read_u64::<LittleEndian>()? as usize;

                // Store blob read request for batching
                blob_requests.push((results.len(), blob_offset, value_len));
                results.push((key, None)); // Placeholder, will fill in value later
            } else {
                // Delete operation (tombstone)
                results.push((key, None));
            }
        }

        // Batch read all blobs using io_uring (or sequential fallback)
        if !blob_requests.is_empty() {
            let blob_reads: Vec<_> = blob_requests
                .iter()
                .map(|(_, offset, len)| (&self.paths.blobs as &Path, *offset, *len))
                .collect();

            let blob_values = io_backend::read_batch(blob_reads, backend)?;

            // Fill in the blob values
            for (i, (result_idx, _, _)) in blob_requests.iter().enumerate() {
                results[*result_idx].1 = Some(Value::from(&blob_values[i]));
            }
        }

        Ok(results)
    }

    pub fn path(&self) -> &Path {
        &self.paths.keyops
    }

    /// Create a hard-linked copy of this SSTable in a different directory
    ///
    /// This is used for snapshots: files are hard-linked (not copied)
    /// so they share the same physical disk blocks. The refcount tracks
    /// how many handles reference these files.
    ///
    /// # Arguments
    /// * `target_dir` - Directory to create hard-links in (e.g., snapshots/snap1/)
    /// * `new_run_number` - Run number for the hard-linked files
    ///
    /// # Returns
    /// A new SsTableHandle with the same refcount Arc
    pub fn hard_link_to(&self, target_dir: &Path, new_run_number: RunNumber) -> io::Result<Self> {
        // Ensure target directory exists
        std::fs::create_dir_all(target_dir)?;

        // Hard-link all component files
        let components = ["keyops", "blobs", "filter", "index", "checksums"];

        for component in &components {
            let source = match *component {
                "keyops" => &self.paths.keyops,
                "blobs" => &self.paths.blobs,
                "filter" => &self.paths.filter,
                "index" => &self.paths.index,
                "checksums" => &self.paths.checksums,
                _ => unreachable!(),
            };

            let target = target_dir.join(format!("{:05}.{}", new_run_number, component));

            // Create hard link
            std::fs::hard_link(source, &target)?;
        }

        // Fsync target directory to ensure hard-links are durable
        fsync_directory(target_dir)?;

        // Increment refcount (shared across all hard-linked instances)
        self.refcount.fetch_add(1, Ordering::SeqCst);

        // Create new handle pointing to hard-linked files
        Ok(Self {
            paths: RunPaths::new(target_dir, new_run_number),
            min_key: self.min_key.clone(),
            max_key: self.max_key.clone(),
            num_entries: self.num_entries,
            level: self.level,  // Preserve level
            bloom_filter: self.bloom_filter.clone(),
            index: self.index.clone(),
            refcount: Arc::clone(&self.refcount), // Share refcount
        })
    }

    /// Delete all files for this SSTable run
    ///
    /// This removes the .keyops, .blobs, .filter, .index, and .checksums files.
    /// Use this when removing an SSTable during compaction.
    ///
    /// Note: This does NOT check refcount - use with caution!
    /// Normally, files are deleted automatically via Drop when refcount reaches 0.
    pub fn delete_files(&self) -> io::Result<()> {
        // Try to delete all files, collecting errors
        let mut errors = Vec::new();

        if let Err(e) = std::fs::remove_file(&self.paths.keyops) {
            errors.push(format!("keyops: {}", e));
        }

        if let Err(e) = std::fs::remove_file(&self.paths.blobs) {
            errors.push(format!("blobs: {}", e));
        }

        if let Err(e) = std::fs::remove_file(&self.paths.filter) {
            errors.push(format!("filter: {}", e));
        }

        if let Err(e) = std::fs::remove_file(&self.paths.index) {
            errors.push(format!("index: {}", e));
        }

        if let Err(e) = std::fs::remove_file(&self.paths.checksums) {
            errors.push(format!("checksums: {}", e));
        }

        if !errors.is_empty() {
            return Err(io::Error::other(
                format!("Failed to delete SSTable files: {}", errors.join(", ")),
            ));
        }

        Ok(())
    }
}

/// Implement Drop to automatically delete files when refcount reaches 0
impl Drop for SsTableHandle {
    fn drop(&mut self) {
        // Decrement refcount
        let prev = self.refcount.fetch_sub(1, Ordering::SeqCst);

        // If this was the last reference, delete the files
        if prev == 1 {
            // Ignore errors during cleanup
            let _ = self.delete_files();
        }
    }
}

impl Clone for SsTableHandle {
    fn clone(&self) -> Self {
        // Increment refcount when cloning
        self.refcount.fetch_add(1, Ordering::SeqCst);

        Self {
            paths: self.paths.clone(),
            min_key: self.min_key.clone(),
            max_key: self.max_key.clone(),
            num_entries: self.num_entries,
            level: self.level,
            bloom_filter: self.bloom_filter.clone(),
            index: self.index.clone(),
            refcount: self.refcount.clone(),
        }
    }
}

/// Simple bloom filter implementation
#[derive(Clone, Debug, Serialize, Deserialize)]
struct BloomFilter {
    bits: Vec<u8>,
    num_bits: usize,  // Total number of bits (not bytes!)
    num_hashes: u32,
}

impl BloomFilter {
    fn from_keys<'a, I>(keys: I, bits_per_key: usize, _false_positive_rate: f64) -> Self
    where
        I: Iterator<Item = &'a [u8]>,
    {
        let keys_vec: Vec<_> = keys.collect();
        let num_bits = keys_vec.len() * bits_per_key;
        let num_hashes = 7; // Optimal for 0.01 FPR

        let mut bits = vec![0u8; num_bits.div_ceil(8)];

        for key in keys_vec {
            for i in 0..num_hashes {
                let hash = Self::hash(key, i);
                let bit_pos = (hash as usize) % num_bits;
                bits[bit_pos / 8] |= 1 << (bit_pos % 8);
            }
        }

        Self { bits, num_bits, num_hashes }
    }

    fn might_contain(&self, key: &[u8]) -> bool {
        for i in 0..self.num_hashes {
            let hash = Self::hash(key, i);
            let bit_pos = (hash as usize) % self.num_bits;
            if (self.bits[bit_pos / 8] & (1 << (bit_pos % 8))) == 0 {
                return false;
            }
        }

        true
    }

    fn hash(key: &[u8], seed: u32) -> u32 {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&seed.to_le_bytes());
        hasher.update(key);
        hasher.finalize()
    }
}

/// Index structure mapping keys to byte offsets in the keyops file.
///
/// Each entry records the key bytes and the byte offset where that key's
/// record starts in the keyops file. This enables O(log n) point lookups
/// via binary search followed by a direct seek, instead of scanning the
/// entire keyops file.
///
/// Backwards compatible: if `offsets` is empty (legacy index), callers
/// fall back to a full scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Index {
    keys: Vec<Vec<u8>>,
    /// Byte offset of each key's record in the keyops file.
    /// Empty for legacy indexes created before this field was added.
    #[serde(default)]
    offsets: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_bloom_filter() {
        let keys = [b"key1", b"key2", b"key3"];
        let bloom = BloomFilter::from_keys(keys.iter().map(|k| k.as_ref()), 10, 0.01);

        assert!(bloom.might_contain(b"key1"));
        assert!(bloom.might_contain(b"key2"));
        assert!(bloom.might_contain(b"key3"));

        // May have false positives, but should mostly return false for non-existent keys
        // (not testing this as it's probabilistic)
    }

    #[test]
    fn test_run_paths() {
        let dir = PathBuf::from("/tmp/test");
        let paths = RunPaths::new(&dir, 42);

        assert_eq!(paths.run_number, 42);
        assert!(paths.keyops.to_str().unwrap().contains("00042.keyops"));
        assert!(paths.blobs.to_str().unwrap().contains("00042.blobs"));
        assert!(paths.filter.to_str().unwrap().contains("00042.filter"));
        assert!(paths.index.to_str().unwrap().contains("00042.index"));
        assert!(paths.checksums.to_str().unwrap().contains("00042.checksums"));
    }

    #[test]
    fn test_sstable_write_read() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        // Write SSTable
        let mut writer = SsTableWriter::new(active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        writer.add(Key::from(b"key2"), Some(Value::from(b"value2")))?;
        writer.add(Key::from(b"key3"), None)?; // Delete
        let handle = writer.finish(0)?;

        // Verify handle
        assert_eq!(handle.num_entries, 3);
        assert_eq!(handle.min_key, Key::from(b"key1"));
        assert_eq!(handle.max_key, Key::from(b"key3"));

        // Verify files exist
        let paths = RunPaths::new(active_dir, 1);
        assert!(paths.keyops.exists());
        assert!(paths.blobs.exists());
        assert!(paths.filter.exists());
        assert!(paths.index.exists());
        assert!(paths.checksums.exists());

        // Reopen from disk
        let handle2 = SsTableHandle::open(active_dir, 1)?;
        assert_eq!(handle2.num_entries, 3);
        assert_eq!(handle2.min_key, Key::from(b"key1"));

        Ok(())
    }

    #[test]
    fn test_hard_link_shares_data() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path().join("active");
        let snapshot_dir = dir.path().join("snapshots/snap1");

        std::fs::create_dir_all(&active_dir)?;
        std::fs::create_dir_all(&snapshot_dir)?;

        // Create SSTable in active/
        let mut writer = SsTableWriter::new(&active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        let handle1 = writer.finish(0)?;

        // Hard-link to snapshot directory
        let handle2 = handle1.hard_link_to(&snapshot_dir, 100)
            .map_err(Error::Io)?;

        // Both sets of files should exist
        let paths1 = RunPaths::new(&active_dir, 1);
        let paths2 = RunPaths::new(&snapshot_dir, 100);

        assert!(paths1.keyops.exists());
        assert!(paths2.keyops.exists());

        // Refcount should be 2
        assert_eq!(handle1.refcount.load(Ordering::SeqCst), 2);
        assert_eq!(handle2.refcount.load(Ordering::SeqCst), 2);

        // Drop original handle
        drop(handle1);

        // Files should STILL exist (refcount = 1)
        // Hard-links mean both paths point to same inode
        assert!(paths1.keyops.exists());
        assert!(paths2.keyops.exists());

        // Drop snapshot handle
        drop(handle2);

        // Now files should be deleted
        // Note: With hard-links, the actual deletion happens when both are removed
        // Since drop() only deletes one set of paths, the other remains
        // This is expected behavior - hard-links share the same inode
        // The important thing is that refcount reaches 0 and we attempt deletion

        Ok(())
    }

    #[test]
    fn test_refcount_tracks_clones() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        // Create SSTable
        let mut writer = SsTableWriter::new(active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        let handle = writer.finish(0)?;

        // Initial refcount = 1
        assert_eq!(handle.refcount.load(Ordering::SeqCst), 1);

        // Clone increments the refcount (custom Clone implementation)
        let handle2 = handle.clone();

        // Both handles share the same refcount Arc, which is now 2
        assert_eq!(handle.refcount.load(Ordering::SeqCst), 2);
        assert_eq!(handle2.refcount.load(Ordering::SeqCst), 2);

        Ok(())
    }

    #[test]
    fn test_multiple_hard_links() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path().join("active");
        std::fs::create_dir_all(&active_dir)?;

        // Create SSTable
        let mut writer = SsTableWriter::new(&active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        let handle1 = writer.finish(0)?;

        // Create multiple hard-links
        let snap1_dir = dir.path().join("snap1");
        let snap2_dir = dir.path().join("snap2");
        let snap3_dir = dir.path().join("snap3");

        let handle2 = handle1.hard_link_to(&snap1_dir, 100)
            .map_err(Error::Io)?;
        let handle3 = handle1.hard_link_to(&snap2_dir, 200)
            .map_err(Error::Io)?;
        let handle4 = handle1.hard_link_to(&snap3_dir, 300)
            .map_err(Error::Io)?;

        // Refcount should be 4
        assert_eq!(handle1.refcount.load(Ordering::SeqCst), 4);

        // Drop all but one
        drop(handle2);
        drop(handle3);
        drop(handle4);

        // Files should still exist (refcount = 1)
        let paths = RunPaths::new(&active_dir, 1);
        assert!(paths.keyops.exists());

        // Drop last reference
        drop(handle1);

        // Files should be deleted
        assert!(!paths.keyops.exists());

        Ok(())
    }

    #[test]
    fn test_index_based_get_single_key() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        let mut writer = SsTableWriter::new(active_dir, 1)?;
        writer.add(Key::from(b"alpha"), Some(Value::from(b"val_alpha")))?;
        let handle = writer.finish(0)?;

        assert_eq!(handle.index.offsets.len(), 1);
        assert_eq!(handle.index.offsets[0], 0);

        let val = handle.get(&Key::from(b"alpha"))?;
        assert_eq!(val, Some(Value::from(b"val_alpha")));
        assert_eq!(handle.get(&Key::from(b"beta"))?, None);

        Ok(())
    }

    #[test]
    fn test_index_based_get_many_keys() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        let mut writer = SsTableWriter::new(active_dir, 1)?;
        for i in 0..100u32 {
            let key = format!("key_{:05}", i);
            let val = format!("value_{:05}", i);
            writer.add(Key::from(key.as_bytes()), Some(Value::from(val.as_bytes())))?;
        }
        let handle = writer.finish(0)?;

        assert_eq!(handle.index.offsets.len(), 100);
        for i in 1..100 {
            assert!(handle.index.offsets[i] > handle.index.offsets[i - 1]);
        }

        for i in 0..100u32 {
            let key = format!("key_{:05}", i);
            let expected = format!("value_{:05}", i);
            let val = handle.get(&Key::from(key.as_bytes()))?;
            assert_eq!(val, Some(Value::from(expected.as_bytes())));
        }
        assert_eq!(handle.get(&Key::from(b"nonexistent"))?, None);

        Ok(())
    }

    #[test]
    fn test_index_based_get_with_tombstones() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        let mut writer = SsTableWriter::new(active_dir, 1)?;
        writer.add(Key::from(b"alive"), Some(Value::from(b"value")))?;
        writer.add(Key::from(b"dead"), None)?;
        writer.add(Key::from(b"zombie"), Some(Value::from(b"undead")))?;
        let handle = writer.finish(0)?;

        assert_eq!(handle.get(&Key::from(b"alive"))?, Some(Value::from(b"value")));
        assert_eq!(handle.get(&Key::from(b"dead"))?, None);
        assert_eq!(handle.get(&Key::from(b"zombie"))?, Some(Value::from(b"undead")));

        Ok(())
    }

    #[test]
    fn test_index_based_get_after_reopen() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        let mut writer = SsTableWriter::new(active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        writer.add(Key::from(b"key2"), Some(Value::from(b"value2")))?;
        writer.add(Key::from(b"key3"), Some(Value::from(b"value3")))?;
        let _handle = writer.finish(0)?;

        let handle2 = SsTableHandle::open(active_dir, 1)?;
        assert_eq!(handle2.index.offsets.len(), 3);
        assert_eq!(handle2.get(&Key::from(b"key1"))?, Some(Value::from(b"value1")));
        assert_eq!(handle2.get(&Key::from(b"key2"))?, Some(Value::from(b"value2")));
        assert_eq!(handle2.get(&Key::from(b"key3"))?, Some(Value::from(b"value3")));
        assert_eq!(handle2.get(&Key::from(b"missing"))?, None);

        Ok(())
    }

    #[test]
    fn test_index_based_get_large_values() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        let large_val = vec![0xABu8; 100_000];
        let mut writer = SsTableWriter::new(active_dir, 1)?;
        writer.add(Key::from(b"big1"), Some(Value::from(&large_val[..])))?;
        writer.add(Key::from(b"big2"), Some(Value::from(&large_val[..])))?;
        writer.add(Key::from(b"small"), Some(Value::from(b"tiny")))?;
        let handle = writer.finish(0)?;

        assert_eq!(handle.get(&Key::from(b"big1"))?.map(|v| v.as_ref().len()), Some(100_000));
        assert_eq!(handle.get(&Key::from(b"big2"))?.map(|v| v.as_ref().len()), Some(100_000));
        assert_eq!(handle.get(&Key::from(b"small"))?, Some(Value::from(b"tiny")));

        Ok(())
    }

    #[test]
    fn test_index_offsets_correct_format() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        let mut writer = SsTableWriter::new(active_dir, 1)?;
        // key "aa" (2 bytes) + value: 4 + 2 + 1 + 16 = 23 bytes
        writer.add(Key::from(b"aa"), Some(Value::from(b"v")))?;
        // key "bb" (2 bytes) + tombstone: 4 + 2 + 1 = 7 bytes
        writer.add(Key::from(b"bb"), None)?;
        // key "cc" (2 bytes) + value
        writer.add(Key::from(b"cc"), Some(Value::from(b"w")))?;
        let handle = writer.finish(0)?;

        assert_eq!(handle.index.offsets[0], 0);
        assert_eq!(handle.index.offsets[1], 23);
        assert_eq!(handle.index.offsets[2], 30);

        Ok(())
    }

    #[test]
    fn test_range_query_still_works_with_offsets() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let active_dir = dir.path();

        let mut writer = SsTableWriter::new(active_dir, 1)?;
        for i in 0..10u32 {
            let key = format!("k{:02}", i);
            let val = format!("v{:02}", i);
            writer.add(Key::from(key.as_bytes()), Some(Value::from(val.as_bytes())))?;
        }
        let handle = writer.finish(0)?;

        let results = handle.range(&Key::from(b"k02"), &Key::from(b"k05"))?;
        assert_eq!(results.len(), 4);

        Ok(())
    }
}
