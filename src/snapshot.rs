// File-based persistent snapshots matching Haskell implementation
//
// Snapshots are immutable copies of the LSM tree state stored in
// the snapshots/ directory. They use hard-links to share files with
// active/ without copying data.

use std::path::{Path, PathBuf};
use std::io;
use serde::{Serialize, Deserialize};

use crate::sstable_new::{SsTableHandle, RunNumber};
use crate::atomic_file::{AtomicFileWriter, fsync_directory};
use crate::checksum::CRC32C;
use crate::{Result, Error, LsmConfig};

/// Metadata for a persistent snapshot (CBOR format matching Haskell)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Format version
    pub version: u32,

    /// User-provided label/description
    pub label: String,

    /// LSM tree configuration
    pub config: LsmConfig,

    /// Run numbers of SSTables in this snapshot
    pub runs: Vec<SnapshotRun>,

    /// Sequence number at snapshot time
    pub sequence_number: u64,
}

/// Information about a single SSTable run in a snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRun {
    pub run_number: RunNumber,
    pub min_key: Vec<u8>,
    pub max_key: Vec<u8>,
    pub num_entries: u64,
}

/// A persistent snapshot on disk
pub struct PersistentSnapshot {
    pub name: String,
    pub path: PathBuf,
    pub metadata: SnapshotMetadata,
}

impl PersistentSnapshot {
    /// Create a new snapshot by hard-linking SSTables
    ///
    /// This:
    /// 1. Creates `snapshots/<name>/` directory
    /// 2. Hard-links all SSTable files from `active/` to `snapshots/<name>/`
    /// 3. Writes snapshot metadata in CBOR format
    /// 4. Writes metadata checksum
    /// 5. Fsyncs everything
    pub fn create(
        lsm_path: &Path,
        name: &str,
        label: &str,
        sstables: &[SsTableHandle],
        sequence_number: u64,
        config: &LsmConfig,
    ) -> Result<Self> {
        let snapshot_dir = lsm_path.join("snapshots").join(name);

        // Check if snapshot already exists
        if snapshot_dir.exists() {
            return Err(Error::InvalidOperation(
                format!("Snapshot '{}' already exists", name)
            ));
        }

        std::fs::create_dir_all(&snapshot_dir)?;

        // Hard-link all SSTables to snapshot directory
        // Renumber sequentially starting from 1
        let mut snapshot_runs = Vec::new();

        for (i, sstable) in sstables.iter().enumerate() {
            let new_run_number = (i + 1) as u64;

            // Create hard-linked copy
            let _linked_handle = sstable.hard_link_to(&snapshot_dir, new_run_number)
                .map_err(Error::Io)?;

            snapshot_runs.push(SnapshotRun {
                run_number: new_run_number,
                min_key: sstable.min_key.as_ref().to_vec(),
                max_key: sstable.max_key.as_ref().to_vec(),
                num_entries: sstable.num_entries,
            });
        }

        // Create metadata
        let metadata = SnapshotMetadata {
            version: 1,
            label: label.to_string(),
            config: config.clone(),
            runs: snapshot_runs,
            sequence_number,
        };

        // Write metadata file in CBOR format (matching Haskell)
        let metadata_path = snapshot_dir.join("metadata");
        let mut metadata_bytes = Vec::new();
        ciborium::into_writer(&metadata, &mut metadata_bytes)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        let mut writer = AtomicFileWriter::new(&metadata_path)?;
        writer.write_all(&metadata_bytes)?;
        writer.commit()?;

        // Write metadata checksum
        let checksum = CRC32C::hash(&metadata_bytes);
        let checksum_path = snapshot_dir.join("metadata.checksum");
        std::fs::write(&checksum_path, checksum.to_hex())?;

        // Fsync snapshot directory
        fsync_directory(&snapshot_dir)?;

        Ok(PersistentSnapshot {
            name: name.to_string(),
            path: snapshot_dir,
            metadata,
        })
    }

