// SSTable (Sorted String Table) implementation
// File format:
//   [Header: metadata]
//   [Data blocks: sorted key-value pairs]
//   [Index block: key offsets for binary search]
//   [Bloom filter: for fast negative lookups]
//   [Footer: offsets to index and bloom filter]
//
// NOTE: This is the old SSTable implementation, kept for reference.
// The active implementation is in sstable_new.rs.
#![allow(dead_code)]

use std::fs::{File, OpenOptions};
use std::io::{Write, Read, Seek, SeekFrom, BufWriter};
use std::path::PathBuf;
use std::hash::{Hash, Hasher};
use crate::{Key, Value, Result, Error};
use serde::{Serialize, Deserialize};

const SSTABLE_MAGIC: u32 = 0x4C534D54; // "LSMT" in hex
const SSTABLE_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsTableHeader {
    magic: u32,
    version: u16,
    num_entries: u64,
    min_key: Vec<u8>,
    max_key: Vec<u8>,
    compression: CompressionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionType {
    None,
    Lz4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsTableFooter {
    index_offset: u64,
    index_size: u64,
    bloom_offset: u64,
    bloom_size: u64,
    header_checksum: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexEntry {
    key: Vec<u8>,
    offset: u64,
    size: u32,
}

pub struct SsTableWriter {
    file: BufWriter<File>,
    path: PathBuf,
    entries: Vec<(Key, Option<Value>)>,
    current_offset: u64,
}

impl SsTableWriter {
    pub fn new(path: PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        
        Ok(Self {
            file: BufWriter::new(file),
            path,
            entries: Vec::new(),
            current_offset: 0,
        })
    }
    
    pub fn add(&mut self, key: Key, value: Option<Value>) {
        self.entries.push((key, value));
    }
    
    pub fn finish(mut self) -> Result<SsTableHandle> {
        if self.entries.is_empty() {
            return Err(Error::InvalidOperation("Cannot create empty SSTable".to_string()));
        }
        
        // Sort entries by key
        self.entries.sort_by(|a, b| a.0.cmp(&b.0));
        
        let min_key = self.entries.first().unwrap().0.as_ref().to_vec();
        let max_key = self.entries.last().unwrap().0.as_ref().to_vec();
        let num_entries = self.entries.len() as u64;
        
        // Write header placeholder
        let header = SsTableHeader {
            magic: SSTABLE_MAGIC,
            version: SSTABLE_VERSION,
            num_entries,
            min_key: min_key.clone(),
            max_key: max_key.clone(),
            compression: CompressionType::None, // Compression not used - blockchain data doesn't compress well
        };
        
        let header_bytes = bincode::serialize(&header)?;
        let header_size = header_bytes.len() as u64;
        self.file.write_all(&header_bytes)?;
        self.current_offset += header_size;
        
        // Write data blocks and build index
        let mut index_entries = Vec::new();
        
        for (key, value_opt) in &self.entries {
            let entry_offset = self.current_offset;
            
            // Serialize entry
            let entry = (key, value_opt);
            let entry_bytes = bincode::serialize(&entry)?;
            let entry_size = entry_bytes.len() as u32;
            
            // Write entry
            self.file.write_all(&entry_bytes)?;
            self.current_offset += entry_bytes.len() as u64;
            
            // Add to index
            index_entries.push(IndexEntry {
                key: key.as_ref().to_vec(),
                offset: entry_offset,
                size: entry_size,
            });
        }
        
        // Write index block
        let index_offset = self.current_offset;
        let index_bytes = bincode::serialize(&index_entries)?;
        let index_size = index_bytes.len() as u64;
        self.file.write_all(&index_bytes)?;
        self.current_offset += index_size;
        
        // Build and write bloom filter
        let bloom_offset = self.current_offset;
        let bloom_filter = BloomFilter::from_keys(
            self.entries.iter().map(|(k, _)| k.as_ref()),
            10, // bits per key
            0.01, // false positive rate
        );
        let bloom_bytes = bincode::serialize(&bloom_filter)?;
        let bloom_size = bloom_bytes.len() as u64;
        self.file.write_all(&bloom_bytes)?;
        self.current_offset += bloom_size;
        
        // Write footer
        let footer = SsTableFooter {
            index_offset,
            index_size,
            bloom_offset,
            bloom_size,
            header_checksum: crc32fast::hash(&header_bytes),
        };
        let footer_bytes = bincode::serialize(&footer)?;
        self.file.write_all(&footer_bytes)?;
        
        // Write footer size at end
        let footer_size = footer_bytes.len() as u32;
        self.file.write_all(&footer_size.to_le_bytes())?;
        
        self.file.flush()?;
        
        Ok(SsTableHandle {
            path: self.path,
            min_key: Key::from(&min_key),
            max_key: Key::from(&max_key),
            num_entries,
            bloom_filter,
        })
    }
}

#[derive(Clone)]
pub struct SsTableHandle {
    pub path: PathBuf,
    pub min_key: Key,
    pub max_key: Key,
    pub num_entries: u64,
    bloom_filter: BloomFilter,
}

impl SsTableHandle {
    pub fn open(path: PathBuf) -> Result<Self> {
        let mut file = File::open(&path)?;
        
        // Read footer size from end
        file.seek(SeekFrom::End(-4))?;
        let mut footer_size_bytes = [0u8; 4];
        file.read_exact(&mut footer_size_bytes)?;
        let footer_size = u32::from_le_bytes(footer_size_bytes) as u64;
        
        // Read footer
        file.seek(SeekFrom::End(-4 - footer_size as i64))?;
        let mut footer_bytes = vec![0u8; footer_size as usize];
        file.read_exact(&mut footer_bytes)?;
        let footer: SsTableFooter = bincode::deserialize(&footer_bytes)?;
        
        // Read header
        file.seek(SeekFrom::Start(0))?;
        let mut header_bytes = vec![0u8; footer.index_offset as usize];
        file.read_exact(&mut header_bytes)?;
        let header: SsTableHeader = bincode::deserialize(&header_bytes)?;
        
        // Verify header checksum
        let computed_checksum = crc32fast::hash(&bincode::serialize(&header)?);
        if computed_checksum != footer.header_checksum {
            return Err(Error::Corruption("SSTable header checksum mismatch".to_string()));
        }
        
        // Read bloom filter
        file.seek(SeekFrom::Start(footer.bloom_offset))?;
        let mut bloom_bytes = vec![0u8; footer.bloom_size as usize];
        file.read_exact(&mut bloom_bytes)?;
        let bloom_filter: BloomFilter = bincode::deserialize(&bloom_bytes)?;
        
        Ok(Self {
            path,
            min_key: Key::from(&header.min_key),
            max_key: Key::from(&header.max_key),
            num_entries: header.num_entries,
            bloom_filter,
        })
    }
    
    pub fn get(&self, key: &Key) -> Result<Option<Value>> {
        // Check bloom filter first
        if !self.bloom_filter.might_contain(key.as_ref()) {
            return Ok(None);
        }
        
        // Read footer to get index offset
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::End(-4))?;
        let mut footer_size_bytes = [0u8; 4];
        file.read_exact(&mut footer_size_bytes)?;
        let footer_size = u32::from_le_bytes(footer_size_bytes) as u64;
        
        file.seek(SeekFrom::End(-4 - footer_size as i64))?;
        let mut footer_bytes = vec![0u8; footer_size as usize];
        file.read_exact(&mut footer_bytes)?;
        let footer: SsTableFooter = bincode::deserialize(&footer_bytes)?;
        
        // Read index
        file.seek(SeekFrom::Start(footer.index_offset))?;
        let mut index_bytes = vec![0u8; footer.index_size as usize];
        file.read_exact(&mut index_bytes)?;
        let index: Vec<IndexEntry> = bincode::deserialize(&index_bytes)?;
        
        // Binary search in index
        let result = index.binary_search_by(|entry| entry.key.as_slice().cmp(key.as_ref()));
        
        match result {
            Ok(idx) => {
                let entry = &index[idx];
                
                // Read the data entry
                file.seek(SeekFrom::Start(entry.offset))?;
                let mut entry_bytes = vec![0u8; entry.size as usize];
                file.read_exact(&mut entry_bytes)?;
                
                let (_, value_opt): (Key, Option<Value>) = bincode::deserialize(&entry_bytes)?;
                Ok(value_opt)
            }
            Err(_) => Ok(None),
        }
    }
    
    pub fn range(&self, from: &Key, to: &Key) -> Result<Vec<(Key, Value)>> {
        let entries = self.range_with_tombstones(from, to)?;
        Ok(entries.into_iter().filter_map(|(k, v)| v.map(|val| (k, val))).collect())
    }
    
    /// Range query that includes tombstones (for compaction)
    pub fn range_with_tombstones(&self, from: &Key, to: &Key) -> Result<Vec<(Key, Option<Value>)>> {
        // Check if range overlaps with this SSTable
        if to < &self.min_key || from > &self.max_key {
            return Ok(Vec::new());
        }
        
        let mut file = File::open(&self.path)?;
        
        // Read footer
        file.seek(SeekFrom::End(-4))?;
        let mut footer_size_bytes = [0u8; 4];
        file.read_exact(&mut footer_size_bytes)?;
        let footer_size = u32::from_le_bytes(footer_size_bytes) as u64;
        
        file.seek(SeekFrom::End(-4 - footer_size as i64))?;
        let mut footer_bytes = vec![0u8; footer_size as usize];
        file.read_exact(&mut footer_bytes)?;
        let footer: SsTableFooter = bincode::deserialize(&footer_bytes)?;
        
        // Read index
        file.seek(SeekFrom::Start(footer.index_offset))?;
        let mut index_bytes = vec![0u8; footer.index_size as usize];
        file.read_exact(&mut index_bytes)?;
        let index: Vec<IndexEntry> = bincode::deserialize(&index_bytes)?;
        
        let mut results = Vec::new();
        
        for entry in index {
            let key = Key::from(&entry.key);
            
            if &key >= from && &key <= to {
                // Read the data entry
                file.seek(SeekFrom::Start(entry.offset))?;
                let mut entry_bytes = vec![0u8; entry.size as usize];
                file.read_exact(&mut entry_bytes)?;
                
                let (k, value_opt): (Key, Option<Value>) = bincode::deserialize(&entry_bytes)?;
                results.push((k, value_opt));
            }
        }
        
        Ok(results)
    }
}

// Simple Bloom Filter implementation
#[derive(Clone, Serialize, Deserialize)]
pub struct BloomFilter {
    bits: Vec<u64>,
    num_bits: usize,
    num_hashes: usize,
}

impl BloomFilter {
    pub fn from_keys<'a, I>(keys: I, bits_per_key: usize, _fp_rate: f64) -> Self
    where
        I: Iterator<Item = &'a [u8]>,
    {
        let keys: Vec<_> = keys.collect();
        let num_keys = keys.len().max(1);
        let num_bits = num_keys * bits_per_key;
        let num_hashes = ((bits_per_key as f64 * 0.69) as usize).clamp(1, 30);
        
        let num_words = num_bits.div_ceil(64);
        let mut bits = vec![0u64; num_words];
        
        for key in keys {
            let hash1 = Self::hash1(key);
            let hash2 = Self::hash2(key);
            
            for i in 0..num_hashes {
                let hash = hash1.wrapping_add((i as u64).wrapping_mul(hash2));
                let bit_pos = (hash % num_bits as u64) as usize;
                let word_idx = bit_pos / 64;
                let bit_idx = bit_pos % 64;
                bits[word_idx] |= 1u64 << bit_idx;
            }
        }
        
        Self {
            bits,
            num_bits,
            num_hashes,
        }
    }
    
    pub fn might_contain(&self, key: &[u8]) -> bool {
        let hash1 = Self::hash1(key);
        let hash2 = Self::hash2(key);
        
        for i in 0..self.num_hashes {
            let hash = hash1.wrapping_add((i as u64).wrapping_mul(hash2));
            let bit_pos = (hash % self.num_bits as u64) as usize;
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            
            if (self.bits[word_idx] & (1u64 << bit_idx)) == 0 {
                return false;
            }
        }
        
        true
    }
    
    fn hash1(key: &[u8]) -> u64 {
        let hash = crc32fast::hash(key);
        hash as u64
    }
    
    fn hash2(key: &[u8]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        Hash::hash_slice(key, &mut hasher);
        Hasher::finish(&hasher)
    }
}
