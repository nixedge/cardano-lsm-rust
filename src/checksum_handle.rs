// Port of Database.LSMTree.Internal.ChecksumHandle from Haskell
//
// A handle that maintains a running CRC32C checksum while writing to a file.
// This is critical for ensuring data integrity - we compute checksums
// incrementally as we write, avoiding the need to re-read files later.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use crate::checksum::CRC32C;

/// A file handle that tracks CRC32C checksum of all data written
///
/// This allows incremental checksum computation during file writes,
/// which is essential for large files where re-reading to compute
/// checksums would be expensive.
///
/// The pattern is:
/// 1. Create ChecksumHandle via `create()`
/// 2. Write data via `write()` or `write_all()`
/// 3. Close and get checksum via `close()`
pub struct ChecksumHandle {
    file: File,
    hasher: crc32fast::Hasher,
}

impl ChecksumHandle {
    /// Create a new ChecksumHandle for writing
    ///
    /// Opens the file in write mode (must not exist) and initializes
    /// the checksum to zero.
    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::create_new(path)?;
        Ok(ChecksumHandle {
            file,
            hasher: crc32fast::Hasher::new(),
        })
    }

    /// Create from an existing File handle
    ///
    /// Useful when you've already opened the file and want to start
    /// tracking checksums from this point forward.
    #[allow(dead_code)]
    pub fn from_file(file: File) -> Self {
        ChecksumHandle {
            file,
            hasher: crc32fast::Hasher::new(),
        }
    }

    /// Write data to file and update checksum
    ///
    /// Returns the number of bytes written (like std::io::Write::write).
    /// Note: Unlike write_all, this may do a partial write.
    #[allow(dead_code)]
    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.file.write(buf)?;
        // Only update checksum for bytes actually written
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    /// Write all data to file and update checksum
    ///
    /// Ensures all bytes are written (like std::io::Write::write_all).
    pub fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.file.write_all(buf)?;
        self.hasher.update(buf);
        Ok(())
    }

    /// Flush any buffered data to disk
    ///
    /// This ensures data is written to the OS, but does not guarantee
    /// it's on persistent storage. Use sync_all() for durability.
    #[allow(dead_code)]
    pub fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }

    /// Sync all data and metadata to disk
    ///
    /// This is what you need for durability - ensures data is on
    /// persistent storage, not just in OS buffers.
    #[allow(dead_code)]
    pub fn sync_all(&mut self) -> io::Result<()> {
        self.file.sync_all()
    }

    /// Get current checksum without closing the handle
    ///
    /// Useful for debugging or progress tracking.
    /// Note: This clones the hasher to avoid consuming it.
    #[allow(dead_code)]
    pub fn current_checksum(&self) -> CRC32C {
        CRC32C(self.hasher.clone().finalize())
    }

    /// Close the handle and return the final checksum
    ///
    /// This syncs all data to disk before returning.
    /// The handle is consumed and cannot be used after this.
    pub fn close(self) -> io::Result<CRC32C> {
        self.file.sync_all()?;
        Ok(CRC32C(self.hasher.finalize()))
    }

    /// Close without syncing (not recommended for production use)
    ///
    /// Returns the checksum but does NOT sync data to disk.
    /// Only use this if you're going to sync via other means.
    #[allow(dead_code)]
    pub fn close_no_sync(self) -> CRC32C {
        CRC32C(self.hasher.finalize())
    }
}

/// Specialized write functions for SSTable components
///
/// These mirror the Haskell implementations for writing specific
/// data types with proper checksumming.
impl ChecksumHandle {
    /// Write a bloom filter to the handle
    ///
    /// The bloom filter is serialized and written, with checksum
    /// updated accordingly.
    #[allow(dead_code)]
    pub fn write_bloom_filter(&mut self, filter_bytes: &[u8]) -> io::Result<()> {
        self.write_all(filter_bytes)
    }

    /// Write an index chunk to the handle
    ///
    /// Index data is written with checksum tracking.
    #[allow(dead_code)]
    pub fn write_index(&mut self, index_bytes: &[u8]) -> io::Result<()> {
        self.write_all(index_bytes)
    }

    /// Write a data page (key-ops) to the handle
    ///
    /// This writes the raw page data with checksum tracking.
    #[allow(dead_code)]
    pub fn write_keyops_page(&mut self, page_bytes: &[u8]) -> io::Result<()> {
        self.write_all(page_bytes)
    }

    /// Write blob data to the handle
    ///
    /// Returns the number of bytes written (useful for tracking blob offsets).
    #[allow(dead_code)]
    pub fn write_blob(&mut self, blob_data: &[u8]) -> io::Result<usize> {
        self.write_all(blob_data)?;
        Ok(blob_data.len())
    }
}

/// Helper function to write a file with checksum in one shot
///
/// This is a convenience wrapper for:
/// 1. Create ChecksumHandle
/// 2. Write all data
/// 3. Close and get checksum
///
/// Returns the checksum of the written data.
pub fn write_file_with_checksum<P: AsRef<Path>>(
    path: P,
    data: &[u8],
) -> io::Result<CRC32C> {
    let mut handle = ChecksumHandle::create(path)?;
    handle.write_all(data)?;
    handle.close()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_create_and_write() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        let mut handle = ChecksumHandle::create(&path)?;
        handle.write_all(b"hello")?;
        handle.write_all(b" world")?;
        let crc = handle.close()?;

        // Verify file was written
        let content = fs::read(&path)?;
        assert_eq!(content, b"hello world");

        // Verify checksum matches what we'd get from hashing the data
        assert_eq!(crc, CRC32C::hash(b"hello world"));

        Ok(())
    }

    #[test]
    fn test_incremental_checksum() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        let mut handle = ChecksumHandle::create(&path)?;

        // Write in chunks
        handle.write_all(b"hello")?;
        let crc1 = handle.current_checksum();

        handle.write_all(b" world")?;
        let crc2 = handle.current_checksum();

        // Checksum should change after each write
        assert_ne!(crc1, CRC32C::INITIAL);
        assert_ne!(crc2, crc1);

        let final_crc = handle.close()?;

        // Final checksum should match computing over full data
        assert_eq!(final_crc, CRC32C::hash(b"hello world"));

        Ok(())
    }

    #[test]
    fn test_write_file_with_checksum() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        let crc = write_file_with_checksum(&path, b"test data")?;

        // Verify file exists and has correct content
        let content = fs::read(&path)?;
        assert_eq!(content, b"test data");

        // Verify checksum
        assert_eq!(crc, CRC32C::hash(b"test data"));

        Ok(())
    }

    #[test]
    fn test_empty_file() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("empty.dat");

        let handle = ChecksumHandle::create(&path)?;
        let crc = handle.close()?;

        // Empty file should have initial checksum
        assert_eq!(crc, CRC32C::INITIAL);

        Ok(())
    }

    #[test]
    fn test_sync_all() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        let mut handle = ChecksumHandle::create(&path)?;
        handle.write_all(b"data")?;

        // sync_all should not fail
        handle.sync_all()?;

        let crc = handle.close()?;
        assert_eq!(crc, CRC32C::hash(b"data"));

        Ok(())
    }
}
