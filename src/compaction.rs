//! Compaction strategies for LSM tree
//!
//! This module implements three compaction strategies:
//! - **Tiered**: Merges SSTables of similar size, optimized for write-heavy workloads
//! - **Leveled**: Organizes SSTables into levels, optimized for read-heavy workloads
//! - **LazyLevelling** (Hybrid): Uses tiering for L0 to L(max-1), leveling for L(max)
//!
//! The LazyLevelling policy is the recommended strategy for blockchain indexing,
//! balancing write amplification and space amplification.

use crate::{Key, Value, Result};
use crate::sstable::{SsTableHandle, SsTableWriter, RunNumber};
use std::path::Path;
use std::collections::BTreeMap;
use serde::{Serialize, Deserialize};

/// Compaction strategy for the LSM tree
///
/// Determines how SSTables are merged during compaction to maintain performance
/// and manage disk space usage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CompactionStrategy {
    /// Tiered compaction: merges SSTables of similar size
    ///
    /// Best for write-heavy workloads. Groups SSTables into size tiers
    /// and merges within each tier when enough tables accumulate.
    Tiered {
        /// Maximum size ratio between SSTables in the same tier (e.g., 2.0 = within 2x size)
        size_ratio: f64,
        /// Minimum number of SSTables required before triggering compaction
        min_merge_width: usize,
        /// Maximum number of SSTables to merge in one compaction operation
        max_merge_width: usize,
    },
    /// Leveled compaction: organizes SSTables into fixed-size levels
    ///
    /// Best for read-heavy workloads. Each level has a target size,
    /// and SSTables are compacted level-by-level.
    Leveled {
        /// Size multiplier between levels (e.g., 10.0 = each level is 10x larger)
        size_ratio: f64,
        /// Maximum number of levels in the tree
        max_level: u8,
    },
    /// Hybrid compaction: combines tiering and leveling
    ///
    /// Uses one strategy for early levels and another for later levels.
    /// LazyLevelling uses tiering for L0 to L(max-1) and leveling for L(max).
    Hybrid {
        /// Strategy for initial levels (typically tiering)
        l0_strategy: Box<CompactionStrategy>,
        /// Strategy for later levels (typically leveling)
        ln_strategy: Box<CompactionStrategy>,
        /// Level at which to transition between strategies
        transition_level: u8,
    },
}

/// Compaction manager for the LSM tree
///
/// Selects which SSTables need compaction and executes merge operations
/// using the LazyLevelling policy.
pub struct Compactor {
}

impl Compactor {
    /// Creates a new compactor
    pub fn new() -> Self {
        Self {}
    }

    /// Select compaction for level-based LSM tree using LazyLevelling policy
    ///
    /// LazyLevelling:
    /// - L0 to L(max-1): Tiering (multiple runs per level)
    /// - L(max): Leveling (single merged run)
    /// - Compact level i to level i+1 when level i exceeds size threshold
    pub fn select_level_compaction(
        &self,
        levels: &[Vec<SsTableHandle>],
        max_level: u8,
        size_ratio: usize,
    ) -> Option<LevelCompactionJob> {
        // Check each level from L0 to L(max-1)
        for level_idx in 0..max_level as usize {
            let level_size: u64 = levels[level_idx].iter().map(|r| r.num_entries).sum();
            let target_size = Self::level_target_size(level_idx, size_ratio);

            if level_size > target_size {
                // This level needs compaction
                let source_runs: Vec<usize> = (0..levels[level_idx].len()).collect();
                let target_level = (level_idx + 1) as u8;

                return Some(LevelCompactionJob {
                    source_level: level_idx as u8,
                    target_level,
                    source_runs,
                    target_level_runs: if (target_level as usize) < levels.len() {
                        levels[target_level as usize].clone()
                    } else {
                        Vec::new()
                    },
                });
            }
        }

        None
    }

    /// Calculate target size for a level
    ///
    /// L0: 10K entries (base)
    /// L1: 10K * size_ratio
    /// L2: 10K * size_ratio^2
    /// etc.
    fn level_target_size(level: usize, size_ratio: usize) -> u64 {
        10_000 * (size_ratio as u64).pow(level as u32)
    }

    /// Execute a level-based compaction job
    ///
    /// For LazyLevelling:
    /// - If target is max level: Leveling (merge everything into single run, remove tombstones)
    /// - Otherwise: Tiering (merge source runs, keep as separate run, preserve tombstones)
    pub fn compact_levels(
        &self,
        job: LevelCompactionJob,
        source_level_runs: &[SsTableHandle],
        active_dir: &Path,
        run_number: RunNumber,
        max_level: u8,
    ) -> Result<CompactionResult> {
        // Collect all entries from source runs
        let mut all_entries: BTreeMap<Key, Option<Value>> = BTreeMap::new();

        // Sort source runs by run_number in ASCENDING order (oldest first)
        // so that newer values overwrite older ones when inserted into BTreeMap
        let mut sorted_indices: Vec<usize> = job.source_runs.clone();
        sorted_indices.sort_by_key(|&idx| source_level_runs[idx].run_number());

        for &idx in &sorted_indices {
            let sstable = &source_level_runs[idx];

            // Read all entries including tombstones
            let entries = sstable.range_with_tombstones(&Key::from(b""), &Key::from([0xFF; 256]))?;

            for (key, value_opt) in entries {
                // Later (newer) entries overwrite earlier (older) ones
                all_entries.insert(key, value_opt);
            }
        }

        // For bottom level (leveling): merge with overlapping runs and remove tombstones
        // For other levels (tiering): keep tombstones, don't merge with target level
        let is_bottom_level = job.target_level == max_level;

        if is_bottom_level {
            // Leveling: merge with ALL runs in target level, remove tombstones
            for target_run in &job.target_level_runs {
                let entries = target_run.range_with_tombstones(&Key::from(b""), &Key::from([0xFF; 256]))?;

                for (key, value_opt) in entries {
                    // Only insert if not already present (source has priority)
                    all_entries.entry(key).or_insert(value_opt);
                }
            }

            // Remove tombstones at bottom level
            all_entries.retain(|_, v| v.is_some());
        }

        if all_entries.is_empty() {
            return Ok(CompactionResult {
                output: None,
                inputs_to_remove: job.source_runs.clone(),
                bytes_read: 0,
                bytes_written: 0,
            });
        }

        // Create new SSTable
        let mut writer = SsTableWriter::new(active_dir, run_number)?;

        let mut bytes_written = 0u64;
        for (key, value_opt) in all_entries {
            let key_len = key.as_ref().len() as u64;
            let value_len = value_opt.as_ref().map(|v| v.as_ref().len() as u64).unwrap_or(0);
            bytes_written += key_len + value_len;
            let _ = writer.add(key, value_opt);
        }

        let output_handle = writer.finish(job.target_level)?;

        Ok(CompactionResult {
            output: Some(output_handle),
            inputs_to_remove: job.source_runs.clone(),
            bytes_read: bytes_written,
            bytes_written,
        })
    }