    /// Load an existing snapshot from disk
    pub fn load(lsm_path: &Path, name: &str) -> Result<Self> {
        let snapshot_dir = lsm_path.join("snapshots").join(name);

        if !snapshot_dir.exists() {
            return Err(Error::InvalidOperation(
                format!("Snapshot '{}' does not exist", name)
            ));
        }

        // Read metadata
        let metadata_path = snapshot_dir.join("metadata");
        let metadata_bytes = std::fs::read(&metadata_path)?;

        // Verify checksum
        let checksum_path = snapshot_dir.join("metadata.checksum");
        let expected_checksum_hex = std::fs::read_to_string(&checksum_path)?;
        let expected_checksum = CRC32C::from_hex(expected_checksum_hex.trim())
            .map_err(|e| Error::Corruption(format!("Invalid checksum format: {}", e)))?;

        let actual_checksum = CRC32C::hash(&metadata_bytes);

        if actual_checksum != expected_checksum {
            return Err(Error::Corruption(
                format!("Snapshot metadata checksum mismatch for '{}'", name)
            ));
        }

        // Deserialize metadata
        let metadata: SnapshotMetadata = ciborium::from_reader(&metadata_bytes[..])
            .map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(PersistentSnapshot {
            name: name.to_string(),
            path: snapshot_dir,
            metadata,
        })
    }

    /// Validate that all SSTable files referenced in the snapshot exist
    ///
    /// This checks that all 5 required files (.keyops, .blobs, .filter, .index, .checksums)
    /// exist for each run in the snapshot metadata.
    ///
    /// Returns detailed error information about which files are missing.
    pub fn validate(&self) -> Result<()> {
        use crate::sstable_new::RunPaths;

        for run in &self.metadata.runs {
            let paths = RunPaths::new(&self.path, run.run_number);

            // Check each required file individually for detailed error reporting
            let mut missing_files = Vec::new();

            if !paths.keyops.exists() {
                missing_files.push(paths.keyops.display().to_string());
            }
            if !paths.blobs.exists() {
                missing_files.push(paths.blobs.display().to_string());
            }
            if !paths.filter.exists() {
                missing_files.push(paths.filter.display().to_string());
            }
            if !paths.index.exists() {
                missing_files.push(paths.index.display().to_string());
            }
            if !paths.checksums.exists() {
                missing_files.push(paths.checksums.display().to_string());
            }

            if !missing_files.is_empty() {
                return Err(Error::Corruption(format!(
                    "Snapshot '{}' is corrupted - missing SSTable files for run {}:\n  {}",
                    self.name,
                    run.run_number,
                    missing_files.join("\n  ")
                )));
            }

            // Verify checksums for this run
            let checksums = crate::checksum::read_checksums_file(&paths.checksums)?;

            // Check keyops checksum
            let keyops_expected = crate::checksum::get_checksum(&checksums, "keyops")
                .map_err(|e| Error::Corruption(format!("Snapshot '{}' run {}: {}", self.name, run.run_number, e)))?;
            crate::checksum::check_crc(&paths.keyops, keyops_expected)
                .map_err(|e| Error::Corruption(format!("Snapshot '{}' run {} keyops checksum failed: {}", self.name, run.run_number, e)))?;

            // Check blobs checksum
            let blobs_expected = crate::checksum::get_checksum(&checksums, "blobs")
                .map_err(|e| Error::Corruption(format!("Snapshot '{}' run {}: {}", self.name, run.run_number, e)))?;
            crate::checksum::check_crc(&paths.blobs, blobs_expected)
                .map_err(|e| Error::Corruption(format!("Snapshot '{}' run {} blobs checksum failed: {}", self.name, run.run_number, e)))?;

            // Check filter checksum
            let filter_expected = crate::checksum::get_checksum(&checksums, "filter")
                .map_err(|e| Error::Corruption(format!("Snapshot '{}' run {}: {}", self.name, run.run_number, e)))?;
            crate::checksum::check_crc(&paths.filter, filter_expected)
                .map_err(|e| Error::Corruption(format!("Snapshot '{}' run {} filter checksum failed: {}", self.name, run.run_number, e)))?;

            // Check index checksum
            let index_expected = crate::checksum::get_checksum(&checksums, "index")
                .map_err(|e| Error::Corruption(format!("Snapshot '{}' run {}: {}", self.name, run.run_number, e)))?;
            crate::checksum::check_crc(&paths.index, index_expected)
                .map_err(|e| Error::Corruption(format!("Snapshot '{}' run {} index checksum failed: {}", self.name, run.run_number, e)))?;
        }

        Ok(())
    }

