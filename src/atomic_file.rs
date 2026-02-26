// Atomic file operations - temp file + atomic rename + directory fsync
//
// This module provides atomic file writes to prevent corruption from
// partial writes or crashes. The pattern is:
// 1. Write to temporary file (.tmp extension)
// 2. Fsync the temporary file
// 3. Atomically rename to final path
// 4. Fsync the parent directory to ensure rename is durable
//
// This guarantees that the file is either fully written or not present at all,
// never in a partial state that could corrupt the database.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

/// Writer for atomically creating a file
///
/// Writes to a temporary file and atomically renames it on commit.
/// If dropped without calling commit(), the temporary file is cleaned up.
pub struct AtomicFileWriter {
    temp_path: PathBuf,
    final_path: PathBuf,
    file: Option<BufWriter<File>>,
    committed: bool,
}

impl AtomicFileWriter {
    /// Create a new atomic file writer
    ///
    /// The file will be written to `path.tmp` until commit() is called.
    /// If the file already exists at the final path, it will be overwritten
    /// on commit (but the overwrite is atomic).
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let final_path = path.as_ref().to_path_buf();
        let temp_path = Self::temp_path_for(&final_path);

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)?;

        Ok(Self {
            temp_path,
            final_path,
            file: Some(BufWriter::new(file)),
            committed: false,
        })
    }

    /// Generate temporary path for a given final path
    fn temp_path_for(path: &Path) -> PathBuf {
        // If path has extension, replace it with .tmp
        // Otherwise, append .tmp
        if let Some(ext) = path.extension() {
            let mut new_ext = ext.to_os_string();
            new_ext.push(".tmp");
            path.with_extension(new_ext)
        } else {
            let mut new_path = path.to_path_buf();
            new_path.as_mut_os_string().push(".tmp");
            new_path
        }
    }

    /// Write data to the file
    pub fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.file.as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Writer already closed"))?
            .write_all(data)
    }

    /// Get mutable reference to the underlying writer
    ///
    /// Useful for writing with std::io::Write methods
    pub fn as_writer(&mut self) -> io::Result<&mut dyn Write> {
        self.file.as_mut()
            .map(|f| f as &mut dyn Write)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Writer already closed"))
    }

    /// Commit the file atomically
    ///
    /// This:
    /// 1. Flushes the buffer
    /// 2. Fsyncs the file data
    /// 3. Atomically renames temp -> final
    /// 4. Fsyncs the parent directory
    ///
    /// After this, the file is durably on disk.
    pub fn commit(mut self) -> io::Result<()> {
        let mut file = self.file.take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Writer already closed"))?;

        // 1. Flush buffer
        file.flush()?;

        // 2. Fsync file data and metadata
        file.get_ref().sync_all()?;
        drop(file);

        // 3. Atomic rename
        std::fs::rename(&self.temp_path, &self.final_path)?;

        // 4. Fsync directory to ensure rename is durable
        if let Some(parent) = self.final_path.parent() {
            fsync_directory(parent)?;
        }

        // Mark as committed so Drop doesn't clean up
        self.committed = true;

        Ok(())
    }

    /// Abort the write and clean up the temporary file
    ///
    /// This is called automatically on Drop if commit() wasn't called,
    /// but you can call it explicitly if you want to handle errors.
    pub fn abort(mut self) -> io::Result<()> {
        self.file.take(); // Close the file
        std::fs::remove_file(&self.temp_path)?;
        self.committed = true; // Don't try to clean up again in Drop
        Ok(())
    }
}

impl Drop for AtomicFileWriter {
    fn drop(&mut self) {
        // If commit() wasn't called, clean up the temp file
        if !self.committed {
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

/// Fsync a directory to ensure metadata changes are durable
///
/// This is critical after operations like rename() to ensure the
/// directory entry update is actually on disk.
///
/// On Unix, this opens the directory and calls fsync() on it.
/// On Windows, directories can't be opened, so this is a no-op
/// (Windows fsync semantics are different).
pub fn fsync_directory<P: AsRef<Path>>(path: P) -> io::Result<()> {
    #[cfg(unix)]
    {
        let dir = File::open(path)?;
        dir.sync_all()?;
    }

    #[cfg(not(unix))]
    {
        let _ = path; // Suppress unused warning on Windows
        // On Windows, directory fsync is implicit
    }

    Ok(())
}

/// Atomically write a file in one shot
///
/// This is a convenience wrapper for:
/// 1. Create AtomicFileWriter
/// 2. Write all data
/// 3. Commit
///
/// Returns Ok(()) if the file was successfully written and committed.
pub fn atomic_write<P: AsRef<Path>>(path: P, data: &[u8]) -> io::Result<()> {
    let mut writer = AtomicFileWriter::new(path)?;
    writer.write_all(data)?;
    writer.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_atomic_write_success() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        let mut writer = AtomicFileWriter::new(&path)?;
        writer.write_all(b"test data")?;
        writer.commit()?;

        // File should exist with correct content
        assert!(path.exists());
        let content = fs::read(&path)?;
        assert_eq!(content, b"test data");

        // Temp file should be cleaned up
        let temp_path = path.with_extension("dat.tmp");
        assert!(!temp_path.exists());

        Ok(())
    }

    #[test]
    fn test_atomic_write_abort() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        let writer = AtomicFileWriter::new(&path)?;
        // Drop without committing (simulates abort)
        drop(writer);

        // File should NOT exist
        assert!(!path.exists());

        // Temp file should be cleaned up
        let temp_path = path.with_extension("dat.tmp");
        assert!(!temp_path.exists());

        Ok(())
    }

    #[test]
    fn test_atomic_write_explicit_abort() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        let mut writer = AtomicFileWriter::new(&path)?;
        writer.write_all(b"this will be aborted")?;
        writer.abort()?;

        // File should NOT exist
        assert!(!path.exists());

        Ok(())
    }

    #[test]
    fn test_atomic_write_convenience() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        atomic_write(&path, b"convenience")?;

        let content = fs::read(&path)?;
        assert_eq!(content, b"convenience");

        Ok(())
    }

    #[test]
    fn test_temp_path_with_extension() {
        let path = PathBuf::from("/tmp/foo.dat");
        let temp = AtomicFileWriter::temp_path_for(&path);
        assert_eq!(temp, PathBuf::from("/tmp/foo.dat.tmp"));
    }

    #[test]
    fn test_temp_path_without_extension() {
        let path = PathBuf::from("/tmp/foo");
        let temp = AtomicFileWriter::temp_path_for(&path);
        assert_eq!(temp, PathBuf::from("/tmp/foo.tmp"));
    }

    #[test]
    fn test_overwrite_existing() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        // Write initial file
        fs::write(&path, b"original")?;

        // Atomic overwrite
        let mut writer = AtomicFileWriter::new(&path)?;
        writer.write_all(b"updated")?;
        writer.commit()?;

        // Should have new content
        let content = fs::read(&path)?;
        assert_eq!(content, b"updated");

        Ok(())
    }

    #[test]
    fn test_fsync_directory() -> io::Result<()> {
        let dir = TempDir::new()?;

        // Should not error
        fsync_directory(dir.path())?;

        Ok(())
    }

    #[test]
    fn test_multiple_writes() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.dat");

        let mut writer = AtomicFileWriter::new(&path)?;
        writer.write_all(b"hello")?;
        writer.write_all(b" ")?;
        writer.write_all(b"world")?;
        writer.commit()?;

        let content = fs::read(&path)?;
        assert_eq!(content, b"hello world");

        Ok(())
    }
}
