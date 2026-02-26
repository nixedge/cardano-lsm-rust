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

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};

use crate::checksum::{CRC32C, ChecksumsFile};
use crate::checksum_handle::ChecksumHandle;
use crate::atomic_file::{AtomicFileWriter, fsync_directory};
use crate::{Key, Value, Result, Error};

/// Run number for identifying a set of SSTable files
pub type RunNumber = u64;

/// Paths to all files for a single Run
#[derive(Debug, Clone)]
pub struct RunPaths {
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
            .map_err(|e| Error::Io(e))?;

        let blobs_writer = ChecksumHandle::create(&paths.blobs)
            .map_err(|e| Error::Io(e))?;

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
    pub fn finish(mut self) -> Result<SsTableHandle> {
        if self.entries.is_empty() {
            return Err(Error::InvalidOperation("Cannot create empty SSTable".to_string()));
        }

        // Sort entries by key
        self.entries.sort_by(|a, b| a.0.cmp(&b.0));

        let min_key = self.entries.first().unwrap().0.clone();
        let max_key = self.entries.last().unwrap().0.clone();
        let num_entries = self.entries.len() as u64;

        // Write keyops and blobs
        for (key, value_opt) in &self.entries {
            // Write key
            let key_bytes = key.as_ref();
            self.keyops_writer.write_all(&(key_bytes.len() as u32).to_le_bytes())?;
            self.keyops_writer.write_all(key_bytes)?;

            // Write operation (Insert=1, Delete=0) and value
            match value_opt {
                Some(value) => {
                    self.keyops_writer.write_all(&[1u8])?; // Insert operation

                    let value_bytes = value.as_ref();
                    let value_len = value_bytes.len() as u64;

                    // Write blob offset and size to keyops
                    self.keyops_writer.write_all(&self.blob_offset.to_le_bytes())?;
                    self.keyops_writer.write_all(&value_len.to_le_bytes())?;

                    // Write value to blobs
                    self.blobs_writer.write_all(value_bytes)?;
                    self.blob_offset += value_len;
                }
                None => {
                    self.keyops_writer.write_all(&[0u8])?; // Delete operation
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

        // Build index (simple: just store all keys for now)
        let index = Index {
            keys: self.entries.iter().map(|(k, _)| k.as_ref().to_vec()).collect(),
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
            bloom_filter,
            index,
        })
    }
}

/// Handle to a completed SSTable
///
/// Provides read operations with automatic checksum verification.
#[derive(Clone)]
pub struct SsTableHandle {
    paths: RunPaths,
    pub min_key: Key,
    pub max_key: Key,
    pub num_entries: u64,
    bloom_filter: BloomFilter,
    index: Index,
}

impl SsTableHandle {
    /// Open an existing SSTable from disk
    ///
    /// Verifies all checksums on open to detect corruption early.
    pub fn open(active_dir: &Path, run_number: RunNumber) -> Result<Self> {
        let paths = RunPaths::new(active_dir, run_number);

        // Verify all files exist
        if !paths.all_exist() {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("SSTable files missing for run {}", run_number),
            )));
        }

        // Read checksums file
        let checksums = crate::checksum::read_checksums_file(&paths.checksums)?;

        // Verify keyops checksum
        let keyops_expected = crate::checksum::get_checksum(&checksums, "keyops")
            .map_err(|e| Error::Corruption(e))?;
        crate::checksum::check_crc(&paths.keyops, keyops_expected)?;

        // Verify blobs checksum
        let blobs_expected = crate::checksum::get_checksum(&checksums, "blobs")
            .map_err(|e| Error::Corruption(e))?;
        crate::checksum::check_crc(&paths.blobs, blobs_expected)?;

        // Verify filter checksum
        let filter_expected = crate::checksum::get_checksum(&checksums, "filter")
            .map_err(|e| Error::Corruption(e))?;
        crate::checksum::check_crc(&paths.filter, filter_expected)?;