    /// Delete this snapshot from disk
    pub fn delete(self) -> io::Result<()> {
        // Remove the entire snapshot directory
        std::fs::remove_dir_all(&self.path)
    }
}

/// List all snapshots in an LSM tree directory
pub fn list_snapshots(lsm_path: &Path) -> Result<Vec<String>> {
    let snapshots_dir = lsm_path.join("snapshots");

    if !snapshots_dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();

    for entry in std::fs::read_dir(&snapshots_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                snapshots.push(name.to_string());
            }
        }
    }

    snapshots.sort();
    Ok(snapshots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::{Key, Value};
    use crate::sstable_new::SsTableWriter;

    #[test]
    fn test_create_and_load_snapshot() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let lsm_path = dir.path();
        let active_dir = lsm_path.join("active");
        std::fs::create_dir_all(&active_dir)?;

        // Create some SSTables
        let mut writer1 = SsTableWriter::new(&active_dir, 1)?;
        writer1.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        let handle1 = writer1.finish(0)?;

        let mut writer2 = SsTableWriter::new(&active_dir, 2)?;
        writer2.add(Key::from(b"key2"), Some(Value::from(b"value2")))?;
        let handle2 = writer2.finish(0)?;

        let sstables = vec![handle1, handle2];
        let config = LsmConfig::default();

        // Create snapshot
        let snapshot = PersistentSnapshot::create(
            lsm_path,
            "snap1",
            "Test snapshot",
            &sstables,
            42,
            &config,
        )?;

        assert_eq!(snapshot.name, "snap1");
        assert_eq!(snapshot.metadata.label, "Test snapshot");
        assert_eq!(snapshot.metadata.sequence_number, 42);
        assert_eq!(snapshot.metadata.runs.len(), 2);

        // Load snapshot
        let loaded = PersistentSnapshot::load(lsm_path, "snap1")?;

        assert_eq!(loaded.name, "snap1");
        assert_eq!(loaded.metadata.label, "Test snapshot");
        assert_eq!(loaded.metadata.sequence_number, 42);
        assert_eq!(loaded.metadata.runs.len(), 2);

        Ok(())
    }

    #[test]
    fn test_list_snapshots() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let lsm_path = dir.path();
        let active_dir = lsm_path.join("active");
        std::fs::create_dir_all(&active_dir)?;

        // Initially empty
        let snapshots = list_snapshots(lsm_path)?;
        assert_eq!(snapshots.len(), 0);

        // Create some snapshots
        let mut writer = SsTableWriter::new(&active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        let handle = writer.finish(0)?;

        let sstables = vec![handle];
        let config = LsmConfig::default();

        PersistentSnapshot::create(lsm_path, "snap1", "First", &sstables, 1, &config)?;
        PersistentSnapshot::create(lsm_path, "snap2", "Second", &sstables, 2, &config)?;
        PersistentSnapshot::create(lsm_path, "snap3", "Third", &sstables, 3, &config)?;

        let snapshots = list_snapshots(lsm_path)?;
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots, vec!["snap1", "snap2", "snap3"]);

        Ok(())
    }

    #[test]
    fn test_snapshot_files_shared() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let lsm_path = dir.path();
        let active_dir = lsm_path.join("active");
        std::fs::create_dir_all(&active_dir)?;

        // Create SSTable
        let mut writer = SsTableWriter::new(&active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        let handle = writer.finish(0)?;

        let sstables = vec![handle];
        let config = LsmConfig::default();

        // Create snapshot
        PersistentSnapshot::create(lsm_path, "snap1", "Test", &sstables, 1, &config)?;

        // Both files should exist (active and snapshot)
        let active_keyops = active_dir.join("00001.keyops");
        let snapshot_keyops = lsm_path.join("snapshots/snap1/00001.keyops");

        assert!(active_keyops.exists());
        assert!(snapshot_keyops.exists());

        // They should be hard-links (same inode on Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let active_meta = std::fs::metadata(&active_keyops)?;
            let snapshot_meta = std::fs::metadata(&snapshot_keyops)?;
            assert_eq!(active_meta.ino(), snapshot_meta.ino());
        }

        Ok(())
    }

    #[test]
    fn test_snapshot_validation_detects_missing_files() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let lsm_path = dir.path();
        let active_dir = lsm_path.join("active");
        std::fs::create_dir_all(&active_dir)?;

        // Create an SSTable
        let mut writer = SsTableWriter::new(&active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        let handle = writer.finish(0)?;

        let sstables = vec![handle];
        let config = LsmConfig::default();

        // Create snapshot
        let snapshot = PersistentSnapshot::create(
            lsm_path,
            "snap1",
            "Test snapshot",
            &sstables,
            42,
            &config,
        )?;

        // Validation should succeed initially
        assert!(snapshot.validate().is_ok());

        // Simulate the incident: delete SSTable files but leave metadata intact
        let snapshot_dir = lsm_path.join("snapshots").join("snap1");
        std::fs::remove_file(snapshot_dir.join("00001.blobs"))?;
        std::fs::remove_file(snapshot_dir.join("00001.index"))?;
        std::fs::remove_file(snapshot_dir.join("00001.keyops"))?;
        std::fs::remove_file(snapshot_dir.join("00001.filter"))?;
        std::fs::remove_file(snapshot_dir.join("00001.checksums"))?;

        // Load should still succeed (metadata is intact)
        let loaded = PersistentSnapshot::load(lsm_path, "snap1")?;

        // But validation should now fail and report missing files
        let validation_result = loaded.validate();
        assert!(validation_result.is_err());

        // Check that the error message contains information about missing files
        let err = validation_result.unwrap_err();
        let err_msg = format!("{}", err);
        assert!(err_msg.contains("corrupted"));
        assert!(err_msg.contains("missing"));
        assert!(err_msg.contains("00001.blobs") || err_msg.contains("SSTable files"));

        Ok(())
    }

    #[test]
    fn test_snapshot_validation_detects_checksum_mismatch() -> Result<()> {
        let dir = TempDir::new().map_err(Error::Io)?;
        let lsm_path = dir.path();
        let active_dir = lsm_path.join("active");
        std::fs::create_dir_all(&active_dir)?;

        // Create an SSTable
        let mut writer = SsTableWriter::new(&active_dir, 1)?;
        writer.add(Key::from(b"key1"), Some(Value::from(b"value1")))?;
        let handle = writer.finish(0)?;

        let sstables = vec![handle];
        let config = LsmConfig::default();

        // Create snapshot
        let snapshot = PersistentSnapshot::create(
            lsm_path,
            "snap1",
            "Test snapshot",
            &sstables,
            42,
            &config,
        )?;

        // Validation should succeed initially
        assert!(snapshot.validate().is_ok());

        // Corrupt an SSTable file by modifying its contents
        let snapshot_dir = lsm_path.join("snapshots").join("snap1");
        let blobs_path = snapshot_dir.join("00001.blobs");
        let mut data = std::fs::read(&blobs_path)?;
        if !data.is_empty() {
            data[0] ^= 0xFF; // Flip bits in first byte
            std::fs::write(&blobs_path, data)?;
        }

        // Load should still succeed (metadata is intact)
        let loaded = PersistentSnapshot::load(lsm_path, "snap1")?;

        // But validation should now fail with checksum error
        let validation_result = loaded.validate();
        assert!(validation_result.is_err());

        let err = validation_result.unwrap_err();
        let err_msg = format!("{}", err);
        assert!(err_msg.contains("checksum"));

        Ok(())
    }
}