    /// Execute a compaction job
    pub fn compact(&self, job: CompactionJob, sstables: &[SsTableHandle], active_dir: &Path, run_number: RunNumber) -> Result<CompactionResult> {
        // Collect all entries from input SSTables (including tombstones!)
        let mut all_entries: BTreeMap<Key, Option<Value>> = BTreeMap::new();

        // Sort inputs by run_number in ASCENDING order (oldest first)
        // so that newer values overwrite older ones when inserted into BTreeMap
        let mut sorted_indices: Vec<usize> = job.inputs.clone();
        sorted_indices.sort_by_key(|&idx| sstables[idx].run_number());

        for &idx in &sorted_indices {
            let sstable = &sstables[idx];

            // Read all entries from this SSTable INCLUDING tombstones
            let entries = sstable.range_with_tombstones(&Key::from(b""), &Key::from([0xFF; 256]))?;

            for (key, value_opt) in entries {
                // Later (newer) entries overwrite earlier (older) ones (including tombstones)
                all_entries.insert(key, value_opt);
            }
        }

        // Only remove tombstones if we're compacting ALL SSTables
        // Otherwise, tombstones need to persist to shadow older tables
        let compacting_all = job.inputs.len() == sstables.len();

        if compacting_all {
            all_entries.retain(|_, v| v.is_some());
        }

        if all_entries.is_empty() {
            // All entries were tombstones, just delete the input SSTables
            return Ok(CompactionResult {
                output: None,
                inputs_to_remove: job.inputs.clone(),
                bytes_read: 0,
                bytes_written: 0,
            });
        }

        // Create new SSTable with merged data using new format
        let mut writer = SsTableWriter::new(active_dir, run_number)?;
        
        let mut bytes_written = 0u64;
        for (key, value_opt) in all_entries {
            let key_len = key.as_ref().len() as u64;
            let value_len = value_opt.as_ref().map(|v| v.as_ref().len() as u64).unwrap_or(0);
            bytes_written += key_len + value_len;
            let _ = writer.add(key, value_opt);
        }

        let output_handle = writer.finish(0)?;
        
        Ok(CompactionResult {
            output: Some(output_handle),
            inputs_to_remove: job.inputs.clone(),
            bytes_read: bytes_written, // Approximate
            bytes_written,
        })
    }
}

/// Compaction job specification
///
/// Describes which SSTables should be merged together during compaction.
/// Returned by [`Compactor::select_compaction`] when compaction is needed.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CompactionJob {
    /// Indices of SSTables to compact (refers to positions in the input slice)
    pub inputs: Vec<usize>,
    /// Strategy used to select this compaction
    pub strategy: CompactionStrategy,
}

/// Level-based compaction job for LazyLevelling policy
///
/// Describes a compaction operation that moves data from one level to another,
/// used by the LazyLevelling strategy.
#[derive(Debug, Clone)]
pub struct LevelCompactionJob {
    /// Source level number (0-indexed)
    pub source_level: u8,
    /// Target level number where compacted data will be written
    pub target_level: u8,
    /// Indices of runs within the source level to compact
    pub source_runs: Vec<usize>,
    /// All runs in the target level (for overlap detection in leveling policy)
    pub target_level_runs: Vec<SsTableHandle>,
}

/// Result of a compaction operation
///
/// Contains the newly created SSTable (if any) and metadata about the operation.
#[allow(dead_code)]
pub struct CompactionResult {
    /// Handle to the newly created merged SSTable, or `None` if all entries were tombstones
    pub output: Option<SsTableHandle>,
    /// Indices of input SSTables that should be deleted after compaction
    pub inputs_to_remove: Vec<usize>,
    /// Total bytes read from input SSTables during compaction
    pub bytes_read: u64,
    /// Total bytes written to the output SSTable
    pub bytes_written: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tiered_grouping() {
        // Test that tiered compaction groups similar-sized tables
        let _strategy = CompactionStrategy::Tiered {
            size_ratio: 2.0,
            min_merge_width: 3,
            max_merge_width: 10,
        };
        
        // Would need actual SSTables to test properly
        // This is a placeholder for unit testing
    }
    
    #[test]
    fn test_leveled_selection() {
        // Test that leveled compaction selects appropriate tables
        let _strategy = CompactionStrategy::Leveled {
            size_ratio: 10.0,
            max_level: 7,
        };
        
        // Would need actual SSTables to test properly
    }
}
