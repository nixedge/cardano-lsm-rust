// Compaction strategies for LSM tree
// Implements Tiered, Leveled, and Hybrid compaction

use crate::{Key, Value, Result};
use crate::sstable_new::{SsTableHandle, SsTableWriter, RunNumber};
use std::path::PathBuf;
use std::collections::BTreeMap;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CompactionStrategy {
    Tiered {
        size_ratio: f64,
        min_merge_width: usize,
        max_merge_width: usize,
    },
    Leveled {
        size_ratio: f64,
        max_level: u8,
    },
    Hybrid {
        l0_strategy: Box<CompactionStrategy>,
        ln_strategy: Box<CompactionStrategy>,
        transition_level: u8,
    },
}

pub struct Compactor {
    strategy: CompactionStrategy,
    base_path: PathBuf,
}

impl Compactor {
    pub fn new(strategy: CompactionStrategy, base_path: PathBuf) -> Self {
        Self {
            strategy,
            base_path,
        }
    }
    
    /// Select which SSTables need compaction
    pub fn select_compaction(&self, sstables: &[SsTableHandle]) -> Option<CompactionJob> {
        match &self.strategy {
            CompactionStrategy::Tiered { size_ratio, min_merge_width, max_merge_width } => {
                self.select_tiered_compaction(sstables, *size_ratio, *min_merge_width, *max_merge_width)
            }
            CompactionStrategy::Leveled { size_ratio, max_level } => {
                self.select_leveled_compaction(sstables, *size_ratio, *max_level)
            }
            CompactionStrategy::Hybrid { l0_strategy, ln_strategy, transition_level } => {
                // For hybrid, use tiered for first few levels, leveled after
                if sstables.len() <= *transition_level as usize {
                    let temp_compactor = Compactor::new((**l0_strategy).clone(), self.base_path.clone());
                    temp_compactor.select_compaction(sstables)
                } else {
                    let temp_compactor = Compactor::new((**ln_strategy).clone(), self.base_path.clone());
                    temp_compactor.select_compaction(sstables)
                }
            }
        }
    }
    
    fn select_tiered_compaction(
        &self,
        sstables: &[SsTableHandle],
        size_ratio: f64,
        min_merge_width: usize,
        max_merge_width: usize,
    ) -> Option<CompactionJob> {
        // Tiered compaction: merge SSTables of similar size
        if sstables.len() < min_merge_width {
            return None;
        }
        
        // Group SSTables by size tiers
        let mut size_groups: Vec<Vec<usize>> = Vec::new();
        
        for (idx, sstable) in sstables.iter().enumerate() {
            let size = sstable.num_entries;
            let mut added = false;
            
            // Try to add to existing group
            for group in &mut size_groups {
                if let Some(&first_idx) = group.first() {
                    let first_size = sstables[first_idx].num_entries;
                    let ratio = (size as f64) / (first_size as f64);
                    
                    if ratio >= 1.0 / size_ratio && ratio <= size_ratio {
                        group.push(idx);
                        added = true;
                        break;
                    }
                }
            }
            
            if !added {
                size_groups.push(vec![idx]);
            }
        }
        
        // Find the largest group that's ready to compact
        for group in size_groups {
            if group.len() >= min_merge_width {
                let to_compact = group.into_iter().take(max_merge_width).collect();
                return Some(CompactionJob {
                    inputs: to_compact,
                    strategy: CompactionStrategy::Tiered {
                        size_ratio,
                        min_merge_width,
                        max_merge_width,
                    },
                });
            }
        }
        
        None
    }
    
    fn select_leveled_compaction(
        &self,
        sstables: &[SsTableHandle],
        _size_ratio: f64,
        _max_level: u8,
    ) -> Option<CompactionJob> {
        // Simplified leveled compaction: compact oldest SSTables
        // In a real implementation, we'd organize into levels and compact level-by-level
        
        if sstables.len() >= 4 {
            // Compact the 4 oldest SSTables
            let to_compact: Vec<usize> = (0..4).collect();
            return Some(CompactionJob {
                inputs: to_compact,
                strategy: CompactionStrategy::Leveled {
                    size_ratio: _size_ratio,
                    max_level: _max_level,
                },
            });
        }
        
        None
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
        active_dir: &PathBuf,
        run_number: RunNumber,
        max_level: u8,
    ) -> Result<CompactionResult> {
        // Collect all entries from source runs
        let mut all_entries: BTreeMap<Key, Option<Value>> = BTreeMap::new();

        for &idx in &job.source_runs {
            let sstable = &source_level_runs[idx];

            // Read all entries including tombstones
            let entries = sstable.range_with_tombstones(&Key::from(b""), &Key::from(&[0xFF; 256]))?;

            for (key, value_opt) in entries {
                // Later entries overwrite earlier ones
                all_entries.insert(key, value_opt);
            }
        }

        // For bottom level (leveling): merge with overlapping runs and remove tombstones
        // For other levels (tiering): keep tombstones, don't merge with target level
        let is_bottom_level = job.target_level == max_level;

        if is_bottom_level {
            // Leveling: merge with ALL runs in target level, remove tombstones
            for target_run in &job.target_level_runs {
                let entries = target_run.range_with_tombstones(&Key::from(b""), &Key::from(&[0xFF; 256]))?;

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
    pub fn compact(&self, job: CompactionJob, sstables: &[SsTableHandle], active_dir: &PathBuf, run_number: RunNumber) -> Result<CompactionResult> {
        // Collect all entries from input SSTables (including tombstones!)
        let mut all_entries: BTreeMap<Key, Option<Value>> = BTreeMap::new();

        for &idx in &job.inputs {
            let sstable = &sstables[idx];

            // Read all entries from this SSTable INCLUDING tombstones
            let entries = sstable.range_with_tombstones(&Key::from(b""), &Key::from(&[0xFF; 256]))?;

            for (key, value_opt) in entries {
                // Later entries overwrite earlier ones (including tombstones)
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

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CompactionJob {
    pub inputs: Vec<usize>, // Indices of SSTables to compact
    pub strategy: CompactionStrategy,
}

/// Level-based compaction job for LazyLevelling policy
#[derive(Debug, Clone)]
pub struct LevelCompactionJob {
    pub source_level: u8,
    pub target_level: u8,
    pub source_runs: Vec<usize>, // Indices within source level
    pub target_level_runs: Vec<SsTableHandle>, // All runs in target level for overlap detection
}

#[allow(dead_code)]
pub struct CompactionResult {
    pub output: Option<SsTableHandle>,
    pub inputs_to_remove: Vec<usize>,
    pub bytes_read: u64,
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
