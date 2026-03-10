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

    /// Write all data to file and update checksum
    ///
    /// Ensures all bytes are written (like std::io::Write::write_all).
    pub fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.file.write_all(buf)?;
        self.hasher.update(buf);
        Ok(())
    }

    /// Close the handle and return the final checksum
    ///
    /// This syncs all data to disk before returning.
    /// The handle is consumed and cannot be used after this.
    pub fn close(self) -> io::Result<CRC32C> {
        self.file.sync_all()?;
        Ok(CRC32C(self.hasher.finalize()))
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

}
