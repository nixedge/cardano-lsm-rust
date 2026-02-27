// Session locking to prevent concurrent access to LSM tree
//
// This module implements exclusive locking for LSM tree sessions using
// OS-level file locks (flock on Unix, LockFileEx on Windows).
//
// This prevents multiple processes from accessing the same LSM tree
// simultaneously, which would cause corruption.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

/// Session lock that prevents concurrent access to an LSM tree
///
/// The lock is acquired when opening an LSM tree and held for the
/// lifetime of the session. It's automatically released when dropped.
///
/// Uses OS-level file locks:
/// - Unix: flock(2)
/// - Windows: LockFileEx
pub struct SessionLock {
    #[allow(dead_code)]
    file: File,
    path: PathBuf,
    #[allow(dead_code)]
    lock_info: LockInfo,
}

/// Information about the lock holder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LockInfo {
    pid: u32,
    hostname: String,
    timestamp: u64,
    uuid: String,
}

impl SessionLock {
    /// Acquire an exclusive lock on the session directory
    ///
    /// Returns an error if:
    /// - Another process holds the lock
    /// - The lock file cannot be created
    /// - The OS lock cannot be acquired
    pub fn acquire(session_dir: &Path) -> io::Result<Self> {
        let lock_path = session_dir.join("lock");

        // Check for existing lock file
        if lock_path.exists() {
            // Try to read existing lock info for better error messages
            if let Ok(existing_info) = Self::read_lock_info(&lock_path) {
                if Self::is_process_alive(existing_info.pid) {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "Session locked by PID {} on {} (acquired at timestamp {})",
                            existing_info.pid,
                            existing_info.hostname,
                            existing_info.timestamp
                        ),
                    ));
                }
                // Stale lock - process no longer exists, we can continue
            }
        }

        // Create/open lock file
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&lock_path)?;

        // Acquire exclusive OS-level lock
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = file.as_raw_fd();
            let result = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            if result != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Failed to acquire exclusive file lock (another process may hold it)",
                ));
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use winapi::um::fileapi::LockFileEx;
            use winapi::um::minwinbase::{LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY};

            let handle = file.as_raw_handle();
            let mut overlapped = std::mem::zeroed();
            let result = unsafe {
                LockFileEx(
                    handle as _,
                    LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                    0,
                    u32::MAX,
                    u32::MAX,
                    &mut overlapped,
                )
            };

            if result == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Failed to acquire exclusive file lock (another process may hold it)",
                ));
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            // Platform doesn't support file locking - proceed with warning
            eprintln!("Warning: File locking not supported on this platform");
        }

        // Write lock info to file
        let lock_info = LockInfo {
            pid: std::process::id(),
            hostname: hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            uuid: uuid::Uuid::new_v4().to_string(),
        };

        let lock_json = serde_json::to_string_pretty(&lock_info)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        writeln!(&mut file, "{}", lock_json)?;
        file.sync_all()?;

        Ok(SessionLock {
            file,
            path: lock_path,
            lock_info,
        })
    }

    /// Read lock info from an existing lock file
    fn read_lock_info(path: &Path) -> io::Result<LockInfo> {
        let contents = std::fs::read_to_string(path)?;
        serde_json::from_str(&contents)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Check if a process with the given PID is still alive
    fn is_process_alive(pid: u32) -> bool {
        #[cfg(unix)]
        {
            // Send signal 0 to check if process exists
            // Returns 0 if process exists, -1 if not
            let result = unsafe { libc::kill(pid as i32, 0) };
            result == 0
        }

        #[cfg(windows)]
        {
            use winapi::um::processthreadsapi::OpenProcess;
            use winapi::um::winnt::PROCESS_QUERY_INFORMATION;
            use winapi::um::handleapi::CloseHandle;

            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
                if handle.is_null() {
                    return false;
                }
                CloseHandle(handle);
                true
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = pid;
            // Can't check on this platform, assume alive
            true
        }
    }

    /// Get information about the lock holder
    #[allow(dead_code)]
    pub fn info(&self) -> &LockInfo {
        &self.lock_info
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // Lock is automatically released when file is closed (via Drop on File)
        // We also try to remove the lock file, but ignore errors
        // (another process might be racing to acquire the lock)
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_acquire_lock() -> io::Result<()> {
        let dir = TempDir::new()?;

        let _lock = SessionLock::acquire(dir.path())?;

        // Verify lock file exists
        assert!(dir.path().join("lock").exists());

        Ok(())
    }

    #[test]
    fn test_concurrent_lock_fails() -> io::Result<()> {
        let dir = TempDir::new()?;

        let _lock1 = SessionLock::acquire(dir.path())?;

        // Second lock should fail
        let result = SessionLock::acquire(dir.path());
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_lock_released_on_drop() -> io::Result<()> {
        let dir = TempDir::new()?;

        {
            let _lock = SessionLock::acquire(dir.path())?;
        } // lock dropped here

        // Should be able to acquire again
        let _lock2 = SessionLock::acquire(dir.path())?;

        Ok(())
    }

    #[test]
    fn test_lock_info() -> io::Result<()> {
        let dir = TempDir::new()?;

        let lock = SessionLock::acquire(dir.path())?;
        let info = lock.info();

        assert_eq!(info.pid, std::process::id());
        assert!(!info.hostname.is_empty());
        assert!(info.timestamp > 0);
        assert!(!info.uuid.is_empty());

        Ok(())
    }

    #[test]
    fn test_stale_lock_cleaned_up() -> io::Result<()> {
        let dir = TempDir::new()?;
        let lock_path = dir.path().join("lock");

        // Create a fake stale lock with a PID that doesn't exist
        let stale_info = LockInfo {
            pid: 99999999, // Very unlikely to exist
            hostname: "test".to_string(),
            timestamp: 0,
            uuid: "test-uuid".to_string(),
        };

        let lock_json = serde_json::to_string_pretty(&stale_info)?;
        std::fs::write(&lock_path, lock_json)?;

        // Should be able to acquire despite existing lock file
        let _lock = SessionLock::acquire(dir.path())?;

        Ok(())
    }
}
