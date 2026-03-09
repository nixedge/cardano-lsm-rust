// I/O backend abstraction layer
//
// Provides both synchronous and asynchronous I/O implementations.
// On Linux with io-uring feature enabled, uses tokio-uring for async I/O.
// Otherwise, falls back to standard synchronous I/O.
//
// This matches the Haskell lsm-tree approach:
// - Linux: blockio-uring (asynchronous batched I/O)
// - Other platforms: sequential I/O

use std::path::Path;
use std::io;

/// I/O backend configuration
#[derive(Clone, Debug)]
#[derive(Default)]
pub enum IoBackend {
    /// Synchronous I/O (default, all platforms)
    #[default]
    Sync,

    /// Asynchronous I/O using io_uring (Linux only, requires io-uring feature)
    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    IoUring,
}


/// Read a file completely using the specified backend
pub fn read_file(path: &Path, backend: &IoBackend) -> io::Result<Vec<u8>> {
    match backend {
        IoBackend::Sync => {
            // Standard synchronous read
            std::fs::read(path)
        }

        #[cfg(all(target_os = "linux", feature = "io-uring"))]
        IoBackend::IoUring => {
            // Use io_uring for async I/O
            // Note: This is a blocking wrapper around async for now
            // In a full async implementation, this would be async fn
            tokio_uring::start(async {
                let file = tokio_uring::fs::File::open(path).await?;
                let (res, buf) = file.read_at(Vec::with_capacity(4096), 0).await;
                res?;
                Ok(buf)
            })
        }
    }
}

/// Read a specific range from a file
pub fn read_range(
    path: &Path,
    offset: u64,
    length: usize,
    backend: &IoBackend,
) -> io::Result<Vec<u8>> {
    match backend {
        IoBackend::Sync => {
            // Standard synchronous pread
            let file = std::fs::File::open(path)?;
            let mut buffer = vec![0u8; length];

            #[cfg(unix)]
            {
                use std::os::unix::fs::FileExt;
                file.read_exact_at(&mut buffer, offset)?;
            }

            #[cfg(not(unix))]
            {
                use std::io::Seek;
                file.seek(std::io::SeekFrom::Start(offset))?;
                file.read_exact(&mut buffer)?;
            }

            Ok(buffer)
        }

        #[cfg(all(target_os = "linux", feature = "io-uring"))]
        IoBackend::IoUring => {
            // Use io_uring for async positioned read
            tokio_uring::start(async {
                let file = tokio_uring::fs::File::open(path).await?;
                let buf = vec![0u8; length];
                let (res, buf) = file.read_at(buf, offset).await;
                res?;
                Ok(buf)
            })
        }
    }
}

/// Batch read multiple ranges from multiple files
///
/// This is the key performance optimization from blockio-uring:
/// reading multiple blocks concurrently.
pub fn read_batch(
    requests: Vec<(&Path, u64, usize)>, // (path, offset, length)
    backend: &IoBackend,
) -> io::Result<Vec<Vec<u8>>> {
    match backend {
        IoBackend::Sync => {
            // Sequential reads (fallback)
            requests
                .into_iter()
                .map(|(path, offset, length)| read_range(path, offset, length, backend))
                .collect()
        }

        #[cfg(all(target_os = "linux", feature = "io-uring"))]
        IoBackend::IoUring => {
            // Concurrent reads using io_uring
            tokio_uring::start(async {
                let mut results = Vec::with_capacity(requests.len());

                // Issue all reads concurrently
                let futures: Vec<_> = requests
                    .into_iter()
                    .map(|(path, offset, length)| async move {
                        let file = tokio_uring::fs::File::open(path).await?;
                        let buf = vec![0u8; length];
                        let (res, buf) = file.read_at(buf, offset).await;
                        res?;
                        Ok::<Vec<u8>, io::Error>(buf)
                    })
                    .collect();

                // Await all reads
                for future in futures {
                    results.push(future.await?);
                }

                Ok(results)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_sync_read_file() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.txt");
        fs::write(&path, b"Hello, World!")?;

        let backend = IoBackend::Sync;
        let data = read_file(&path, &backend)?;

        assert_eq!(data, b"Hello, World!");
        Ok(())
    }

    #[test]
    fn test_sync_read_range() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.txt");
        fs::write(&path, b"Hello, World!")?;

        let backend = IoBackend::Sync;
        let data = read_range(&path, 7, 5, &backend)?;

        assert_eq!(data, b"World");
        Ok(())
    }

    #[test]
    fn test_sync_read_batch() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path1 = dir.path().join("test1.txt");
        let path2 = dir.path().join("test2.txt");
        fs::write(&path1, b"File One")?;
        fs::write(&path2, b"File Two")?;

        let backend = IoBackend::Sync;
        let results = read_batch(
            vec![
                (&path1, 0, 8),
                (&path2, 0, 8),
            ],
            &backend,
        )?;

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], b"File One");
        assert_eq!(results[1], b"File Two");
        Ok(())
    }

    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    #[test]
    fn test_io_uring_read_file() -> io::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("test.txt");
        fs::write(&path, b"Hello, io_uring!")?;

        let backend = IoBackend::IoUring;
        let data = read_file(&path, &backend)?;

        assert_eq!(data, b"Hello, io_uring!");
        Ok(())
    }
}