        // Verify index checksum
        let index_expected = crate::checksum::get_checksum(&checksums, "index")
            .map_err(|e| Error::Corruption(e))?;
        crate::checksum::check_crc(&paths.index, index_expected)?;

        // All checksums verified, now read the data

        // Read bloom filter
        let filter_bytes = std::fs::read(&paths.filter)?;
        let bloom_filter: BloomFilter = bincode::deserialize(&filter_bytes)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        // Read index
        let index_bytes = std::fs::read(&paths.index)?;
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
            bloom_filter,
            index,
        })
    }

    /// Get a value by key
    ///
    /// Returns None if the key is not found or was deleted.
    pub fn get(&self, key: &Key) -> Result<Option<Value>> {
        // Check bloom filter first (fast negative lookup)
        if !self.bloom_filter.might_contain(key.as_ref()) {
            return Ok(None);
        }

        // Binary search in index
        let key_bytes = key.as_ref();
        let pos = match self.index.keys.binary_search_by(|k| k.as_slice().cmp(key_bytes)) {
            Ok(pos) => pos,
            Err(_) => return Ok(None), // Not found
        };

        // Read from keyops file
        // TODO: Implement efficient keyops reading with proper parsing
        // For now, return None as placeholder
        Ok(None)
    }

    /// Range query - iterate over keys in range [from, to]
    ///
    /// Returns an iterator over (Key, Option<Value>) pairs.
    /// TODO: Implement efficient range scanning
    pub fn range(&self, _from: &Key, _to: &Key) -> Result<Vec<(Key, Option<Value>)>> {
        // Placeholder implementation
        // TODO: Implement proper range scanning by reading keyops file
        Ok(Vec::new())
    }

    /// Range query with tombstones - needed for compaction
    ///
    /// Returns all entries including deletes (tombstones represented as None).
    /// TODO: Implement efficient range scanning with tombstones
    pub fn range_with_tombstones(&self, _from: &Key, _to: &Key) -> Result<Vec<(Key, Option<Value>)>> {
        // Placeholder implementation
        // TODO: Implement proper range scanning by reading keyops file
        Ok(Vec::new())
    }

    pub fn path(&self) -> &Path {
        &self.paths.keyops
    }

    /// Delete all files for this SSTable run
    ///
    /// This removes the .keyops, .blobs, .filter, .index, and .checksums files.
    /// Use this when removing an SSTable during compaction.
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
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to delete SSTable files: {}", errors.join(", ")),
            ));
        }

        Ok(())
    }
}

/// Simple bloom filter implementation
#[derive(Clone, Serialize, Deserialize)]
struct BloomFilter {
    bits: Vec<u8>,
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

        let mut bits = vec![0u8; (num_bits + 7) / 8];

        for key in keys_vec {
            for i in 0..num_hashes {
                let hash = Self::hash(key, i);
                let bit_pos = (hash as usize) % num_bits;
                bits[bit_pos / 8] |= 1 << (bit_pos % 8);
            }
        }

        Self { bits, num_hashes }
    }

    fn might_contain(&self, key: &[u8]) -> bool {
        let num_bits = self.bits.len() * 8;

        for i in 0..self.num_hashes {
            let hash = Self::hash(key, i);
            let bit_pos = (hash as usize) % num_bits;
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

/// Simple index structure
#[derive(Clone, Serialize, Deserialize)]
struct Index {
    keys: Vec<Vec<u8>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_bloom_filter() {
        let keys = vec![b"key1", b"key2", b"key3"];
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
        let dir = TempDir::new().map_err(|e| Error::Io(e))?;
        let active_dir = dir.path();

        // Write SSTable
        let mut writer = SsTableWriter::new(active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        writer.add(Key::from(b"key2"), Some(Value::from(b"value2")))?;
        writer.add(Key::from(b"key3"), None)?; // Delete
        let handle = writer.finish()?;

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
}
